use memmap2::{Mmap, MmapOptions};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;

use anyhow::Result;
use chrono::{DateTime, Utc};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageBuffer, Luma};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sysinfo::Disks;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::AppState;
use crate::PendingMetadata;
use crate::app_settings::*;
use crate::exif_processing;
use crate::formats::{is_raw_file, is_supported_image_file};
use crate::gpu_processing;
use crate::image_loader;
use crate::image_processing::GpuContext;
use crate::image_processing::{
    ImageMetadata, apply_coarse_rotation, apply_cpu_default_raw_processing, auto_results_to_json,
    get_all_adjustments_from_json, perform_auto_analysis,
};
use crate::mask_generation::MaskDefinition;
use crate::tagging::COLOR_TAG_PREFIX;

fn resolve_thumbnail_cache_dir(app_handle: &AppHandle) -> std::result::Result<PathBuf, String> {
    let cache_dir = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?;
    let thumb_cache_dir = cache_dir.join("thumbnails");
    if !thumb_cache_dir.exists() {
        fs::create_dir_all(&thumb_cache_dir).map_err(|e| e.to_string())?;
    }
    Ok(thumb_cache_dir)
}

fn emit_thumbnail_cache_setup_error(app_handle: &AppHandle, path: &str, reason: &str) {
    let _ = app_handle.emit(
        "thumbnail-generation-error",
        serde_json::json!({ "path": path, "reason": reason }),
    );
}

fn compute_thumbnail_cache_hash(path_str: &str, adjustments_bytes: &[u8]) -> Option<String> {
    let (source_path, _) = parse_virtual_path(path_str);

    let img_mod_time = fs::metadata(&source_path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"raw-color-detail-dehaze-v20");
    hasher.update(path_str.as_bytes());
    hasher.update(&img_mod_time.to_le_bytes());
    hasher.update(adjustments_bytes);
    Some(hasher.finalize().to_hex().to_string())
}

struct ImageFileMetadata {
    is_edited: bool,
    tags: Option<Vec<String>>,
    rating: u8,
    is_raw: bool,
}

fn resolve_image_metadata(
    image_path: &Path,
    sidecar_path: &Path,
    settings: &AppSettings,
) -> ImageFileMetadata {
    let metadata = crate::xmp::load_metadata(image_path, sidecar_path);

    let is_raw = crate::formats::is_raw_file(image_path);
    let tm_override = crate::image_processing::resolve_tonemapper_override(settings, is_raw);
    let is_edited =
        crate::image_processing::is_image_edited(&metadata.adjustments, is_raw, tm_override);
    ImageFileMetadata {
        is_edited,
        tags: metadata.tags,
        rating: metadata.rating,
        is_raw,
    }
}

fn emit_image_metadata_loaded(
    app_handle: &AppHandle,
    path: &str,
    rating: u8,
    is_edited: bool,
    tags: &Option<Vec<String>>,
) {
    let _ = app_handle.emit(
        "image-metadata-loaded",
        serde_json::json!({ "path": path, "rating": rating, "is_edited": is_edited, "tags": tags }),
    );
}

fn enqueue_metadata(
    app_handle: &AppHandle,
    virtual_path: String,
    image_path: PathBuf,
    sidecar_path: PathBuf,
) {
    let state = app_handle.state::<crate::AppState>();
    let manager = &state.metadata_manager;

    let mut pending = manager.pending.lock().unwrap();
    if !pending.insert(sidecar_path.clone()) {
        return;
    }
    drop(pending);

    manager.queue.lock().unwrap().push_back(PendingMetadata {
        virtual_path,
        image_path,
        sidecar_path,
    });
    manager.cvar.notify_one();
}

// Not compute-heavy — these threads mostly block waiting on iCloud to
// materialize a file, not burning CPU — so a small fixed pool is enough and
// doesn't need a user-facing setting the way thumbnail_worker_threads does.
const METADATA_WORKER_THREADS: usize = 4;

pub fn start_metadata_workers(app_handle: tauri::AppHandle) {
    let state = app_handle.state::<crate::AppState>();
    let manager = state.metadata_manager.clone();

    for _ in 0..METADATA_WORKER_THREADS {
        let app_clone = app_handle.clone();
        let manager_clone = manager.clone();

        std::thread::spawn(move || {
            loop {
                let item = {
                    let mut queue = manager_clone.queue.lock().unwrap();
                    while queue.is_empty() {
                        queue = manager_clone.cvar.wait(queue).unwrap();
                    }
                    queue.pop_front().unwrap()
                };

                let settings = load_settings(app_clone.clone()).unwrap_or_default();

                let metadata =
                    resolve_image_metadata(&item.image_path, &item.sidecar_path, &settings);

                emit_image_metadata_loaded(
                    &app_clone,
                    &item.virtual_path,
                    metadata.rating,
                    metadata.is_edited,
                    &metadata.tags,
                );

                manager_clone
                    .pending
                    .lock()
                    .unwrap()
                    .remove(&item.sidecar_path);
            }
        });
    }
}

#[derive(Debug)]
pub enum ReadFileError {
    Io(std::io::Error),
    Locked,
    Empty,
    NotFound,
    Invalid,
}

