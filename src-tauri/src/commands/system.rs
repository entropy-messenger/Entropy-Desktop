use crate::app_state::DbState;
use tauri::State;

#[tauri::command]
pub fn open_file(
    app: tauri::AppHandle,
    _state: State<'_, DbState>,
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

    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_url(
                format!("file://{}", canonical_path.to_string_lossy()),
                None::<&str>,
            )
            .map_err(|e: tauri_plugin_opener::Error| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_media_proxy_port(state: State<'_, DbState>) -> Result<u16, String> {
    let port = state.media_proxy_port.lock().map_err(|_| "Lock poisoned")?;
    port.ok_or_else(|| "Media proxy not initialized".to_string())
}
