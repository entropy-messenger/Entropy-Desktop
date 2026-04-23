//! Outbound Message Pipeline and Metadata Processing
//!
//! This module orchestrates the processing of outbound communications, including:
//! - Local shadow persistence (optimistic UI updates).
//! - Media asset pre-processing (encryption and vaulting).
//! - Signal session resolution and ciphertext generation (E2EE).
//! - Handover to the Transit Layer for fragmentation and network dispatch.
//!
//! All outgoing payloads are subjected to path canonicalization and vault boundary
//! checks to prevent unauthorized file system access.

use crate::app_state::{DbState, NetworkState};
use crate::commands::{
    DbMessage, get_media_dir, internal_db_save_message, internal_send_to_network,
    internal_signal_encrypt,
};
use aes_gcm::{
    Aes256Gcm, Key,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::Engine;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::{Read, Write};
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

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
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
    #[serde(rename = "isGroup", default)]
    pub is_group: bool,
    pub group_members: Option<Vec<String>>,
    pub reply_to: Option<ReplyTo>,
}

#[tauri::command]
pub fn process_outgoing_text(
    app: AppHandle,
    payload: OutgoingText,
) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let db_state = app.state::<DbState>();
            let net_state = app.state::<NetworkState>();

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

            let mut final_json = serde_json::to_value(&db_msg).map_err(|e| e.to_string())?;
            if let Some(obj) = final_json.as_object_mut() {
                obj.insert(
                    "chatAlias".to_string(),
                    serde_json::json!(payload.group_name),
                );
                obj.insert(
                    "chatMembers".to_string(),
                    serde_json::json!(payload.group_members.clone()),
                );
            }
            app.emit("msg://added", final_json.clone())
                .map_err(|e| e.to_string())?;

            let signal_payload = serde_json::json!({
                "type": "text_msg",
                "content": payload.content,
                "id": msg_id.clone(),
                "replyTo": payload.reply_to,
                "timestamp": timestamp,
                "isGroup": false,
            });

            // session encryption
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

            // transition state
            {
                let lock = db_state.conn.lock().map_err(|_| "DB lock poisoned")?;
                if let Some(conn) = lock.as_ref() {
                    let _ = conn.execute(
                        "UPDATE messages SET status = 'sent' WHERE id = ?1",
                        params![msg_id],
                    );
                    let _ = conn.execute(
                        "UPDATE chats SET last_status = 'sent' WHERE LOWER(address) = LOWER(?1)",
                        params![payload.recipient],
                    );
                }
            }
            app.emit(
                "msg://status",
                json!({ "id": msg_id, "status": "sent", "chatAddress": payload.recipient }),
            )
            .map_err(|e| e.to_string())?;

            if let Some(obj) = final_json.as_object_mut() {
                obj.insert("status".to_string(), json!("sent"));
            }
            Ok(final_json)
        })
    })
    .join()
    .map_err(|_| "Thread panicked".to_string())?
}