impl fmt::Display for ReadFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadFileError::Io(err) => write!(f, "IO error: {}", err),
            ReadFileError::Locked => write!(f, "File is locked"),
            ReadFileError::Empty => write!(f, "File is empty"),
            ReadFileError::NotFound => write!(f, "File not found"),
            ReadFileError::Invalid => write!(f, "Invalid file"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageFile {
    pub path: String,
    modified: u64,
    is_edited: bool,
    rating: u8,
    tags: Option<Vec<String>>,
    exif: Option<HashMap<String, String>>,
    is_virtual_copy: bool,
    is_cloud_placeholder: bool,
    is_raw: bool,
    group_id: Option<String>,
}

fn make_group_key(source_path: &Path) -> String {
    let parent = source_path.parent().unwrap_or(Path::new(""));
    let stem = source_path.file_stem().unwrap_or_default();
    format!("{}/{}", parent.to_string_lossy(), stem.to_string_lossy())
}

fn assign_group_ids(files: &mut [ImageFile], settings: &crate::app_settings::AppSettings) {
    let require_matching_exif = settings.require_matching_exif.unwrap_or(false);
    let group_edited_files = settings.group_edited_files.unwrap_or(true);

    #[derive(Clone)]
    struct Candidate {
        index: usize,
        source_path: PathBuf,
        key: String,
    }

    let candidates: Vec<Candidate> = files
        .iter()
        .enumerate()
        .filter(|(_, file)| !file.is_virtual_copy && (group_edited_files || !file.is_edited))
        .map(|(index, file)| {
            let (source_path, _) = parse_virtual_path(&file.path);
            let key = make_group_key(&source_path);
            Candidate {
                index,
                source_path,
                key,
            }
        })
        .collect();

    let mut stem_groups: HashMap<String, Vec<Candidate>> = HashMap::new();
    for candidate in candidates {
        stem_groups
            .entry(candidate.key.clone())
            .or_default()
            .push(candidate);
    }

    if require_matching_exif {
        let groupable_paths: Vec<PathBuf> = stem_groups
            .values()
            .filter(|candidates| candidates.len() >= 2)
            .flat_map(|candidates| candidates.iter().map(|c| c.source_path.clone()))
            .collect();
        let exif_dates: HashMap<PathBuf, Option<chrono::DateTime<chrono::Utc>>> = groupable_paths
            .par_iter()
            .map(|p| {
                (
                    p.clone(),
                    crate::exif_processing::try_get_exif_creation_date(p),
                )
            })
            .collect();

        stem_groups.retain(|_, candidates| {
            if candidates.len() < 2 {
                return false;
            }
            let first = exif_dates
                .get(&candidates[0].source_path)
                .copied()
                .flatten();
            if first.is_none() {
                return false;
            }
            candidates
                .iter()
                .skip(1)
                .all(|c| exif_dates.get(&c.source_path).copied().flatten() == first)
        });
    } else {
        stem_groups.retain(|_, candidates| candidates.len() >= 2);
    }

    for (key, candidates) in stem_groups {
        for candidate in candidates {
            files[candidate.index].group_id = Some(key.clone());
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportSettings {
    pub filename_template: String,
    pub organize_by_date: bool,
    pub date_folder_format: String,
    pub delete_after_import: bool,
    pub import_mode: Option<String>,
}

fn import_subfolder<Tz>(template: &str, date: &chrono::DateTime<Tz>) -> Result<PathBuf, String>
where
    Tz: chrono::TimeZone,
    Tz::Offset: std::fmt::Display,
{
    let format = template
        .replace("YYYY", "%Y")
        .replace("MM", "%m")
        .replace("DD", "%d");
    let rendered = date.format(&format).to_string();
    let path = PathBuf::from(rendered);
    if path.components().any(|part| {
        !matches!(
            part,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    }) {
        return Err("Folder structure must be a relative path without '..'.".into());
    }
    Ok(path)
}

pub fn parse_virtual_path(virtual_path: &str) -> (PathBuf, PathBuf) {
    let (source_path_str, copy_id) = if let Some((base, id)) = virtual_path.rsplit_once("?vc=") {
        (base.to_string(), Some(id.to_string()))
    } else {
        (virtual_path.to_string(), None)
    };

    let source_path = PathBuf::from(source_path_str);

    let sidecar_filename = if let Some(id) = copy_id {
        format!(
            "{}.{}.rrdata",
            source_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            id
        )
    } else {
        format!(
            "{}.rrdata",
            source_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        )
    };

    let sidecar_path = source_path.with_file_name(sidecar_filename);
    (source_path, sidecar_path)
}

#[tauri::command]
pub async fn read_exif_for_paths(
    paths: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<HashMap<String, HashMap<String, String>>, String> {
    let is_hdd = state
        .thumbnail_manager
        .rotational_disk
        .load(Ordering::Relaxed);

    tauri::async_runtime::spawn_blocking(move || {
        let process_path = |virtual_path: &String| {
            let (source_path, _) = parse_virtual_path(virtual_path);
            let source_path_str = source_path.to_string_lossy().to_string();

            let map = if let Some(sidecar_exif) =
                crate::exif_processing::read_rrexif_sidecar(&source_path)
            {
                sidecar_exif
            } else if is_cloud_placeholder(&source_path) {
                HashMap::new()
            } else if let Ok(mmap) = read_file_mapped(&source_path) {
                crate::exif_processing::read_exif_data(&source_path_str, &mmap)
            } else if let Ok(bytes) = fs::read(&source_path) {
                crate::exif_processing::read_exif_data(&source_path_str, &bytes)
            } else {
                HashMap::new()
            };

            if map.is_empty() {
                None
            } else {
                Some((virtual_path.clone(), map))
            }
        };

        let exif_data: HashMap<String, HashMap<String, String>> = if is_hdd {
            paths.iter().filter_map(process_path).collect()
        } else {
            paths.par_iter().filter_map(process_path).collect()
        };

        Ok(exif_data)
    })
    .await
    .unwrap_or_else(|e| Err(format!("Task failed: {}", e)))
}

#[tauri::command]
pub async fn update_exif_fields(
    paths: Vec<String>,
    updates: HashMap<String, String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        paths.par_iter().try_for_each(|path| {
            let source = Path::new(path);
            let edits = crate::exif_processing::get_primary_sidecar_path(source);
            crate::xmp::update_metadata(source, &edits, |metadata| {
                let exif = metadata.exif.get_or_insert_default();
                for (key, value) in &updates {
                    exif.insert(key.clone(), value.trim().to_owned());
                }
            })
        })
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

fn match_disk_kind(disks: &Disks, canonical: &Path) -> Option<bool> {
    let mut best_match: Option<(&Path, bool)> = None;

    for disk in disks.list() {
        let mount_point = disk.mount_point();
        if canonical.starts_with(mount_point) {
            let is_longer_match = best_match
                .map(|(current, _)| mount_point.as_os_str().len() > current.as_os_str().len())
                .unwrap_or(true);
            if is_longer_match {
                best_match = Some((mount_point, disk.kind() == sysinfo::DiskKind::HDD));
            }
        }
    }

    best_match.map(|(_, is_hdd)| is_hdd)
}

fn update_rotational_disk_flag(path: &str, app_handle: &AppHandle) {
    let state = app_handle.state::<crate::AppState>();
    let Ok(canonical) = Path::new(path).canonicalize() else {
        return;
    };

    let cached_match = {
        let cache = state.disks_cache.lock().unwrap();
        cache
            .as_ref()
            .and_then(|disks| match_disk_kind(disks, &canonical))
    };

    match cached_match {
        Some(is_hdd) => {
            state
                .thumbnail_manager
                .rotational_disk
                .store(is_hdd, Ordering::Relaxed);
        }
        None => {
            if !state.disks_cache_refreshing.swap(true, Ordering::Relaxed) {
                let refresh_app_handle = app_handle.clone();
                thread::spawn(move || {
                    let disks = Disks::new_with_refreshed_list();
                    let state = refresh_app_handle.state::<crate::AppState>();
                    *state.disks_cache.lock().unwrap() = Some(disks);
                    state.disks_cache_refreshing.store(false, Ordering::Relaxed);
                });
            }
        }
    }
}

#[tauri::command]
pub fn list_images_in_dir(path: String, app_handle: AppHandle) -> Result<Vec<ImageFile>, String> {
    let settings = load_settings(app_handle.clone()).unwrap_or_default();

    update_rotational_disk_flag(&path, &app_handle);

    let entries = fs::read_dir(&path).map_err(|e| e.to_string())?;
    let mut images = Vec::new();
    let mut sidecars_by_filename: HashMap<String, Vec<Option<String>>> = HashMap::new();

    for entry in entries.filter_map(Result::ok) {
        let entry_path = entry.path();
        let file_name = entry
            .file_name()
            .into_string()
            .unwrap_or_else(|os| os.to_string_lossy().into_owned());

        if file_name.ends_with(".rrdata") {
            let base = &file_name[..file_name.len() - 7];

            let (source_filename, copy_id) =
                if base.len() >= 7 && base.as_bytes()[base.len() - 7] == b'.' {
                    let id = &base[base.len() - 6..];
                    if id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
                        (&base[..base.len() - 7], Some(id.to_string()))
                    } else {
                        (base, None)
                    }
                } else {
                    (base, None)
                };

            sidecars_by_filename
                .entry(source_filename.to_string())
                .or_default()
                .push(copy_id);
        } else if is_supported_image_file(&file_name) {
            images.push((file_name, entry_path));
        }
    }

    let tasks: Vec<_> = images
        .into_iter()
        .map(|(file_name, path_buf)| {
            let sidecars = sidecars_by_filename
                .remove(&file_name)
                .unwrap_or_else(|| vec![None]);
            let path_str = path_buf.to_string_lossy().into_owned();
            (path_str, file_name, path_buf, sidecars)
        })
        .collect();

    let mut result_list: Vec<ImageFile> = tasks
        .into_par_iter()
        .flat_map(|(path_str, file_name, path_buf, sidecars)| {
            let modified = fs::metadata(&path_buf)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let is_cloud_placeholder = is_cloud_placeholder(&path_buf);

            let mut file_results = Vec::with_capacity(sidecars.len());

            for copy_id_opt in sidecars {
                let (virtual_path, is_virtual_copy, sidecar_filename) = match copy_id_opt {
                    Some(id) => (
                        format!("{}?vc={}", path_str, id),
                        true,
                        format!("{}.{}.rrdata", file_name, id),
                    ),
                    None => (path_str.clone(), false, format!("{}.rrdata", file_name)),
                };

                let sidecar_path = path_buf.with_file_name(sidecar_filename);

                let xmp_is_placeholder = crate::xmp::existing_path(&path_buf, &sidecar_path)
                    .is_some_and(|p| crate::file_management::is_cloud_placeholder(&p));

                let metadata = if crate::file_management::is_cloud_placeholder(&sidecar_path)
                    || xmp_is_placeholder
                {
                    enqueue_metadata(
                        &app_handle,
                        virtual_path.clone(),
                        path_buf.clone(),
                        sidecar_path.clone(),
                    );
                    ImageFileMetadata {
                        is_edited: false,
                        tags: None,
                        rating: 0,
                        is_raw: crate::formats::is_raw_file(&path_buf),
                    }
                } else {
                    resolve_image_metadata(&path_buf, &sidecar_path, &settings)
                };

                file_results.push(ImageFile {
                    path: virtual_path,
                    modified,
                    is_edited: metadata.is_edited,
                    tags: metadata.tags,
                    exif: None,
                    is_virtual_copy,
                    is_raw: metadata.is_raw,
                    group_id: None,
                    rating: metadata.rating,
                    is_cloud_placeholder,
                });
            }

            file_results
        })
        .collect();

    assign_group_ids(&mut result_list, &settings);
    Ok(result_list)
}

#[tauri::command]
pub fn list_images_recursive(
    path: String,
    app_handle: AppHandle,
) -> Result<Vec<ImageFile>, String> {
    let settings = load_settings(app_handle.clone()).unwrap_or_default();

    update_rotational_disk_flag(&path, &app_handle);

    let root_path = Path::new(&path);
    let mut images = Vec::new();

    let mut sidecars_by_path: HashMap<PathBuf, Vec<Option<String>>> = HashMap::new();

    for entry in WalkDir::new(root_path).into_iter().filter_map(Result::ok) {
        let entry_path = entry.path();
        if !entry_path.is_file() {
            continue;
        }

        let file_name = entry_path.file_name().unwrap_or_default().to_string_lossy();
        if let Some(base) = file_name.strip_suffix(".rrdata") {
            let (source_filename, copy_id) =
                if base.len() >= 7 && base.as_bytes()[base.len() - 7] == b'.' {
                    let id = &base[base.len() - 6..];
                    if id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
                        (&base[..base.len() - 7], Some(id.to_string()))
                    } else {
                        (base, None)
                    }
                } else {
                    (base, None)
                };

            if let Some(parent) = entry_path.parent() {
                sidecars_by_path
                    .entry(parent.join(source_filename))
                    .or_default()
                    .push(copy_id);
            }
        } else if is_supported_image_file(entry_path.to_string_lossy().as_ref()) {
            images.push(entry_path.to_path_buf());
        }
    }

    let tasks: Vec<_> = images
        .into_iter()
        .map(|path_buf| {
            let sidecars = sidecars_by_path
                .remove(&path_buf)
                .unwrap_or_else(|| vec![None]);
            let path_str = path_buf.to_string_lossy().into_owned();
            let file_name = path_buf
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            (path_str, file_name, path_buf, sidecars)
        })
        .collect();

    let mut result_list: Vec<ImageFile> = tasks
        .into_par_iter()
        .flat_map(|(path_str, file_name, path_buf, sidecars)| {
            let modified = fs::metadata(&path_buf)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let is_cloud_placeholder = is_cloud_placeholder(&path_buf);

            let mut file_results = Vec::with_capacity(sidecars.len());

            for copy_id_opt in sidecars {
                let (virtual_path, is_virtual_copy, sidecar_filename) = match copy_id_opt {
                    Some(id) => (
                        format!("{}?vc={}", path_str, id),
                        true,
                        format!("{}.{}.rrdata", file_name, id),
                    ),
                    None => (path_str.clone(), false, format!("{}.rrdata", file_name)),
                };

                let sidecar_path = path_buf.with_file_name(sidecar_filename);

                let xmp_is_placeholder = crate::xmp::existing_path(&path_buf, &sidecar_path)
                    .is_some_and(|p| crate::file_management::is_cloud_placeholder(&p));

                let metadata = if crate::file_management::is_cloud_placeholder(&sidecar_path)
                    || xmp_is_placeholder
                {
                    enqueue_metadata(
                        &app_handle,
                        virtual_path.clone(),
                        path_buf.clone(),
                        sidecar_path.clone(),
                    );
                    ImageFileMetadata {
                        is_edited: false,
                        tags: None,
                        rating: 0,
                        is_raw: crate::formats::is_raw_file(&path_buf),
                    }
                } else {
                    resolve_image_metadata(&path_buf, &sidecar_path, &settings)
                };

                file_results.push(ImageFile {
                    path: virtual_path,
                    modified,
                    is_edited: metadata.is_edited,
                    tags: metadata.tags,
                    exif: None,
                    is_virtual_copy,
                    is_raw: metadata.is_raw,
                    group_id: None,
                    rating: metadata.rating,
                    is_cloud_placeholder,
                });
            }

            file_results
        })
        .collect();

    assign_group_ids(&mut result_list, &settings);
    Ok(result_list)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AlbumItem {
    Album {
        id: String,
        name: String,
        icon: Option<String>,
        images: Vec<String>,
    },
    Group {
        id: String,
        name: String,
        icon: Option<String>,
        children: Vec<AlbumItem>,
    },
    SmartGroup {
        id: String,
        name: String,
        icon: Option<String>,
        filter: crate::catalog::CatalogFilter,
    },
}

fn get_albums_path(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = crate::portable_paths::app_data_dir(app_handle)?;
    let albums_dir = data_dir.join("albums");
    if !albums_dir.exists() {
        fs::create_dir_all(&albums_dir).map_err(|e| e.to_string())?;
    }
    Ok(albums_dir.join("albums.json"))
}

pub fn sort_album_tree(items: &mut [AlbumItem]) {
    items.sort_by(|a, b| {
        let get_sort_key = |item: &AlbumItem| match item {
            AlbumItem::Group { name, .. } => (0, name.to_lowercase()),
            AlbumItem::Album { name, .. } => (1, name.to_lowercase()),
            AlbumItem::SmartGroup { name, .. } => (2, name.to_lowercase()),
        };

        let key_a = get_sort_key(a);
        let key_b = get_sort_key(b);

        key_a.cmp(&key_b)
    });

    for item in items.iter_mut() {
        if let AlbumItem::Group { children, .. } = item {
            sort_album_tree(children);
        }
    }
}

#[tauri::command]
pub fn get_albums(app_handle: AppHandle) -> Result<Vec<AlbumItem>, String> {
    let path = get_albums_path(&app_handle)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut items: Vec<AlbumItem> = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    sort_album_tree(&mut items);
    Ok(items)
}

#[tauri::command]
pub fn save_albums(mut tree: Vec<AlbumItem>, app_handle: AppHandle) -> Result<(), String> {
    let path = get_albums_path(&app_handle)?;
    sort_album_tree(&mut tree);
    let json_string = serde_json::to_string_pretty(&tree).map_err(|e| e.to_string())?;
    fs::write(path, json_string).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_to_album(
    album_id: String,
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let mut tree = get_albums(app_handle.clone())?;

    fn add_recursive(items: &mut [AlbumItem], target_id: &str, paths_to_add: &Vec<String>) -> bool {
        for item in items.iter_mut() {
            #[allow(clippy::collapsible_match)]
            match item {
                AlbumItem::Album { id, images, .. } if id == target_id => {
                    for p in paths_to_add {
                        if !images.contains(p) {
                            images.push(p.clone());
                        }
                    }
                    return true;
                }
                AlbumItem::Group { children, .. } => {
                    if add_recursive(children, target_id, paths_to_add) {
                        return true;
                    }
                }
                AlbumItem::SmartGroup { .. } => {}
                _ => {}
            }
        }
        false
    }

    if add_recursive(&mut tree, &album_id, &paths) {
        save_albums(tree, app_handle)?;
    }
    Ok(())
}

fn sync_album_path_changes(
    app_handle: &AppHandle,
    renames: Option<&HashMap<String, String>>,
    deletions: Option<&HashSet<String>>,
    folder_rename: Option<(&str, &str)>,
) {
    if let Ok(mut tree) = get_albums(app_handle.clone()) {
        let mut changed = false;

        fn process_nodes(
            nodes: &mut [AlbumItem],
            renames: Option<&HashMap<String, String>>,
            deletions: Option<&HashSet<String>>,
            folder_rename: Option<(&str, &str)>,
            changed: &mut bool,
        ) {
            for node in nodes.iter_mut() {
                match node {
                    AlbumItem::Album { images, .. } => {
                        let mut new_images = Vec::new();

                        for img in images.drain(..) {
                            let mut current_img = img;

                            if let Some((old_folder, new_folder)) = folder_rename {
                                let img_path = Path::new(&current_img);
                                let old_path = Path::new(old_folder);
                                if let Ok(stripped) = img_path.strip_prefix(old_path) {
                                    let new_img_path = Path::new(new_folder).join(stripped);
                                    current_img = new_img_path.to_string_lossy().into_owned();
                                    *changed = true;
                                }
                            }

                            if let Some(r) = renames {
                                if let Some(new_path) = r.get(&current_img) {
                                    current_img = new_path.clone();
                                    *changed = true;
                                } else if let Some((base_path, vc_id)) =
                                    current_img.rsplit_once("?vc=")
                                    && let Some(new_base) = r.get(base_path)
                                {
                                    current_img = format!("{}?vc={}", new_base, vc_id);
                                    *changed = true;
                                }
                            }

                            let mut is_deleted = false;
                            if let Some(d) = deletions {
                                if d.contains(&current_img) {
                                    is_deleted = true;
                                } else {
                                    let img_path = Path::new(&current_img);
                                    for del_path_str in d {
                                        let del_path = Path::new(del_path_str);
                                        if img_path.starts_with(del_path) {
                                            is_deleted = true;
                                            break;
                                        }

                                        if let Some((base_path, _)) =
                                            current_img.rsplit_once("?vc=")
                                            && base_path == del_path_str
                                        {
                                            is_deleted = true;
                                            break;
                                        }
                                    }
                                }
                            }

                            if !is_deleted {
                                new_images.push(current_img);
                            } else {
                                *changed = true;
                            }
                        }
                        *images = new_images;
                    }
                    AlbumItem::Group { children, .. } => {
                        process_nodes(children, renames, deletions, folder_rename, changed);
                    }
                    AlbumItem::SmartGroup { .. } => {}
                }
            }
        }

        process_nodes(&mut tree, renames, deletions, folder_rename, &mut changed);

        if changed {
            let _ = save_albums(tree, app_handle.clone());
        }
    }
}

#[tauri::command]
pub fn get_album_images(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<Vec<ImageFile>, String> {
    let settings = load_settings(app_handle.clone()).unwrap_or_default();

    let mut result_list: Vec<ImageFile> = paths
        .into_par_iter()
        .filter_map(|virtual_path| {
            let (source_path, sidecar_path) = parse_virtual_path(&virtual_path);
            if !source_path.exists() {
                return None;
            }

            let modified = fs::metadata(&source_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let is_virtual_copy = virtual_path.contains("?vc=");
            let is_cloud_placeholder = is_cloud_placeholder(&source_path);

            let xmp_is_placeholder = crate::xmp::existing_path(&source_path, &sidecar_path)
                .is_some_and(|p| crate::file_management::is_cloud_placeholder(&p));

            let metadata = if crate::file_management::is_cloud_placeholder(&sidecar_path)
                || xmp_is_placeholder
            {
                enqueue_metadata(
                    &app_handle,
                    virtual_path.clone(),
                    source_path.clone(),
                    sidecar_path.clone(),
                );
                ImageFileMetadata {
                    is_edited: false,
                    tags: None,
                    rating: 0,
                    is_raw: crate::formats::is_raw_file(&source_path),
                }
            } else {
                resolve_image_metadata(&source_path, &sidecar_path, &settings)
            };

            Some(ImageFile {
                path: virtual_path.clone(),
                modified,
                is_edited: metadata.is_edited,
                tags: metadata.tags,
                exif: None,
                is_virtual_copy,
                is_raw: metadata.is_raw,
                group_id: None,
                rating: metadata.rating,
                is_cloud_placeholder,
            })
        })
        .collect();

    assign_group_ids(&mut result_list, &settings);
    Ok(result_list)
}

/// Checks if the given path exists and is an iCloud placeholder file on macOS.
#[cfg(target_os = "macos")]
pub fn is_cloud_placeholder(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    const SF_DATALESS: u32 = 0x4000_0000;

    let c_path = match std::ffi::CString::new(path.as_os_str().as_bytes()) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::lstat(c_path.as_ptr(), &mut stat_buf) };
    ret == 0 && (stat_buf.st_flags & SF_DATALESS) != 0
}

#[cfg(not(target_os = "macos"))]
pub fn is_cloud_placeholder(_path: &Path) -> bool {
    false
}

pub fn read_file_mapped(path: &Path) -> Result<Mmap, ReadFileError> {
    if !path.is_file() {
        return Err(ReadFileError::Invalid);
    }
    if !path.exists() {
        return Err(ReadFileError::NotFound);
    }
    if path.metadata().map_err(ReadFileError::Io)?.len() == 0 {
        return Err(ReadFileError::Empty);
    }
    let file = fs::File::open(path).map_err(ReadFileError::Io)?;
    if file.try_lock_shared().is_err() {
        return Err(ReadFileError::Locked);
    }
    let mmap = unsafe {
        MmapOptions::new()
            .len(file.metadata().map_err(ReadFileError::Io)?.len() as usize)
            .map(&file)
            .map_err(ReadFileError::Io)?
    };
    Ok(mmap)
}

fn apply_exif_orientation(img: DynamicImage, orientation: u32) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

fn try_load_embedded_raw_preview(source_path: &Path, target_res: u32) -> Option<DynamicImage> {
    let source = exif_processing::read_source_metadata(source_path).ok()?;
    let mmap = read_file_mapped(source_path).ok()?;
    for preview in source.previews {
        let Some(end) = preview.offset.checked_add(preview.length) else {
            continue;
        };
        let (Ok(start), Ok(end)) = (usize::try_from(preview.offset), usize::try_from(end)) else {
            continue;
        };
        let Some(bytes) = mmap.get(start..end).filter(|v| v.starts_with(b"\xff\xd8")) else {
            continue;
        };
        let Ok(image) = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg) else {
            continue;
        };
        if image.width().max(image.height()) >= (target_res as f32 * 0.95) as u32 {
            return Some(apply_exif_orientation(image, preview.orientation));
        }
    }
    None
}

#[cfg(test)]
mod embedded_preview_tests {
    #[test]
    fn large_thumbnail_requests_are_bounded_and_duplicates_do_not_use_slots() {
        let requests = (0..60_000)
            .flat_map(|i| [format!("{i}.nef"), format!("{i}.nef")])
            .collect();
        let unique = super::unique_thumbnail_requests(requests);
        assert_eq!(unique.len(), 500);
        assert_eq!(unique.first().unwrap(), "0.nef");
        assert_eq!(unique.last().unwrap(), "499.nef");
    }

    #[test]
    fn byte_preview_thumbnails_preserve_shape_and_constant_color() {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            1600,
            800,
            image::Rgb([80, 120, 180]),
        ));
        let bytes = super::encode_thumbnail(&image, 320).unwrap();
        let thumbnail = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(thumbnail.dimensions(), (320, 160));
        for (actual, expected) in thumbnail.get_pixel(0, 0).0.iter().zip([80_i16, 120, 180]) {
            assert!((*actual as i16 - expected).abs() <= 3);
        }
    }
}

pub fn generate_thumbnail_data(
    path_str: &str,
    gpu_context: Option<&GpuContext>,
    preloaded_image: Option<&DynamicImage>,
    app_handle: &AppHandle,
) -> anyhow::Result<DynamicImage> {
    let (source_path, sidecar_path) = parse_virtual_path(path_str);
    let source_path_str = source_path.to_string_lossy().to_string();
    let is_raw = is_raw_file(&source_path_str);

    let metadata: Option<ImageMetadata> = if is_cloud_placeholder(&sidecar_path) {
        enqueue_metadata(
            app_handle,
            path_str.to_string(),
            source_path.clone(),
            sidecar_path.clone(),
        );
        None
    } else {
        Some(crate::xmp::load_metadata(&source_path, &sidecar_path))
    };

    let saved = metadata
        .as_ref()
        .map_or(&Value::Null, |metadata| &metadata.adjustments);
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let mut adjustments = saved.clone();
    let applied_default = apply_raw_lens_default(&mut adjustments, &source_path, &settings);
    if adjustments["lensCorrectionMode"].as_str() == Some("auto")
        || matches!(
            adjustments["lensProfileSource"].as_str(),
            Some("embedded" | "off")
        )
    {
        let state = app_handle.state::<AppState>();
        let lens_db = state
            .lens_db
            .lock()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .clone();
        resolve_lens_params_in_adjustments(
            &mut adjustments,
            &metadata.as_ref().and_then(|m| m.exif.clone()),
            lens_db.as_deref(),
            &source_path,
        );
    }

    if applied_default && adjustments["lensDistortionParams"].is_null() {
        adjustments = saved.clone();
    }
    let always_decode_raw = settings.always_decode_raw_thumbnails.unwrap_or(false);

    if is_raw && adjustments.is_null() && preloaded_image.is_none() && !always_decode_raw {
        let target_res = settings.medium_thumbnail_resolution.unwrap_or(1280);
        if let Some(preview) = try_load_embedded_raw_preview(&source_path, target_res) {
            return Ok(preview);
        }
    }

    if let Some(context) = gpu_context
        && !adjustments.is_null()
    {
        let state = app_handle.state::<AppState>();
        let target_res = settings.medium_thumbnail_resolution.unwrap_or(1280);

        let composite_image = if let Some(img) = preloaded_image {
            crate::spot_removal::composite_spot_removals(img, &adjustments)
        } else {
            let mmap_guard;
            let vec_guard;

            let file_slice: &[u8] = match read_file_mapped(&source_path) {
                Ok(mmap) => {
                    mmap_guard = Some(mmap);
                    mmap_guard.as_ref().unwrap()
                }
                Err(e) => {
                    if preloaded_image.is_none() {
                        log::warn!("Fallback read for {}: {}", source_path_str, e);
                    }
                    let bytes = fs::read(&source_path).map_err(|io_err| {
                        anyhow::anyhow!("Fallback read failed for {}: {}", source_path_str, io_err)
                    })?;
                    vec_guard = Some(bytes);
                    vec_guard.as_ref().unwrap()
                }
            };

            let img = image_loader::load_and_composite(
                file_slice,
                &source_path_str,
                &adjustments,
                true,
                &settings,
                None,
            )?;

            img
        };

        let source = Arc::new(composite_image);
        let mut plan = crate::gpu_geometry::Plan::new(&source, &adjustments, target_res);
        plan.patches_applied = true;
        let (preview_w, preview_h) = plan.size;
        let mask_definitions: Vec<MaskDefinition> = adjustments
            .get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_default();
        let warped_image = mask_definitions
            .iter()
            .any(|mask| mask.requires_warped_image())
            .then(|| {
                crate::mask_generation::prepare_mask_source(source.as_ref(), &adjustments, is_raw)
            });
        let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions
            .iter()
            .filter_map(|def| {
                crate::mask_generation::generate_mask_bitmap(
                    def,
                    preview_w,
                    preview_h,
                    plan.scale,
                    (plan.offset.0 * plan.scale, plan.offset.1 * plan.scale),
                    warped_image.as_ref(),
                )
            })
            .collect();

        let tm_override = crate::image_processing::resolve_tonemapper_override(&settings, is_raw);
        let gpu_adjustments = get_all_adjustments_from_json(
            &adjustments,
            is_raw,
            crate::white_balance::as_shot_white_balance(&source_path_str),
            tm_override,
        );

        let mut hasher = DefaultHasher::new();
        path_str.hash(&mut hasher);
        adjustments.to_string().hash(&mut hasher);
        let unique_hash = hasher.finish();

        if let Ok(processed_image) = gpu_processing::process_and_get_dynamic_image(
            context,
            &state,
            source.as_ref(),
            unique_hash,
            gpu_processing::RenderRequest {
                source: Some(&plan),
                adjustments: gpu_adjustments,
                mask_bitmaps: &mask_bitmaps,
                roi: None,
            },
            "generate_thumbnail_data",
        ) {
            return Ok(processed_image);
        } else {
            return Err(anyhow::anyhow!("GPU thumbnail processing failed"));
        }
    }

    let mut final_image = if let Some(img) = preloaded_image {
        crate::spot_removal::composite_spot_removals(img, &adjustments)
    } else {
        match read_file_mapped(&source_path) {
            Ok(mmap) => image_loader::load_and_composite(
                &mmap,
                &source_path_str,
                &adjustments,
                true,
                &settings,
                None,
            )?,
            Err(e) => {
                log::warn!("Fallback read for {}: {}", source_path_str, e);
                let bytes = fs::read(&source_path)?;
                image_loader::load_and_composite(
                    &bytes,
                    &source_path_str,
                    &adjustments,
                    true,
                    &settings,
                    None,
                )?
            }
        }
    };

    if adjustments.is_null() {
        let tm_override = crate::image_processing::resolve_tonemapper_override(&settings, is_raw);
        let use_agx = tm_override == Some(1);

        if use_agx {
            if !is_raw {
                final_image = crate::image_processing::apply_srgb_to_linear(final_image);
            }
            crate::image_processing::apply_cpu_agx_tonemap(&mut final_image);
        } else if is_raw {
            apply_cpu_default_raw_processing(&mut final_image);
        }
    }

    let fallback_orientation_steps = adjustments["orientationSteps"].as_u64().unwrap_or(0) as u8;
    Ok(apply_coarse_rotation(Cow::Owned(final_image), fallback_orientation_steps).into_owned())
}

fn encode_thumbnail(image: &DynamicImage, target_width: u32) -> Result<Vec<u8>> {
    // Camera JPEG previews already contain display pixels. Avoid allocating a
    // full-resolution RGB32F copy for each of the two cached thumbnail sizes.
    let thumbnail = if matches!(
        image,
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_)
    ) && image.width().max(image.height()) > target_width
    {
        image.thumbnail(target_width, target_width)
    } else {
        crate::image_processing::downscale_f32_image(image, target_width, target_width)
    };
    let mut buf = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut buf, 75);
    encoder.encode_image(&thumbnail.to_rgb8())?;
    Ok(buf.into_inner())
}

fn write_cached_thumbnail(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    std::io::Write::write_all(temporary.as_file_mut(), bytes)?;
    temporary.persist(path)?;
    Ok(())
}

pub(crate) fn generate_single_thumbnail_and_cache(
    path_str: &str,
    thumb_cache_dir: &Path,
    gpu_context: Option<&GpuContext>,
    preloaded_image: Option<&DynamicImage>,
    force_regenerate: bool,
    app_handle: &AppHandle,
    settings: &AppSettings,
) -> Option<(String, String, u8, bool)> {
    let (source_path, sidecar_path) = parse_virtual_path(path_str);

    let (rating, is_edited, adjustments_bytes) = if is_cloud_placeholder(&sidecar_path) {
        enqueue_metadata(
            app_handle,
            path_str.to_string(),
            source_path.clone(),
            sidecar_path.clone(),
        );
        (0, false, Vec::new())
    } else {
        let mut meta = crate::xmp::load_metadata(&source_path, &sidecar_path);
        let is_raw = crate::formats::is_raw_file(path_str);
        let tm = crate::image_processing::resolve_tonemapper_override(settings, is_raw);
        let is_edited = crate::image_processing::is_image_edited(&meta.adjustments, is_raw, tm);
        apply_raw_lens_default(&mut meta.adjustments, &source_path, settings);
        (
            meta.rating,
            is_edited,
            if meta.adjustments.is_null() {
                Vec::new()
            } else {
                serde_json::to_vec(&meta.adjustments).unwrap_or_default()
            },
        )
    };

    let cache_hash = compute_thumbnail_cache_hash(path_str, &adjustments_bytes)?;

    let small_path = thumb_cache_dir.join(format!("{}_small.jpg", cache_hash));
    let medium_path = thumb_cache_dir.join(format!("{}_medium.jpg", cache_hash));

    if !force_regenerate && small_path.exists() && medium_path.exists() {
        return Some((
            small_path.to_string_lossy().into_owned(),
            medium_path.to_string_lossy().into_owned(),
            rating,
            is_edited,
        ));
    }

    if is_cloud_placeholder(&source_path) {
        return None;
    }

    let target_width_small = settings.small_thumbnail_resolution.unwrap_or(480);
    let target_width_medium = settings.medium_thumbnail_resolution.unwrap_or(1280);

    if let Ok(thumb_image) =
        generate_thumbnail_data(path_str, gpu_context, preloaded_image, app_handle)
        && let (Ok(small_data), Ok(medium_data)) = (
            encode_thumbnail(&thumb_image, target_width_small),
            encode_thumbnail(&thumb_image, target_width_medium),
        )
        && write_cached_thumbnail(&small_path, &small_data).is_ok()
        && write_cached_thumbnail(&medium_path, &medium_data).is_ok()
    {
        return Some((
            small_path.to_string_lossy().into_owned(),
            medium_path.to_string_lossy().into_owned(),
            rating,
            is_edited,
        ));
    }
    None
}

fn prefetch_source_file(path_str: &str) {
    let (source_path, _) = parse_virtual_path(path_str);
    if let Ok(mut file) = std::fs::File::open(&source_path) {
        let _ = std::io::copy(&mut file, &mut std::io::sink());
    }
}

pub fn start_thumbnail_workers(app_handle: tauri::AppHandle) {
    let state = app_handle.state::<crate::AppState>();
    let manager = state.thumbnail_manager.clone();
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let thread_count = settings.thumbnail_worker_threads.unwrap_or(4).clamp(1, 16);

    for _ in 0..thread_count {
        let app_clone = app_handle.clone();
        let manager_clone = manager.clone();

        std::thread::spawn(move || {
            loop {
                let path_to_process: String = {
                    let mut queue = manager_clone.queue.lock().unwrap();
                    while queue.is_empty() {
                        queue = manager_clone.cvar.wait(queue).unwrap();
                    }
                    let path = queue.pop_back().unwrap();

                    let mut processing = manager_clone.processing_now.lock().unwrap();
                    if processing.contains(&path) {
                        let state = app_clone.state::<crate::AppState>();
                        increment_thumbnail_progress(&state, &app_clone);
                        continue;
                    }
                    processing.insert(path.clone());
                    path
                };

                let state = app_clone.state::<crate::AppState>();
                let gpu_context =
                    crate::gpu_processing::get_or_init_gpu_context(&state, &app_clone).ok();

                let current_settings = load_settings(app_clone.clone()).unwrap_or_default();

                if let Ok(cache_dir) = get_thumb_cache_dir(&app_clone) {
                    if manager_clone.rotational_disk.load(Ordering::Relaxed) {
                        let _io_permit = manager_clone.io_gate.lock().unwrap();
                        prefetch_source_file(&path_to_process);
                    }

                    let result = generate_single_thumbnail_and_cache(
                        &path_to_process,
                        &cache_dir,
                        gpu_context.as_ref(),
                        None,
                        false,
                        &app_clone,
                        &current_settings,
                    );

                    if let Some((small_path, medium_path, rating, is_edited)) = result {
                        emit_thumbnail_generated(
                            &app_clone,
                            &path_to_process,
                            &small_path,
                            &medium_path,
                            rating,
                            is_edited,
                        );
                    }
                    increment_thumbnail_progress(&state, &app_clone);
                }
                manager_clone
                    .processing_now
                    .lock()
                    .unwrap()
                    .remove(&path_to_process);
            }
        });
    }
}

fn unique_thumbnail_requests(paths: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .take(500)
        .collect()
}

#[tauri::command]
pub fn update_thumbnail_queue(
    paths: Vec<String>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let state = app_handle.state::<crate::AppState>();

    let mut queue = state.thumbnail_manager.queue.lock().unwrap();

    if paths.is_empty() {
        queue.clear();
        let mut tracker = state.thumbnail_progress.lock().unwrap();
        tracker.total = 0;
        tracker.completed = 0;
        drop(tracker);

        let _ = app_handle.emit(
            "thumbnail-progress",
            serde_json::json!({ "current": 0, "total": 0 }),
        );
        state.thumbnail_manager.cvar.notify_all();
        return Ok(());
    }

    let mut unique_paths = unique_thumbnail_requests(paths);
    let seen: HashSet<_> = unique_paths.iter().collect();

    queue.retain(|p| !seen.contains(p));

    while queue.len() + unique_paths.len() > 500 {
        queue.pop_front();
    }

    if state
        .thumbnail_manager
        .rotational_disk
        .load(Ordering::Relaxed)
    {
        unique_paths.sort();
        for path in unique_paths.into_iter().rev() {
            queue.push_back(path);
        }
    } else {
        for path in unique_paths {
            queue.push_back(path);
        }
    }

    let queue_len = queue.len();
    drop(queue);

    let mut tracker = state.thumbnail_progress.lock().unwrap();
    tracker.total = tracker.completed + queue_len;

    let current = tracker.completed;
    let total = tracker.total;
    drop(tracker);

    let _ = app_handle.emit(
        "thumbnail-progress",
        serde_json::json!({ "current": current, "total": total }),
    );

    state.thumbnail_manager.cvar.notify_all();
    Ok(())
}

pub fn add_to_thumbnail_queue(state: &AppState, count: usize, app_handle: &AppHandle) {
    let mut tracker = state.thumbnail_progress.lock().unwrap();
    tracker.total += count;
    let current = tracker.completed;
    let total = tracker.total;
    drop(tracker);

    let _ = app_handle.emit(
        "thumbnail-progress",
        serde_json::json!({ "current": current, "total": total }),
    );
}

pub fn increment_thumbnail_progress(state: &AppState, app_handle: &AppHandle) {
    let mut tracker = state.thumbnail_progress.lock().unwrap();
    tracker.completed += 1;
    let current = tracker.completed;
    let total = tracker.total;

    if current >= total {
        tracker.total = 0;
        tracker.completed = 0;
        drop(tracker);

        let _ = app_handle.emit(
            "thumbnail-progress",
            serde_json::json!({ "current": 0, "total": 0 }),
        );
        let _ = app_handle.emit("thumbnail-generation-complete", true);
    } else {
        drop(tracker);
        let _ = app_handle.emit(
            "thumbnail-progress",
            serde_json::json!({ "current": current, "total": total }),
        );
    }
}

fn emit_thumbnail_generated(
    app_handle: &AppHandle,
    path: &str,
    small_thumbnail_path: &str,
    medium_thumbnail_path: &str,
    rating: u8,
    is_edited: bool,
) {
    let _ = app_handle.emit(
        "thumbnail-generated",
        serde_json::json!({
            "path": path,
            "thumbnailPath": small_thumbnail_path,
            "previewPath": medium_thumbnail_path,
            "rating": rating,
            "is_edited": is_edited
        }),
    );
}

pub(crate) fn apply_raw_lens_default(
    adjustments: &mut Value,
    source: &Path,
    settings: &AppSettings,
) -> bool {
    if !settings.raw_preprocessing_auto_lens
        || !is_raw_file(source)
        || [
            "lensCorrectionMode",
            "lensProfileSource",
            "lensMaker",
            "lensModel",
            "lensDistortionParams",
        ]
        .iter()
        .any(|key| adjustments.get(*key).is_some_and(|value| !value.is_null()))
    {
        return false;
    }
    if adjustments.is_null() {
        *adjustments = serde_json::json!({});
    }
    let Some(map) = adjustments.as_object_mut() else {
        return false;
    };
    map.insert("lensCorrectionMode".into(), Value::from("auto"));
    for key in [
        "lensDistortionEnabled",
        "lensTcaEnabled",
        "lensVignetteEnabled",
    ] {
        map.entry(key).or_insert(Value::Bool(true));
    }
    true
}

#[cfg(test)]
#[test]
fn raw_lens_default_excludes_tiff_and_preserves_saved_choices() {
    let mut settings = AppSettings::default();
    let mut adjustments = Value::Null;
    let raw = Path::new("photo.NEF");
    assert!(!apply_raw_lens_default(&mut adjustments, raw, &settings));
    settings.raw_preprocessing_auto_lens = true;
    for filename in ["photo.tif", "photo.TIFF", "photo.jpg"] {
        assert!(!apply_raw_lens_default(
            &mut adjustments,
            Path::new(filename),
            &settings
        ));
        assert!(adjustments.is_null());
    }
    assert!(apply_raw_lens_default(&mut adjustments, raw, &settings));
    assert_eq!(adjustments["lensCorrectionMode"], "auto");
    assert_eq!(adjustments["lensVignetteEnabled"], true);
    let mut manual = serde_json::json!({"lensCorrectionMode":"manual", "lensProfileSource":"off"});
    let saved = manual.clone();
    assert!(!apply_raw_lens_default(&mut manual, raw, &settings));
    assert_eq!(manual, saved);
}

pub fn resolve_lens_params_in_adjustments(
    adjustments: &mut Value,
    exif_data: &Option<HashMap<String, String>>,
    lens_db: Option<&crate::lens_correction::LensDatabase>,
    source_path: &Path,
) {
    let Some(map) = adjustments.as_object_mut() else {
        return;
    };
    let auto = map.get("lensCorrectionMode").and_then(Value::as_str) == Some("auto");
    let source = map
        .get("lensProfileSource")
        .and_then(Value::as_str)
        .unwrap_or("lensfun")
        .to_owned();
    if !auto && source == "off" {
        for key in ["lensMaker", "lensModel", "lensDistortionParams"] {
            map.remove(key);
        }
        return;
    }
    if auto || source == "embedded" {
        let metadata = crate::exif_processing::read_source_metadata(source_path).ok();
        let exif = metadata
            .as_ref()
            .map(|data| &data.fields)
            .or(exif_data.as_ref());
        let embedded = metadata
            .as_ref()
            .and_then(|data| data.lens_profile.as_ref());
        let profile = exif.and_then(|exif| {
            crate::lens_correction::select_auto_profile(
                if auto { lens_db } else { None },
                exif,
                embedded,
            )
        });
        if let Some(profile) = profile {
            map.insert("lensMaker".into(), serde_json::json!(profile.maker));
            map.insert("lensModel".into(), serde_json::json!(profile.model));
            map.insert(
                "lensDistortionParams".into(),
                serde_json::to_value(profile.params).unwrap(),
            );
        } else {
            for key in ["lensMaker", "lensModel", "lensDistortionParams"] {
                map.remove(key);
            }
            if !auto {
                map.insert("lensProfileSource".into(), serde_json::json!("off"));
            }
        }
        return;
    }
    let params = lens_db.zip(exif_data.as_ref()).and_then(|(db, exif)| {
        crate::lens_correction::resolve_lens_params(
            db,
            map.get("lensMaker")?.as_str()?,
            map.get("lensModel")?.as_str()?,
            crate::lens_correction::metadata_number(exif, "FocalLength")?,
            crate::lens_correction::metadata_number(exif, "FNumber"),
            crate::lens_correction::metadata_number(exif, "SubjectDistance"),
        )
    });
    if let Some(params) = params {
        map.insert(
            "lensDistortionParams".into(),
            serde_json::to_value(params).unwrap(),
        );
    } else {
        map.remove("lensDistortionParams");
    }
}

#[tauri::command]
pub fn get_supported_file_types() -> Result<serde_json::Value, String> {
    let raw_extensions: Vec<&str> = crate::formats::RAW_EXTENSIONS
        .iter()
        .map(|(ext, _)| *ext)
        .collect();
    let non_raw_extensions: Vec<&str> = crate::formats::NON_RAW_EXTENSIONS.to_vec();
    let video_extensions: Vec<&str> = crate::formats::VIDEO_EXTENSIONS.to_vec();

    Ok(serde_json::json!({
        "raw": raw_extensions,
        "nonRaw": non_raw_extensions,
        "video": video_extensions
    }))
}

#[tauri::command]
pub fn duplicate_file(
    path: String,
    target_album_id: Option<String>,
    app_handle: AppHandle,
) -> Result<String, String> {
    let (source_path, source_sidecar_path) = parse_virtual_path(&path);
    if !source_path.is_file() {
        return Err("Source path is not a file.".to_string());
    }

    let parent = source_path
        .parent()
        .ok_or("Could not get parent directory")?;
    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("Could not get file stem")?;
    let extension = source_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let mut counter = 1;
    let mut dest_path;
    loop {
        let new_stem = if counter == 1 {
            format!("{}_copy", stem)
        } else {
            format!("{}_copy_{}", stem, counter - 1)
        };
        dest_path = parent.join(format!("{}.{}", new_stem, extension));
        if !dest_path.exists() {
            break;
        }
        counter += 1;
    }

    fs::copy(&source_path, &dest_path).map_err(|e| e.to_string())?;

    if source_sidecar_path.exists()
        && let Some(dest_str) = dest_path.to_str()
    {
        let (_, dest_sidecar_path) = parse_virtual_path(dest_str);
        fs::copy(&source_sidecar_path, &dest_sidecar_path).map_err(|e| e.to_string())?;
    }

    crate::xmp::copy_sidecar(
        &source_path,
        &source_sidecar_path,
        &dest_path,
        &crate::exif_processing::get_primary_sidecar_path(&dest_path),
    )?;
    let mut source_rrexif_name = source_path.file_name().unwrap().to_os_string();
    source_rrexif_name.push(".rrexif");
    let source_rrexif = source_path.with_file_name(source_rrexif_name);

    if source_rrexif.exists() {
        let mut dest_rrexif_name = dest_path.file_name().unwrap().to_os_string();
        dest_rrexif_name.push(".rrexif");
        let dest_rrexif = dest_path.with_file_name(dest_rrexif_name);
        let _ = fs::copy(&source_rrexif, &dest_rrexif);
    }

    let dest_path_str = dest_path.to_string_lossy().into_owned();

    if let Some(album_id) = target_album_id {
        let _ = add_to_album(album_id, vec![dest_path_str.clone()], app_handle);
    }

    Ok(dest_path_str)
}

fn renamed_associated_file(file: &Path, source: &Path, destination: &Path) -> PathBuf {
    if let Some(ext) = file.extension()
        && ext.eq_ignore_ascii_case("xmp")
        && file.with_extension("") == source.with_extension("")
    {
        return destination.with_extension(ext);
    }
    let original = source.file_name().unwrap_or_default().to_string_lossy();
    let renamed = destination
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let companion = file.file_name().unwrap_or_default().to_string_lossy();
    destination.with_file_name(companion.replacen(&*original, &renamed, 1))
}

fn find_all_associated_files(source_image_path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut associated_files = vec![source_image_path.to_path_buf()];
    if let Some(path) = crate::xmp::primary_path(source_image_path) {
        associated_files.push(path);
    }

    let mut rrexif_name = source_image_path
        .file_name()
        .unwrap_or_default()
        .to_os_string();
    rrexif_name.push(".rrexif");
    let rrexif_path = source_image_path.with_file_name(rrexif_name);

    if rrexif_path.exists() {
        associated_files.push(rrexif_path);
    }

    let parent_dir = source_image_path
        .parent()
        .ok_or("Could not determine parent directory")?;
    let source_filename = source_image_path
        .file_name()
        .ok_or("Could not get source filename")?
        .to_string_lossy();

    let primary_sidecar_name = format!("{}.rrdata", source_filename);
    let virtual_copy_prefix = format!("{}.", source_filename);

    if let Ok(entries) = fs::read_dir(parent_dir) {
        for entry in entries.filter_map(Result::ok) {
            let entry_path = entry.path();
            if !entry_path.is_file() {
                continue;
            }

            let entry_os_filename = entry.file_name();
            let entry_filename = entry_os_filename.to_string_lossy();

            if entry_filename == primary_sidecar_name
                || (entry_filename.starts_with(&virtual_copy_prefix)
                    && (entry_filename.ends_with(".rrdata")
                        || entry_path
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("xmp"))))
            {
                if !associated_files.contains(&entry_path) {
                    associated_files.push(entry_path);
                }
            }
        }
    }

    Ok(associated_files)
}

#[tauri::command]
pub fn copy_files(source_paths: Vec<String>, destination_folder: String) -> Result<(), String> {
    let dest_path = Path::new(&destination_folder);
    if !dest_path.is_dir() {
        return Err(format!(
            "Destination is not a folder: {}",
            destination_folder
        ));
    }

    let unique_source_images: HashSet<PathBuf> = source_paths
        .iter()
        .map(|p| parse_virtual_path(p).0)
        .collect();

    let mut operations_to_perform = Vec::new();

    for source_image_path in &unique_source_images {
        let all_files_to_copy = find_all_associated_files(source_image_path)?;

        let source_parent = source_image_path
            .parent()
            .ok_or("Could not get parent directory")?;

        if source_parent == dest_path {
            let stem = source_image_path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or("Could not get file stem")?;
            let extension = source_image_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            let mut counter = 1;
            let new_base_path = loop {
                let new_stem = format!("{}_copy_{}", stem, counter);
                let temp_path = source_parent.join(format!("{}.{}", new_stem, extension));
                if !temp_path.exists() {
                    break temp_path;
                }
                counter += 1;
            };
            for original_file in all_files_to_copy {
                let destination =
                    renamed_associated_file(&original_file, source_image_path, &new_base_path);
                operations_to_perform.push((original_file, destination));
            }
        } else {
            for file_to_copy in all_files_to_copy {
                if let Some(file_name) = file_to_copy.file_name() {
                    let dest_file_path = dest_path.join(file_name);

                    if dest_file_path.exists() {
                        return Err(format!(
                            "Copy aborted: File already exists at destination: {}",
                            dest_file_path.display()
                        ));
                    }

                    operations_to_perform.push((file_to_copy, dest_file_path));
                }
            }
        }
    }

    for (source, dest) in operations_to_perform {
        fs::copy(&source, &dest)
            .map_err(|e| format!("Copy failed for {}: {}", source.display(), e))?;
    }

    Ok(())
}

#[tauri::command]
pub fn move_files(
    source_paths: Vec<String>,
    destination_folder: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    let dest_path = Path::new(&destination_folder);
    if !dest_path.is_dir() {
        return Err(format!(
            "Destination is not a folder: {}",
            destination_folder
        ));
    }

    let unique_source_images: HashSet<PathBuf> = source_paths
        .iter()
        .map(|p| parse_virtual_path(p).0)
        .collect();

    let mut operations_to_perform = Vec::new();
    let mut renames = HashMap::new();

    for source_image_path in &unique_source_images {
        let source_parent = source_image_path
            .parent()
            .ok_or("Could not get parent directory")?;

        if source_parent == dest_path {
            return Err("Cannot move files into the same folder they are already in.".to_string());
        }

        let all_files_to_move = find_all_associated_files(source_image_path)?;

        for file_to_move in &all_files_to_move {
            if let Some(file_name) = file_to_move.file_name() {
                let dest_file_path = dest_path.join(file_name);

                if dest_file_path.exists() {
                    return Err(format!(
                        "Move aborted: File already exists at destination: {}",
                        dest_file_path.display()
                    ));
                }

                operations_to_perform.push((file_to_move.clone(), dest_file_path));
            }
        }

        let dest_image_path = dest_path.join(source_image_path.file_name().unwrap());
        renames.insert(
            source_image_path.to_string_lossy().into_owned(),
            dest_image_path.to_string_lossy().into_owned(),
        );
    }

    for (source, dest) in operations_to_perform {
        if fs::rename(&source, &dest).is_err() {
            fs::copy(&source, &dest)
                .map_err(|e| format!("Move failed during copy for {}: {}", source.display(), e))?;

            if let Err(e) = fs::remove_file(&source) {
                log::warn!(
                    "Moved file successfully, but failed to delete original {}: {}",
                    source.display(),
                    e
                );
            }
        }
    }

    sync_album_path_changes(&app_handle, Some(&renames), None, None);

    Ok(())
}

#[tauri::command]
pub fn save_metadata_and_update_thumbnail(
    path: String,
    adjustments: Value,
    app_handle: AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let (source_path, sidecar_path) = parse_virtual_path(&path);

    let metadata = crate::exif_processing::load_sidecar_with_exif(&sidecar_path, &source_path);

    let mut final_adjustments = adjustments;
    {
        let lens_db_guard = state.lens_db.lock().unwrap();
        resolve_lens_params_in_adjustments(
            &mut final_adjustments,
            &metadata.exif,
            lens_db_guard.as_deref(),
            &source_path,
        );
    }

    crate::xmp::save_edits(&source_path, &sidecar_path, final_adjustments)?;

    let loaded_image_lock = state.original_image.lock().unwrap();
    let preloaded_image_option = if let Some(loaded_image) = loaded_image_lock.as_ref() {
        if loaded_image.path == path {
            Some(loaded_image.image.clone())
        } else {
            None
        }
    } else {
        None
    };
    drop(loaded_image_lock);

    let gpu_context = gpu_processing::get_or_init_gpu_context(&state, &app_handle).ok();
    let app_handle_clone = app_handle.clone();
    let path_clone = path.clone();

    add_to_thumbnail_queue(&state, 1, &app_handle);

    thread::spawn(move || {
        let state = app_handle_clone.state::<AppState>();
        let settings = load_settings(app_handle_clone.clone()).unwrap_or_default();

        let thumb_cache_dir = match resolve_thumbnail_cache_dir(&app_handle_clone) {
            Ok(dir) => dir,
            Err(e) => {
                log::warn!(
                    "Unable to initialize thumbnail cache directory for '{}': {}",
                    path_clone,
                    e
                );
                emit_thumbnail_cache_setup_error(&app_handle_clone, &path_clone, &e);
                increment_thumbnail_progress(&state, &app_handle_clone);
                return;
            }
        };

        let result = generate_single_thumbnail_and_cache(
            &path_clone,
            &thumb_cache_dir,
            gpu_context.as_ref(),
            preloaded_image_option.as_deref(),
            true,
            &app_handle_clone,
            &settings,
        );

        if let Some((small_path, medium_path, rating, is_edited)) = result {
            emit_thumbnail_generated(
                &app_handle_clone,
                &path_clone,
                &small_path,
                &medium_path,
                rating,
                is_edited,
            );
        }

        increment_thumbnail_progress(&state, &app_handle_clone);
    });

    Ok(())
}

#[tauri::command]
pub async fn apply_adjustments_to_paths(
    paths: Vec<String>,
    adjustments: Value,
    app_handle: AppHandle,
) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    add_to_thumbnail_queue(&state, paths.len(), &app_handle);

    tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(app_handle.clone()).unwrap_or_default();

        let lens_db = app_handle
            .state::<AppState>()
            .lens_db
            .lock()
            .unwrap()
            .clone();

        paths.par_iter().for_each(|path| {
            let (source_path, sidecar_path) = parse_virtual_path(path);

            let mut existing_metadata =
                crate::exif_processing::load_sidecar_with_exif(&sidecar_path, &source_path);

            let mut new_adjustments = existing_metadata.adjustments;
            if new_adjustments.is_null() {
                new_adjustments = serde_json::json!({});
            }

            if let (Some(new_map), Some(pasted_map)) =
                (new_adjustments.as_object_mut(), adjustments.as_object())
            {
                for (k, v) in pasted_map {
                    new_map.insert(k.clone(), v.clone());
                }
            }

            resolve_lens_params_in_adjustments(
                &mut new_adjustments,
                &existing_metadata.exif,
                lens_db.as_deref(),
                &source_path,
            );

            existing_metadata.adjustments = new_adjustments;

            if let Err(error) =
                crate::xmp::save_edits(&source_path, &sidecar_path, existing_metadata.adjustments)
            {
                log::error!("Could not save edits for {path}: {error}");
            }
        });

        let state = app_handle.state::<AppState>();
        let thumb_cache_dir = match resolve_thumbnail_cache_dir(&app_handle) {
            Ok(dir) => dir,
            Err(e) => {
                log::warn!("Unable to initialize thumbnail cache directory: {}", e);
                for path in &paths {
                    emit_thumbnail_cache_setup_error(&app_handle, path, &e);
                }
                for _ in 0..paths.len() {
                    increment_thumbnail_progress(&state, &app_handle);
                }
                return;
            }
        };

        let gpu_context = gpu_processing::get_or_init_gpu_context(&state, &app_handle).ok();

        paths.par_iter().for_each(|path_str| {
            let result = generate_single_thumbnail_and_cache(
                path_str,
                &thumb_cache_dir,
                gpu_context.as_ref(),
                None,
                true,
                &app_handle,
                &settings,
            );

            if let Some((small_path, medium_path, rating, is_edited)) = result {
                emit_thumbnail_generated(
                    &app_handle,
                    path_str,
                    &small_path,
                    &medium_path,
                    rating,
                    is_edited,
                );
            }

            increment_thumbnail_progress(&state, &app_handle);
        });
    });

    Ok(())
}

