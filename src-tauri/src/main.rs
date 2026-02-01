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
            commands::crypto_sha256,
            commands::crypto_pbkdf2,
            commands::crypto_encrypt,
            commands::crypto_decrypt,
            commands::protocol_init,
            commands::protocol_get_safety_number,
            commands::protocol_establish_session,
            commands::protocol_encrypt,
            commands::protocol_decrypt,
            commands::protocol_encrypt_media,
            commands::protocol_decrypt_media,
            commands::protocol_get_pending,
            commands::protocol_save_pending,
            commands::protocol_remove_pending,
            commands::protocol_replenish_pre_keys,
            commands::protocol_verify_session,
            commands::protocol_secure_vacuum,
            commands::protocol_encrypt_sealed,
            commands::protocol_decrypt_sealed,
            commands::protocol_create_group_distribution,
            commands::protocol_group_init,
            commands::protocol_group_encrypt,
            commands::protocol_group_decrypt,
            commands::protocol_process_group_distribution,
            commands::connect_network,
            commands::send_to_network,
            commands::nuclear_reset,
            commands::crypto_mine_pow,
            commands::clear_vault,
            commands::protocol_sign,
            commands::protocol_export_vault,
            commands::protocol_import_vault,
            commands::protocol_save_vault_to_path,
            commands::protocol_read_vault_from_path
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
