use crate::app_state::DbState;
use crate::commands::{db_set_contact_global_nickname, db_update_messages};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

pub async fn handle_receipt(
    app: AppHandle,
    sender: String,
    decrypted_json: serde_json::Value,
) -> Result<(), String> {
    if let Some(ids) = decrypted_json["msgIds"].as_array() {
        let id_strs: Vec<String> = ids
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if let Some(status) = decrypted_json["status"].as_str() {
            let _ = db_update_messages(
                app.state::<DbState>(),
                id_strs.clone(),
                Some(status.to_string()),
                None,
                None,
            )
            .await;
            app.emit(
                "msg://status",
                json!({ "chat_address": sender, "ids": id_strs, "status": status }),
            )
            .map_err(|e: tauri::Error| e.to_string())?;
        }
    }
    Ok(())
}

pub async fn handle_typing(
    app: AppHandle,
    sender: String,
    decrypted_json: serde_json::Value,
) -> Result<(), String> {
    app.emit(
        "msg://typing",
        json!({ "sender": sender, "payload": decrypted_json }),
    )
    .map_err(|e: tauri::Error| e.to_string())?;
    Ok(())
}

pub async fn handle_profile_update(
    app: AppHandle,
    sender: String,
    decrypted_json: serde_json::Value,
) -> Result<(), String> {
    let alias = decrypted_json["alias"].as_str().map(|s| s.to_string());
    let db_state = app.state::<DbState>();
    let _ = db_set_contact_global_nickname(
        db_state.clone(),
        sender.clone(),
        alias.clone(),
    )
    .await;
    app.emit(
        "contact-update",
        json!({ "hash": sender, "alias": alias }),
    )
    .map_err(|e: tauri::Error| e.to_string())?;
    Ok(())
}