#[tauri::command]
pub async fn reset_adjustments_for_paths(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    add_to_thumbnail_queue(&state, paths.len(), &app_handle);

    tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(app_handle.clone()).unwrap_or_default();

        paths.par_iter().for_each(|path| {
            let (source_path, sidecar_path) = parse_virtual_path(path);

            let mut existing_metadata =
                crate::exif_processing::load_sidecar_with_exif(&sidecar_path, &source_path);

            existing_metadata.adjustments = serde_json::json!({});

            if let Err(error) =
                crate::xmp::save_edits(&source_path, &sidecar_path, existing_metadata.adjustments)
            {
                log::error!("Could not save edits for {path}: {error}");
            }
        });

        let state = app_handle.state::<AppState>();
        let thumb_cache_dir = match resolve_thumbnail_cache_dir(&app_handle) {
            Ok(dir) => dir,
            Err(e) => {
                log::warn!("Unable to initialize thumbnail cache directory: {}", e);
                for path in &paths {
                    emit_thumbnail_cache_setup_error(&app_handle, path, &e);
                }
                for _ in 0..paths.len() {
                    increment_thumbnail_progress(&state, &app_handle);
                }
                return;
            }
        };

        let gpu_context = gpu_processing::get_or_init_gpu_context(&state, &app_handle).ok();

        paths.par_iter().for_each(|path_str| {
            let result = generate_single_thumbnail_and_cache(
                path_str,
                &thumb_cache_dir,
                gpu_context.as_ref(),
                None,
                true,
                &app_handle,
                &settings,
            );

            if let Some((small_path, medium_path, rating, is_edited)) = result {
                emit_thumbnail_generated(
                    &app_handle,
                    path_str,
                    &small_path,
                    &medium_path,
                    rating,
                    is_edited,
                );
            }

            increment_thumbnail_progress(&state, &app_handle);
        });
    });

    Ok(())
}

