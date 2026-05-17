use crate::app_state::{DbState, NetworkState};
use crate::commands::messaging::inbox::internal_send_volatile;
use crate::commands::{
    DbMessage, get_media_dir, internal_db_save_message, internal_signal_encrypt,
};
use base64::Engine;
use chacha20poly1305::{
    Key, XChaCha20Poly1305,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use rusqlite::params;
use serde_json::json;
use std::io::Write;
use tauri::{AppHandle, Emitter, Manager};

pub async fn handle_media_msg(
    app: AppHandle,
    sender: String,
    decrypted_json: serde_json::Value,
    net_state: &NetworkState,
) -> Result<(), String> {
    let raw_msg_id = decrypted_json["id"].as_str().unwrap_or("");
    let msg_id = raw_msg_id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>();

    let result = handle_media_msg_inner(&app, &sender, &decrypted_json, &msg_id).await;

    // Always send delivery receipt if we got a valid msg_id,
    // even if the message was malformed — the sender needs to know it arrived.
    let is_group = decrypted_json["isGroup"].as_bool().unwrap_or(false);
    if !msg_id.is_empty() && !is_group {
        let receipt_payload = json!({
            "type": "receipt",
            "msgIds": vec![msg_id],
            "status": "delivered"
        });
        if let Ok(encrypted) =
            internal_signal_encrypt(app.clone(), net_state, &sender, receipt_payload.to_string())
                .await
        {
            let _ = internal_send_volatile(app.clone(), net_state, &sender, encrypted).await;
        }
    }

    result
}

async fn handle_media_msg_inner(
    app: &AppHandle,
    sender: &str,
    decrypted_json: &serde_json::Value,
    msg_id: &str,
) -> Result<(), String> {
    if msg_id.is_empty() {
        return Err("Invalid message ID".into());
    }

    let bundle = decrypted_json["bundle"].clone();
    let size = decrypted_json["size"].as_u64().ok_or("Missing size")?;
    if size > 10_737_418_240u64 {
        return Err("File metadata exceeds size limit".into());
    }
    let m_type = decrypted_json["msg_type"]
        .as_str()
        .ok_or("Missing msg_type")?
        .to_string();
    let download_url = decrypted_json["download_url"]
        .as_str()
        .ok_or("Missing download_url")?
        .to_string();
    let key_b64 = bundle["key"].as_str().ok_or("Missing bundle key")?;
    let duration = decrypted_json["duration"].as_f64().unwrap_or(0.0);
    let timestamp = decrypted_json["timestamp"].as_i64().unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    });

    let db_state = app.state::<DbState>();
    let media_dir = get_media_dir(app, &db_state)?;

    // Save thumbnail to vault (small, safe to do immediately)
    if let Some(thumb_b64) = decrypted_json["thumbnail"].as_str()
        && let Ok(thumb_bytes) = base64::engine::general_purpose::STANDARD.decode(thumb_b64)
        && let Ok(vault_key_bytes) = db_state.media_key.lock().map(|l| l.clone())
        && let Some(vk) = vault_key_bytes
    {
        let vault_key = Key::from_slice(&vk);
        let vault_cipher = XChaCha20Poly1305::new(vault_key);
        let thumb_path = media_dir.join(format!("{}_thumb", msg_id));
        let v_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        if let Ok(v_cipher) = vault_cipher.encrypt(&v_nonce, thumb_bytes.as_slice())
            && let Ok(mut f) = std::fs::File::create(&thumb_path)
        {
            let _ = f.write_all(&v_nonce);
            let _ = f.write_all(&v_cipher);
            let _ = f.sync_all();
        }
    }

    // Build DB message — no vault file yet, user must tap Download
    let is_group = decrypted_json["isGroup"].as_bool().unwrap_or(false);
    let group_name = decrypted_json["groupName"].as_str().map(|s| s.to_string());
    let chat_address = if is_group {
        decrypted_json["groupId"]
            .as_str()
            .unwrap_or(sender)
            .to_string()
    } else {
        sender.to_string()
    };

    let db_msg = DbMessage {
        id: msg_id.to_string(),
        chat_address: chat_address.clone(),
        sender_hash: sender.to_string(),
        content: if m_type == "voice_note" {
            "Voice Note".to_string()
        } else {
            format!(
                "File: {}",
                bundle["file_name"].as_str().unwrap_or("Unnamed File")
            )
        },
        timestamp,
        r#type: m_type.clone(),
        status: "delivered".to_string(),
        attachment_json: Some(
            json!({
                "fileName": bundle["file_name"],
                "fileType": bundle["file_type"],
                "size": size,
                "duration": duration,
                "thumbnail": decrypted_json["thumbnail"],
                "bundle": bundle,
                "download_url": download_url,
                "key": key_b64,
                "isDownloaded": false,
            })
            .to_string(),
        ),
        is_starred: false,
        is_group,
        reply_to_json: decrypted_json["replyTo"]
            .as_object()
            .map(|r| serde_json::to_string(r).unwrap_or_default()),
        reactions_json: None,
    };

    if is_group {
        let conn = db_state.get_conn()?;
        let _ = conn.execute(
            "INSERT INTO chats (address, is_group, alias) VALUES (?1, 1, ?2)
             ON CONFLICT(address) DO UPDATE SET 
                 alias = CASE WHEN excluded.alias IS NOT NULL THEN excluded.alias ELSE alias END,
                 is_group = 1",
            params![chat_address, group_name],
        );

        if let Some(members) = decrypted_json["groupMembers"].as_array() {
            let m_strings: Vec<String> = members
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if !m_strings.is_empty() {
                let _ = conn.execute(
                    "DELETE FROM chat_members WHERE chat_address = ?1",
                    params![chat_address],
                );
                for m in m_strings {
                    let _ = conn.execute(
                        "INSERT OR IGNORE INTO chat_members (chat_address, member_hash) VALUES (?1, ?2)",
                        params![chat_address, m],
                    );
                }
            }
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

    crate::notification::send_message_notification(&app, &sender, &chat_address, &db_msg.content);

    // Auto-download all received files (skip own messages — sender already has the vault file)
    if let Ok(hash_lock) = app.state::<NetworkState>().identity_hash.lock()
        && let Some(own_hash) = hash_lock.clone()
        && sender == own_hash
    {
    } else {
        let app_clone = app.clone();
        let msg_id_clone = msg_id.to_string();
        tokio::spawn(async move {
            let _ = crate::commands::download_media::download_media(app_clone, msg_id_clone).await;
        });
    }

    Ok(())
}
