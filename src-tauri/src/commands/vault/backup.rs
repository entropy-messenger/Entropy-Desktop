use crate::app_state::DbState;
use crate::commands::{get_db_filename, get_media_dirname};
use tauri::{Manager, State};
use walkdir::WalkDir;
use zip::write::FileOptions;

#[tauri::command]
pub async fn export_database(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    target_path: String,
) -> Result<(), String> {
    {
        if let Ok(conn) = state.get_conn() {
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(|e| format!("Failed to checkpoint DB: {}", e))?;
        }
    }

    let filename = get_db_filename();
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let src_path = app_dir.join(&filename);

    let target_path_buf = std::path::PathBuf::from(&target_path);

    // Prevent overwriting critical system files/dotfiles
    if let Some(target_dir) = target_path_buf.parent()
        && let Ok(abs_target) = std::fs::canonicalize(target_dir)
    {
        let app_dir_canonical = std::fs::canonicalize(&app_dir).unwrap_or(app_dir.clone());
        if abs_target == app_dir_canonical {
            return Err("Cannot export directly into the application data directory".into());
        }
    }

    if target_path_buf
        .file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
    {
        return Err("Cannot export to a hidden file".into());
    }

    let file = std::fs::File::create(&target_path_buf).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    let mut f = std::fs::File::open(&src_path)
        .map_err(|e| format!("Failed to open DB for export: {}", e))?;
    zip.start_file(filename, options)
        .map_err(|e| e.to_string())?;
    std::io::copy(&mut f, &mut zip).map_err(|e| format!("Failed to stream DB to zip: {}", e))?;

    // Include the unique salt in the backup
    let salt_path = app_dir.join("vault.salt");
    if salt_path.exists() {
        let mut sf = std::fs::File::open(&salt_path).map_err(|e| e.to_string())?;
        zip.start_file("vault.salt", options)
            .map_err(|e| e.to_string())?;
        std::io::copy(&mut sf, &mut zip).map_err(|e| e.to_string())?;
    }

    let media_dir_name = get_media_dirname();
    let media_path = app_dir.join(&media_dir_name);
    if media_path.exists() {
        let walker = WalkDir::new(&media_path).into_iter();
        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = path
                .strip_prefix(&app_dir)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned();

            if path.is_file() {
                zip.start_file(name, options).map_err(|e| e.to_string())?;
                let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
                std::io::copy(&mut f, &mut zip).map_err(|e| e.to_string())?;
            } else if !name.is_empty() {
                zip.add_directory(name, options)
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn import_database(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    src_path: String,
) -> Result<(), String> {
    {
        let mut pool_lock = state.pool.lock().map_err(|_| "Database connection lock poisoned")?;
        *pool_lock = None;
    }

    let backup_path = std::path::Path::new(&src_path);
    let extension = backup_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if extension != "entropy" && extension != "zip" {
        return Err("Invalid backup file: Must be a .entropy or .zip archive".into());
    }

    if !backup_path.exists() {
        return Err("Selected backup file does not exist".to_string());
    }

    let filename = get_db_filename();
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let dest_path = app_dir.join(&filename);
    let wal_path = app_dir.join(format!("{}-wal", filename));
    let shm_path = app_dir.join(format!("{}-shm", filename));

    if dest_path.exists() {
        std::fs::remove_file(&dest_path).map_err(|e| e.to_string())?;
    }
    if wal_path.exists() {
        let _ = std::fs::remove_file(wal_path);
    }
    if shm_path.exists() {
        let _ = std::fs::remove_file(shm_path);
    }

    let media_dir_name = get_media_dirname();
    let media_path = app_dir.join(&media_dir_name);
    if media_path.exists() {
        std::fs::remove_dir_all(&media_path).map_err(|e| e.to_string())?;
    }

    let src_path_buf = std::path::PathBuf::from(&src_path);
    let canonical_src = std::fs::canonicalize(&src_path_buf)
        .map_err(|e| format!("Invalid or blocked backup path: {}", e))?;

    // Prevent importing from hidden files or internal app data
    if canonical_src
        .file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
    {
        return Err("Cannot import from a hidden file".into());
    }

    if let Ok(abs_app_dir) = std::fs::canonicalize(&app_dir)
        && canonical_src.starts_with(&abs_app_dir)
    {
        return Err("Cannot import from within the application data directory".into());
    }

    let file = std::fs::File::open(&canonical_src).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;

        let mut name = file.name().to_string();

        // Prevent Path Traversal
        if name.contains("..") || name.starts_with('/') || name.contains(':') {
            continue; 
        }

        // Ensure backups from other profiles map to the ACTIVE profile
        if name.starts_with("entropy") && name.ends_with(".db") {
            name = filename.clone();
        } else if name.starts_with("media") {
            if let Some(pos) = name.find('/') {
                name = format!("{}{}", media_dir_name, &name[pos..]);
            } else {
                name = media_dir_name.clone() + "/";
            }
        }

        let outpath = app_dir.join(name);

        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = outpath.parent()
                && !p.exists()
            {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut outfile = std::fs::File::create(&outpath).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
            outfile.sync_all().map_err(|e| e.to_string())?;
        }
    }

    app.restart();

    #[allow(unreachable_code)]
    Ok(())
}
