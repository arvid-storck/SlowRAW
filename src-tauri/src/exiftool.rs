//! Read-only bridge to Phil Harvey's official ExifTool executable.
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};
use tauri::Manager;

static PROGRAM: OnceLock<PathBuf> = OnceLock::new();
static SESSION: OnceLock<Result<Mutex<Session>, String>> = OnceLock::new();
static WORKERS: OnceLock<Vec<Mutex<Option<Session>>>> = OnceLock::new();

pub fn initialize(app: &tauri::AppHandle) {
    let name = if cfg!(windows) {
        "exiftool.exe"
    } else {
        "exiftool"
    };
    let bundled = app
        .path()
        .resolve(
            format!("resources/exiftool/{name}"),
            tauri::path::BaseDirectory::Resource,
        )
        .ok();
    let development = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources/exiftool")
        .join(name);
    let program = std::env::var_os("SLOWRAW_EXIFTOOL")
        .map(PathBuf::from)
        .or_else(|| bundled.filter(|p| p.is_file()))
        .or_else(|| development.is_file().then_some(development))
        .or_else(|| {
            [
                "/opt/homebrew/bin/exiftool",
                "/usr/local/bin/exiftool",
                "/usr/bin/exiftool",
            ]
            .into_iter()
            .map(PathBuf::from)
            .find(|p| p.is_file())
        })
        .unwrap_or_else(|| name.into());
    let _ = PROGRAM.set(program);
}

struct Session {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<String>,
    request: u64,
}

impl Session {
    fn start() -> Result<Self, String> {
        let default = if cfg!(windows) {
            "exiftool.exe"
        } else {
            "exiftool"
        };
        let program = PROGRAM
            .get()
            .cloned()
            .or_else(|| std::env::var_os("SLOWRAW_EXIFTOOL").map(PathBuf::from))
            .unwrap_or_else(|| {
                let development = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("resources/exiftool")
                    .join(default);
                if development.is_file() {
                    development
                } else {
                    default.into()
                }
            });
        let mut command = Command::new(program);
        command
            .arg("-config")
            .arg("")
            .args(["-stay_open", "True", "-@", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Could not start official ExifTool: {e}"))?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::sync_channel(64);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else {
                    break;
                };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut session = Self {
            child,
            input,
            lines,
            request: 0,
        };
        writeln!(session.input, "-ver\n-execute0").map_err(|e| e.to_string())?;
        session.input.flush().map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut version = String::new();
        loop {
            let line = session
                .lines
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|e| format!("ExifTool startup failed: {e}"))?;
            if line == "{ready0}" {
                break;
            }
            version.push_str(&line);
        }
        version
            .trim()
            .parse::<f32>()
            .map_err(|_| "Invalid ExifTool version response")?;
        Ok(session)
    }

