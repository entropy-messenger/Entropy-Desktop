
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod app_state;
mod commands;
mod signal_store;
mod security;

use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Manager,
};
use app_state::{DbState, NetworkState, AudioState};
use audio::AudioRecorder;

fn main() {
    let profile = std::env::var("ENTROPY_PROFILE").unwrap_or_else(|_| "default".to_string());
    println!("[*] Starting Entropy (Profile: {})", profile);

    tauri::Builder::default()
        .manage(DbState {
            conn: Mutex::new(None),
            media_key: Mutex::new(None),
            profile: Mutex::new(profile),
        })
        .manage(NetworkState {
            is_enabled: Mutex::new(false),
            url: Mutex::new(None),
            proxy_url: Mutex::new(None),
            queue: Mutex::new(std::collections::VecDeque::new()),
            sender: Mutex::new(None),
            cancel: Mutex::new(None),
            response_channels: Mutex::new(std::collections::HashMap::new()),
            is_authenticated: Mutex::new(false),
            identity_hash: Mutex::new(None),
            session_token: Mutex::new(None),
            halted_targets: Mutex::new(std::collections::HashSet::new()),
            media_assembler: Mutex::new(std::collections::HashMap::new()),
            pending_media_links: Mutex::new(std::collections::HashMap::new()),
        })
        .manage(AudioState {
            recorder: Mutex::new(AudioRecorder::new()),
        })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::init_vault,
            commands::vault_save,
            commands::vault_load,
            commands::crypto_sha256,
            commands::crypto_encrypt_media,
            commands::crypto_encrypt_file,
            commands::crypto_decrypt_media,
            commands::connect_network,
            commands::disconnect_network,
            commands::send_to_network,
            commands::flush_outbox,
            commands::nuclear_reset,
            commands::crypto_mine_pow,
            commands::clear_vault,
            commands::vault_exists,
            commands::vault_delete,
            commands::start_native_recording,
            commands::stop_native_recording,
            commands::save_file,
            commands::export_database,
            commands::import_database,
            commands::signal_init,
            commands::signal_get_bundle,
            commands::signal_sync_keys,
            commands::signal_establish_session,
            commands::signal_encrypt,
            commands::signal_decrypt,
            commands::set_panic_password,
            commands::vault_save_media,
            commands::vault_load_media,
            commands::signal_sign_message,
            commands::signal_get_peer_identity,
            commands::signal_set_peer_trust,
            commands::signal_get_own_identity,
            commands::signal_get_identity_hash,
            commands::signal_get_fingerprint,
            commands::send_typing_status,
            commands::send_receipt,
            commands::send_profile_update,
            commands::show_in_folder,
            commands::db_save_message,
            commands::db_get_messages,
            commands::db_search_messages,
            commands::db_update_messages_status,
            commands::db_upsert_chat,
            commands::db_get_chats,
            commands::db_upsert_contact,
            commands::db_get_contacts,
            commands::db_set_contact_blocked,
            commands::db_set_chat_pinned,
            commands::db_set_chat_archived,
            commands::db_set_message_starred,
            commands::process_outgoing_text,
            commands::process_outgoing_media
        ])
        .setup(|app| {
            
            // Setup tray and menu as before
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
