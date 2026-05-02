//! Outbound message pipeline.
//! Orchestrates encryption, media vaulting, and network dispatch.

use crate::app_state::{DbState, NetworkState};
use crate::commands::{
    DbMessage, get_media_dir, internal_db_save_message, internal_dispatch_fragment,
    internal_send_to_network, internal_signal_encrypt,
};
use base64::Engine;
use chacha20poly1305::{
    Key as ChaKey, XChaCha20Poly1305,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Write;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReplyTo {
    pub id: String,
    pub content: String,
    pub sender_hash: Option<String>,
    pub sender_alias: Option<String>,
    pub r#type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingText {
    pub recipient: String,
    pub content: String,
    pub reply_to: Option<ReplyTo>,
    pub group_name: Option<String>,
    #[serde(rename = "isGroup", default)]
    pub is_group: bool,
    pub group_members: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingMedia {
    pub recipient: String,
    pub file_path: Option<String>,
    pub file_data: Option<Vec<u8>>,
    pub file_name: Option<String>,
    pub file_type: Option<String>,
    pub msg_type: Option<String>,
    pub group_name: Option<String>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    #[serde(rename = "isGroup", default)]
    pub is_group: bool,
    pub group_members: Option<Vec<String>>,
    pub reply_to: Option<ReplyTo>,
}

#[tauri::command]
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

#[tauri::command]
pub fn process_outgoing_media(
    app: AppHandle,
    payload: OutgoingMedia,
) -> Result<serde_json::Value, String> {
    let msg_id = uuid::Uuid::new_v4().to_string();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("Clock error: {}", e))?
        .as_millis() as i64;
    let transfer_id: u32 = rand::random();

    let canonical_path_opt = if let Some(p) = &payload.file_path {
        let path_buf = std::path::PathBuf::from(p);
        let canonical_path = std::fs::canonicalize(&path_buf)
            .map_err(|_| "Access denied: Invalid or inaccessible file path".to_string())?;

        let metadata =
            std::fs::metadata(&canonical_path).map_err(|e: std::io::Error| e.to_string())?;
        if metadata.len() > 256 * 1024 * 1024u64 {
            return Err("File too large. Maximum size is 256MB.".to_string());
        }
        Some(canonical_path)
    } else if let Some(ref d) = payload.file_data {
        if d.len() as u64 > 256 * 1024 * 1024u64 {
            return Err("File too large. Maximum size is 256MB.".to_string());
        }
        None
    } else {
        return Err("No file path or data provided".into());
    };

    let file_size = if let Some(ref p) = canonical_path_opt {
        std::fs::metadata(p)
            .map_err(|e: std::io::Error| e.to_string())?
            .len()
    } else if let Some(ref d) = payload.file_data {
        d.len() as u64
    } else {
        0
    };

    let db_state = app.state::<DbState>();
    let net_state = app.state::<NetworkState>();
    let own_id = net_state
        .identity_hash
        .lock()
        .map_err(|_| "State poisoned")?
        .clone()
        .unwrap_or_default();

    let db_msg = DbMessage {
        id: msg_id.clone(),
        chat_address: payload.recipient.clone(),
        sender_hash: own_id.clone(),
        content: if payload.msg_type.as_deref() == Some("voice_note") {
            "Voice Note".to_string()
        } else {
            format!("File: {}", payload.file_name.as_deref().unwrap_or("Media"))
        },
        timestamp,
        r#type: payload
            .msg_type
            .clone()
            .unwrap_or_else(|| "file".to_string()),
        status: "sending".to_string(),
        attachment_json: Some(
            serde_json::json!({
                "fileName": payload.file_name,
                "fileType": payload.file_type,
                "size": file_size,
                "duration": payload.duration,
                "thumbnail": payload.thumbnail,
                "originalPath": canonical_path_opt.clone().map(|p| p.to_string_lossy().to_string()),
                "transferId": transfer_id
            })
            .to_string(),
        ),
        is_starred: false,
        is_group: false,
        reply_to_json: payload
            .reply_to
            .as_ref()
            .map(|r| serde_json::to_string(&r).unwrap_or_default()),
    };

    {
        let db_state = db_state.clone();
        let db_msg = db_msg.clone();
        let app = app.clone();
        tauri::async_runtime::block_on(async move {
            let _ = crate::commands::messaging::chat::internal_db_save_message(
                &db_state,
                db_msg.clone(),
            )
            .await;
            let _ = app.emit("msg://added", db_msg);
        });
    }

    // 3. Background Processing
    let app_bg = app.clone();
    let payload_bg = payload.clone();
    let msg_id_bg = msg_id.clone();
    let canonical_path_bg = canonical_path_opt.clone();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let app = app_bg;
            let payload = payload_bg;
            let msg_id = msg_id_bg;
            let db_state = app.state::<DbState>();
            let net_state = app.state::<NetworkState>();

            let net_key = XChaCha20Poly1305::generate_key(&mut OsRng);
            {
                if let Ok(mut active) = net_state.active_outgoing_transfers.lock() {
                    active.insert(
                        transfer_id,
                        crate::app_state::OutgoingTransferInfo {
                            file_path: canonical_path_opt.clone().unwrap_or_default(),
                            transit_key: net_key.into(),
                        },
                    );
                }
            }
            let vault_key_bytes = {
                let lock = db_state.media_key.lock().unwrap();
                lock.clone().unwrap()
            };
            let vault_key = ChaKey::from_slice(&vault_key_bytes);
            let vault_cipher = XChaCha20Poly1305::new(vault_key);

            let media_dir = get_media_dir(&app, &db_state).unwrap();
            let vault_path = media_dir.join(&msg_id);
            let mut vault_file = std::fs::File::create(&vault_path).unwrap();

            let key_b64 = base64::engine::general_purpose::STANDARD.encode(net_key);

            let announcement = serde_json::json!({
                "type": "file",
                "id": msg_id.clone(),
                "transfer_id": transfer_id,
                "size": file_size,
                "msg_type": payload.msg_type.as_deref().unwrap_or("file"),
                "duration": payload.duration,
                "thumbnail": payload.thumbnail,
                "replyTo": payload.reply_to,
                "timestamp": timestamp,
                "bundle": {
                    "key": key_b64,
                    "file_name": payload.file_name,
                    "file_type": payload.file_type
                }
            });

            if let Ok(encrypted) = internal_signal_encrypt(
                app.clone(),
                &net_state,
                &payload.recipient,
                announcement.to_string(),
            )
            .await
            {
                let _ = crate::commands::network::transit::internal_send_to_network(
                    app.clone(),
                    &net_state,
                    Some(payload.recipient.clone()),
                    Some(msg_id.clone()),
                    None,
                    Some(encrypted.to_string().into_bytes()),
                    true,
                    false,
                    Some(transfer_id),
                    true,
                )
                .await;
            }

            let mut reader: Box<dyn std::io::Read> = if let Some(ref p) = canonical_path_bg {
                Box::new(std::io::BufReader::new(std::fs::File::open(p).unwrap()))
            } else if let Some(ref d) = payload.file_data {
                Box::new(std::io::Cursor::new(d.to_vec()))
            } else {
                return;
            };

            let mut fragment_index = 0;
            let mut buffer = vec![0u8; 1279];
            let total_fragments = (file_size as f64 / 1279.0).ceil() as u32;

            loop {
                let mut n = 0;
                let mut read_retries = 0;
                while n < 1279 {
                    match reader.read(&mut buffer[n..]) {
                        Ok(0) => break,
                        Ok(read) => {
                            n += read;
                            read_retries = 0;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) if read_retries < 3 => {
                            read_retries += 1;
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                        Err(e) => {
                            let _ = app.emit(
                                "network-bin-error",
                                serde_json::json!({
                                    "msg_id": msg_id.clone(),
                                    "error": format!("Disk read error: {}", e)
                                }),
                            );
                            return;
                        }
                    }
                }

                if n == 0 {
                    break;
                }
                let chunk = &buffer[..n];

                let v_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                let v_cipher = vault_cipher.encrypt(&v_nonce, chunk).unwrap();
                let mut retries = 0;
                loop {
                    match vault_file
                        .write_all(&v_nonce)
                        .and_then(|_| vault_file.write_all(&v_cipher))
                    {
                        Ok(_) => break,
                        Err(e) if retries < 3 => {
                            retries += 1;
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                        Err(_e) => return, // Abort background task if vault is unusable
                    }
                }

                let transit_cipher = XChaCha20Poly1305::new(ChaKey::from_slice(&net_key));
                let t_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                let t_cipher = transit_cipher.encrypt(&t_nonce, chunk).unwrap();
                let mut packet = Vec::with_capacity(t_cipher.len() + 24);
                packet.extend_from_slice(&t_nonce);
                packet.extend_from_slice(&t_cipher);

                let mut routing_hash = [0u8; 64];
                let r_bytes = payload.recipient.as_bytes();
                let r_len = std::cmp::min(r_bytes.len(), 64);
                routing_hash[..r_len].copy_from_slice(&r_bytes[..r_len]);

                let _ = internal_dispatch_fragment(
                    app.clone(),
                    &net_state,
                    routing_hash,
                    Some(msg_id.clone()),
                    transfer_id,
                    fragment_index,
                    total_fragments,
                    &packet,
                    true,
                    false,
                    false,
                )
                .await;
                fragment_index += 1;
            }
            let _ = vault_file.sync_all();

            if fragment_index != total_fragments {
                let _ = app.emit(
                    "network-bin-error",
                    serde_json::json!({
                        "msg_id": msg_id.clone(),
                        "error": "Incomplete read: fragment mismatch"
                    }),
                );
                return;
            }

            // Save our own thumbnail to vault so we can see it
            if let Some(thumb_b64) = &payload.thumbnail {
                if let Ok(thumb_bytes) = base64::engine::general_purpose::STANDARD.decode(thumb_b64)
                {
                    let thumb_path = media_dir.join(format!("{}_thumb", msg_id));
                    let v_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                    if let Ok(v_cipher) = vault_cipher.encrypt(&v_nonce, thumb_bytes.as_slice()) {
                        if let Ok(mut f) = std::fs::File::create(&thumb_path) {
                            let _ = f.write_all(&v_nonce);
                            let _ = f.write_all(&v_cipher);
                            let _ = f.sync_all();
                        }
                    }
                }
            }

            let final_meta = serde_json::json!({
                "type": "file",
                "id": msg_id.clone(),
                "transfer_id": transfer_id,
                "size": file_size,
                "msg_type": payload.msg_type.as_deref().unwrap_or("file"),
                "duration": payload.duration,
                "thumbnail": payload.thumbnail,
                "replyTo": payload.reply_to,
                "timestamp": timestamp,
                "bundle": {
                    "key": key_b64,
                    "file_name": payload.file_name,
                    "file_type": payload.file_type
                }
            });

            if let Ok(encrypted) = internal_signal_encrypt(
                app.clone(),
                &net_state,
                &payload.recipient,
                final_meta.to_string(),
            )
            .await
            {
                let _ = crate::commands::network::transit::internal_send_to_network(
                    app.clone(),
                    &net_state,
                    Some(payload.recipient.clone()),
                    Some(msg_id.clone()),
                    None,
                    Some(encrypted.to_string().into_bytes()),
                    true,
                    false,
                    Some(transfer_id),
                    true,
                )
                .await;
            }

            let final_attachment_obj = serde_json::json!({
                "fileName": payload.file_name,
                "fileType": payload.file_type,
                "size": file_size,
                "duration": payload.duration,
                "thumbnail": payload.thumbnail,
                "originalPath": canonical_path_bg.as_ref().map(|p| p.to_string_lossy().to_string()),
                "transferId": transfer_id,
                "vaultPath": vault_path.to_string_lossy().to_string()
            });

            if let Ok(conn) = db_state.get_conn() {
                let _ = conn.execute(
                    "UPDATE messages SET status = 'sent', attachment_json = ?2 WHERE id = ?1",
                    rusqlite::params![msg_id, final_attachment_obj.to_string()],
                );
            }
            let _ = app.emit(
                "msg://status",
                serde_json::json!({
                    "id": msg_id,
                    "status": "sent",
                    "chatAddress": payload.recipient,
                    "attachment": final_attachment_obj
                }),
            );

            // Success cleanup
            if let Ok(mut active) = net_state.active_outgoing_transfers.lock() {
                active.remove(&transfer_id);
            }
        });
    });

    Ok(serde_json::to_value(&db_msg).unwrap())
}

