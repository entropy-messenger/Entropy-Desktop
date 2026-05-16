use crate::app_state::{DbState, NetworkState};
use rusqlite::params;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

pub fn send_message_notification(
    app: &AppHandle,
    sender_hash: &str,
    chat_address: &str,
    content: &str,
) {
    // Check if notifications are enabled
    if let Ok(conn) = app.state::<DbState>().get_conn() {
        let enabled: Result<String, _> = conn.query_row(
            "SELECT value FROM settings WHERE key = 'notifications_enabled'",
            [],
            |row| row.get(0),
        );
        if let Ok(val) = enabled {
            if val != "1" {
                return;
            }
        }
    }

    let own_hash = app
        .state::<NetworkState>()
        .identity_hash
        .lock()
        .ok()
        .and_then(|h| h.clone());

    if let Some(ref own) = own_hash {
        if sender_hash == own {
            return;
        }
    }

    let display_name = get_display_name(app, chat_address, sender_hash);

    let body = if content.len() > 50 {
        format!("{}...", &content[..47])
    } else {
        content.to_string()
    };

    let _ = app
        .notification()
        .builder()
        .title(format!("Message from {}", display_name))
        .body(body)
        .show();
}

fn get_display_name(app: &AppHandle, chat_address: &str, sender_hash: &str) -> String {
    if let Ok(conn) = app.state::<DbState>().get_conn() {
        let result: Result<(Option<String>, Option<String>), _> =
            conn.query_row(
                "SELECT alias, global_nickname FROM chats WHERE address = ?1",
                params![chat_address],
                |row| Ok((row.get(0)?, row.get(1)?)),
            );
        if let Ok((alias, global_nick)) = result {
            if let Some(name) = global_nick.or(alias) {
                return name;
            }
        }
    }
    sender_hash.chars().take(8).collect()
}