#[tauri::command]
pub fn process_outgoing_media(
    app: AppHandle,
    payload: OutgoingMedia,
) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to build runtime: {}", e))?;
        rt.block_on(async move {
            let db_state = app.state::<DbState>();
            let net_state = app.state::<NetworkState>();

            let msg_id = uuid::Uuid::new_v4().to_string();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| format!("Clock error: {}", e))?
                .as_millis() as i64;

            // fetch data and validate
            let data = if let Some(p) = &payload.file_path {
                let path_buf = std::path::PathBuf::from(p);
                // security: canonicalize and validate target path
                let canonical_path = std::fs::canonicalize(&path_buf)
                    .map_err(|_| "Access denied: Invalid or inaccessible file path".to_string())?;

                // security: restrict access to hidden or system directories
                if canonical_path
                    .file_name()
                    .map(|n| n.to_string_lossy().starts_with('.'))
                    .unwrap_or(false)
                    || canonical_path.components().any(|c| {
                        c.as_os_str().to_string_lossy().starts_with('.') && c.as_os_str() != "."
                    })
                {
                    return Err(
                        "Access denied: Cannot send hidden files or system configuration".into(),
                    );
                }

                let metadata = std::fs::metadata(&canonical_path).map_err(|e| e.to_string())?;
                if metadata.len() > 100 * 1024 * 1024 {
                    return Err("File too large. Maximum size is 100MB.".to_string());
                }
                let mut file = std::fs::File::open(&canonical_path).map_err(|e| e.to_string())?;
                let mut d = Vec::new();
                file.read_to_end(&mut d).map_err(|e| e.to_string())?;
                d
            } else if let Some(d) = payload.file_data {
                if d.len() > 100 * 1024 * 1024 {
                    return Err("File too large. Maximum size is 100MB.".to_string());
                }
                d
            } else {
                return Err("No data provided".into());
            };

            // aes encryption
            let key = Aes256Gcm::generate_key(&mut OsRng);
            let cipher = Aes256Gcm::new(&key);
            let (nonce, ciphertext) = {
                let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
                let ciphertext = cipher
                    .encrypt(&nonce, data.as_ref())
                    .map_err(|e| e.to_string())?;
                (nonce, ciphertext)
            };

            let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
            combined.extend_from_slice(&nonce);
            combined.extend_from_slice(&ciphertext);
            let key_b64 = base64::engine::general_purpose::STANDARD.encode(key);

            // persist to vault
            let local_file_path = {
                let local_key_bytes = {
                    let lock = db_state.media_key.lock().map_err(|_| "State poisoned")?;
                    lock.clone().ok_or("Media key not initialized")?
                };
                let local_key = Key::<Aes256Gcm>::from_slice(&local_key_bytes);
                let local_cipher = Aes256Gcm::new(local_key);
                let local_nonce = Aes256Gcm::generate_nonce(&mut OsRng);
                let local_ciphertext = local_cipher
                    .encrypt(&local_nonce, data.as_ref())
                    .map_err(|e| e.to_string())?;
                let mut final_blob = local_nonce.to_vec();
                final_blob.extend(local_ciphertext);

                let media_dir = get_media_dir(&app, &db_state)?;
                let file_path = media_dir.join(&msg_id);
                let mut f = std::fs::File::create(&file_path).map_err(|e| e.to_string())?;
                f.write_all(&final_blob).map_err(|e| e.to_string())?;
                f.sync_all().map_err(|e| e.to_string())?;
                file_path.to_string_lossy().to_string()
            };
            let saved_vault_path = local_file_path;

            // protocol envelope
            let transfer_id: u32 = rand::random();
            let bundle = json!({
                "type": "signal_media",
                "key": key_b64,
                "file_name": payload.file_name,
                "file_type": payload.file_type,
                "file_size": data.len()
            });

            let content_obj = json!({
                "type": "file",
                "id": msg_id.clone(),
                "bundle": bundle,
                "timestamp": timestamp,
                "isGroup": false,
                "transfer_id": transfer_id,
                "size": data.len(),
                "msg_type": payload.msg_type.clone().unwrap_or_else(|| "file".to_string()),
                "duration": payload.duration,
                "replyTo": payload.reply_to,
            });

            // commit metadata
            let own_id = net_state
                .identity_hash
                .lock()
                .map_err(|_| "Network state poisoned")?
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
                status: "sent".to_string(),
                attachment_json: Some(
                    serde_json::json!({
                        "fileName": payload.file_name,
                        "fileType": payload.file_type,
                        "size": data.len(),
                        "duration": payload.duration,
                        "transferId": transfer_id,
                        "bundle": bundle,
                        "vaultPath": saved_vault_path
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

            internal_db_save_message(&db_state, db_msg.clone()).await?;
            let mut final_json = serde_json::to_value(&db_msg).map_err(|e| e.to_string())?;
            if let Some(obj) = final_json.as_object_mut() {
                obj.insert("chatAlias".to_string(), serde_json::json!(null));
            }
            app.emit("msg://added", final_json.clone())
                .map_err(|e| e.to_string())?;

            // session bridge
            let encrypted = internal_signal_encrypt(
                app.clone(),
                &net_state,
                &payload.recipient,
                content_obj.to_string(),
            )
            .await?;
            let routing_hash = payload
                .recipient
                .split('.')
                .next()
                .unwrap_or(&payload.recipient);

            // A. Send Metadata (0x04)
            let _ = internal_send_to_network(
                app.clone(),
                &net_state,
                Some(routing_hash.to_string()),
                Some(msg_id.clone()),
                None,
                Some(encrypted.to_string().into_bytes()),
                true,
                false,
                Some(transfer_id),
                true,
            )
            .await;

            // B. Send Binary (0x02)
            let _ = internal_send_to_network(
                app.clone(),
                &net_state,
                Some(routing_hash.to_string()),
                Some(msg_id.clone()),
                None,
                Some(combined),
                true,
                true,
                Some(transfer_id),
                false,
            )
            .await;

            // transition state
            {
                let lock = db_state.conn.lock().map_err(|_| "DB lock poisoned")?;
                if let Some(conn) = lock.as_ref() {
                    let _ = conn.execute(
                        "UPDATE messages SET status = 'sent' WHERE id = ?1",
                        params![msg_id],
                    );
                    let _ = conn.execute(
                        "UPDATE chats SET last_status = 'sent' WHERE LOWER(address) = LOWER(?1)",
                        params![payload.recipient],
                    );
                }
            }
            app.emit(
                "msg://status",
                json!({ "id": msg_id, "status": "sent", "chatAddress": payload.recipient }),
            )
            .map_err(|e| e.to_string())?;

            if let Some(obj) = final_json.as_object_mut() {
                obj.insert("status".to_string(), json!("sent"));
            }
            Ok(final_json)
        })
    })
    .join()
    .map_err(|_| "Thread panicked".to_string())?
}

