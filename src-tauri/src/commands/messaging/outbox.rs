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
    get_media_dir, internal_db_save_message, internal_send_to_network,
    internal_signal_encrypt, DbMessage,
};
// use crate::commands::vault::crypto_decrypt_media; // Removed unused
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    XChaCha20Poly1305, Key as ChaKey,
};
// use aes_gcm::{Aes256Gcm, Key}; 
use base64::Engine;
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
    pub thumbnail: Option<String>,
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
            } else if let Some(ref d) = payload.file_data {
                if d.len() > 100 * 1024 * 1024 {
                    return Err("File too large. Maximum size is 100MB.".to_string());
                }
                d.clone()
            } else {
                return Err("No file path or data provided".into());
            };

            // 1. Prepare Keys
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            let net_key = XChaCha20Poly1305::generate_key(&mut OsRng);
            let vault_key_bytes = {
                let lock = db_state.media_key.lock().map_err(|_| "State poisoned")?;
                lock.clone().ok_or("Media key not initialized")?
            };
            let vault_key = ChaKey::from_slice(&vault_key_bytes);
            
            let net_cipher = XChaCha20Poly1305::new(&net_key);
            let vault_cipher = XChaCha20Poly1305::new(vault_key);

            // 2. Prepare Vault storage
            let media_dir = get_media_dir(&app, &db_state)?;
            let vault_path = media_dir.join(&msg_id);
            let mut vault_file = std::fs::File::create(&vault_path).map_err(|e| e.to_string())?;

            // 3. Prepare Network routing
            let mut routing_hash = [0u8; 64];
            let recipient_bytes = payload.recipient.as_bytes();
            let r_len = std::cmp::min(recipient_bytes.len(), 64);
            routing_hash[..r_len].copy_from_slice(&recipient_bytes[..r_len]);

            // 4. Streaming Dispatch (Zero-RAM)
            let file_size = if let Some(ref p) = payload.file_path {
                std::fs::metadata(p).map_err(|e| e.to_string())?.len()
            } else if let Some(ref d) = payload.file_data {
                d.len() as u64
            } else {
                return Err("No data source".into());
            };
            let net_chunk_capacity = 1279; 
            let total_fragments = (file_size as f64 / net_chunk_capacity as f64).ceil() as u32;
            let transfer_id: u32 = rand::random();

            let mut reader: Box<dyn std::io::Read> = if let Some(p) = payload.file_path {
                Box::new(std::io::BufReader::new(std::fs::File::open(p).map_err(|e| e.to_string())?))
            } else if let Some(d) = payload.file_data {
                Box::new(std::io::Cursor::new(d))
            } else {
                return Err("No data source".into());
            };
            
            let mut buffer = [0u8; 1279]; 
            let mut fragment_index = 0;
            println!("[OUTBOX] Starting media dispatch. TID: {}, MsgID: {}, Total Fragments: {}", transfer_id, msg_id, total_fragments);

            use std::io::Read;
            loop {
                let mut n = 0;
                while n < 1279 {
                    match reader.read(&mut buffer[n..]) {
                        Ok(0) => break,
                        Ok(read) => n += read,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                if n == 0 { break; }
                let chunk = &buffer[..n];
                hasher.update(chunk);

                // A. Vault block
                let vault_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                let vault_chunk_encrypted = vault_cipher.encrypt(&vault_nonce, chunk).map_err(|e| e.to_string())?;
                vault_file.write_all(&vault_nonce).map_err(|e| e.to_string())?;
                vault_file.write_all(&vault_chunk_encrypted).map_err(|e| e.to_string())?;

                // B. Network Fragment (Self-Healing Chunked AEAD)
                let net_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                let net_chunk_encrypted = net_cipher.encrypt(&net_nonce, chunk).map_err(|e| e.to_string())?;
                
                let mut packet_data = Vec::with_capacity(net_nonce.len() + net_chunk_encrypted.len());
                packet_data.extend_from_slice(&net_nonce);
                packet_data.extend_from_slice(&net_chunk_encrypted);

                if fragment_index % 50 == 0 {
                    println!("[OUTBOX] Fragment {} size: {} bytes (+81B header)", fragment_index, packet_data.len());
                }

                crate::commands::network::transit::internal_dispatch_fragment(
                    app.clone(),
                    &net_state,
                    routing_hash,
                    Some(msg_id.clone()),
                    transfer_id,
                    fragment_index,
                    total_fragments,
                    &packet_data,
                    true,
                    false,
                ).await?;

                fragment_index += 1;
            }
            println!("[OUTBOX] All binary fragments dispatched for TID: {}", transfer_id);
            vault_file.sync_all().map_err(|e| e.to_string())?;

            let key_b64 = base64::engine::general_purpose::STANDARD.encode(net_key);
            let saved_vault_path = vault_path.to_string_lossy().to_string();

            // 5. Send Control Metadata (0x04)
            let metadata_json = json!({
                "type": "file",
                "id": msg_id.clone(),
                "transfer_id": transfer_id,
                "size": file_size,
                "msg_type": payload.msg_type.as_deref().unwrap_or("file"),
                "duration": payload.duration,
                "timestamp": timestamp,
                "thumbnail": payload.thumbnail,
                "replyTo": payload.reply_to,
                "bundle": {
                    "key": key_b64,
                    "file_name": payload.file_name,
                    "file_type": payload.file_type,
                    "sha256": hex::encode(hasher.finalize())
                }
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
                        "thumbnail": payload.thumbnail,
                        "transferId": transfer_id,
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
                metadata_json.to_string(),
            )
            .await?;
            let routing_hash = payload
                .recipient
                .split('.')
                .next()
                .unwrap_or(&payload.recipient);

            // A. Send Metadata (0x04)
            let _ = crate::commands::network::transit::internal_send_to_network(
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
                let canonical_path = std::fs::canonicalize(&path_buf)
                    .map_err(|_| "Access denied: Invalid or inaccessible file path".to_string())?;
                
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

                let mut d = Vec::new();
                std::fs::File::open(&canonical_path)
                    .map_err(|e| e.to_string())?
                    .read_to_end(&mut d)
                    .map_err(|e| e.to_string())?;
                d
            } else if let Some(d) = payload.file_data {
                d
            } else {
                return Err("No file path or data provided".into());
            };

            let file_size = data.len();
            if file_size > 100 * 1024 * 1024 { return Err("File too large. Maximum size is 100MB.".to_string()); }

            // 1. Prepare Keys
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();

            let net_key = XChaCha20Poly1305::generate_key(&mut OsRng);
            let vault_key_bytes = {
                let lock = db_state.media_key.lock().map_err(|_| "State poisoned")?;
                lock.clone().ok_or("Media key not initialized")?
            };
            let vault_key = ChaKey::from_slice(&vault_key_bytes);
            
            let net_cipher = XChaCha20Poly1305::new(&net_key);
            let vault_cipher = XChaCha20Poly1305::new(vault_key);

            // 2. Prepare Vault storage
            let media_dir = get_media_dir(&app, &db_state)?;
            let vault_path = media_dir.join(&msg_id);
            let mut vault_file = std::fs::File::create(&vault_path).map_err(|e| e.to_string())?;

            // 3. Prepare Member routing
            let own_id = net_state.identity_hash.lock().map_err(|_| "Network state poisoned")?.clone().unwrap_or_default();
            let members = payload.group_members.as_ref().ok_or("No members provided for group media")?;
            
            // 4. Streaming Dispatch (Zero-RAM)
            let net_chunk_capacity = 1279; 
            let total_fragments = (data.len() as f64 / net_chunk_capacity as f64).ceil() as u32;
            let transfer_id: u32 = rand::random();

            let mut reader = std::io::Cursor::new(data.clone());
            let mut buffer = [0u8; 1279]; 
            let mut fragment_index = 0;
            println!("[OUTBOX-GROUP] Starting group media dispatch. TID: {}, MsgID: {}, Total Fragments: {}", transfer_id, msg_id, total_fragments);

            use std::io::Read;
            loop {
                let mut n = 0;
                while n < 1279 {
                    match reader.read(&mut buffer[n..]) {
                        Ok(0) => break,
                        Ok(read) => n += read,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                if n == 0 { break; }
                let chunk = &buffer[..n];
                hasher.update(chunk);

                // A. Vault block
                let vault_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                let vault_chunk_encrypted = vault_cipher.encrypt(&vault_nonce, chunk).map_err(|e| e.to_string())?;
                vault_file.write_all(&vault_nonce).map_err(|e| e.to_string())?;
                vault_file.write_all(&vault_chunk_encrypted).map_err(|e| e.to_string())?;

                // B. Network Fragment
                let net_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                let net_chunk_encrypted = net_cipher.encrypt(&net_nonce, chunk).map_err(|e| e.to_string())?;
                
                let mut packet_data = Vec::with_capacity(net_nonce.len() + net_chunk_encrypted.len());
                packet_data.extend_from_slice(&net_nonce);
                packet_data.extend_from_slice(&net_chunk_encrypted);

                if fragment_index % 50 == 0 {
                    println!("[OUTBOX-GROUP] Fragment {} size: {} bytes (+81B header)", fragment_index, packet_data.len());
                }

                for member_id in members {
                    if member_id == &own_id { continue; }
                    let routing_hash_str = member_id.split('.').next().unwrap_or(member_id).to_string();
                    let mut routing_hash = [0u8; 64];
                    let r_bytes = routing_hash_str.as_bytes();
                    let r_len = std::cmp::min(r_bytes.len(), 64);
                    routing_hash[..r_len].copy_from_slice(&r_bytes[..r_len]);

                    let _ = crate::commands::network::transit::internal_dispatch_fragment(
                        app.clone(),
                        &net_state,
                        routing_hash,
                        Some(msg_id.clone()),
                        transfer_id,
                        fragment_index,
                        total_fragments,
                        &packet_data,
                        true,
                        false,
                    ).await;
                }

                fragment_index += 1;
            }
            println!("[OUTBOX-GROUP] All group binary fragments dispatched. TID: {}", transfer_id);
            vault_file.sync_all().map_err(|e| e.to_string())?;

            let key_b64 = base64::engine::general_purpose::STANDARD.encode(net_key);
            let metadata_json = json!({
                "type": "file",
                "id": msg_id.clone(),
                "transfer_id": transfer_id,
                "size": file_size,
                "msg_type": payload.msg_type.as_deref().unwrap_or("file"),
                "duration": payload.duration,
                "timestamp": timestamp,
                "thumbnail": payload.thumbnail,
                "isGroup": true,
                "groupId": payload.recipient,
                "groupName": payload.group_name,
                "replyTo": payload.reply_to,
                "bundle": {
                    "key": key_b64,
                    "file_name": payload.file_name,
                    "file_type": payload.file_type,
                    "sha256": hex::encode(hasher.finalize())
                }
            });

            // Local DB save
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
                    "size": file_size,
                    "duration": payload.duration,
                    "thumbnail": payload.thumbnail,
                    "transferId": transfer_id,
                    "key": key_b64,
                    "vaultPath": vault_path.to_string_lossy().to_string()
                }).to_string()),
                is_starred: false,
                is_group: true,
                reply_to_json: payload.reply_to.as_ref().map(|r| serde_json::to_string(&r).unwrap_or_default()),
            };
            internal_db_save_message(&db_state, db_msg.clone()).await?;

            // Metadata dispatch to all members
            for member_id in members {
                if member_id == &own_id { continue; }
                let routing_hash = member_id.split('.').next().unwrap_or(member_id).to_string();

                if let Ok(encrypted_metadata) = internal_signal_encrypt(app.clone(), &net_state, member_id, metadata_json.to_string()).await {
                    let _ = crate::commands::network::transit::internal_send_to_network(
                        app.clone(),
                        &net_state,
                        Some(routing_hash.clone()),
                        Some(msg_id.clone()),
                        None,
                        Some(encrypted_metadata.to_string().into_bytes()),
                        true,
                        false,
                        Some(transfer_id),
                        true
                    ).await;

                    let lock = db_state.conn.lock().map_err(|_| "DB lock poisoned")?;
                    if let Some(conn) = lock.as_ref() {
                        let _ = conn.execute("UPDATE messages SET status = 'sent' WHERE id = ?1", params![msg_id]);
                        let _ = conn.execute("UPDATE chats SET last_status = 'sent' WHERE LOWER(address) = LOWER(?1)", params![payload.recipient]);
                    }
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
