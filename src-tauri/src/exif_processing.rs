use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::formats::is_raw_file;
use crate::image_processing::ImageMetadata;
use chrono::{DateTime, Local, LocalResult, NaiveDateTime, TimeZone, Utc};
use little_exif::exif_tag::ExifTag;
use little_exif::filetype::FileExtension;
use little_exif::ifd::ExifTagGroup;
use little_exif::metadata::Metadata;
use little_exif::rational::{iR64, uR64};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SourceMetadata {
    pub fields: HashMap<String, String>,
    pub lens_profile: Option<crate::embedded_lens::EmbeddedLensProfile>,
    #[serde(default)]
    pub previews: Vec<EmbeddedPreview>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EmbeddedPreview {
    pub offset: u64,
    pub length: u64,
    pub orientation: u32,
}

#[derive(Serialize, Deserialize, Clone)]
struct CachedExifEntry {
    mtime_ns: u128,
    size: u64,
    metadata: SourceMetadata,
    source_read: bool,
}

struct ExifCacheState {
    cache: HashMap<PathBuf, HashMap<String, CachedExifEntry>>,
    dirty: HashSet<PathBuf>,
    cache_dir: Option<PathBuf>,
}

impl ExifCacheState {
    fn get_cache_file_path(&self, folder: &Path) -> Option<PathBuf> {
        let base_dir = self.cache_dir.as_ref()?;
        let hash = blake3::hash(folder.to_string_lossy().as_bytes())
            .to_hex()
            .to_string();
        Some(base_dir.join(format!("exiftool-v5-{}.json", hash)))
    }
}

fn get_exif_cache() -> &'static Mutex<ExifCacheState> {
    static EXIF_CACHE: OnceLock<Mutex<ExifCacheState>> = OnceLock::new();
    EXIF_CACHE.get_or_init(|| {
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(Duration::from_secs(3));
                flush_all_dirty_caches();
            }
        });

        Mutex::new(ExifCacheState {
            cache: HashMap::new(),
            dirty: HashSet::new(),
            cache_dir: None,
        })
    })
}

pub fn initialize_cache_dir(cache_dir: PathBuf) {
    let dir = cache_dir.join("exif");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut state) = get_exif_cache().lock() {
        state.cache_dir = Some(dir);
    }
}

pub fn flush_all_dirty_caches() {
    let mut state = match get_exif_cache().lock() {
        Ok(s) => s,
        Err(_) => return,
    };

    let dirty_folders: Vec<PathBuf> = state.dirty.drain().collect();
    let mut to_write = Vec::new();

    for folder in &dirty_folders {
        if let Some(folder_map) = state.cache.get(folder)
            && !folder_map.is_empty()
            && let Some(cache_path) = state.get_cache_file_path(folder)
        {
            to_write.push((cache_path, folder_map.clone()));
        }
    }

    drop(state);

    for (path, map) in to_write {
        if let Ok(json) = serde_json::to_string(&map) {
            let tmp_path = path.with_extension("tmp");
            if std::fs::write(&tmp_path, json).is_ok() {
                let _ = std::fs::rename(tmp_path, path);
            }
        }
    }
}

fn load_rrcache_for_folder(folder: &Path) {
    let mut cache_path = None;

    if let Ok(state) = get_exif_cache().lock() {
        if state.cache.contains_key(folder) {
            return;
        }
        cache_path = state.get_cache_file_path(folder);
    }

    let Some(path) = cache_path else {
        return;
    };

    let loaded_map = if let Ok(content) = std::fs::read_to_string(&path) {
        serde_json::from_str::<HashMap<String, CachedExifEntry>>(&content).unwrap_or_default()
    } else {
        HashMap::new()
    };

    if let Ok(mut state) = get_exif_cache().lock() {
        make_cache_room(&mut state, folder);
        state
            .cache
            .entry(folder.to_path_buf())
            .or_insert_with(|| loaded_map.into_iter().take(512).collect());
    }
}

