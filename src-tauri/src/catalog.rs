use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use rusqlite::{
    Connection, OptionalExtension, ToSql, TransactionBehavior, params, params_from_iter,
};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use walkdir::WalkDir;

use crate::app_settings::{load_settings, save_settings};
use crate::exif_processing::{get_primary_sidecar_path, load_sidecar, read_exif_data};
use crate::formats::{is_raw_file, is_supported_media_file, is_supported_video_file};

const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStatus {
    database_path: String,
    folders: Vec<String>,
    image_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogImage {
    pub path: String,
    pub modified: u64,
    pub is_edited: bool,
    pub rating: u8,
    pub tags: Option<Vec<String>>,
    pub exif: Option<HashMap<String, String>>,
    pub is_virtual_copy: bool,
    pub is_cloud_placeholder: bool,
    pub is_raw: bool,
    pub group_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFilter {
    #[serde(default)]
    pub cameras: Vec<String>,
    #[serde(default)]
    pub lenses: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub colors: Vec<String>,
    #[serde(default)]
    pub file_types: Vec<String>,
    #[serde(default)]
    pub ratings: Vec<u8>,
    pub sort_key: Option<String>,
    pub sort_descending: Option<bool>,
    pub text: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub date: Option<String>,
    pub minimum_rating: Option<u8>,
    pub tag: Option<String>,
    pub color: Option<String>,
    pub flag: Option<String>,
    pub file_type: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFacets {
    cameras: Vec<String>,
    lenses: Vec<String>,
    dates: Vec<String>,
    tags: Vec<String>,
    colors: Vec<String>,
    file_types: Vec<String>,
    date_days: Vec<CatalogDateCount>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDateCount {
    date: String,
    count: u64,
}

#[tauri::command]
pub fn open_video_in_system(path: String) -> Result<(), String> {
    let path = Path::new(&path);
    if !is_supported_video_file(path) || !path.is_file() {
        return Err(format!(
            "Video file not found or unsupported: {}",
            path.display()
        ));
    }
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        open::that(path).map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err("Opening videos in the system player is not supported on this platform.".into())
    }
}

fn unix_seconds(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = match crate::portable_paths::portable_root() {
        Some(root) => root,
        None => crate::portable_paths::app_data_dir(app)?,
    };
    Ok(dir.join("rapidraw.db"))
}

// Integrity checking walks the database; run it only once per process/database.
static CHECKED_DATABASES: std::sync::LazyLock<std::sync::Mutex<HashSet<PathBuf>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashSet::new()));

// Serialize folder scans; ordinary readers and short metadata writes remain concurrent.
static CATALOG_SCAN: std::sync::Mutex<()> = std::sync::Mutex::new(());
const INDEX_BATCH_SIZE: usize = 256;
const UPSERT_IMAGE: &str = "INSERT INTO catalog_images(path,folder_path,filename,extension,modified,sidecar_modified,date_taken,camera,lens,focal_length,rating,color,flag,tags_json,exif_json,is_raw,is_edited,indexed_at)
 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
 ON CONFLICT(path) DO UPDATE SET folder_path=excluded.folder_path,filename=excluded.filename,extension=excluded.extension,modified=excluded.modified,sidecar_modified=excluded.sidecar_modified,date_taken=excluded.date_taken,camera=excluded.camera,lens=excluded.lens,focal_length=excluded.focal_length,rating=excluded.rating,color=excluded.color,flag=excluded.flag,tags_json=excluded.tags_json,exif_json=excluded.exif_json,is_raw=excluded.is_raw,is_edited=excluded.is_edited,indexed_at=excluded.indexed_at";

fn migrate_catalog_schema(conn: &Connection) -> rusqlite::Result<()> {
    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM catalog_meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if version.as_deref() == Some("2") {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    let columns: Vec<String> = tx
        .prepare("PRAGMA table_info(catalog_images)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<_>>()?;
    if columns.iter().any(|name| name == "thumbnail_status") {
        tx.execute_batch("ALTER TABLE catalog_images DROP COLUMN thumbnail_status;")?;
    }
    tx.execute_batch(
        "UPDATE catalog_images SET is_raw=CASE WHEN is_raw=0 THEN 0 ELSE 1 END,
         is_edited=CASE WHEN is_edited=0 THEN 0 ELSE 1 END
         WHERE is_raw NOT IN (0,1) OR is_edited NOT IN (0,1) OR is_raw IS NULL OR is_edited IS NULL;
         CREATE TRIGGER IF NOT EXISTS catalog_boolean_insert BEFORE INSERT ON catalog_images
         WHEN NEW.is_raw IS NULL OR NEW.is_raw NOT IN (0,1) OR NEW.is_edited IS NULL OR NEW.is_edited NOT IN (0,1)
         BEGIN SELECT RAISE(ABORT,'Catalog flags must be 0 or 1'); END;
         CREATE TRIGGER IF NOT EXISTS catalog_boolean_update BEFORE UPDATE OF is_raw,is_edited ON catalog_images
         WHEN NEW.is_raw IS NULL OR NEW.is_raw NOT IN (0,1) OR NEW.is_edited IS NULL OR NEW.is_edited NOT IN (0,1)
         BEGIN SELECT RAISE(ABORT,'Catalog flags must be 0 or 1'); END;"
    )?;
    {
        let mut query = tx.prepare("SELECT path,exif_json FROM catalog_images")?;
        let mut rows = query.query([])?;
        let mut update = tx.prepare("UPDATE catalog_images SET exif_json=?1 WHERE path=?2")?;
        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            let old: String = row.get(1)?;
            if let Ok(exif) = serde_json::from_str::<BTreeMap<String, serde_json::Value>>(&old) {
                let sorted = serde_json::to_string(&exif).unwrap();
                if sorted != old {
                    update.execute(params![sorted, path])?;
                }
            }
        }
    }
    tx.commit()
}

fn open_db(app: &AppHandle) -> Result<Connection, String> {
    let path = db_path(app)?;
    open_db_path(&path)
}

fn open_db_path(path: &Path) -> Result<Connection, String> {
    let path = path.to_path_buf();
    let mut checked = CHECKED_DATABASES.lock().map_err(|e| e.to_string())?;
    let open = || -> Result<Connection, rusqlite::Error> {
        let conn = Connection::open(&path)?;
        conn.busy_timeout(std::time::Duration::from_secs(15))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        if checked.contains(&path) {
            return Ok(conn);
        }
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS catalog_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS catalog_folders(path TEXT PRIMARY KEY, added_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS catalog_images(
                path TEXT PRIMARY KEY,
                folder_path TEXT NOT NULL,
                filename TEXT NOT NULL,
                extension TEXT NOT NULL,
                modified INTEGER NOT NULL,
                sidecar_modified INTEGER NOT NULL DEFAULT 0,
                date_taken TEXT,
                camera TEXT,
                lens TEXT,
                focal_length TEXT,
                rating INTEGER NOT NULL DEFAULT 0,
                color TEXT,
                flag TEXT,
                tags_json TEXT NOT NULL DEFAULT '[]',
                exif_json TEXT NOT NULL DEFAULT '{}',
                is_raw INTEGER NOT NULL DEFAULT 0 CHECK(is_raw IN (0,1)),
                is_edited INTEGER NOT NULL DEFAULT 0 CHECK(is_edited IN (0,1)),
                indexed_at INTEGER NOT NULL,
                FOREIGN KEY(folder_path) REFERENCES catalog_folders(path) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_catalog_images_folder ON catalog_images(folder_path);
             CREATE INDEX IF NOT EXISTS idx_catalog_images_camera ON catalog_images(camera);
             CREATE INDEX IF NOT EXISTS idx_catalog_images_lens ON catalog_images(lens);
             CREATE INDEX IF NOT EXISTS idx_catalog_images_date ON catalog_images(date_taken);
             CREATE INDEX IF NOT EXISTS idx_catalog_images_rating ON catalog_images(rating);
             CREATE INDEX IF NOT EXISTS idx_catalog_images_color ON catalog_images(color);
             CREATE INDEX IF NOT EXISTS idx_catalog_images_modified ON catalog_images(modified);
             CREATE INDEX IF NOT EXISTS idx_catalog_images_page ON catalog_images(date_taken DESC, modified DESC, path ASC);
             CREATE INDEX IF NOT EXISTS idx_catalog_images_extension ON catalog_images(extension);
             CREATE INDEX IF NOT EXISTS idx_catalog_images_name ON catalog_images(filename COLLATE NOCASE, path);"
        )?;
        migrate_catalog_schema(&conn)?;
        conn.execute(
            "INSERT INTO catalog_meta(key,value) VALUES('schema_version',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [SCHEMA_VERSION.to_string()],
        )?;
        Ok(conn)
    };

    match open() {
        Ok(conn) => {
            if checked.contains(&path) {
                return Ok(conn);
            }
            let healthy = conn.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0));
            match healthy {
                Ok(value) if value.eq_ignore_ascii_case("ok") => {
                    checked.insert(path.clone());
                    return Ok(conn);
                }
                Err(error) => return Err(error.to_string()),
                _ => {}
            }
            drop(conn);
        }
        Err(error) => {
            if !matches!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase)
            ) {
                return Err(error.to_string());
            }
            log::warn!("Could not open corrupt catalog index: {}", error);
        }
    }

    if path.exists() {
        let backup = path.with_extension(format!("corrupt-{}", unix_seconds(SystemTime::now())));
        fs::rename(&path, &backup)
            .map_err(|e| format!("Catalog is corrupt and could not be preserved: {e}"))?;
        log::warn!("Preserved corrupt catalog at {}", backup.display());
    }
    let _ = fs::remove_file(path.with_extension("db-wal"));
    let _ = fs::remove_file(path.with_extension("db-shm"));
    open().map_err(|e| e.to_string())
}