#[tauri::command]
pub async fn send_typing_status(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    is_typing: bool,
) -> Result<(), String> {
    {
        if let Ok(conn) = db_state.get_conn() {
            let is_group = conn
                .query_row(
                    "SELECT is_group FROM chats WHERE address = ?1",
                    [&peer_hash],
                    |r: &rusqlite::Row| r.get::<_, i32>(0),
                )
                .unwrap_or(0)
                != 0;
            if is_group {
                return Ok(());
            }
        }
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let message =
        json!({ "type": "typing", "isTyping": is_typing, "timestamp": timestamp }).to_string();
    if let Ok(encrypted) =
        internal_signal_encrypt(app.clone(), &net_state, &peer_hash, message).await
    {
        let _ = internal_send_to_network(
            app.clone(),
            &net_state,
            Some(peer_hash.clone()),
            None,
            None,
            Some(encrypted.to_string().into_bytes()),
            true,
            false,
            None,
            true,
        )
        .await;
    }
    Ok(())
}

#[tauri::command]
pub async fn send_receipt(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    msg_ids: Vec<String>,
    status: String,
) -> Result<(), String> {
    if let Ok(conn) = db_state.get_conn() {
        let is_group = conn
            .query_row(
                "SELECT is_group FROM chats WHERE address = ?1",
                [&peer_hash],
                |r: &rusqlite::Row| r.get::<_, i32>(0),
            )
            .unwrap_or(0)
            != 0;
        if is_group {
            return Ok(());
        }
    }

    let message = json!({ "type": "receipt", "msgIds": msg_ids, "status": status }).to_string();
    if let Ok(encrypted) =
        internal_signal_encrypt(app.clone(), &net_state, &peer_hash, message).await
    {
        // Flag as is_binary=true, is_media=false
        let _ = internal_send_to_network(
            app.clone(),
            &net_state,
            Some(peer_hash.clone()),
            None,
            None,
            Some(encrypted.to_string().into_bytes()),
            true,
            false,
            None,
            true,
        )
        .await;
    }
    Ok(())
}

#[tauri::command]
pub async fn send_profile_update(
    app: AppHandle,
    _db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    alias: Option<String>,
) -> Result<(), String> {
    let message = json!({
        "type": "profile_update",
        "alias": alias,
        "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }).to_string();

    if let Ok(encrypted) =
        internal_signal_encrypt(app.clone(), &net_state, &peer_hash, message).await
    {
        let payload_bytes = encrypted.to_string().into_bytes();
        let _ = internal_send_to_network(
            app.clone(),
            &net_state,
            Some(peer_hash.clone()),
            None,
            None,
            Some(payload_bytes),
            true,  // is_binary
            false, // is_media
            None,  // transfer_id_override
            false, // is_volatile
        )
        .await;
    }
    Ok(())
}

#[tauri::command]
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
        // Emit with the final resolved status so the frontend never receives 'sending'
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

#[tauri::command]
pub async fn process_outgoing_group_media(
    app: AppHandle,
    payload: OutgoingMedia,
) -> Result<serde_json::Value, String> {
    let msg_id = uuid::Uuid::new_v4().to_string();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("Clock error: {}", e))?
        .as_millis() as i64;
    let transfer_id: u32 = rand::random();

    let canonical_path_opt = if let Some(p) = &payload.file_path {
        let path_buf = std::path::PathBuf::from(p);
        let canonical_path = std::fs::canonicalize(&path_buf)
            .map_err(|_| "Access denied: Invalid or inaccessible file path".to_string())?;

        let metadata =
            std::fs::metadata(&canonical_path).map_err(|e: std::io::Error| e.to_string())?;
        if metadata.len() > 256 * 1024 * 1024u64 {
            return Err("File too large. Maximum size is 256MB.".to_string());
        }
        Some(canonical_path)
    } else if let Some(ref d) = payload.file_data {
        if d.len() as u64 > 256 * 1024 * 1024u64 {
            return Err("File too large. Maximum size is 256MB.".to_string());
        }
        None
    } else {
        return Err("No file path or data provided".into());
    };

    let file_size = if let Some(ref p) = canonical_path_opt {
        std::fs::metadata(p)
            .map_err(|e: std::io::Error| e.to_string())?
            .len()
    } else if let Some(ref d) = payload.file_data {
        d.len() as u64
    } else {
        0
    };

    let db_state = app.state::<DbState>();
    let net_state = app.state::<NetworkState>();
    let own_id = net_state
        .identity_hash
        .lock()
        .map_err(|_| "State poisoned")?
        .clone()
        .unwrap_or_default();

    let db_msg = DbMessage {
        id: msg_id.clone(),
        chat_address: payload.recipient.clone(),
        sender_hash: own_id.clone(),
        content: if payload.msg_type.as_deref() == Some("voice_note") {
            "Voice Note".to_string()
        } else {
            format!("File: {}", payload.file_name.as_deref().unwrap_or("Media"))
        },
        timestamp,
        r#type: payload
            .msg_type
            .clone()
            .unwrap_or_else(|| "file".to_string()),
        status: "sending".to_string(),
        attachment_json: Some(
            serde_json::json!({
                "fileName": payload.file_name,
                "fileType": payload.file_type,
                "size": file_size,
                "duration": payload.duration,
                "thumbnail": payload.thumbnail,
                "originalPath": canonical_path_opt.clone().map(|p| p.to_string_lossy().to_string()),
                "transferId": transfer_id
            })
            .to_string(),
        ),
        is_starred: false,
        is_group: true,
        reply_to_json: payload
            .reply_to
            .as_ref()
            .map(|r| serde_json::to_string(&r).unwrap_or_default()),
    };

    internal_db_save_message(&db_state, db_msg.clone()).await?;
    let _ = app.emit("msg://added", db_msg.clone());

    let app_bg = app.clone();
    let payload_bg = payload.clone();
    let msg_id_bg = msg_id.clone();
    let canonical_path_bg = canonical_path_opt.clone();

    tokio::spawn(async move {
        let app = app_bg;
        let payload = payload_bg;
        let msg_id = msg_id_bg;
        let db_state = app.state::<DbState>();
        let net_state = app.state::<NetworkState>();

        let net_key = XChaCha20Poly1305::generate_key(&mut OsRng);
        {
            if let Ok(mut active) = net_state.active_outgoing_transfers.lock() {
                active.insert(
                    transfer_id,
                    crate::app_state::OutgoingTransferInfo {
                        file_path: canonical_path_bg.clone().unwrap_or_default(),
                        transit_key: net_key.into(),
                    },
                );
            }
        }
        let vault_key_bytes = {
            let lock = db_state.media_key.lock().unwrap();
            lock.clone().unwrap()
        };
        let vault_key = ChaKey::from_slice(&vault_key_bytes);
        let vault_cipher = XChaCha20Poly1305::new(vault_key);

        let media_dir = get_media_dir(&app, &db_state).unwrap();
        let vault_path = media_dir.join(&msg_id);
        let mut vault_file = std::fs::File::create(&vault_path).unwrap();

        let key_b64 = base64::engine::general_purpose::STANDARD.encode(net_key);
        let members = payload.group_members.as_ref().unwrap();

        let announcement = serde_json::json!({
            "type": "file",
            "id": msg_id.clone(),
            "transfer_id": transfer_id,
            "size": file_size,
            "msg_type": payload.msg_type.as_deref().unwrap_or("file"),
            "duration": payload.duration,
            "thumbnail": payload.thumbnail,
            "replyTo": payload.reply_to,
            "isGroup": true,
            "groupId": payload.recipient,
            "groupName": payload.group_name,
            "timestamp": timestamp,
            "bundle": {
                "key": key_b64,
                "file_name": payload.file_name,
                "file_type": payload.file_type
            }
        });

        for member_id in members {
            if member_id == &own_id {
                continue;
            }
            if let Ok(encrypted) = internal_signal_encrypt(
                app.clone(),
                &net_state,
                member_id,
                announcement.to_string(),
            )
            .await
            {
                let routing_hash_str = member_id
                    .split('.')
                    .next()
                    .unwrap_or(member_id.as_str())
                    .to_string();
                let _ = crate::commands::network::transit::internal_send_to_network(
                    app.clone(),
                    &net_state,
                    Some(routing_hash_str),
                    Some(msg_id.clone()),
                    None,
                    Some(encrypted.to_string().into_bytes()),
                    true,
                    false,
                    Some(transfer_id),
                    true,
                )
                .await;
            }
        }

        // Fragments
        let mut reader: Box<dyn std::io::Read + Send> = if let Some(ref p) = canonical_path_bg {
            Box::new(std::io::BufReader::new(std::fs::File::open(p).unwrap()))
        } else if let Some(ref d) = payload.file_data {
            Box::new(std::io::Cursor::new(d.to_vec()))
        } else {
            return;
        };

        let mut fragment_index = 0;
        let mut buffer = vec![0u8; 1279];
        let total_fragments = (file_size as f64 / 1279.0).ceil() as u32;

        loop {
            let mut n = 0;
            let mut read_retries = 0;
            while n < 1279 {
                match reader.read(&mut buffer[n..]) {
                    Ok(0) => break,
                    Ok(read) => {
                        n += read;
                        read_retries = 0;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) if read_retries < 3 => {
                        read_retries += 1;
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    Err(e) => {
                        let _ = app.emit(
                            "network-bin-error",
                            serde_json::json!({
                                "msg_id": msg_id.clone(),
                                "error": format!("Disk read error: {}", e)
                            }),
                        );
                        return;
                    }
                }
            }
            if n == 0 {
                break;
            }
            let chunk = &buffer[..n];

            let v_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
            let v_cipher = vault_cipher.encrypt(&v_nonce, chunk).unwrap();
            let mut retries = 0;
            loop {
                match vault_file
                    .write_all(&v_nonce)
                    .and_then(|_| vault_file.write_all(&v_cipher))
                {
                    Ok(_) => break,
                    Err(e) if retries < 3 => {
                        retries += 1;
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    Err(_e) => return,
                }
            }

            let transit_cipher = XChaCha20Poly1305::new(ChaKey::from_slice(&net_key));
            let t_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
            let t_cipher = transit_cipher.encrypt(&t_nonce, chunk).unwrap();
            let mut packet = Vec::with_capacity(t_cipher.len() + 24);
            packet.extend_from_slice(&t_nonce);
            packet.extend_from_slice(&t_cipher);

            for member_id in members {
                if member_id == &own_id {
                    continue;
                }
                let routing_hash_str = member_id
                    .split('.')
                    .next()
                    .unwrap_or(member_id.as_str())
                    .to_string();
                let mut routing_hash = [0u8; 64];
                let r_bytes = routing_hash_str.as_bytes();
                let r_len = std::cmp::min(r_bytes.len(), 64);
                routing_hash[..r_len].copy_from_slice(&r_bytes[..r_len]);

                let _ = internal_dispatch_fragment(
                    app.clone(),
                    &net_state,
                    routing_hash,
                    Some(msg_id.clone()),
                    transfer_id,
                    fragment_index,
                    total_fragments,
                    &packet,
                    true,
                    false,
                    false,
                )
                .await;
            }
            fragment_index += 1;
        }
        let _ = vault_file.sync_all();

        if fragment_index != total_fragments {
            let _ = app.emit(
                "network-bin-error",
                serde_json::json!({
                    "msg_id": msg_id.clone(),
                    "error": "Incomplete read: fragment mismatch"
                }),
            );
            return;
        }

        // Final Metadata
        let final_meta = serde_json::json!({
            "type": "file",
            "id": msg_id.clone(),
            "transfer_id": transfer_id,
            "size": file_size,
            "msg_type": payload.msg_type.as_deref().unwrap_or("file"),
            "duration": payload.duration,
            "thumbnail": payload.thumbnail,
            "replyTo": payload.reply_to,
            "isGroup": true,
            "groupId": payload.recipient,
            "groupName": payload.group_name,
            "timestamp": timestamp,
            "bundle": {
                "key": key_b64,
                "file_name": payload.file_name,
                "file_type": payload.file_type
            }
        });

        for member_id in members {
            if member_id == &own_id {
                continue;
            }
            if let Ok(encrypted) =
                internal_signal_encrypt(app.clone(), &net_state, member_id, final_meta.to_string())
                    .await
            {
                let routing_hash_str = member_id
                    .split('.')
                    .next()
                    .unwrap_or(member_id.as_str())
                    .to_string();
                let _ = crate::commands::network::transit::internal_send_to_network(
                    app.clone(),
                    &net_state,
                    Some(routing_hash_str),
                    Some(msg_id.clone()),
                    None,
                    Some(encrypted.to_string().into_bytes()),
                    true,
                    false,
                    Some(transfer_id),
                    true,
                )
                .await;
            }
        }

        let final_attachment_obj = serde_json::json!({
            "fileName": payload.file_name,
            "fileType": payload.file_type,
            "size": file_size,
            "duration": payload.duration,
            "thumbnail": payload.thumbnail,
            "originalPath": canonical_path_bg.as_ref().map(|p| p.to_string_lossy().to_string()),
            "transferId": transfer_id,
            "vaultPath": vault_path.to_string_lossy().to_string()
        });

        if let Ok(conn) = db_state.get_conn() {
            let _ = conn.execute(
                "UPDATE messages SET status = 'sent', attachment_json = ?2 WHERE id = ?1",
                rusqlite::params![msg_id, final_attachment_obj.to_string()],
            );
        }
        let _ = app.emit(
            "msg://status",
            serde_json::json!({
                "id": msg_id,
                "status": "sent",
                "chatAddress": payload.recipient,
                "attachment": final_attachment_obj
            }),
        );

        // Success cleanup
        if let Ok(mut active) = net_state.active_outgoing_transfers.lock() {
            active.remove(&transfer_id);
        }
    });

    Ok(serde_json::to_value(&db_msg).unwrap())
}