fn make_cache_room(state: &mut ExifCacheState, folder: &Path) {
    if state.cache.len() >= 8 && !state.cache.contains_key(folder) {
        if let Some(old) = state.cache.keys().next().cloned() {
            state.cache.remove(&old);
            state.dirty.remove(&old);
        }
    }
}

fn get_file_stamp(path: &Path) -> Option<(u128, u64)> {
    let meta = fs::metadata(path).ok()?;
    let mtime_ns = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((mtime_ns, meta.len()))
}

fn get_cached_metadata(image_path: &Path) -> Option<CachedExifEntry> {
    let folder = image_path.parent()?;
    let filename = image_path.file_name()?.to_string_lossy().to_string();

    let (current_mtime, current_size) = get_file_stamp(image_path)?;

    load_rrcache_for_folder(folder);

    let mut state = get_exif_cache().lock().ok()?;
    let folder_map = state.cache.get_mut(folder)?;

    if let Some(entry) = folder_map.get(&filename) {
        if entry.mtime_ns == current_mtime && entry.size == current_size {
            return Some(entry.clone());
        }

        folder_map.remove(&filename);
        state.dirty.insert(folder.to_path_buf());
    }

    None
}

fn get_exif_from_rrcache(image_path: &Path) -> Option<HashMap<String, String>> {
    get_cached_metadata(image_path).map(|entry| entry.metadata.fields)
}

/// Source fields and correction data share one read, file stamp and bounded cache.
pub fn read_source_metadata(image_path: &Path) -> Result<SourceMetadata, String> {
    if let Some(entry) = get_cached_metadata(image_path).filter(|entry| entry.source_read) {
        return Ok(entry.metadata);
    }
    let metadata = crate::exiftool::read(image_path)?;
    save_metadata_to_rrcache(image_path, metadata.clone(), true);
    Ok(metadata)
}

fn save_exif_to_rrcache(image_path: &Path, fields: HashMap<String, String>) {
    // Migrated sidecars must not overwrite complete source metadata.
    if get_cached_metadata(image_path).is_none() {
        save_metadata_to_rrcache(
            image_path,
            SourceMetadata {
                fields,
                lens_profile: None,
                previews: Vec::new(),
            },
            false,
        );
    }
}

fn save_metadata_to_rrcache(image_path: &Path, metadata: SourceMetadata, source_read: bool) {
    let Some(folder) = image_path.parent() else {
        return;
    };
    let Some(filename) = image_path.file_name() else {
        return;
    };

    let Some((mtime_ns, size)) = get_file_stamp(image_path) else {
        return;
    };

    load_rrcache_for_folder(folder);

    let mut state = match get_exif_cache().lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    make_cache_room(&mut state, folder);
    let folder_map = state.cache.entry(folder.to_path_buf()).or_default();
    folder_map.insert(
        filename.to_string_lossy().to_string(),
        CachedExifEntry {
            mtime_ns,
            size,
            metadata,
            source_read,
        },
    );
    while folder_map.len() > 512 {
        let Some(old) = folder_map
            .keys()
            .find(|key| key.as_str() != filename.to_string_lossy())
            .cloned()
        else {
            break;
        };
        folder_map.remove(&old);
    }
    state.dirty.insert(folder.to_path_buf());
}

pub fn truncate_large_exif(value: &str) -> String {
    if value.len() <= 500 {
        return value.to_string();
    }

    let mut start_idx = 200;
    while !value.is_char_boundary(start_idx) {
        start_idx -= 1;
    }

    let mut end_idx = value.len() - 200;
    while !value.is_char_boundary(end_idx) {
        end_idx += 1;
    }

    if start_idx < end_idx {
        let start_str = &value[..start_idx];
        let end_str = &value[end_idx..];
        return format!("{}...{}", start_str, end_str);
    }

    value.to_string()
}