fn metadata_value<'a>(exif: &'a HashMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| exif.get(*key).map(String::as_str))
        .filter(|s| !s.trim().is_empty())
}

fn normalized_capture_date(value: &str) -> Option<String> {
    let value = value.trim();
    let prefix_bytes = value.as_bytes().get(..10)?;
    if !prefix_bytes.iter().enumerate().all(|(i, byte)| {
        if i == 4 || i == 7 {
            *byte == b':' || *byte == b'-'
        } else {
            byte.is_ascii_digit()
        }
    }) {
        return None;
    }
    let mut prefix = value.get(..10)?.to_string();
    prefix.replace_range(4..5, "-");
    prefix.replace_range(7..8, "-");
    let bytes = prefix.as_bytes();
    if bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        Some(format!("{}{}", prefix, value.get(10..).unwrap_or_default()))
    } else {
        None
    }
}

fn index_folder(
    conn: &mut Connection,
    folder: &Path,
    enable_xmp_sync: bool,
    force_metadata: bool,
) -> Result<u64, String> {
    if !folder.is_dir() {
        return Err(format!(
            "Catalog folder does not exist: {}",
            folder.display()
        ));
    }
    let folder_text = folder.to_string_lossy().into_owned();
    conn.execute(
        "INSERT OR IGNORE INTO catalog_folders(path,added_at) VALUES(?1,?2)",
        params![folder_text, unix_seconds(SystemTime::now())],
    )
    .map_err(|e| e.to_string())?;

    crate::exiftool::ensure_available()?;
    let mut seen = HashSet::new();
    let mut pending = Vec::with_capacity(INDEX_BATCH_SIZE);
    let mut indexed = 0u64;
    for entry in WalkDir::new(folder).follow_links(false).into_iter() {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !entry.file_type().is_file() || !is_supported_media_file(path) {
            continue;
        }
        let path_text = path.to_string_lossy().into_owned();
        seen.insert(path_text.clone());
        let file_meta = match fs::metadata(path) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let modified = file_meta.modified().map(unix_seconds).unwrap_or(0);
        let sidecar = get_primary_sidecar_path(path);
        let xmp_path = crate::file_management::resolve_xmp_path(path);
        let sidecar_modified = fs::metadata(&sidecar)
            .and_then(|m| m.modified())
            .map(unix_seconds)
            .unwrap_or(0)
            .max(
                xmp_path
                    .as_ref()
                    .and_then(|xmp| fs::metadata(xmp).ok())
                    .and_then(|m| m.modified().ok())
                    .map(unix_seconds)
                    .unwrap_or(0),
            );
        let existing: Option<(i64, i64)> = conn
            .query_row(
                "SELECT modified,sidecar_modified FROM catalog_images WHERE path=?1",
                [&path_text],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if !force_metadata && existing == Some((modified, sidecar_modified)) {
            continue;
        }

        let mut sidecar_data = load_sidecar(&sidecar);
        if enable_xmp_sync {
            crate::file_management::sync_metadata_from_xmp(path, &mut sidecar_data);
        }
        let exif = if is_supported_video_file(path) {
            HashMap::new()
        } else if let Some(exif) = sidecar_data.exif.clone().filter(|_| !force_metadata) {
            exif
        } else {
            read_exif_data(&path_text, &[])
        };
        let tags = sidecar_data.tags.unwrap_or_default();
        let color = tags
            .iter()
            .find_map(|tag| tag.strip_prefix("color:"))
            .map(str::to_owned);
        let flag = tags
            .iter()
            .find_map(|tag| tag.strip_prefix("flag:"))
            .map(str::to_owned);
        let camera = {
            let make = metadata_value(&exif, &["Make"]).unwrap_or("");
            let model = metadata_value(&exif, &["Model", "CameraModelName"]).unwrap_or("");
            let value = format!("{} {}", make, model).trim().to_string();
            (!value.is_empty()).then_some(value)
        };
        let lens = metadata_value(&exif, &["LensModel", "Lens", "LensInfo"]).map(str::to_owned);
        let date_taken = metadata_value(&exif, &["DateTimeOriginal", "CreateDate", "DateTime"])
            .and_then(normalized_capture_date)
            .or_else(|| {
                file_meta.modified().ok().map(|time| {
                    let date: DateTime<Utc> = time.into();
                    date.format("%Y-%m-%d %H:%M:%S").to_string()
                })
            });
        let focal =
            metadata_value(&exif, &["FocalLength", "FocalLengthIn35mmFilm"]).map(str::to_owned);
        let extension = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let filename = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
        use rusqlite::types::Value;
        let optional = |text: Option<String>| text.map(Value::from).unwrap_or(Value::Null);
        pending.push(vec![
            path_text.into(),
            folder_text.clone().into(),
            filename.to_owned().into(),
            extension.into(),
            modified.into(),
            sidecar_modified.into(),
            optional(date_taken),
            optional(camera),
            optional(lens),
            optional(focal),
            i64::from(sidecar_data.rating).into(),
            optional(color),
            optional(flag),
            serde_json::to_string(&tags)
                .map_err(|e| e.to_string())?
                .into(),
            serde_json::to_string(&exif.iter().collect::<BTreeMap<_, _>>())
                .map_err(|e| e.to_string())?
                .into(),
            i64::from(is_raw_file(path)).into(),
            i64::from(sidecar.exists()).into(),
            unix_seconds(SystemTime::now()).into(),
        ]);
        if pending.len() == INDEX_BATCH_SIZE {
            write_catalog_batch(conn, &mut pending, UPSERT_IMAGE)?;
        }
        indexed += 1;
    }
    write_catalog_batch(conn, &mut pending, UPSERT_IMAGE)?;

    let mut stale = conn
        .prepare("SELECT path FROM catalog_images WHERE folder_path=?1")
        .map_err(|e| e.to_string())?;
    let stale_paths: Vec<String> = stale
        .query_map([&folder_text], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|p| !seen.contains(p))
        .collect();
    drop(stale);
    for batch in stale_paths.chunks(INDEX_BATCH_SIZE) {
        let transaction = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        for path in batch {
            transaction
                .execute("DELETE FROM catalog_images WHERE path=?1", [path])
                .map_err(|e| e.to_string())?;
        }
        transaction.commit().map_err(|e| e.to_string())?;
    }
    Ok(indexed)
}

fn write_catalog_batch(
    conn: &mut Connection,
    rows: &mut Vec<Vec<rusqlite::types::Value>>,
    sql: &str,
) -> Result<u64, String> {
    if rows.is_empty() {
        return Ok(0);
    }
    let transaction = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let mut updated = 0;
    {
        let mut insert = transaction.prepare_cached(sql).map_err(|e| e.to_string())?;
        for row in rows.iter() {
            updated += insert
                .execute(params_from_iter(row))
                .map_err(|e| e.to_string())? as u64;
        }
    }
    transaction.commit().map_err(|e| e.to_string())?;
    rows.clear();
    Ok(updated)
}

fn scan_roots(folders: HashSet<PathBuf>) -> Vec<PathBuf> {
    let mut folders: Vec<_> = folders.into_iter().collect();
    folders.sort();
    let mut roots: Vec<PathBuf> = Vec::new();
    for folder in folders {
        if !roots.iter().any(|root| folder.starts_with(root)) {
            roots.push(folder);
        }
    }
    roots
}

fn status_from(conn: &Connection, app: &AppHandle) -> Result<CatalogStatus, String> {
    let folders = conn
        .prepare("SELECT path FROM catalog_folders ORDER BY path")
        .and_then(|mut s| {
            s.query_map([], |r| r.get(0))
                .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .map_err(|e| e.to_string())?;
    let image_count = conn
        .query_row("SELECT COUNT(*) FROM catalog_images", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(|e| e.to_string())?
        .max(0) as u64;
    Ok(CatalogStatus {
        database_path: db_path(app)?.to_string_lossy().into_owned(),
        folders,
        image_count,
    })
}

#[tauri::command]
pub fn catalog_status(app_handle: AppHandle) -> Result<CatalogStatus, String> {
    let conn = open_db(&app_handle)?;
    status_from(&conn, &app_handle)
}

#[tauri::command]
pub async fn catalog_add_folder(
    path: String,
    app_handle: AppHandle,
) -> Result<CatalogStatus, String> {
    let worker_app = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _scan = CATALOG_SCAN.lock().map_err(|e| e.to_string())?;
        let mut conn = open_db(&worker_app)?;
        let mut settings = load_settings(worker_app.clone()).unwrap_or_default();
        index_folder(
            &mut conn,
            Path::new(&path),
            settings.enable_xmp_sync.unwrap_or(false),
            false,
        )?;
        if !settings.catalog_folders.iter().any(|p| p == &path) {
            settings.catalog_folders.push(path);
        }
        save_settings(settings, worker_app.clone())?;
        status_from(&conn, &worker_app)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn catalog_import_paths_in_place(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(CatalogStatus, usize), String> {
    let worker_app = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _scan = CATALOG_SCAN.lock().map_err(|e| e.to_string())?;
        let mut folders = HashSet::new();
        let mut selected_paths = HashSet::new();
        let total_selected = paths.len();
        for virtual_path in paths {
            let (path, _) = crate::file_management::parse_virtual_path(&virtual_path);
            if !path.is_file() {
                return Err(format!("Source file not found: {}", path.display()));
            }
            if !is_supported_media_file(&path) {
                return Err(format!("Unsupported media file: {}", path.display()));
            }
            selected_paths.insert(
                path.canonicalize()
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .into_owned(),
            );
            let parent = path
                .parent()
                .ok_or_else(|| format!("Source folder not found: {}", path.display()))?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            folders.insert(parent);
        }

        let mut conn = open_db(&worker_app)?;
        let mut already_exists = total_selected - selected_paths.len();
        for path in &selected_paths {
            if conn
                .query_row(
                    "SELECT 1 FROM catalog_images WHERE path=?1",
                    [path],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .is_some()
            {
                already_exists += 1;
            }
        }
        let mut settings = load_settings(worker_app.clone()).unwrap_or_default();
        for folder in scan_roots(folders) {
            let folder_text = folder.to_string_lossy().into_owned();
            index_folder(
                &mut conn,
                &folder,
                settings.enable_xmp_sync.unwrap_or(false),
                false,
            )?;
            if !settings
                .catalog_folders
                .iter()
                .any(|path| path == &folder_text)
            {
                settings.catalog_folders.push(folder_text);
            }
        }
        save_settings(settings, worker_app.clone())?;
        Ok((status_from(&conn, &worker_app)?, already_exists))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn catalog_remove_folder(path: String, app_handle: AppHandle) -> Result<CatalogStatus, String> {
    let conn = open_db(&app_handle)?;
    conn.execute("DELETE FROM catalog_images WHERE folder_path=?1", [&path])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM catalog_folders WHERE path=?1", [&path])
        .map_err(|e| e.to_string())?;
    let mut settings = load_settings(app_handle.clone()).unwrap_or_default();
    settings.catalog_folders.retain(|p| p != &path);
    save_settings(settings, app_handle.clone())?;
    status_from(&conn, &app_handle)
}

#[tauri::command]
pub fn catalog_remove_images(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<CatalogStatus, String> {
    let mut conn = open_db(&app_handle)?;
    let transaction = conn.transaction().map_err(|e| e.to_string())?;
    for virtual_path in paths {
        let (physical_path, _) = crate::file_management::parse_virtual_path(&virtual_path);
        transaction
            .execute(
                "DELETE FROM catalog_images WHERE path=?1 OR path=?2",
                params![virtual_path, physical_path.to_string_lossy()],
            )
            .map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())?;
    status_from(&conn, &app_handle)
}

#[tauri::command]
pub fn catalog_refresh_paths(paths: Vec<String>, app_handle: AppHandle) -> Result<u64, String> {
    let mut conn = open_db(&app_handle)?;
    let mut pending = Vec::with_capacity(INDEX_BATCH_SIZE);
    let sql = "UPDATE catalog_images SET rating=?1,color=?2,flag=?3,tags_json=?4,sidecar_modified=?5,is_edited=?6,indexed_at=?7 WHERE path=?8";
    let mut updated = 0u64;
    for virtual_path in paths {
        let (path, _) = crate::file_management::parse_virtual_path(&virtual_path);
        let path_text = path.to_string_lossy().into_owned();
        let sidecar = get_primary_sidecar_path(&path);
        let sidecar_data = load_sidecar(&sidecar);
        let tags = sidecar_data.tags.unwrap_or_default();
        let color = tags
            .iter()
            .find_map(|tag| tag.strip_prefix("color:"))
            .map(str::to_owned);
        let flag = tags
            .iter()
            .find_map(|tag| tag.strip_prefix("flag:"))
            .map(str::to_owned);
        let sidecar_modified = fs::metadata(&sidecar)
            .and_then(|m| m.modified())
            .map(unix_seconds)
            .unwrap_or(0);
        use rusqlite::types::Value;
        pending.push(vec![
            i64::from(sidecar_data.rating).into(),
            color.map(Value::from).unwrap_or(Value::Null),
            flag.map(Value::from).unwrap_or(Value::Null),
            serde_json::to_string(&tags)
                .map_err(|e| e.to_string())?
                .into(),
            sidecar_modified.into(),
            i64::from(sidecar.exists()).into(),
            unix_seconds(SystemTime::now()).into(),
            path_text.into(),
        ]);
        if pending.len() == INDEX_BATCH_SIZE {
            updated += write_catalog_batch(&mut conn, &mut pending, sql)?;
        }
    }
    updated += write_catalog_batch(&mut conn, &mut pending, sql)?;
    Ok(updated)
}

#[tauri::command]
pub async fn catalog_rebuild(app_handle: AppHandle) -> Result<CatalogStatus, String> {
    let worker_app = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _scan = CATALOG_SCAN.lock().map_err(|e| e.to_string())?;
        let folders = load_settings(worker_app.clone())
            .unwrap_or_default()
            .catalog_folders;
        let enable_xmp_sync = load_settings(worker_app.clone())
            .unwrap_or_default()
            .enable_xmp_sync
            .unwrap_or(false);
        let mut conn = open_db(&worker_app)?;
        for folder in scan_roots(folders.into_iter().map(PathBuf::from).collect()) {
            index_folder(&mut conn, &folder, enable_xmp_sync, true)
                .map_err(|e| format!("Catalog rescan failed for {}: {e}", folder.display()))?;
        }
        status_from(&conn, &worker_app)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPage {
    images: Vec<CatalogImage>,
    total: u64,
}

#[tauri::command]
pub async fn catalog_query_page(
    filter: CatalogFilter,
    app_handle: AppHandle,
) -> Result<CatalogPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = open_db(&app_handle)?;
        let transaction = conn.transaction().map_err(|e| e.to_string())?;
        let (sql, values) = filtered_query(filter.clone(), true)?;
        let total = transaction
            .query_row(&sql, params_from_iter(values.iter().map(|v| &**v)), |row| {
                row.get::<_, i64>(0).map(|count| count.max(0) as u64)
            })
            .map_err(|e| e.to_string())?;
        let images = query_from_connection(&transaction, filter)?;
        Ok(CatalogPage { images, total })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn query_from_connection(
    conn: &Connection,
    filter: CatalogFilter,
) -> Result<Vec<CatalogImage>, String> {
    let (sql, values) = filtered_query(filter, false)?;
    read_query(conn, sql, values)
}

fn filtered_query(
    filter: CatalogFilter,
    count_only: bool,
) -> Result<(String, Vec<Box<dyn ToSql>>), String> {
    let mut sql = String::from(if count_only {
        "SELECT COUNT(*) FROM catalog_images WHERE 1=1"
    } else {
        "SELECT path,modified,rating,tags_json,exif_json,is_raw,is_edited FROM catalog_images WHERE 1=1"
    });
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();
    if let Some(v) = filter.text.filter(|v| !v.trim().is_empty()) {
        let pattern = format!("%{}%", v);
        sql.push_str(" AND (filename LIKE ? OR tags_json LIKE ?)");
        values.push(Box::new(pattern.clone()));
        values.push(Box::new(pattern));
    }
    let mut add = |clause: &str, value: String| {
        sql.push_str(clause);
        values.push(Box::new(value));
    };
    if let Some(v) = filter.camera.filter(|v| !v.is_empty()) {
        add(" AND camera=?", v);
    }
    if let Some(v) = filter.lens.filter(|v| !v.is_empty()) {
        add(" AND lens=?", v);
    }
    if let Some(v) = filter.date.filter(|v| !v.is_empty()) {
        add(
            " AND COALESCE(NULLIF(replace(substr(date_taken,1,10), ':', '-'), ''), strftime('%Y-%m-%d', modified, 'unixepoch')) LIKE ?",
            format!("{}%", v),
        );
    }
    if let Some(v) = filter.tag.filter(|v| !v.is_empty()) {
        add(" AND tags_json LIKE ?", format!("%\"user:{}\"%", v));
    }
    if let Some(v) = filter.color.filter(|v| !v.is_empty()) {
        add(" AND color=?", v);
    }
    if let Some(v) = filter.flag.filter(|v| !v.is_empty()) {
        add(" AND COALESCE(NULLIF(flag, ''), 'unflagged')=?", v);
    }
    if let Some(v) = filter.file_type.filter(|v| !v.is_empty()) {
        add(" AND extension=?", v);
    }
    drop(add);
    for (column, selections) in [
        ("camera", &filter.cameras),
        ("lens", &filter.lenses),
        ("color", &filter.colors),
        ("extension", &filter.file_types),
    ] {
        if !selections.is_empty() {
            sql.push_str(&format!(
                " AND {column} IN (SELECT value FROM json_each(?))"
            ));
            values.push(Box::new(
                serde_json::to_string(selections).map_err(|e| e.to_string())?,
            ));
        }
    }
    if !filter.ratings.is_empty() {
        sql.push_str(" AND rating IN (SELECT value FROM json_each(?))");
        values.push(Box::new(
            serde_json::to_string(&filter.ratings).map_err(|e| e.to_string())?,
        ));
    }
    if !filter.tags.is_empty() {
        sql.push_str(" AND EXISTS (SELECT 1 FROM json_each(catalog_images.tags_json) AS tag WHERE tag.value IN (SELECT 'user:' || value FROM json_each(?)))");
        values.push(Box::new(
            serde_json::to_string(&filter.tags).map_err(|e| e.to_string())?,
        ));
    }
    if let Some(v) = filter.minimum_rating {
        sql.push_str(if v == 0 {
            " AND rating=?"
        } else {
            " AND rating>=?"
        });
        values.push(Box::new(v));
    }
    if count_only {
        return Ok((sql, values));
    }
    let sort_column = match filter.sort_key.as_deref() {
        Some("name") => "filename COLLATE NOCASE",
        Some("date") => "modified",
        Some("rating") => "rating",
        Some("edited") => "is_edited",
        Some("iso") => {
            "CAST(COALESCE(NULLIF(json_extract(exif_json, '$.ISO'), 65535), json_extract(exif_json, '$.PhotographicSensitivity'), json_extract(exif_json, '$.ISOSpeedRatings'), json_extract(exif_json, '$.RecommendedExposureIndex'), json_extract(exif_json, '$.StandardOutputSensitivity')) AS REAL)"
        }
        Some("focal_length") => "CAST(focal_length AS REAL)",
        Some("aperture") => "CAST(json_extract(exif_json, '$.FNumber') AS REAL)",
        _ => "date_taken",
    };
    let direction = if filter.sort_descending.unwrap_or(true) {
        "DESC"
    } else {
        "ASC"
    };
    sql.push_str(&format!(
        " ORDER BY {sort_column} {direction}, path ASC LIMIT ? OFFSET ?"
    ));
    values.push(Box::new(filter.limit.unwrap_or(501).min(5_001)));
    values.push(Box::new(filter.offset.unwrap_or(0)));
    Ok((sql, values))
}

fn read_query(
    conn: &Connection,
    sql: String,
    values: Vec<Box<dyn ToSql>>,
) -> Result<Vec<CatalogImage>, String> {
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(values.iter().map(|v| &**v)), |row| {
            let tags_json: String = row.get(3)?;
            let exif_json: String = row.get(4)?;
            let modified = row.get::<_, i64>(1)?.max(0) as u64;
            Ok(CatalogImage {
                path: row.get(0)?,
                modified,
                rating: row.get(2)?,
                tags: serde_json::from_str(&tags_json).ok(),
                exif: serde_json::from_str(&exif_json).ok(),
                is_raw: row.get(5)?,
                is_edited: row.get(6)?,
                is_virtual_copy: false,
                is_cloud_placeholder: false,
                group_id: None,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(Result::ok).collect())
}

fn distinct(conn: &Connection, column: &str) -> Result<Vec<String>, String> {
    let sql = format!(
        "SELECT DISTINCT {column} FROM catalog_images WHERE {column} IS NOT NULL AND {column}<>'' ORDER BY {column}"
    );
    conn.prepare(&sql)
        .and_then(|mut s| {
            s.query_map([], |r| r.get(0))
                .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn catalog_facets(app_handle: AppHandle) -> Result<CatalogFacets, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open_db(&app_handle)?;
        facets_from_connection(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn facets_from_connection(conn: &Connection) -> Result<CatalogFacets, String> {
    // Deduplicate in SQLite instead of allocating every image's tags in Rust.
    let tags: Vec<String> = conn.prepare(
        "SELECT DISTINCT substr(tag.value,6) FROM catalog_images, json_each(catalog_images.tags_json) AS tag
         WHERE tag.type='text' AND substr(tag.value,1,5)='user:' AND length(tag.value)>5 ORDER BY 1"
    ).and_then(|mut stmt| stmt.query_map([], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()).map_err(|e| e.to_string())?;
    let date_days: Vec<CatalogDateCount> = conn
        .prepare(
            "SELECT COALESCE(NULLIF(replace(substr(date_taken,1,10), ':', '-'), ''), strftime('%Y-%m-%d', modified, 'unixepoch')) AS catalog_date, COUNT(*)
             FROM catalog_images
             WHERE COALESCE(NULLIF(replace(substr(date_taken,1,10), ':', '-'), ''), strftime('%Y-%m-%d', modified, 'unixepoch')) IS NOT NULL
             GROUP BY catalog_date ORDER BY catalog_date DESC",
        )
        .and_then(|mut statement| {
            statement
                .query_map([], |row| {
                    Ok(CatalogDateCount {
                        date: row.get(0)?,
                        count: row.get::<_, i64>(1)?.max(0) as u64,
                    })
                })
                .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|entry| {
            let bytes = entry.date.as_bytes();
            bytes.len() == 10
                && bytes[4] == b'-'
                && bytes[7] == b'-'
                && bytes
                    .iter()
                    .enumerate()
                    .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
        })
        .collect();
    let mut dates: Vec<String> = date_days
        .iter()
        .filter_map(|entry| entry.date.get(0..4).map(str::to_owned))
        .collect();
    dates.sort_unstable();
    dates.dedup();
    dates.reverse();
    Ok(CatalogFacets {
        cameras: distinct(&conn, "camera")?,
        lenses: distinct(&conn, "lens")?,
        dates,
        tags,
        colors: distinct(&conn, "color")?,
        file_types: distinct(&conn, "extension")?,
        date_days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sixty_thousand_files_commit_and_rescan_with_a_concurrent_writer() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let directory = tempfile::tempdir().unwrap();
        let photos = directory.path().join("media");
        fs::create_dir(&photos).unwrap();
        for index in 0..60_000 {
            fs::write(photos.join(format!("{index:05}.mp4")), []).unwrap();
        }
        let database = directory.path().join("catalog.db");
        let mut conn = open_db_path(&database).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let writer = std::thread::spawn(move || {
            let conn = Connection::open(database).unwrap();
            conn.busy_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            let mut writes = 0;
            while !worker_stop.load(Ordering::Relaxed) {
                conn.execute("INSERT INTO catalog_meta(key,value) VALUES('heartbeat','1') ON CONFLICT(key) DO UPDATE SET value=value+1", []).unwrap();
                writes += 1;
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            writes
        });
        let imported = index_folder(&mut conn, &photos, false, false);
        stop.store(true, Ordering::Relaxed);
        assert!(writer.join().unwrap() > 0);
        assert_eq!(imported.unwrap(), 60_000);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM catalog_images", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            60_000
        );
        assert_eq!(index_folder(&mut conn, &photos, false, false).unwrap(), 0);
        fs::remove_file(photos.join("00000.mp4")).unwrap();
        index_folder(&mut conn, &photos, false, true).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM catalog_images", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            59_999
        );
        assert_eq!(
            scan_roots(
                [photos.clone(), photos.join("child"), photos]
                    .into_iter()
                    .collect()
            )
            .len(),
            1
        );
    }
    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE catalog_images(
          path TEXT, filename TEXT, extension TEXT, modified INTEGER, date_taken TEXT,
          camera TEXT, lens TEXT, focal_length TEXT, rating INTEGER, color TEXT, flag TEXT,
          tags_json TEXT, exif_json TEXT, is_raw INTEGER, is_edited INTEGER);
          INSERT INTO catalog_images VALUES
          ('a','a','nef',0,'2008:06:21 12:00:00','Nikon','Zoom','50',0,'red',NULL,'[\"user:Landscape\"]','{}',1,0),
          ('b','b','jpg',0,'2008-06-21','Canon','Prime','35',5,'blue',NULL,'[\"user:Family\"]','{}',0,0),
          ('c','c','nef',0,'2009-01-01','Nikon','Prime','35',4,'red',NULL,'[\"user:Landscape_extra\"]','{}',1,0),
          ('d','d','jpg',0,NULL,'Sony','Zoom','50',5,'blue',NULL,'[]','{}',0,0);").unwrap();
        conn
    }
    #[test]
    fn multi_selections_are_or_within_and_between() {
        let rows = query_from_connection(
            &fixture(),
            CatalogFilter {
                cameras: vec!["Nikon".into(), "Canon".into()],
                lenses: vec!["Zoom".into(), "Prime".into()],
                ratings: vec![0, 5],
                tags: vec!["Landscape".into(), "Family".into()],
                colors: vec!["red".into(), "blue".into()],
                file_types: vec!["nef".into(), "jpg".into()],
                sort_key: Some("name".into()),
                sort_descending: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }
    #[test]
    fn flags_filter_with_metadata_and_unflagged() {
        let conn = fixture();
        conn.execute("UPDATE catalog_images SET flag='pick' WHERE path='a'", [])
            .unwrap();
        conn.execute("UPDATE catalog_images SET flag='reject' WHERE path='b'", [])
            .unwrap();
        for (flag, expected) in [
            ("pick", vec!["a"]),
            ("reject", vec!["b"]),
            ("unflagged", vec!["c", "d"]),
        ] {
            let rows = query_from_connection(
                &conn,
                CatalogFilter {
                    flag: Some(flag.into()),
                    sort_key: Some("name".into()),
                    sort_descending: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                rows.iter()
                    .map(|image| image.path.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
        }
        assert!(
            query_from_connection(
                &conn,
                CatalogFilter {
                    flag: Some("reject".into()),
                    cameras: vec!["Nikon".into()],
                    ..Default::default()
                }
            )
            .unwrap()
            .is_empty()
        );
    }
    #[test]
    fn tags_match_exactly_and_zero_does_not_mean_all() {
        let rows = query_from_connection(
            &fixture(),
            CatalogFilter {
                tags: vec!["Landscape".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "a");
        let rows = query_from_connection(
            &fixture(),
            CatalogFilter {
                ratings: vec![0],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rating, 0);
    }
    #[test]
    fn page_boundaries_and_legacy_smart_groups() {
        let conn = fixture();
        let page = |offset| {
            query_from_connection(
                &conn,
                CatalogFilter {
                    limit: Some(2),
                    offset: Some(offset),
                    sort_key: Some("name".into()),
                    sort_descending: Some(false),
                    ..Default::default()
                },
            )
            .unwrap()
            .into_iter()
            .map(|r| r.path)
            .collect::<Vec<_>>()
        };
        assert_eq!(page(0), ["a", "b"]);
        assert_eq!(page(2), ["c", "d"]);
        assert!(page(4).is_empty());
        let legacy: CatalogFilter =
            serde_json::from_str(r#"{"camera":"Nikon","minimumRating":4}"#).unwrap();
        assert_eq!(query_from_connection(&conn, legacy).unwrap()[0].path, "c");
    }
    #[test]
    fn legacy_dates_and_missing_dates_have_facets() {
        let facets = facets_from_connection(&fixture()).unwrap();
        assert_eq!(facets.tags, ["Family", "Landscape", "Landscape_extra"]);
        assert!(
            facets
                .date_days
                .iter()
                .any(|d| d.date == "2008-06-21" && d.count == 2)
        );
        assert!(
            facets
                .date_days
                .iter()
                .any(|d| d.date == "1970-01-01" && d.count == 1)
        );
    }

    #[test]
    fn result_count_uses_filters_but_not_page_limits() {
        let conn = fixture();
        let filter = CatalogFilter {
            cameras: vec!["Nikon".into()],
            limit: Some(1),
            offset: Some(1),
            ..Default::default()
        };
        let (sql, values) = filtered_query(filter.clone(), true).unwrap();
        let total: i64 = conn
            .query_row(&sql, params_from_iter(values.iter().map(|v| &**v)), |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(total, 2);
        assert_eq!(query_from_connection(&conn, filter).unwrap().len(), 1);
        let filter = CatalogFilter {
            ratings: vec![0],
            tags: vec!["Landscape".into()],
            ..Default::default()
        };
        let (sql, values) = filtered_query(filter, true).unwrap();
        let total: i64 = conn
            .query_row(&sql, params_from_iter(values.iter().map(|v| &**v)), |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(total, 1);
    }

    #[test]
    fn malformed_capture_dates_do_not_panic() {
        assert_eq!(
            normalized_capture_date("2008:06:21 12:00:00").as_deref(),
            Some("2008-06-21 12:00:00")
        );
        assert_eq!(normalized_capture_date("ååååå"), None);
        assert_eq!(normalized_capture_date("not-a-date"), None);
    }

    #[test]
    fn schema_migration_preserves_rows_sorts_exif_and_constrains_flags() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE catalog_meta(key TEXT PRIMARY KEY,value TEXT);
            INSERT INTO catalog_meta VALUES('schema_version','1');
            CREATE TABLE catalog_images(path TEXT PRIMARY KEY,exif_json TEXT,is_raw INTEGER,is_edited INTEGER,thumbnail_status INTEGER);
            INSERT INTO catalog_images VALUES('photo','{\"Z\":\"last\",\"A\":\"first\"}',0,1,0);").unwrap();
        migrate_catalog_schema(&conn).unwrap();
        let exif: String = conn
            .query_row(
                "SELECT exif_json FROM catalog_images WHERE path='photo'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exif, r#"{"A":"first","Z":"last"}"#);
        assert!(
            conn.prepare("SELECT thumbnail_status FROM catalog_images")
                .is_err()
        );
        for value in ["2", "-1", "0.5", "NULL", "'invalid'"] {
            for column in ["is_raw", "is_edited"] {
                assert!(
                    conn.execute_batch(&format!("UPDATE catalog_images SET {column}={value}"))
                        .is_err()
                );
            }
        }
        assert!(
            conn.execute_batch("INSERT INTO catalog_images VALUES('bad','{}',2,0)")
                .is_err()
        );
        conn.execute_batch("UPDATE catalog_images SET is_raw=1,is_edited=0")
            .unwrap();
        migrate_catalog_schema(&conn).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM catalog_images", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
