#[tauri::command]
pub fn open_file(path: String) -> Result<(), String> {
    // 🛡️ SECURITY: Prevent shell injection by validating the path
    if path.contains(';') || path.contains('&') || path.contains('|') || path.contains('$') || path.contains('`') {
        return Err("Invalid characters in path".into());
    }

    let path_buf = std::path::PathBuf::from(&path);
    if !path_buf.exists() {
        return Err("File does not exist".into());
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