pub fn load_sidecar(sidecar_path: &Path) -> ImageMetadata {
    if !sidecar_path.exists() {
        return ImageMetadata::default();
    }

    let Ok(content) = fs::read_to_string(sidecar_path) else {
        return ImageMetadata::default();
    };

    let mut meta = serde_json::from_str::<ImageMetadata>(&content).unwrap_or_default();
    let mut healed = false;

    if let Some(ref mut exif_map) = meta.exif {
        for val in exif_map.values_mut() {
            if val.len() > 500 {
                *val = truncate_large_exif(val);
                healed = true;
            }
        }
    }

    if healed && let Ok(json) = serde_json::to_string_pretty(&meta) {
        let _ = fs::write(sidecar_path, json);
        log::info!(
            "Auto-healed bloated sidecar for: {}",
            sidecar_path.display()
        );
    }

    meta
}

pub fn load_sidecar_with_exif(sidecar_path: &Path, source_path: &Path) -> ImageMetadata {
    let mut meta = load_sidecar(sidecar_path);

    if crate::formats::is_supported_video_file(source_path) {
        return meta;
    }

    if meta.exif.is_none() {
        if let Some(cached_exif) = read_rrexif_sidecar(source_path) {
            meta.exif = Some(cached_exif);
        } else {
            let source_path_str = source_path.to_string_lossy();
            let extracted_exif =
                if let Ok(mmap) = crate::file_management::read_file_mapped(source_path) {
                    read_exif_data(&source_path_str, &mmap)
                } else if let Ok(bytes) = std::fs::read(source_path) {
                    read_exif_data(&source_path_str, &bytes)
                } else {
                    std::collections::HashMap::new()
                };

            if !extracted_exif.is_empty() {
                meta.exif = Some(extracted_exif);
            }
        }
    }

    meta
}

fn clean_creation_datetime_str(s: &str) -> &str {
    s.trim().trim_matches('"').trim_matches('\'').trim()
}

fn parse_creation_datetime(s: &str) -> Option<NaiveDateTime> {
    let clean = clean_creation_datetime_str(s).replace('T', " ");
    for format in [
        "%Y:%m:%d %H:%M:%S",
        "%Y:%m:%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(&clean, format) {
            return Some(dt);
        }
    }
    None
}

fn creation_datetime_to_utc(dt: NaiveDateTime) -> DateTime<Utc> {
    match Local.from_local_datetime(&dt) {
        LocalResult::Single(local_dt) | LocalResult::Ambiguous(local_dt, _) => {
            local_dt.with_timezone(&Utc)
        }
        LocalResult::None => DateTime::from_naive_utc_and_offset(dt, Utc),
    }
}

fn metadata_number(value: &str) -> Option<f64> {
    let value = value.trim().trim_matches('"').trim_end_matches(" s").trim();
    let number = if let Some((n, d)) = value.split_once('/') {
        n.parse::<f64>().ok()? / d.parse::<f64>().ok()?
    } else {
        value.parse().ok()?
    };
    number.is_finite().then_some(number)
}

#[cfg(test)]
mod metadata_reuse_tests {
    use super::*;