#[tauri::command]
pub async fn apply_auto_lens_correction_to_paths(
    paths: Vec<String>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let state = app_handle.state::<crate::AppState>();
    add_to_thumbnail_queue(&state, paths.len(), &app_handle);

    tauri::async_runtime::spawn_blocking(move || {
        let settings = crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default();

        let state = app_handle.state::<crate::AppState>();
        let thumb_cache_dir = match resolve_thumbnail_cache_dir(&app_handle) {
            Ok(dir) => dir,
            Err(e) => {
                log::warn!("Unable to initialize thumbnail cache directory: {}", e);
                for _ in 0..paths.len() {
                    increment_thumbnail_progress(&state, &app_handle);
                }
                return;
            }
        };

        let gpu_context = crate::gpu_processing::get_or_init_gpu_context(&state, &app_handle).ok();
        let lens_db = state.lens_db.lock().unwrap().clone();

        paths.par_iter().for_each(|path| {
            let (source_path, sidecar_path) = parse_virtual_path(path);
            let mut existing_metadata =
                crate::exif_processing::load_sidecar_with_exif(&sidecar_path, &source_path);

            if existing_metadata.adjustments.is_null() {
                existing_metadata.adjustments = serde_json::json!({});
            }

            if let Some(obj) = existing_metadata.adjustments.as_object_mut() {
                obj.insert("lensCorrectionMode".to_string(), serde_json::json!("auto"));
                obj.insert("lensDistortionEnabled".to_string(), serde_json::json!(true));
                obj.insert("lensTcaEnabled".to_string(), serde_json::json!(true));
                obj.insert("lensVignetteEnabled".to_string(), serde_json::json!(true));
            }

            resolve_lens_params_in_adjustments(
                &mut existing_metadata.adjustments,
                &existing_metadata.exif,
                lens_db.as_deref(),
                &source_path,
            );

            if let Err(error) =
                crate::xmp::save_edits(&source_path, &sidecar_path, existing_metadata.adjustments)
            {
                log::error!("Could not save edits for {path}: {error}");
            }

            let result = generate_single_thumbnail_and_cache(
                path,
                &thumb_cache_dir,
                gpu_context.as_ref(),
                None,
                true,
                &app_handle,
                &settings,
            );

            if let Some((small_path, medium_path, rating, is_edited)) = result {
                emit_thumbnail_generated(
                    &app_handle,
                    path,
                    &small_path,
                    &medium_path,
                    rating,
                    is_edited,
                );
            }

            increment_thumbnail_progress(&state, &app_handle);
        });
    });

    Ok(())
}

