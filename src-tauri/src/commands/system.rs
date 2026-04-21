use crate::app_state::DbState;
use tauri::State;

#[tauri::command]
pub fn open_file(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    path: String,
) -> Result<(), String> {
    // resolve absolute path and prevent traversal
    let path_buf = std::path::PathBuf::from(&path);
    let canonical_path = std::fs::canonicalize(&path_buf)
        .map_err(|e| format!("Invalid or inaccessible path: {}", e))?;

    // No longer strictly enforcing vault boundary for open_file, 
    // as users need to open files they've exported to their local filesystem.
    // The hidden file check below still provides a baseline security layer.

    // reject hidden files
    if canonical_path
        .file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
    {
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
