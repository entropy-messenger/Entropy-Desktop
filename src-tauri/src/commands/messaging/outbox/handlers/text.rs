use super::super::OutgoingText;
use crate::app_state::{DbState, NetworkState};
use crate::commands::{
    DbMessage, internal_db_save_message, internal_send_to_network, internal_signal_encrypt,
};
use rusqlite::params;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

pub async fn process_outgoing_text(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    payload: OutgoingText,
) -> Result<serde_json::Value, String> {
    if payload.content.chars().count() > 2000 {
        return Err("Message too long (max 2000 characters)".into());
    }

    let msg_id = uuid::Uuid::new_v4().to_string();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let own_id = {
        let id_lock = net_state
            .identity_hash
            .lock()
            .map_err(|_| "Network state poisoned")?;
        id_lock.clone().ok_or("Not authenticated")?
    };

    let db_msg = DbMessage {
        id: msg_id.clone(),
        chat_address: payload.recipient.clone(),
        sender_hash: own_id.clone(),
        content: payload.content.clone(),
        timestamp,
        r#type: "text".to_string(),
        status: "sending".to_string(),
        attachment_json: None,
        is_starred: false,
        is_group: false,
        reply_to_json: payload
            .reply_to
            .as_ref()
            .map(|r| serde_json::to_string(&r).unwrap_or_default()),
    };

    internal_db_save_message(&db_state, db_msg.clone()).await?;

    let mut final_json =
        serde_json::to_value(&db_msg).map_err(|e: serde_json::Error| e.to_string())?;
    if let Some(obj) = final_json.as_object_mut() {
        let _ = obj.insert(
            "chatAlias".to_string(),
            serde_json::json!(payload.group_name),
        );
        let _ = obj.insert(
            "chatMembers".to_string(),
            serde_json::json!(payload.group_members.clone()),
        );
    }
    app.emit("msg://added", final_json.clone())
        .map_err(|e: tauri::Error| e.to_string())?;

    let signal_payload = serde_json::json!({
        "type": "text_msg",
        "content": payload.content,
        "id": msg_id.clone(),
        "replyTo": payload.reply_to,
        "timestamp": timestamp,
        "isGroup": false,
    });

    let ciphertext_obj = internal_signal_encrypt(
        app.clone(),
        &net_state,
        &payload.recipient,
        signal_payload.to_string(),
    )
    .await?;

    let routing_hash = payload
        .recipient
        .split('.')
        .next()
        .unwrap_or(&payload.recipient);
    let payload_bytes = ciphertext_obj.to_string().into_bytes();

    let _ = internal_send_to_network(
        app.clone(),
        &net_state,
        Some(routing_hash.to_string()),
        Some(msg_id.clone()),
        None,
        Some(payload_bytes),
        true,
        false,
        None,
        false,
    )
    .await;

    {
        let conn = db_state.get_conn()?;
        let _ = conn.execute(
            "UPDATE messages SET status = 'sent' WHERE id = ?1",
            params![msg_id],
        );
        let _ = conn.execute(
            "UPDATE chats SET last_status = 'sent' WHERE LOWER(address) = LOWER(?1)",
            params![payload.recipient],
        );
    }
    app.emit(
        "msg://status",
        json!({ "id": msg_id, "status": "sent", "chatAddress": payload.recipient }),
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    if let Some(obj) = final_json.as_object_mut() {
        let _ = obj.insert("status".to_string(), json!("sent"));
    }
    Ok(final_json)
}

pub async fn process_outgoing_group_text(
    app: AppHandle,
    payload: OutgoingText,
) -> Result<serde_json::Value, String> {
    let db_state = app.state::<DbState>();
    let net_state = app.state::<NetworkState>();

    if payload.content.chars().count() > 2000 {
        return Err("Message too long (max 2000 characters)".into());
    }

    let msg_id = uuid::Uuid::new_v4().to_string();
    let timestamp = chrono::Utc::now().timestamp_millis();
    let own_id = net_state
        .identity_hash
        .lock()
        .map_err(|_| "State poisoned")?
        .clone()
        .ok_or("Not authenticated")?;

    let db_msg = DbMessage {
        id: msg_id.clone(),
        chat_address: payload.recipient.clone(),
        sender_hash: own_id.clone(),
        content: payload.content.clone(),
        timestamp,
        r#type: "text".to_string(),
        status: "sending".to_string(),
        attachment_json: None,
        is_starred: false,
        is_group: true,
        reply_to_json: payload
            .reply_to
            .as_ref()
            .map(|r| serde_json::to_string(&r).unwrap_or_default()),
    };

    internal_db_save_message(&db_state, db_msg.clone()).await?;

    let members = payload
        .group_members
        .as_ref()
        .ok_or("Group members missing")?;

    let signal_inner_payload = serde_json::json!({
        "type": "text_msg",
        "content": payload.content,
        "id": msg_id.clone(),
        "replyTo": payload.reply_to,
        "timestamp": timestamp,
        "isGroup": true,
        "groupId": payload.recipient,
        "groupName": payload.group_name,
        "groupMembers": payload.group_members.clone(),
    });
    let payload_str = signal_inner_payload.to_string();

    for member_id in members {
        if member_id == &own_id {
            continue;
        }
        let routing_hash = member_id
            .split('.')
            .next()
            .unwrap_or(member_id.as_str())
            .to_string();
        match internal_signal_encrypt(app.clone(), &net_state, member_id, payload_str.clone()).await
        {
            Ok(ciphertext_obj) => {
                let payload_bytes = ciphertext_obj.to_string().into_bytes();
                let _ = internal_send_to_network(
                    app.clone(),
                    &net_state,
                    Some(routing_hash),
                    Some(msg_id.clone()),
                    None,
                    Some(payload_bytes),
                    true,
                    false,
                    None,
                    false,
                )
                .await;
            }
            Err(_e) => {
                // Skipping member
            }
        }
    }

    if let Ok(conn) = db_state.get_conn() {
        let _ = conn.execute(
            "UPDATE messages SET status = 'sent' WHERE id = ?1",
            params![msg_id],
        );
        let _ = conn.execute(
            "UPDATE chats SET last_status = 'sent' WHERE LOWER(address) = LOWER(?1)",
            params![payload.recipient],
        );
    }

    let mut final_json =
        serde_json::to_value(&db_msg).map_err(|e: serde_json::Error| e.to_string())?;
    if let Some(obj) = final_json.as_object_mut() {
        obj.insert(
            "chatAlias".to_string(),
            serde_json::json!(payload.group_name),
        );
        obj.insert(
            "chatMembers".to_string(),
            serde_json::json!(payload.group_members.clone()),
        );
        obj.insert("status".to_string(), json!("sent"));
    }
    app.emit("msg://added", final_json.clone())
        .map_err(|e| e.to_string())?;
    app.emit(
        "msg://status",
        json!({ "id": msg_id, "status": "sent", "chatAddress": payload.recipient }),
    )
    .map_err(|e| e.to_string())?;

    Ok(final_json)
}