    #[test]
    fn cached_exiftool_fields_drive_helpers_orientation_and_raw_export() {
        let dir = tempfile::tempdir().unwrap();
        // A JPEG container with a RAW extension exercises the RAW metadata writer.
        let source = dir.path().join("source.dng");
        let mut bytes = Vec::new();
        image::DynamicImage::new_rgb8(3, 2)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        let mut metadata = Metadata::new();
        metadata.set_tag(ExifTag::Orientation(vec![6]));
        metadata.set_tag(ExifTag::ISO(vec![4000]));
        metadata.set_tag(ExifTag::ExposureTime(vec![uR64 {
            nominator: 1,
            denominator: 8000,
        }]));
        metadata.set_tag(ExifTag::DateTimeOriginal("2025:06:07 08:09:10".into()));
        metadata.set_tag(ExifTag::Artist("Camera author".into()));
        metadata.set_tag(ExifTag::Copyright("Camera rights".into()));
        metadata.set_tag(ExifTag::GPSLatitude(vec![
            uR64 {
                nominator: 1,
                denominator: 1,
            },
            uR64 {
                nominator: 30,
                denominator: 1,
            },
            uR64 {
                nominator: 0,
                denominator: 1,
            },
        ]));
        metadata.set_tag(ExifTag::GPSLatitudeRef("S".into()));
        metadata.set_tag(ExifTag::GPSLongitude(vec![
            uR64 {
                nominator: 2,
                denominator: 1,
            },
            uR64 {
                nominator: 30,
                denominator: 1,
            },
            uR64 {
                nominator: 0,
                denominator: 1,
            },
        ]));
        metadata.set_tag(ExifTag::GPSLongitudeRef("W".into()));
        metadata
            .write_to_vec(&mut bytes, FileExtension::JPEG)
            .unwrap();
        fs::write(&source, &bytes).unwrap();
        save_exif_to_rrcache(&source, HashMap::from([("ISO".into(), "64".into())]));
        assert_eq!(read_iso(source.to_str().unwrap(), &bytes), Some(4000));
        let fields = read_source_metadata(&source).unwrap().fields;
        let requests = crate::exiftool::request_count();
        let path = source.to_str().unwrap();
        assert_eq!(read_iso(path, &bytes), Some(4000));
        assert!((read_exposure_time_secs(path, &bytes).unwrap() - 0.000125).abs() < 1e-9);
        assert!(try_get_exif_creation_date(&source).is_some());
        let oriented =
            crate::image_loader::load_image_with_orientation(&bytes, path, None).unwrap();
        assert_eq!((oriented.width(), oriented.height()), (2, 3));
        let mut exported = bytes.clone();
        write_image_with_metadata(&mut exported, path, "jpeg", true, false, true).unwrap();
        assert_eq!(crate::exiftool::request_count(), requests);
        let output = dir.path().join("export.jpg");
        fs::write(&output, &exported).unwrap();
        let result = read_source_metadata(&output).unwrap().fields;
        for key in [
            "ISO",
            "Artist",
            "Copyright",
            "DateTimeOriginal",
            "GPSLatitude",
            "GPSLatitudeRef",
            "GPSLongitude",
            "GPSLongitudeRef",
            "ExposureTime",
        ] {
            assert_eq!(result.get(key), fields.get(key), "{key}");
        }
        assert_eq!(result.get("Orientation").map(String::as_str), Some("1"));
        write_image_with_metadata(&mut exported, path, "jpeg", true, true, true).unwrap();
        let stripped = dir.path().join("stripped.jpg");
        fs::write(&stripped, exported).unwrap();
        assert!(
            !read_source_metadata(&stripped)
                .unwrap()
                .fields
                .contains_key("GPSLatitude")
        );
    }
}

pub fn read_exposure_time_secs(path: &str, file_bytes: &[u8]) -> Option<f32> {
    let map = read_exif_data(path, file_bytes);
    metadata_number(
        map.get("ExposureTime")
            .or_else(|| map.get("ShutterSpeedValue"))?,
    )
    .map(|v| v as f32)
    .filter(|v| v.is_finite() && *v > 0.0)
}

pub fn read_iso(path: &str, file_bytes: &[u8]) -> Option<u32> {
    let map = read_exif_data(path, file_bytes);
    [
        "ISO",
        "PhotographicSensitivity",
        "ISOSpeedRatings",
        "ISOSpeed",
    ]
    .iter()
    .filter_map(|key| map.get(*key).and_then(|v| metadata_number(v)))
    .find(|v| *v > 0.0 && *v != 65535.0)
    .map(|v| v.round() as u32)
}

pub fn get_creation_date_from_path(path: &Path) -> DateTime<Utc> {
    try_get_exif_creation_date(path)
        .or_else(|| {
            fs::metadata(path)
                .ok()?
                .created()
                .ok()
                .map(DateTime::<Utc>::from)
        })
        .unwrap_or_else(Utc::now)
}