    fn read(&mut self, path: &Path) -> Result<String, String> {
        // Argument files are line based. Canonical paths also prevent leading-dash options.
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let path = path
            .to_str()
            .ok_or("ExifTool requires a Unicode filename")?;
        if path.contains(['\r', '\n']) {
            return Err("Unsupported newline in filename".into());
        }
        self.request += 1;
        // Keep previews out of the read; explicitly requested correction blocks
        // still use binary JSON. Preserve unformatted correction coefficients.
        let tags = "-b\n-api\nBinary=0\n-u\n-Orientation#\n-Olympus_ImageProcessing_0x150a#\n-Olympus_ImageProcessing_0x150c#\n-PreviewImageStart#\n-PreviewImageLength#\n-JpgFromRawStart#\n-JpgFromRawLength#\n-ThumbnailOffset#\n-ThumbnailLength#\n-GPSLatitude#\n-GPSLongitude#\n-GPSLatitudeRef#\n-GPSLongitudeRef#\n-GPSAltitude#\n-GPSAltitudeRef#\n-ShutterSpeedValue#\n-ExposureCompensation#\n-Flash#\n-MeteringMode#\n-WhiteBalance#\n-ExposureProgram#\n-FocalLength#\n-FNumber#\n-SubjectDistance#\n-OpcodeList2#\n-OpcodeList3#\n-DistortionCorrectionVersion\n-RadialDistortionCoefficient1#\n-RadialDistortionCoefficient2#\n-RadialDistortionCoefficient3#\n-VignetteCorrectionVersion\n-VignetteCoefficient1#\n-VignetteCoefficient2#\n-VignetteCoefficient3#\n-ActiveArea#\n-DefaultCropOrigin#\n-DefaultCropSize#\n-DefaultScale#\n-DistortionCorrParams#\n-DistortionCorrParamsNumber#\n-DistortionCorrParamsPresent#\n-ChromaticAberrationCorrParams#\n-VignettingCorrParams#\n-GeometricDistortionParams#\n-ChromaticAberrationParams#\n-VignettingParams#\n-CropMode#\n-DistortionScale#\n-DistortionParam08#\n-DistortionParam04#\n-DistortionParam11#\n-all";
        writeln!(
            self.input,
            "-j\n-G1\n-struct\n-charset\nfilename=UTF8\n{tags}\n{path}\n-execute{}",
            self.request
        )
        .map_err(|e| e.to_string())?;
        self.input.flush().map_err(|e| e.to_string())?;
        let ready = format!("{{ready{}}}", self.request);
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut json = String::new();
        loop {
            let line = self
                .lines
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|e| format!("ExifTool response failed: {e}"))?;
            if line == ready {
                break;
            }
            if json.len() + line.len() > 16 * 1024 * 1024 {
                return Err("ExifTool metadata exceeds limit".into());
            }
            json.push_str(&line);
            json.push('\n');
        }
        Ok(json)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn embedded_previews(raw: &serde_json::Value) -> Vec<crate::exif_processing::EmbeddedPreview> {
    let Some(tags) = raw
        .as_array()
        .and_then(|v| v.first())
        .and_then(|v| v.as_object())
    else {
        return Vec::new();
    };
    let mut previews = Vec::new();
    for (key, value) in tags {
        let Some((group, name)) = key.rsplit_once(':') else {
            continue;
        };
        let length_key = match name {
            "PreviewImageStart" => "PreviewImageLength",
            "JpgFromRawStart" => "JpgFromRawLength",
            "ThumbnailOffset" => "ThumbnailLength",
            _ => continue,
        };
        if let Some(offset) = value.as_u64()
            && let Some(length) = tags
                .get(&format!("{group}:{length_key}"))
                .and_then(|v| v.as_u64())
                .filter(|v| *v > 0)
        {
            let orientation = tags
                .get(&format!("{group}:Orientation"))
                .or_else(|| tags.get("IFD0:Orientation"))
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32;
            previews.push(crate::exif_processing::EmbeddedPreview {
                offset,
                length,
                orientation,
            });
        }
    }
    previews.sort_by_key(|preview| preview.length);
    previews
}

pub fn read(path: &Path) -> Result<crate::exif_processing::SourceMetadata, String> {
    let json = request(path)?;
    let raw = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    Ok(crate::exif_processing::SourceMetadata {
        fields: normalize(&json)?,
        lens_profile: crate::embedded_lens::from_metadata(&raw),
        previews: embedded_previews(&raw),
    })
}

#[cfg(test)]
pub(crate) fn request_count() -> u64 {
    let primary = SESSION
        .get()
        .unwrap()
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .request;
    primary
        + WORKERS.get().map_or(0, |workers| {
            workers
                .iter()
                .map(|worker| {
                    worker
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map_or(0, |session| session.request)
                })
                .sum::<u64>()
        })
}

pub fn ensure_available() -> Result<(), String> {
    {
        SESSION
            .get_or_init(|| Session::start().map(Mutex::new))
            .as_ref()
            .map(|_| ())
            .map_err(Clone::clone)
    }
}

pub fn shutdown() {
    if let Some(Ok(session)) = SESSION.get() {
        if let Ok(mut session) = session.try_lock() {
            let _ = session.child.kill();
            let _ = session.child.wait();
        }
    }
    if let Some(workers) = WORKERS.get() {
        for worker in workers {
            if let Ok(mut session) = worker.try_lock() {
                session.take();
            }
        }
    }
}

fn request(path: &Path) -> Result<String, String> {
    let session = SESSION.get_or_init(|| Session::start().map(Mutex::new));
    let session = session.as_ref().map_err(Clone::clone)?;
    if let Ok(mut session) = session.try_lock() {
        return read_session(&mut session, path);
    }
    // Separate processes allow import and visible thumbnails to read concurrently.
    // Keep the pool small: RAW pixel buffers already use the CPU/RAM budget.
    let workers = WORKERS.get_or_init(|| {
        let count = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .clamp(1, 4);
        (1..count).map(|_| Mutex::new(None)).collect()
    });
    for worker in workers {
        if let Ok(mut slot) = worker.try_lock() {
            if slot.is_none() {
                *slot = Some(Session::start()?);
            }
            return read_session(slot.as_mut().unwrap(), path);
        }
    }
    let mut session = session.lock().map_err(|e| e.to_string())?;
    read_session(&mut session, path)
}

fn read_session(session: &mut Session, path: &Path) -> Result<String, String> {
    if session
        .child
        .try_wait()
        .map_err(|e| e.to_string())?
        .is_some()
    {
        *session = Session::start()?;
    }
    let result = session.read(path);
    if result.is_err() {
        let _ = session.child.kill();
        let _ = session.child.wait();
    }
    result
}

fn normalize(json: &str) -> Result<HashMap<String, String>, String> {
    let documents: Vec<serde_json::Map<String, serde_json::Value>> =
        serde_json::from_str(json).map_err(|e| e.to_string())?;
    let tags = documents.first().ok_or("Empty ExifTool response")?;
    let mut map = HashMap::new();
    // Standard EXIF wins over duplicate preview/XMP fields; Composite fills resolved lens IDs.
    let mut ordered: Vec<_> = tags.iter().collect();
    ordered.sort_by_key(|(key, _)| match key.split(':').next().unwrap_or("") {
        "ExifIFD" | "IFD0" | "GPS" => 2,
        "Composite" => 1,
        _ => 0,
    });
    for (key, value) in ordered {
        let name = key.rsplit(':').next().unwrap_or(key);
        if name == "Error" {
            return Err(value.to_string());
        }
        if matches!(
            name,
            "SourceFile" | "Directory" | "FileName" | "ExifToolVersion"
        ) {
            continue;
        }
        let text = match value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => continue,
            _ => value.to_string(),
        };
        if text.starts_with("(Binary data") || text.starts_with("base64:") || text.trim().is_empty()
        {
            continue;
        }
        map.insert(
            name.to_string(),
            crate::exif_processing::truncate_large_exif(&text),
        );
    }
    if !map.contains_key("LensModel") {
        if let Some(lens) = tags
            .get("Composite:LensID")
            .or_else(|| tags.get("Composite:Lens"))
            .and_then(|v| v.as_str())
        {
            if !lens.contains("Unknown") && !lens.chars().all(|c| c.is_ascii_digit()) {
                map.insert("LensModel".into(), lens.into());
            }
        }
    }
    // ISO in MakerNotes can be an internal measurement (e.g. Nikon 3805.46)
    // while the camera reports EXIF ISO 4000. Prefer the standard value.
    let camera_iso = [
        "ExifIFD:ISO",
        "IFD0:ISO",
        "ExifIFD:PhotographicSensitivity",
        "ExifIFD:RecommendedExposureIndex",
        "ExifIFD:StandardOutputSensitivity",
    ]
    .into_iter()
    .filter_map(|key| tags.get(key))
    .find_map(|value| {
        let number = value
            .as_f64()
            .or_else(|| value.as_str()?.parse::<f64>().ok())?;
        (number.is_finite() && number > 0.0 && number != 65535.0).then(|| number.to_string())
    });
    if let Some(iso) = camera_iso {
        map.insert("ISO".into(), iso.clone());
        map.insert("PhotographicSensitivity".into(), iso);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_cache_misses_read_the_same_file_once() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("shared.jpg");
        image::RgbImage::new(24, 16).save(&path).unwrap();
        ensure_available().unwrap();
        let before = request_count();
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    barrier.wait();
                    let metadata = crate::exif_processing::read_source_metadata(&path).unwrap();
                    assert_eq!(metadata.fields["ImageWidth"], "24");
                });
            }
        });
        assert_eq!(request_count(), before + 1);
    }

    #[test]
    fn another_reader_can_work_while_the_primary_session_is_busy() {
        if std::thread::available_parallelism().map_or(1, usize::from) == 1 {
            return;
        }
        ensure_available().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("parallel.jpg");
        image::RgbImage::new(31, 19).save(&path).unwrap();
        let primary = SESSION.get().unwrap().as_ref().unwrap().lock().unwrap();
        let (send, receive) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = send.send(read(&path));
        });
        let result = receive.recv_timeout(Duration::from_secs(15));
        drop(primary);
        worker.join().unwrap();
        assert_eq!(result.unwrap().unwrap().fields["ImageWidth"], "31");
        assert!(WORKERS.get().unwrap().len() <= 3);
    }

    #[test]
    fn iso_uses_camera_reported_value_not_internal_or_iso_speed_fields() {
        let fields =
            normalize(r#"[{"ExifIFD:ISO":4000,"Nikon:ISO":3805.462768,"ExifIFD:ISOSpeed":64}]"#)
                .unwrap();
        assert_eq!(fields["ISO"], "4000");
        assert_eq!(fields["PhotographicSensitivity"], "4000");
        let fields = normalize(r#"[{"ExifIFD:ISO":65535,"Nikon:ISO":3805.46,"ExifIFD:RecommendedExposureIndex":102400}]"#).unwrap();
        assert_eq!(fields["ISO"], "102400");
    }
    #[test]
    fn official_process_reads_unicode_filenames_and_is_reused() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("räksmörgås-日本.jpg");
        image::RgbImage::new(8, 6).save(&file).unwrap();
        let first = crate::exif_processing::read_source_metadata(&file).unwrap();
        assert_eq!(first.fields["ImageWidth"], "8");
        let id = SESSION
            .get()
            .unwrap()
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .child
            .id();
        let requests = SESSION
            .get()
            .unwrap()
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .request;
        for _ in 0..5 {
            assert_eq!(
                crate::exif_processing::read_source_metadata(&file)
                    .unwrap()
                    .fields["ImageHeight"],
                "6"
            );
        }
        assert_eq!(
            SESSION
                .get()
                .unwrap()
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .request,
            requests
        );
        // A changed source invalidates fields and lens data together.
        image::RgbImage::new(12, 9).save(&file).unwrap();
        assert_eq!(
            crate::exif_processing::read_source_metadata(&file)
                .unwrap()
                .fields["ImageWidth"],
            "12"
        );
        assert_eq!(
            SESSION
                .get()
                .unwrap()
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .request,
            requests + 1
        );
        assert_eq!(
            SESSION
                .get()
                .unwrap()
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .child
                .id(),
            id
        );
    }
    #[test]
    fn metadata_prefers_primary_and_resolved_lens_without_binary_payloads() {
        let map = normalize(r#"[{"IFD0:Model":"GR II","XMP:Model":"preview","Composite:LensID":"GR LENS 18.3mm F2.8","ExifIFD:FocalLength":18.3,"ExifIFD:MakerNote":"(Binary data 1000 bytes)"}]"#).unwrap();
        assert_eq!(map["Model"], "GR II");
        assert_eq!(map["LensModel"], "GR LENS 18.3mm F2.8");
        assert_eq!(map["FocalLength"], "18.3");
        assert!(!map.contains_key("MakerNote"));
        assert!(normalize(r#"[{"ExifTool:Error":"Unreadable image"}]"#).is_err());
    }
}
