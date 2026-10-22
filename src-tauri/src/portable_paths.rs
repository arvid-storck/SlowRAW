use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

fn is_portable_directory(root: &Path) -> bool {
    root.join("portable.flag").is_file()
        || fs::read_to_string(root.join("README.txt")).is_ok_and(|text| {
            text.starts_with("RapidRAW portable") || text.starts_with("SlowRAW portable")
        })
}

/// A marker is required so an installed build never writes beside its executable.
pub fn portable_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let root = std::env::current_exe().ok()?.parent()?.to_path_buf();
        if is_portable_directory(&root) {
            return Some(root);
        }
    }
    None
}

pub fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = match portable_root() {
        Some(root) => root.join("data"),
        None => app.path().app_data_dir().map_err(|e| e.to_string())?,
    };
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Cannot create app data directory {}: {e}", dir.display()))?;
    Ok(dir)
}

pub fn app_config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = match portable_root() {
        Some(root) => root.join("data").join("config"),
        None => app.path().app_config_dir().map_err(|e| e.to_string())?,
    };
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Cannot create app config directory {}: {e}", dir.display()))?;
    Ok(dir)
}

fn legacy_directory(current: &Path) -> PathBuf {
    current.with_file_name("io.github.CyberTimon.RapidRAW")
}

fn copy_missing_tree(source: &Path, destination: &Path) -> Result<(), String> {
    if source.is_file() {
        if !destination.exists() {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            if source.extension().is_some_and(|ext| ext == "db") {
                let connection = rusqlite::Connection::open_with_flags(
                    source,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .map_err(|e| e.to_string())?;
                if let Err(error) = connection.execute(
                    "VACUUM main INTO ?1",
                    [destination.to_string_lossy().as_ref()],
                ) {
                    let _ = fs::remove_file(destination);
                    return Err(format!(
                        "Cannot migrate catalog {}: {error}",
                        source.display()
                    ));
                }
            } else {
                fs::copy(source, destination)
                    .map_err(|e| format!("Cannot copy {}: {e}", source.display()))?;
            }
        }
    } else if source.is_dir() {
        fs::create_dir_all(destination).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            let name_text = name.to_string_lossy();
            if matches!(name_text.as_ref(), "logs" | "luts" | "models" | "cache")
                || name_text.ends_with("-wal")
                || name_text.ends_with("-shm")
            {
                continue;
            }
            let file_type = entry.file_type().map_err(|e| e.to_string())?;
            if file_type.is_file() || file_type.is_dir() {
                copy_missing_tree(&entry.path(), &destination.join(entry.file_name()))?;
            }
        }
    }
    Ok(())
}

/// Copies installed data once per destination while preserving existing files.
pub fn migrate_legacy_app_data(app: &AppHandle) -> Result<(), String> {
    if portable_root().is_some() {
        return migrate_legacy_portable_data(app);
    }
    for destination in [
        app.path().app_data_dir(),
        app.path().app_config_dir(),
        app.path().app_local_data_dir(),
    ] {
        let destination = destination.map_err(|e| e.to_string())?;
        let source = legacy_directory(&destination);
        let marker = destination.join(".legacy-id-migrated");
        if source.is_dir() && !marker.exists() {
            copy_missing_tree(&source, &destination)?;
            fs::write(marker, b"").map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// One-time, non-destructive migration for older portable copies that stored
/// persistent data in the system app-data directory. Fresh ZIPs start clean.
pub fn migrate_legacy_portable_data(app: &AppHandle) -> Result<(), String> {
    let Some(root) = portable_root() else {
        return Ok(());
    };
    let destination = app_data_dir(app)?;
    let migration_marker = destination.join(".legacy-data-checked");
    if migration_marker.exists() {
        return Ok(());
    }

    if root.join("rapidraw.db").exists() || !root.join("portable.flag").exists() {
        let source = legacy_directory(&app.path().app_data_dir().map_err(|e| e.to_string())?);
        for name in ["settings.json", "albums", "library"] {
            copy_missing_tree(&source.join(name), &destination.join(name))?;
        }
        let legacy_config =
            legacy_directory(&app.path().app_config_dir().map_err(|e| e.to_string())?);
        copy_missing_tree(
            &legacy_config.join("window_state.json"),
            &destination.join("config").join("window_state.json"),
        )?;
    }

    fs::write(&migration_marker, b"").map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_detection_requires_marker_or_legacy_readme() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_portable_directory(dir.path()));
        fs::write(dir.path().join("portable.flag"), b"").unwrap();
        assert!(is_portable_directory(dir.path()));
        fs::remove_file(dir.path().join("portable.flag")).unwrap();
        fs::write(dir.path().join("README.txt"), "RapidRAW portable\n").unwrap();
        assert!(is_portable_directory(dir.path()));
    }

    #[test]
    fn migration_snapshots_committed_wal_data() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("legacy.db");
        let destination = dir.path().join("current.db");
        let connection = rusqlite::Connection::open(&source).unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE images (path TEXT); INSERT INTO images VALUES ('photo.raw');").unwrap();
        copy_missing_tree(&source, &destination).unwrap();
        let snapshot = rusqlite::Connection::open(&destination).unwrap();
        assert_eq!(
            snapshot
                .query_row("SELECT path FROM images", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "photo.raw"
        );
        connection
            .execute("INSERT INTO images VALUES ('another.raw')", [])
            .unwrap();
        copy_missing_tree(&source, &destination).unwrap();
        assert_eq!(
            snapshot
                .query_row("SELECT COUNT(*) FROM images", [], |row| row
                    .get::<_, i32>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn migration_copy_never_overwrites_existing_data() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let destination = dir.path().join("destination");
        fs::create_dir_all(source.join("albums")).unwrap();
        fs::create_dir_all(destination.join("albums")).unwrap();
        fs::write(source.join("albums/albums.json"), "old").unwrap();
        fs::write(source.join("settings.json"), "settings").unwrap();
        for name in ["models", "cache"] {
            fs::create_dir_all(source.join(name)).unwrap();
            fs::write(source.join(name).join("obsolete.bin"), b"cache").unwrap();
        }
        fs::write(destination.join("albums/albums.json"), "new").unwrap();
        copy_missing_tree(&source, &destination).unwrap();
        assert!(!destination.join("models").exists());
        assert!(!destination.join("cache").exists());
        assert_eq!(
            fs::read_to_string(destination.join("albums/albums.json")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(destination.join("settings.json")).unwrap(),
            "settings"
        );
    }
}