pub fn try_get_exif_creation_date(path: &Path) -> Option<DateTime<Utc>> {
    let source = read_source_metadata(path).ok()?;
    ["DateTimeOriginal", "CreateDate", "ModifyDate"]
        .iter()
        .find_map(|key| {
            source
                .fields
                .get(*key)
                .and_then(|v| parse_creation_datetime(v))
        })
        .map(creation_datetime_to_utc)
}

fn copy_full_exif_from_source(
    metadata: &mut Metadata,
    original_path: &Path,
    strip_gps: bool,
) -> bool {
    let Ok(source_metadata) = Metadata::new_from_path(original_path) else {
        return false;
    };

    let mut copied_any = false;
    for ifd in source_metadata.get_ifds() {
        if ifd.get_generic_ifd_nr() != 0 {
            continue;
        }
        if strip_gps && ifd.get_ifd_type() == ExifTagGroup::GPS {
            continue;
        }
        for tag in ifd.get_tags() {
            metadata.set_tag(tag.clone());
            copied_any = true;
        }
    }
    copied_any
}

fn apply_sidecar_field_overrides(metadata: &mut Metadata, map: &HashMap<String, String>) {
    let clean_s = |s: &String| s.replace('"', "").trim().to_string();
    let is_user_edit = |s: &str| !s.is_empty() && s != "...";

    match map.get("Artist").map(clean_s) {
        Some(val) => {
            if is_user_edit(&val) {
                metadata.set_tag(ExifTag::Artist(val));
            }
        }
        None => {
            metadata.remove_tag(ExifTag::Artist(String::new()));
        }
    }
    match map.get("Copyright").map(clean_s) {
        Some(val) => {
            if is_user_edit(&val) {
                metadata.set_tag(ExifTag::Copyright(val));
            }
        }
        None => {
            metadata.remove_tag(ExifTag::Copyright(String::new()));
        }
    }
    match map.get("ImageDescription").map(clean_s) {
        Some(val) => {
            if is_user_edit(&val) {
                metadata.set_tag(ExifTag::ImageDescription(val));
            }
        }
        None => {
            metadata.remove_tag(ExifTag::ImageDescription(String::new()));
        }
    }
    match map.get("UserComment").map(clean_s) {
        Some(val) => {
            if is_user_edit(&val) && !val.starts_with("0x") {
                metadata.set_tag(ExifTag::UserComment(val.into_bytes()));
            }
        }
        None => {
            metadata.remove_tag(ExifTag::UserComment(Vec::new()));
        }
    }
}

fn apply_gps_from_metadata(metadata: &mut Metadata, map: &HashMap<String, String>) {
    for (key, reference, limit, positive, negative, tag, ref_tag) in [
        (
            "GPSLatitude",
            "GPSLatitudeRef",
            90.0,
            "N",
            "S",
            ExifTag::GPSLatitude as fn(Vec<uR64>) -> ExifTag,
            ExifTag::GPSLatitudeRef as fn(String) -> ExifTag,
        ),
        (
            "GPSLongitude",
            "GPSLongitudeRef",
            180.0,
            "E",
            "W",
            ExifTag::GPSLongitude,
            ExifTag::GPSLongitudeRef,
        ),
    ] {
        if let Some(value) = map
            .get(key)
            .and_then(|v| metadata_number(v))
            .filter(|v| v.abs() <= limit)
        {
            let degrees = value.abs().floor();
            let minutes = (value.abs() - degrees) * 60.0;
            metadata.set_tag(tag(vec![
                uR64 {
                    nominator: degrees as u32,
                    denominator: 1,
                },
                uR64 {
                    nominator: minutes.floor() as u32,
                    denominator: 1,
                },
                uR64 {
                    nominator: (minutes.fract() * 60.0 * 1_000_000.0).round() as u32,
                    denominator: 1_000_000,
                },
            ]));
            let direction = map
                .get(reference)
                .map(String::as_str)
                .filter(|v| *v == positive || *v == negative)
                .unwrap_or(if value < 0.0 { negative } else { positive });
            metadata.set_tag(ref_tag(direction.to_string()));
        }
    }
    if let Some(value) = map.get("GPSAltitude").and_then(|v| metadata_number(v)) {
        metadata.set_tag(ExifTag::GPSAltitude(vec![uR64 {
            nominator: (value.abs() * 1000.0).round() as u32,
            denominator: 1000,
        }]));
        let reference = map
            .get("GPSAltitudeRef")
            .and_then(|v| v.parse::<u8>().ok())
            .filter(|v| *v <= 1)
            .unwrap_or(u8::from(value < 0.0));
        metadata.set_tag(ExifTag::GPSAltitudeRef(vec![reference]));
    }
}

