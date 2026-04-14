use crate::app_state::DbState;
use tauri::State;

#[tauri::command]
pub fn open_file(app: tauri::AppHandle, state: State<'_, DbState>, path: String) -> Result<(), String> {
    // resolve absolute path and prevent traversal
    let path_buf = std::path::PathBuf::from(&path);
    let canonical_path = std::fs::canonicalize(&path_buf)
        .map_err(|e| format!("Invalid or inaccessible path: {}", e))?;

    // bound enforcement: restrict access to application media vault
    let media_dir = crate::commands::vault::media::get_media_dir(&app, &state)?;
    let canonical_media = std::fs::canonicalize(&media_dir)
        .map_err(|_| "Media vault not initialized".to_string())?;

    if !canonical_path.starts_with(&canonical_media) {
        return Err("Access denied: You can only open files stored in your media vault".into());
    }

    // reject hidden files
    if canonical_path.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(false) {
        return Err("Access to hidden files is denied".into());
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&canonical_path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&canonical_path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&canonical_path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