#[tauri::command]
pub async fn apply_auto_adjustments_to_paths(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    add_to_thumbnail_queue(&state, paths.len(), &app_handle);

    tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(app_handle.clone()).unwrap_or_default();

        let state = app_handle.state::<AppState>();
        let thumb_cache_dir = match resolve_thumbnail_cache_dir(&app_handle) {
            Ok(dir) => dir,
            Err(e) => {
                log::warn!("Unable to initialize thumbnail cache directory: {}", e);
                for path in &paths {
                    emit_thumbnail_cache_setup_error(&app_handle, path, &e);
                }
                for _ in 0..paths.len() {
                    increment_thumbnail_progress(&state, &app_handle);
                }
                return Err(e);
            }
        };

        let gpu_context = gpu_processing::get_or_init_gpu_context(&state, &app_handle).ok();

        let failures = std::sync::atomic::AtomicUsize::new(0);
        let mut memory = sysinfo::System::new();
        memory.refresh_memory();
        let workers = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min((memory.available_memory() / (1024 * 1024 * 1024)) as usize)
            .clamp(1, 4);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        pool.install(|| {
            paths.par_iter().for_each(|path| {
                let loaded_image: Option<DynamicImage> = (|| -> Result<DynamicImage, String> {
                    let (source_path, sidecar_path) = parse_virtual_path(path);
                    let source_path_str = source_path.to_string_lossy().to_string();

                    let file_bytes = fs::read(&source_path).map_err(|e| e.to_string())?;
                    let image = image_loader::load_base_image_from_bytes(
                        &file_bytes,
                        &source_path_str,
                        true,
                        &settings,
                        None,
                    )
                    .map_err(|e| e.to_string())?;

                    let auto_results = perform_auto_analysis(&image);
                    let auto_adjustments_json = auto_results_to_json(&auto_results);

                    let mut existing_metadata = crate::exif_processing::load_sidecar(&sidecar_path);

                    if existing_metadata.adjustments.is_null() {
                        existing_metadata.adjustments = serde_json::json!({});
                    }

                    if let (Some(existing_map), Some(auto_map)) = (
                        existing_metadata.adjustments.as_object_mut(),
                        auto_adjustments_json.as_object(),
                    ) {
                        for (k, v) in auto_map {
                            if k == "sectionVisibility" {
                                if let Some(existing_vis_val) = existing_map.get_mut(k) {
                                    if let (Some(existing_vis), Some(auto_vis)) =
                                        (existing_vis_val.as_object_mut(), v.as_object())
                                    {
                                        for (vis_k, vis_v) in auto_vis {
                                            existing_vis.insert(vis_k.clone(), vis_v.clone());
                                        }
                                    }
                                } else {
                                    existing_map.insert(k.clone(), v.clone());
                                }
                            } else {
                                existing_map.insert(k.clone(), v.clone());
                            }
                        }
                    }

                    crate::xmp::save_edits(
                        &source_path,
                        &sidecar_path,
                        existing_metadata.adjustments,
                    )?;
                    Ok(image)
                })()
                .map_err(|e| {
                    failures.fetch_add(1, Ordering::Relaxed);
                    log::error!("Failed to apply auto adjustments to {}: {}", path, e);
                })
                .ok();

                let result = generate_single_thumbnail_and_cache(
                    path,
                    &thumb_cache_dir,
                    gpu_context.as_ref(),
                    loaded_image.as_ref(),
                    true,
                    &app_handle,
                    &settings,
                );

                if let Some((small_path, medium_path, rating, is_edited)) = result {
                    emit_thumbnail_generated(
                        &app_handle,
                        path,
                        &small_path,
                        &medium_path,
                        rating,
                        is_edited,
                    );
                }

                increment_thumbnail_progress(&state, &app_handle);
            })
        });
        let failed = failures.load(Ordering::Relaxed);
        if failed > 0 {
            Err(format!("{failed} images could not be auto-adjusted"))
        } else {
            Ok(())
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(())
}

#[tauri::command]
pub fn set_color_label_for_paths(paths: Vec<String>, color: Option<String>) -> Result<(), String> {
    paths.par_iter().try_for_each(|path| {
        let (source, edits) = parse_virtual_path(path);
        crate::xmp::update_metadata(&source, &edits, |metadata| {
            let tags = metadata.tags.get_or_insert_default();
            tags.retain(|tag| !tag.starts_with(COLOR_TAG_PREFIX));
            if let Some(color) = color.as_ref().filter(|value| !value.is_empty()) {
                tags.push(format!("{COLOR_TAG_PREFIX}{color}"));
            }
        })
    })
}

#[tauri::command]
pub fn set_rating_for_paths(paths: Vec<String>, rating: u8) -> Result<(), String> {
    paths.par_iter().try_for_each(|path| {
        let (source, edits) = parse_virtual_path(path);
        crate::xmp::update_metadata(&source, &edits, |metadata| metadata.rating = rating.min(5))
    })
}

#[tauri::command]
pub fn load_metadata(
    path: String,
    for_editor: Option<bool>,
    app_handle: AppHandle,
) -> Result<ImageMetadata, String> {
    let (source, edits) = parse_virtual_path(&path);
    let mut metadata = crate::exif_processing::load_sidecar_with_exif(&edits, &source);
    if for_editor.unwrap_or(false) {
        let settings = load_settings(app_handle.clone()).unwrap_or_default();
        apply_raw_lens_default(&mut metadata.adjustments, &source, &settings);
        if metadata.adjustments["lensCorrectionMode"].as_str() == Some("auto") {
            let state = app_handle.state::<AppState>();
            let lens_db = state.lens_db.lock().map_err(|e| e.to_string())?.clone();
            resolve_lens_params_in_adjustments(
                &mut metadata.adjustments,
                &metadata.exif,
                lens_db.as_deref(),
                &source,
            );
        }
    }
    Ok(metadata)
}

#[tauri::command]
pub async fn delete_edit_sidecars(folders: Vec<PathBuf>, confirmed: bool) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || remove_edit_sidecars(folders, confirmed))
        .await
        .map_err(|error| error.to_string())?
}