pub fn write_image_with_metadata(
    image_bytes: &mut Vec<u8>,
    original_path_str: &str,
    output_format: &str,
    keep_metadata: bool,
    strip_gps: bool,
    is_srgb: bool,
) -> Result<(), String> {
    // FIXME: temporary solution until I find a way to write metadata to TIFF
    if !keep_metadata || output_format.to_lowercase() == "tiff" {
        return Ok(());
    }

    let original_path = Path::new(original_path_str);
    if !original_path.exists() {
        return Ok(());
    }

    // Skip TIFF sources to avoid potential tag corruption issues
    let original_ext = original_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if original_ext == "tiff" || original_ext == "tif" {
        return Ok(());
    }

    let file_type = match output_format.to_lowercase().as_str() {
        "jpg" | "jpeg" => FileExtension::JPEG,
        "png" => FileExtension::PNG {
            as_zTXt_chunk: true,
        },
        "tiff" => FileExtension::TIFF,
        "webp" => FileExtension::WEBP,
        _ => return Ok(()),
    };

    let mut metadata = Metadata::new();

    let full_exif_copied = !is_raw_file(original_path_str)
        && copy_full_exif_from_source(&mut metadata, original_path, strip_gps);
    let map = read_source_metadata(original_path)?.fields;

    if !full_exif_copied {
        for (key, tag) in [
            ("Flash", ExifTag::Flash as fn(Vec<u16>) -> ExifTag),
            ("MeteringMode", ExifTag::MeteringMode),
            ("WhiteBalance", ExifTag::WhiteBalance),
            ("ExposureProgram", ExifTag::ExposureProgram),
        ] {
            if let Some(value) = map.get(key).and_then(|value| value.parse::<u16>().ok()) {
                metadata.set_tag(tag(vec![value]));
            }
        }
        if let Some(value) = map
            .get("ExposureCompensation")
            .and_then(|value| metadata_number(value))
        {
            metadata.set_tag(ExifTag::ExposureCompensation(vec![iR64 {
                nominator: (value * 1_000_000.0).round() as i32,
                denominator: 1_000_000,
            }]));
        }
        let clean_s = |s: &String| s.replace('"', "").trim().to_string();

        let parse_ur64 = |s: &str| -> Option<uR64> {
            let cleaned_string = s
                .replace("f/", "")
                .replace(" s", "")
                .replace(" mm", "")
                .replace("\"", "");

            let val = cleaned_string.trim();

            if val.contains('/') {
                let parts: Vec<&str> = val.split('/').collect();
                if parts.len() == 2
                    && let (Ok(n), Ok(d)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>())
                {
                    return Some(uR64 {
                        nominator: n,
                        denominator: d,
                    });
                }
            } else if let Ok(f) = val.parse::<f64>()
                && f.is_finite()
                && f >= 0.0
            {
                let denominator = if f == 0.0 {
                    1_000_000.0
                } else {
                    1_000_000.0_f64.min(u32::MAX as f64 / f).floor().max(1.0)
                };
                return Some(uR64 {
                    nominator: (f * denominator).round() as u32,
                    denominator: denominator as u32,
                });
            }
            None
        };
        if let Some(val) = map.get("Make") {
            metadata.set_tag(ExifTag::Make(clean_s(val)));
        }
        if let Some(val) = map.get("Model") {
            metadata.set_tag(ExifTag::Model(clean_s(val)));
        }
        if let Some(val) = map.get("LensMake") {
            metadata.set_tag(ExifTag::LensMake(clean_s(val)));
        }
        if let Some(val) = map.get("LensModel") {
            metadata.set_tag(ExifTag::LensModel(clean_s(val)));
        }
        if let Some(val) = map.get("Artist") {
            metadata.set_tag(ExifTag::Artist(clean_s(val)));
        }
        if let Some(val) = map.get("Copyright") {
            metadata.set_tag(ExifTag::Copyright(clean_s(val)));
        }
        if let Some(val) = map.get("UserComment") {
            metadata.set_tag(ExifTag::UserComment(clean_s(val).into_bytes()));
        }
        if let Some(val) = map.get("ImageDescription") {
            metadata.set_tag(ExifTag::ImageDescription(clean_s(val)));
        }
        if let Some(val) = map.get("DateTimeOriginal") {
            metadata.set_tag(ExifTag::DateTimeOriginal(clean_s(val)));
        }
        if let Some(val) = map.get("CreateDate") {
            metadata.set_tag(ExifTag::CreateDate(clean_s(val)));
        }
        if let Some(val) = map.get("FNumber")
            && let Some(ur) = parse_ur64(val)
        {
            metadata.set_tag(ExifTag::FNumber(vec![ur]));
        }
        if let Some(val) = map.get("ExposureTime")
            && let Some(ur) = parse_ur64(val)
        {
            metadata.set_tag(ExifTag::ExposureTime(vec![ur]));
        }
        if let Some(val) = map.get("FocalLength")
            && let Some(ur) = parse_ur64(val)
        {
            metadata.set_tag(ExifTag::FocalLength(vec![ur]));
        }
        if let Some(val) = map
            .get("FocalLengthIn35mmFormat")
            .or(map.get("FocalLengthIn35mmFilm"))
        {
            let cleaned = val.replace(" mm", "").replace("\"", "");
            let trimmed = cleaned.trim();
            if let Ok(f_val) = trimmed.parse::<f32>() {
                metadata.set_tag(ExifTag::FocalLengthIn35mmFormat(vec![f_val.round() as u16]));
            }
        }
        if let Some(val) = map.get("ISO").or(map.get("PhotographicSensitivity"))
            && let Ok(iso) = val.replace('"', "").trim().parse::<u16>()
        {
            metadata.set_tag(ExifTag::ISO(vec![iso]));
        }
    }

    if !strip_gps && !full_exif_copied {
        apply_gps_from_metadata(&mut metadata, &map);
    }

    if let Some(map) = read_rrexif_sidecar(original_path) {
        apply_sidecar_field_overrides(&mut metadata, &map);
    }

    metadata.set_tag(ExifTag::Software("RapidRAW".to_string()));
    metadata.set_tag(ExifTag::Orientation(vec![1u16]));
    metadata.set_tag(ExifTag::ColorSpace(vec![if is_srgb { 1 } else { 65535 }]));

    if let Ok(reader) =
        image::ImageReader::new(Cursor::new(image_bytes.as_slice())).with_guessed_format()
        && let Ok((width, height)) = reader.into_dimensions()
    {
        metadata.set_tag(ExifTag::ExifImageWidth(vec![width]));
        metadata.set_tag(ExifTag::ExifImageHeight(vec![height]));
    }

    if let Err(e) = metadata.write_to_vec(image_bytes, file_type) {
        log::warn!("Failed to write metadata: {}", e);
    }

    Ok(())
}