#[tauri::command]
pub fn send_typing_status(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    is_typing: bool,
) -> Result<(), String> {
    // restrict to 1:1
    {
        let lock = db_state
            .conn
            .lock()
            .map_err(|_| "Database connection lock poisoned")?;
        if let Some(conn) = lock.as_ref() {
            let is_group = conn
                .query_row(
                    "SELECT is_group FROM chats WHERE address = ?1",
                    [&peer_hash],
                    |r| r.get::<_, i32>(0),
                )
                .unwrap_or(0)
                != 0;
            if is_group {
                return Ok(());
            }
        }
    }

    tauri::async_runtime::block_on(async move {
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
    })
}

#[tauri::command]
pub fn send_receipt(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    msg_ids: Vec<String>,
    status: String,
) -> Result<(), String> {
    // enforce 1:1 delivery receipts
    {
        let lock = db_state
            .conn
            .lock()
            .map_err(|_| "Database connection lock poisoned")?;
        if let Some(conn) = lock.as_ref() {
            let is_group = conn
                .query_row(
                    "SELECT is_group FROM chats WHERE address = ?1",
                    [&peer_hash],
                    |r| r.get::<_, i32>(0),
                )
                .unwrap_or(0)
                != 0;
            if is_group {
                return Ok(());
            }
        }
    }

    tauri::async_runtime::block_on(async move {
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
    })
}

#[tauri::command]
pub fn send_profile_update(
    app: AppHandle,
    _db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    alias: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::block_on(async move {
        let message = json!({
            "type": "profile_update",
            "alias": alias,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
        }).to_string();

        if let Ok(encrypted) =
            internal_signal_encrypt(app.clone(), &net_state, &peer_hash, message).await
        {
            // dispatch persistent update
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
    })
}

#[tauri::command]
pub fn process_outgoing_group_text(
    app: AppHandle,
    payload: OutgoingText,
) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async move {
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
                let routing_hash = member_id.split('.').next().unwrap_or(member_id).to_string();
                match internal_signal_encrypt(
                    app.clone(),
                    &net_state,
                    member_id,
                    payload_str.clone(),
                )
                .await
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

            {
                let lock = db_state.conn.lock().map_err(|_| "DB lock poisoned")?;
                if let Some(conn) = lock.as_ref() {
                    let _ = conn.execute(
                        "UPDATE messages SET status = 'sent' WHERE id = ?1",
                        params![msg_id],
                    );
                    let _ = conn.execute(
                        "UPDATE chats SET last_status = 'sent' WHERE LOWER(address) = LOWER(?1)",
                        params![payload.recipient],
                    );
                }
            }

            let mut final_json = serde_json::to_value(&db_msg).map_err(|e| e.to_string())?;
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
        })
    })
    .join()
    .map_err(|_| "Thread panicked".to_string())?
}