fn remove_edit_sidecars(folders: Vec<PathBuf>, confirmed: bool) -> Result<usize, String> {
    if !confirmed {
        return Err("Deleting edit sidecars requires confirmation.".into());
    }
    let mut roots = HashSet::new();
    for folder in folders {
        if !folder.is_absolute() {
            return Err(format!("Choose a photo folder: {}", folder.display()));
        }
        let root = folder
            .canonicalize()
            .map_err(|error| format!("{}: {error}", folder.display()))?;
        if !root.is_dir() || root.parent().is_none() {
            return Err(format!("Choose a photo folder: {}", folder.display()));
        }
        roots.insert(root);
    }

    let mut deleted = 0;
    for root in roots {
        for entry in WalkDir::new(root).follow_links(false) {
            let entry = entry.map_err(|error| format!("Deleted {deleted} files. {error}"))?;
            if entry.file_type().is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".rrdata")
            {
                fs::remove_file(entry.path()).map_err(|error| {
                    format!(
                        "Deleted {deleted} files. {}: {error}",
                        entry.path().display()
                    )
                })?;
                deleted += 1;
            }
        }
    }
    Ok(deleted)
}

#[cfg(test)]
#[test]
fn edit_sidecar_cleanup_requires_confirmation_and_preserves_other_files() {
    let folder = tempfile::tempdir().unwrap();
    let nested = folder.path().join("nested");
    fs::create_dir(&nested).unwrap();
    let sidecar = nested.join("photo.NEF.rrdata");
    fs::write(&sidecar, r#"{"rating":4,"adjustments":{"exposure":0.5}}"#).unwrap();
    let retained = ["photo.NEF", "photo.xmp", "settings.json"];
    for name in retained {
        fs::write(nested.join(name), "{}").unwrap();
    }

    let roots = vec![folder.path().to_path_buf(), nested.clone()];
    assert!(remove_edit_sidecars(roots.clone(), false).is_err());
    assert!(sidecar.exists());
    assert_eq!(remove_edit_sidecars(roots.clone(), true).unwrap(), 1);
    assert!(!sidecar.exists());
    assert!(retained.iter().all(|name| nested.join(name).exists()));
    assert_eq!(remove_edit_sidecars(roots, true).unwrap(), 0);
}

#[tauri::command]
pub fn clear_thumbnail_cache(app_handle: AppHandle) -> Result<(), String> {
    let cache_dir = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?;
    let thumb_cache_dir = cache_dir.join("thumbnails");

    if thumb_cache_dir.exists() {
        fs::remove_dir_all(&thumb_cache_dir)
            .map_err(|e| format!("Failed to remove thumbnail cache: {}", e))?;
    }

    fs::create_dir_all(&thumb_cache_dir)
        .map_err(|e| format!("Failed to recreate thumbnail cache directory: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn show_in_finder(path: String) -> Result<(), String> {
    let (source_path, _) = parse_virtual_path(&path);

    #[cfg(target_os = "windows")]
    {
        let source_path_str = source_path.to_string_lossy().to_string();
        Command::new("explorer")
            .args(["/select,", &source_path_str])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        let source_path_str = source_path.to_string_lossy().to_string();
        Command::new("open")
            .args(["-R", &source_path_str])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        let location = if source_path.is_dir() {
            Some(source_path.as_path())
        } else {
            source_path.parent()
        };
        if let Some(location) = location {
            Command::new("xdg-open")
                .arg(location)
                .spawn()
                .map_err(|e| e.to_string())?;
        } else {
            return Err("Could not get parent directory".into());
        }
    }

    Ok(())
}

#[tauri::command]
pub fn delete_files_from_disk(paths: Vec<String>, app_handle: AppHandle) -> Result<(), String> {
    let mut files_to_trash = HashSet::new();
    let mut deletions = HashSet::new();

    for path_str in paths {
        let (source_path, sidecar_path) = parse_virtual_path(&path_str);
        deletions.insert(path_str.clone());

        if path_str.contains("?vc=") {
            if let Some(xmp) = crate::xmp::existing_path(&source_path, &sidecar_path) {
                files_to_trash.insert(xmp);
            }
            if sidecar_path.exists() {
                files_to_trash.insert(sidecar_path);
            }
        } else {
            if source_path.exists() {
                match find_all_associated_files(&source_path) {
                    Ok(associated_files) => {
                        for file in associated_files {
                            files_to_trash.insert(file);
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "Could not find associated files for {}: {}",
                            source_path.display(),
                            e
                        );
                    }
                }
            }
        }
    }

    if files_to_trash.is_empty() {
        return Ok(());
    }

    let final_paths_to_delete: Vec<PathBuf> = files_to_trash.into_iter().collect();

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if let Err(trash_error) = trash::delete_all(&final_paths_to_delete) {
        log::warn!(
            "Failed to move files to trash: {}. Falling back to permanent delete.",
            trash_error
        );
        for path in final_paths_to_delete {
            if path.is_file() {
                if let Err(e) = fs::remove_file(&path) {
                    log::warn!("Failed to delete file {}: {}", path.display(), e);
                }
            } else if path.is_dir()
                && let Err(e) = fs::remove_dir_all(&path)
            {
                log::warn!("Failed to delete directory {}: {}", path.display(), e);
            }
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    for path in final_paths_to_delete {
        if path.is_file() {
            if let Err(e) = fs::remove_file(&path) {
                log::warn!("Failed to delete file {}: {}", path.display(), e);
            }
        } else if path.is_dir() {
            if let Err(e) = fs::remove_dir_all(&path) {
                log::warn!("Failed to delete directory {}: {}", path.display(), e);
            }
        }
    }

    sync_album_path_changes(&app_handle, None, Some(&deletions), None);

    Ok(())
}

fn deletion_stem_for(filename: &str) -> Option<&str> {
    let (base, extension) = filename.rsplit_once('.')?;
    let image_filename =
        if extension.eq_ignore_ascii_case("rrdata") || extension.eq_ignore_ascii_case("xmp") {
            let original = base
                .rsplit_once('.')
                .filter(|(_, id)| id.len() == 6 && id.chars().all(|c| c.is_ascii_hexdigit()))
                .map_or(base, |(original, _)| original);
            if extension.eq_ignore_ascii_case("xmp")
                && !crate::formats::is_supported_media_file(original)
            {
                return Some(base);
            }
            original
        } else if extension.eq_ignore_ascii_case("rrexif") {
            base
        } else if is_supported_image_file(filename) {
            filename
        } else {
            return None;
        };
    Path::new(image_filename)
        .file_stem()
        .and_then(|s| s.to_str())
}

#[tauri::command]
pub fn delete_files_with_associated(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let mut stems_to_delete = HashSet::new();
    let mut parent_dirs = HashSet::new();
    let mut deletions = HashSet::new();

    for path_str in &paths {
        deletions.insert(path_str.clone());
        let (source_path, _) = parse_virtual_path(path_str);
        if let Some(stem) = source_path.file_stem().and_then(|s| s.to_str()) {
            stems_to_delete.insert(stem.to_string());
        }
        if let Some(parent) = source_path.parent() {
            parent_dirs.insert(parent.to_path_buf());
        }
    }

    if stems_to_delete.is_empty() {
        return Ok(());
    }

    let mut files_to_trash = HashSet::new();

    for parent_dir in parent_dirs {
        if let Ok(entries) = fs::read_dir(parent_dir) {
            for entry in entries.filter_map(Result::ok) {
                let entry_path = entry.path();
                if !entry_path.is_file() {
                    continue;
                }

                let entry_filename = entry.file_name();
                let entry_filename_str = entry_filename.to_string_lossy();

                if let Some(stem) = deletion_stem_for(&entry_filename_str)
                    && stems_to_delete.contains(stem)
                {
                    files_to_trash.insert(entry_path);
                }
            }
        }
    }

    if files_to_trash.is_empty() {
        return Ok(());
    }

    let final_paths_to_delete: Vec<PathBuf> = files_to_trash.into_iter().collect();

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if let Err(trash_error) = trash::delete_all(&final_paths_to_delete) {
        log::warn!(
            "Failed to move files to trash: {}. Falling back to permanent delete.",
            trash_error
        );
        for path in final_paths_to_delete {
            if path.is_file()
                && let Err(e) = fs::remove_file(&path)
            {
                log::warn!("Failed to delete file {}: {}", path.display(), e);
            }
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    for path in final_paths_to_delete {
        if path.is_file() {
            if let Err(e) = fs::remove_file(&path) {
                log::warn!("Failed to delete file {}: {}", path.display(), e);
            }
        }
    }

    sync_album_path_changes(&app_handle, None, Some(&deletions), None);

    Ok(())
}

pub fn get_thumb_cache_dir(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let cache_dir = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?;
    let thumb_cache_dir = cache_dir.join("thumbnails");
    if !thumb_cache_dir.exists() {
        fs::create_dir_all(&thumb_cache_dir).map_err(|e| e.to_string())?;
    }
    Ok(thumb_cache_dir)
}

#[tauri::command]
pub async fn import_files(
    source_paths: Vec<String>,
    destination_folder: String,
    settings: ImportSettings,
    app_handle: AppHandle,
) -> Result<(), String> {
    let total_files = source_paths.len();
    let import_mode = settings
        .import_mode
        .as_deref()
        .unwrap_or(if settings.delete_after_import {
            "move"
        } else {
            "copy"
        })
        .to_owned();
    if !matches!(import_mode.as_str(), "copy" | "move" | "inPlace") {
        return Err(format!("Unsupported import mode: {import_mode}"));
    }
    let _ = app_handle.emit(
        "import-start",
        serde_json::json!({ "total": total_files, "mode": import_mode }),
    );

    if import_mode == "inPlace" {
        let (catalog_status, already_exists) =
            crate::catalog::catalog_import_paths_in_place(source_paths, app_handle.clone()).await?;
        let _ = app_handle.emit(
            "import-progress",
            serde_json::json!({ "current": total_files, "total": total_files, "path": "" }),
        );
        let _ = app_handle.emit(
            "import-complete",
            serde_json::json!({
                "imported": total_files - already_exists,
                "alreadyExists": already_exists,
                "failed": 0,
                "mode": "inPlace",
                "catalogStatus": catalog_status
            }),
        );
        return Ok(());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let mut imported = 0usize;
        let mut failed = 0usize;
        let mut already_exists = 0usize;
        let mut seen_sources = HashSet::new();
        for (i, source_path_str) in source_paths.iter().enumerate() {
            let _ = app_handle.emit(
                "import-progress",
                serde_json::json!({ "current": i, "total": total_files, "path": source_path_str }),
            );

            let source_key = Path::new(source_path_str)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(source_path_str))
                .to_string_lossy()
                .to_string();
            #[cfg(target_os = "windows")]
            let source_key = source_key.to_lowercase();
            if !seen_sources.insert(source_key) {
                already_exists += 1;
                continue;
            }

            let import_result: Result<(), String> = (|| {
                let (source_path, source_sidecar) = parse_virtual_path(source_path_str);
                if !source_path.exists() {
                    return Err(format!("Source file not found: {}", source_path_str));
                }

                if import_mode != "move" {
                    let source_abs = source_path.canonicalize().map_err(|e| e.to_string())?;
                    let destination_abs = PathBuf::from(&destination_folder)
                        .canonicalize()
                        .map_err(|e| e.to_string())?;
                    if source_abs.starts_with(&destination_abs) {
                        return Err(format!(
                            "Source is already inside the destination folder and was skipped to prevent a duplicate: {}",
                            source_path.display()
                        ));
                    }
                }

                let file_date = exif_processing::get_creation_date_from_path(&source_path);

                let mut final_dest_folder = PathBuf::from(&destination_folder);
                if settings.organize_by_date {
                    let subfolder = import_subfolder(&settings.date_folder_format, &file_date)?;
                    final_dest_folder.push(subfolder);
                }

                fs::create_dir_all(&final_dest_folder)
                    .map_err(|e| format!("Failed to create destination folder: {}", e))?;

                let new_stem = generate_filename_from_template(
                    &settings.filename_template,
                    &source_path,
                    i + 1,
                    total_files,
                    &file_date,
                );
                let extension = source_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let new_filename = format!("{}.{}", new_stem, extension);
                let dest_file_path = final_dest_folder.join(new_filename);

                if dest_file_path.exists() {
                    return Err(format!(
                        "File already exists at destination: {}",
                        dest_file_path.display()
                    ));
                }

                fs::copy(&source_path, &dest_file_path).map_err(|e| e.to_string())?;
                if source_sidecar.exists()
                    && let Some(dest_str) = dest_file_path.to_str()
                {
                    let (_, dest_sidecar) = parse_virtual_path(dest_str);
                    fs::copy(&source_sidecar, &dest_sidecar).map_err(|e| e.to_string())?;
                }

                let mut source_rrexif_name = source_path.file_name().unwrap().to_os_string();
                source_rrexif_name.push(".rrexif");
                let source_rrexif = source_path.with_file_name(source_rrexif_name);

                if source_rrexif.exists() {
                    let mut dest_rrexif_name = dest_file_path.file_name().unwrap().to_os_string();
                    dest_rrexif_name.push(".rrexif");
                    let dest_rrexif = dest_file_path.with_file_name(dest_rrexif_name);
                    let _ = fs::copy(&source_rrexif, &dest_rrexif);
                }

                crate::xmp::copy_sidecar(
                    &source_path,
                    &source_sidecar,
                    &dest_file_path,
                    &crate::exif_processing::get_primary_sidecar_path(&dest_file_path),
                )?;
                let source_xmp = crate::xmp::existing_path(&source_path, &source_sidecar);
                if import_mode == "move" {
                    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                    {
                        if let Err(trash_error) = trash::delete(&source_path) {
                            log::warn!(
                                "Failed to trash source file {}: {}. Deleting permanently.",
                                source_path.display(),
                                trash_error
                            );
                            fs::remove_file(&source_path).map_err(|e| e.to_string())?;
                        }
                        if let Some(source_xmp) = source_xmp {
                            trash::delete(&source_xmp).map_err(|error| error.to_string())?;
                        }
                        if source_sidecar.exists()
                            && let Err(trash_error) = trash::delete(&source_sidecar)
                        {
                            log::warn!(
                                "Failed to trash source sidecar {}: {}. Deleting permanently.",
                                source_sidecar.display(),
                                trash_error
                            );
                            fs::remove_file(&source_sidecar).map_err(|e| e.to_string())?;
                        }
                        if source_rrexif.exists()
                            && let Err(trash_error) = trash::delete(&source_rrexif)
                        {
                            log::warn!(
                                "Failed to trash source EXIF sidecar {}: {}. Deleting permanently.",
                                source_rrexif.display(),
                                trash_error
                            );
                            fs::remove_file(&source_rrexif).map_err(|e| e.to_string())?;
                        }
                    }

                    #[cfg(not(any(
                        target_os = "windows",
                        target_os = "macos",
                        target_os = "linux"
                    )))]
                    {
                        fs::remove_file(&source_path).map_err(|e| e.to_string())?;
                        if source_sidecar.exists() {
                            fs::remove_file(&source_sidecar).map_err(|e| e.to_string())?;
                        }
                        if source_rrexif.exists() {
                            let _ = fs::remove_file(&source_rrexif);
                        }
                    }
                }

                Ok(())
            })();

            if let Err(e) = import_result {
                if e.starts_with("File already exists at destination:")
                    || e.starts_with("Source is already inside the destination folder")
                {
                    already_exists += 1;
                    log::info!("Skipping existing import {}: {}", source_path_str, e);
                } else {
                    failed += 1;
                    log::error!("Failed to import {}: {}", source_path_str, e);
                    let _ = app_handle.emit("import-error", e);
                }
                continue;
            }
            imported += 1;
        }

        let _ = app_handle.emit(
            "import-progress",
            serde_json::json!({ "current": total_files, "total": total_files, "path": "" }),
        );
        let catalog_status = if imported > 0 {
            match tauri::async_runtime::block_on(crate::catalog::catalog_add_folder(
                destination_folder.clone(),
                app_handle.clone(),
            )) {
                Ok(status) => Some(status),
                Err(error) => {
                    let _ = app_handle.emit(
                        "import-error",
                        format!("Files imported but catalog indexing failed: {error}"),
                    );
                    failed += 1;
                    None
                }
            }
        } else {
            None
        };
        let _ = app_handle.emit(
            "import-complete",
            serde_json::json!({ "imported": imported, "alreadyExists": already_exists, "failed": failed, "mode": import_mode, "catalogStatus": catalog_status }),
        );
    });

    Ok(())
}

pub fn generate_filename_from_template(
    template: &str,
    original_path: &std::path::Path,
    sequence: usize,
    total: usize,
    file_date: &DateTime<Utc>,
) -> String {
    let stem = original_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let sequence_str = format!(
        "{:0width$}",
        sequence,
        width = total.to_string().len().max(1)
    );
    let local_date = file_date.with_timezone(&chrono::Local);

    let mut result = template.to_string();
    result = result.replace("{original_filename}", stem);
    result = result.replace("{sequence}", &sequence_str);
    result = result.replace("{YYYY}", &local_date.format("%Y").to_string());
    result = result.replace("{MM}", &local_date.format("%m").to_string());
    result = result.replace("{DD}", &local_date.format("%d").to_string());
    result = result.replace("{hh}", &local_date.format("%H").to_string());
    result = result.replace("{mm}", &local_date.format("%M").to_string());

    result
}

#[tauri::command]
pub fn rename_files(
    paths: Vec<String>,
    name_template: String,
    app_handle: AppHandle,
) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut operations: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut final_new_paths = Vec::with_capacity(paths.len());
    let mut renames = HashMap::new();

    for (i, path_str) in paths.iter().enumerate() {
        let (original_path, _) = parse_virtual_path(path_str);
        if !original_path.exists() {
            return Err(format!("File not found: {}", path_str));
        }

        let parent = original_path
            .parent()
            .ok_or("Could not get parent directory")?;
        let extension = original_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let file_date = exif_processing::get_creation_date_from_path(&original_path);

        let new_stem = generate_filename_from_template(
            &name_template,
            &original_path,
            i + 1,
            paths.len(),
            &file_date,
        );
        let new_filename = format!("{}.{}", new_stem, extension);
        let new_path = parent.join(new_filename);

        if new_path.exists() && new_path != original_path {
            return Err(format!(
                "A file with the name {} already exists.",
                new_path.display()
            ));
        }

        operations.push((original_path, new_path));
    }

    let mut sidecar_operations = Vec::new();
    for (original_path, new_path) in &operations {
        for companion in find_all_associated_files(original_path)?
            .into_iter()
            .skip(1)
        {
            let target = renamed_associated_file(&companion, original_path, new_path);
            if target != companion && target.exists() {
                return Err(format!(
                    "A file with the name {} already exists.",
                    target.display()
                ));
            }
            sidecar_operations.push((companion, target));
        }
    }
    operations.extend(sidecar_operations);

    for (old_path, new_path) in operations {
        if let Err(e) = fs::rename(&old_path, &new_path) {
            log::warn!(
                "Failed to rename {} to {}: {}",
                old_path.display(),
                new_path.display(),
                e
            );
            continue;
        }

        let old_str = old_path.to_string_lossy().into_owned();
        let new_str = new_path.to_string_lossy().into_owned();

        renames.insert(old_str, new_str.clone());

        if is_supported_image_file(&new_path) {
            final_new_paths.push(new_str);
        }
    }

    sync_album_path_changes(&app_handle, Some(&renames), None, None);

    Ok(final_new_paths)
}