pub fn get_primary_sidecar_path(image_path: &Path) -> PathBuf {
    let mut filename = image_path.file_name().unwrap_or_default().to_os_string();
    filename.push(".rrdata");
    image_path.with_file_name(filename)
}

pub fn get_rrexif_path(image_path: &Path) -> PathBuf {
    let mut filename = image_path.file_name().unwrap_or_default().to_os_string();
    filename.push(".rrexif");
    image_path.with_file_name(filename)
}

fn load_primary_metadata(image_path: &Path) -> ImageMetadata {
    let primary = get_primary_sidecar_path(image_path);
    load_sidecar(&primary)
}

fn save_primary_metadata(image_path: &Path, metadata: &ImageMetadata) -> std::io::Result<()> {
    let primary = get_primary_sidecar_path(image_path);
    let json = serde_json::to_string_pretty(metadata).map_err(std::io::Error::other)?;
    fs::write(&primary, json)
}

pub fn read_rrexif_sidecar(image_path: &Path) -> Option<HashMap<String, String>> {
    let primary = get_primary_sidecar_path(image_path);

    if primary.exists() {
        let metadata = load_primary_metadata(image_path);
        if let Some(exif) = metadata.exif {
            return Some(exif);
        }
    }

    if let Some(exif) = get_exif_from_rrcache(image_path) {
        return Some(exif);
    }

    let legacy = get_rrexif_path(image_path);
    if legacy.exists()
        && let Ok(content) = fs::read_to_string(&legacy)
        && let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&content)
    {
        save_exif_to_rrcache(image_path, map.clone());
        let _ = fs::remove_file(&legacy);
        return Some(map);
    }

    None
}

