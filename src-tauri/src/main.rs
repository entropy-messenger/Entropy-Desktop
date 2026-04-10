#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod commands;
mod noise;
mod signal_store;

use app_state::{DbState, NetworkState};
use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Manager,
};

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
            binary_receiver: Mutex::new(None),
            is_refilling: Mutex::new(false),
            jailed_until: Mutex::new(None),
            pending_transfers: Mutex::new(std::collections::HashMap::new()),
        })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            commands::reset_database,
            commands::crypto_mine_pow,
            commands::vault_exists,
            commands::export_database,
            commands::import_database,
            commands::signal_init,
            commands::signal_sync_keys,
            commands::signal_encrypt,
            commands::signal_decrypt_media,
            commands::set_panic_password,
            commands::vault_save_media,
            commands::vault_load_media,
            commands::vault_delete_media,
            commands::signal_sign_message,
            commands::signal_get_peer_identity,
            commands::signal_set_peer_trust,
            commands::signal_get_own_identity,
            commands::signal_get_identity_hash,
            commands::signal_get_fingerprint,
            commands::send_typing_status,
            commands::send_receipt,
            commands::send_profile_update,
            commands::open_file,
            commands::db_save_message,
            commands::db_get_messages,
            commands::db_search_messages,
            commands::db_update_messages,
            commands::db_upsert_chat,
            commands::db_get_chats,
            commands::db_delete_chat,
            commands::db_get_contacts,
            commands::db_set_contact_blocked,
            commands::db_set_contact_nickname,
            commands::db_set_contact_global_nickname,
            commands::db_set_chat_pinned,
            commands::db_set_chat_archived,
            commands::db_get_starred_messages,
            commands::db_export_media,
            commands::db_delete_messages,
            commands::register_nickname,
            commands::nickname_lookup,
            commands::identity_resolve,
            commands::create_group,
            commands::add_to_group,
            commands::update_group_name,
            commands::remove_from_group,
            commands::leave_group,
            commands::burn_account,
            commands::process_outgoing_text,
            commands::process_outgoing_group_text,
            commands::process_outgoing_media,
            commands::process_outgoing_group_media
        ])
        .setup(|app| {
            // 🎙️ LINUX Fix: WebKitGTK requires manual signal handling for microphone permission
            #[cfg(target_os = "linux")]
            {
                use webkit2gtk::{PermissionRequest, PermissionRequestExt, WebViewExt};
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.with_webview(|w| {
                        let webview = w.inner();
                        webview.connect_permission_request(
                            |_webview, request: &PermissionRequest| {
                                request.allow();
                                true
                            },
                        );
                    });
                }
            }
            // Setup tray and menu as before
            let quit_i = MenuItem::with_id(app, "quit", "Quit Entropy", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let tray_builder = TrayIconBuilder::new();
            let tray_icon = app.default_window_icon().cloned();

            let builder = if let Some(icon) = tray_icon {
                tray_builder.icon(icon)
            } else {
                tray_builder
            };

            let _tray = builder
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
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