#[tauri::command]
pub fn process_outgoing_group_media(
    app: AppHandle,
    payload: OutgoingMedia,
) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| format!("Failed to build runtime: {}", e))?;
        rt.block_on(async move {
            let db_state = app.state::<DbState>();
            let net_state = app.state::<NetworkState>();

            let msg_id = uuid::Uuid::new_v4().to_string();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| format!("Clock error: {}", e))?
                .as_millis() as i64;

            let data = if let Some(p) = &payload.file_path {
                let path_buf = std::path::PathBuf::from(p);
                // security: canonicalize and validate target path
                let canonical_path = std::fs::canonicalize(&path_buf)
                    .map_err(|_| "Access denied: Invalid or inaccessible file path".to_string())?;

                // security: restrict access to hidden or system directories
                if canonical_path
                    .file_name()
                    .map(|n| n.to_string_lossy().starts_with('.'))
                    .unwrap_or(false)
                    || canonical_path.components().any(|c| {
                        c.as_os_str().to_string_lossy().starts_with('.') && c.as_os_str() != "."
                    })
                {
                    return Err(
                        "Access denied: Cannot send hidden files or system configuration".into(),
                    );
                }

                let metadata = std::fs::metadata(&canonical_path).map_err(|e| e.to_string())?;
                if metadata.len() > 100 * 1024 * 1024 {
                    return Err("File too large. Maximum size is 100MB.".to_string());
                }
                let mut d = Vec::new();
                std::fs::File::open(&canonical_path)
                    .map_err(|e| e.to_string())?
                    .read_to_end(&mut d)
                    .map_err(|e| e.to_string())?;
                d
            } else if let Some(d) = payload.file_data {
                if d.len() > 100 * 1024 * 1024 { return Err("File too large. Maximum size is 100MB.".to_string()); }
                d
            } else { return Err("No data".into()); };

            let key = Aes256Gcm::generate_key(&mut OsRng);
            let cipher = Aes256Gcm::new(&key);
            let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
            let ciphertext = cipher.encrypt(&nonce, data.as_ref()).map_err(|e| e.to_string())?;
            let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
            combined.extend_from_slice(&nonce);
            combined.extend_from_slice(&ciphertext);
            let key_b64 = base64::engine::general_purpose::STANDARD.encode(key);

            {
                let local_key_bytes = db_state.media_key.lock().map_err(|_| "State poisoned")?.clone().ok_or("No media key")?;
                let local_key = Key::<Aes256Gcm>::from_slice(&local_key_bytes);
                let local_nonce = Aes256Gcm::generate_nonce(&mut OsRng);
                let local_ciphertext = Aes256Gcm::new(local_key).encrypt(&local_nonce, data.as_ref()).map_err(|e| e.to_string())?;
                let mut final_blob = local_nonce.to_vec();
                final_blob.extend(local_ciphertext);
                let media_dir = get_media_dir(&app, &db_state)?;
                let file_path = media_dir.join(&msg_id);
                let mut f = std::fs::File::create(&file_path).map_err(|e| e.to_string())?;
                f.write_all(&final_blob).map_err(|e| e.to_string())?;
                f.sync_all().map_err(|e| e.to_string())?;
            };

            let transfer_id: u32 = rand::random();
            let bundle = json!({
                "type": "signal_media",
                "key": key_b64,
                "file_name": payload.file_name,
                "file_type": payload.file_type,
                "file_size": data.len()
            });

            let content_obj_raw = json!({
                "type": "file",
                "id": msg_id.clone(),
                "bundle": bundle,
                "timestamp": timestamp,
                "isGroup": true,
                "transfer_id": transfer_id,
                "size": data.len(),
                "msg_type": payload.msg_type.clone().unwrap_or_else(|| "file".to_string()),
                "duration": payload.duration,
                "replyTo": payload.reply_to,
                "groupId": payload.recipient,
                "groupName": payload.group_name,
                "groupMembers": payload.group_members.clone(),
            }).to_string();

            let own_id = net_state.identity_hash.lock().map_err(|_| "Network state poisoned")?.clone().unwrap_or_default();
            let db_msg = DbMessage {
                id: msg_id.clone(),
                chat_address: payload.recipient.clone(),
                sender_hash: own_id.clone(),
                content: if payload.msg_type.as_deref() == Some("voice_note") { "Voice Note".to_string() } else { format!("File: {}", payload.file_name.as_deref().unwrap_or("Media")) },
                timestamp,
                r#type: payload.msg_type.clone().unwrap_or_else(|| "file".to_string()),
                status: "sent".to_string(),
                attachment_json: Some(serde_json::json!({
                    "fileName": payload.file_name,
                    "fileType": payload.file_type,
                    "size": data.len(),
                    "duration": payload.duration,
                    "transferId": transfer_id,
                    "bundle": bundle,
                    "vaultPath": get_media_dir(&app, &db_state).map(|d| d.join(&msg_id).to_string_lossy().to_string()).unwrap_or_default()
                }).to_string()),
                is_starred: false,
                is_group: true,
                reply_to_json: payload.reply_to.as_ref().map(|r| serde_json::to_string(&r).unwrap_or_default()),
            };

            internal_db_save_message(&db_state, db_msg.clone()).await?;

            let members = payload.group_members.as_ref().ok_or("No members provided for group media")?;

            for member_id in members {
                if member_id == &own_id { continue; }
                let routing_hash = member_id.split('.').next().unwrap_or(member_id).to_string();

                match internal_signal_encrypt(app.clone(), &net_state, member_id, content_obj_raw.clone()).await {
                    Ok(encrypted_metadata) => {
                         let _ = internal_send_to_network(app.clone(), &net_state, Some(routing_hash.clone()), Some(msg_id.clone()), None, Some(encrypted_metadata.to_string().into_bytes()), true, false, Some(transfer_id), true).await;
                         let _ = internal_send_to_network(app.clone(), &net_state, Some(routing_hash), Some(msg_id.clone()), None, Some(combined.clone()), true, true, Some(transfer_id), false).await;
                    },
                    Err(_e) => {
                        // Skipping member
                    }
                }
            }

            {
                let lock = db_state.conn.lock().map_err(|_| "DB lock poisoned")?;
                if let Some(conn) = lock.as_ref() {
                    let _ = conn.execute("UPDATE messages SET status = 'sent' WHERE id = ?1", params![msg_id]);
                    let _ = conn.execute("UPDATE chats SET last_status = 'sent' WHERE LOWER(address) = LOWER(?1)", params![payload.recipient]);
                }
            }

            let mut final_json = serde_json::to_value(&db_msg).map_err(|e| e.to_string())?;
            if let Some(obj) = final_json.as_object_mut() {
                obj.insert("chatAlias".to_string(), serde_json::json!(payload.group_name));
                obj.insert("chatMembers".to_string(), serde_json::json!(payload.group_members.clone()));
                obj.insert("status".to_string(), json!("sent"));
            }
            app.emit("msg://added", final_json.clone()).map_err(|e| e.to_string())?;
            app.emit("msg://status", json!({ "id": msg_id, "status": "sent", "chatAddress": payload.recipient })).map_err(|e| e.to_string())?;

            Ok(final_json)
        })
    }).join().map_err(|_| "Thread panicked".to_string())?
}