pub fn read_exif_data_from_bytes(path: &str, file_bytes: &[u8]) -> HashMap<String, String> {
    let read = || -> Result<HashMap<String, String>, String> {
        if Path::new(path).is_file() {
            return read_source_metadata(Path::new(path)).map(|metadata| metadata.fields);
        }
        // In-memory inputs need a real path for ExifTool.
        let temporary = tempfile::Builder::new()
            .suffix(".image")
            .tempfile()
            .map_err(|e| e.to_string())?;
        fs::write(temporary.path(), file_bytes).map_err(|e| e.to_string())?;
        crate::exiftool::read(temporary.path()).map(|metadata| metadata.fields)
    };
    read().unwrap_or_else(|error| {
        log::warn!("Metadata read failed for {path}: {error}");
        HashMap::new()
    })
}

pub fn read_exif_data(path: &str, file_bytes: &[u8]) -> HashMap<String, String> {
    let source_path = Path::new(path);

    if let Some(cached) = get_cached_metadata(source_path).filter(|entry| entry.source_read) {
        return cached.metadata.fields;
    }

    let exif_map = read_exif_data_from_bytes(path, file_bytes);
    if !exif_map.is_empty() {
        save_exif_to_rrcache(source_path, exif_map.clone());
    }

    exif_map
}

pub fn persist_exif_if_missing(source_path: &Path, source_path_str: &str, file_bytes: &[u8]) {
    if read_rrexif_sidecar(source_path).is_some() {
        return;
    }

    let exif_map = read_exif_data_from_bytes(source_path_str, file_bytes);
    if exif_map.is_empty() {
        return;
    }

    let primary = get_primary_sidecar_path(source_path);

    if primary.exists() {
        let mut metadata = load_primary_metadata(source_path);
        if metadata.exif.is_none() {
            metadata.exif = Some(exif_map);
            let _ = save_primary_metadata(source_path, &metadata);
        }
    } else {
        save_exif_to_rrcache(source_path, exif_map);
    }
}

pub fn write_rrexif_sidecar(source_path_str: &str, target_image_path: &Path) -> Result<(), String> {
    let source_path = Path::new(source_path_str);

    let exif_data = if let Some(existing) = read_rrexif_sidecar(source_path) {
        existing
    } else if let Ok(bytes) = fs::read(source_path) {
        read_exif_data_from_bytes(source_path_str, &bytes)
    } else {
        return Ok(());
    };

    if exif_data.is_empty() {
        return Ok(());
    }

    let mut metadata = load_primary_metadata(target_image_path);
    metadata.exif = Some(exif_data);
    save_primary_metadata(target_image_path, &metadata)
        .map_err(|e| format!("Failed to write sidecar: {}", e))
}
