use crate::app_state::{DbState, NetworkState};
use crate::commands::messaging::inbox::internal_send_volatile;
use crate::commands::{DbMessage, internal_db_save_message, internal_signal_encrypt};
use rusqlite::params;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

pub async fn handle_text_msg(
    app: AppHandle,
    sender: String,
    decrypted_json: serde_json::Value,
) -> Result<(), String> {
    let msg_id = decrypted_json["id"]
        .as_str()
        .ok_or("Missing msg id")?
        .to_string();
    let content = decrypted_json["content"]
        .as_str()
        .ok_or("Missing content")?
        .to_string();
    let timestamp = decrypted_json["timestamp"]
        .as_i64()
        .ok_or("Missing timestamp")?;

    let is_group = decrypted_json["isGroup"].as_bool().unwrap_or(false);
    let group_name = decrypted_json["groupName"].as_str().map(|s| s.to_string());
    let chat_address = if is_group {
        decrypted_json["groupId"]
            .as_str()
            .unwrap_or(&sender)
            .to_string()
    } else {
        sender.clone()
    };

    let db_msg = DbMessage {
        id: msg_id.clone(),
        chat_address: chat_address.clone(),
        sender_hash: sender.clone(),
        content,
        timestamp,
        r#type: "text".to_string(),
        status: "delivered".to_string(),
        attachment_json: None,
        is_starred: false,
        is_group,
        reply_to_json: decrypted_json["replyTo"]
            .as_object()
            .map(|r| serde_json::to_string(r).unwrap_or_default()),
    };

    let db_state = app.state::<DbState>();

    // If it's a group, check if the chat is active
    if is_group {
        let conn = db_state.get_conn()?;
        let is_active: i32 = conn
            .query_row(
                "SELECT is_active FROM chats WHERE address = ?1",
                params![chat_address],
                |r: &rusqlite::Row| r.get(0),
            )
            .unwrap_or(1);

        if is_active == 0 {
            return Ok(());
        }
    }

    internal_db_save_message(&db_state, db_msg.clone()).await?;
    let mut final_json =
        serde_json::to_value(&db_msg).map_err(|e: serde_json::Error| e.to_string())?;

    if is_group && let Some(obj) = final_json.as_object_mut() {
        let _ = obj.insert("chatAlias".to_string(), json!(group_name));
        if let Some(members) = decrypted_json["groupMembers"].as_array() {
            let _ = obj.insert("chatMembers".to_string(), json!(members));
        }
    }

    app.emit("msg://added", final_json.clone())
        .map_err(|e: tauri::Error| e.to_string())?;

    // Enforce 1:1 delivery receipts
    if !is_group {
        let receipt_payload = json!({
            "type": "receipt",
            "msgIds": vec![msg_id],
            "status": "delivered"
        });
        let net_state = app.state::<NetworkState>();
        if let Ok(encrypted) = internal_signal_encrypt(
            app.clone(),
            &net_state,
            &sender,
            receipt_payload.to_string(),
        )
        .await
        {
            let _ = internal_send_volatile(app.clone(), &net_state, &sender, encrypted).await;
        }
    }

    Ok(())
}
