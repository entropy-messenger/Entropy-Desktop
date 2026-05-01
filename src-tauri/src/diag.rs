use tauri::Manager;
pub fn print_paths(app: &tauri::App) {
    if let Ok(path) = app.path().app_data_dir() {

    } else {

    }
}
