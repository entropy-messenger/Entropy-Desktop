#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod commands;
mod protocol;
#[cfg(test)]
mod tests;

use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Manager,
};
use app_state::{DbState, NetworkState};

fn main() {
    tauri::Builder::default()
        .manage(DbState {
            conn: Mutex::new(None),
        })
        .manage(NetworkState {
            sender: Mutex::new(None),
        })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::store_secret,
            commands::get_secret,
            commands::init_vault,
            commands::vault_save,
            commands::vault_load,
            commands::dump_vault,
            commands::restore_vault,
            commands::protocol_init,
            commands::protocol_establish_session,
            commands::protocol_encrypt,
            commands::protocol_decrypt,
            commands::protocol_encrypt_media,
            commands::protocol_decrypt_media,
            commands::protocol_get_pending,
            commands::protocol_save_pending,
            commands::protocol_remove_pending,
            commands::protocol_verify_session,
            commands::protocol_group_init,
            commands::protocol_group_encrypt,
            commands::protocol_group_decrypt,
            commands::connect_network,
            commands::send_to_network,
            commands::get_link_preview,
            commands::protocol_import_vault,
            commands::protocol_export_vault,
            commands::nuclear_reset,
            commands::clear_vault,
            commands::protocol_save_message,
            commands::protocol_search_messages,
            commands::protocol_sign,
            commands::protocol_get_identity_key,
            commands::protocol_blob_put,
            commands::protocol_blob_get,
            commands::protocol_blob_delete
        ])
        .setup(|app| {
            let quit_i = MenuItem::with_id(app, "quit", "Quit Entropy", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        let window = app.get_webview_window("main").unwrap();
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