#[tauri::command]
pub fn create_virtual_copy(
    source_virtual_path: String,
    target_album_id: Option<String>,
    app_handle: AppHandle,
) -> Result<String, String> {
    let (source_path, source_sidecar_path) = parse_virtual_path(&source_virtual_path);

    let new_copy_id = Uuid::new_v4().to_string()[..6].to_string();
    let new_virtual_path = format!("{}?vc={}", source_path.to_string_lossy(), new_copy_id);
    let (_, new_sidecar_path) = parse_virtual_path(&new_virtual_path);

    if source_sidecar_path.exists() {
        fs::copy(&source_sidecar_path, &new_sidecar_path)
            .map_err(|e| format!("Failed to copy sidecar file: {}", e))?;
    } else {
        let default_metadata = ImageMetadata::default();
        let json_string =
            serde_json::to_string_pretty(&default_metadata).map_err(|e| e.to_string())?;
        fs::write(&new_sidecar_path, json_string).map_err(|e| e.to_string())?;
    }

    crate::xmp::copy_sidecar(
        &source_path,
        &source_sidecar_path,
        &source_path,
        &new_sidecar_path,
    )?;
    if let Some(album_id) = target_album_id {
        let _ = add_to_album(album_id, vec![new_virtual_path.clone()], app_handle);
    }

    Ok(new_virtual_path)
}
