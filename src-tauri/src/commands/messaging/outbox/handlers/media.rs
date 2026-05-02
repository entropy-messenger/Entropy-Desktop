use crate::app_state::{DbState, NetworkState, OutgoingTransferInfo};
use crate::commands::{
    get_media_dir, internal_db_save_message, internal_dispatch_fragment, internal_send_to_network,
    internal_signal_encrypt, DbMessage,
};
use base64::Engine;
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Key as ChaKey, XChaCha20Poly1305,
};
use serde_json::json;
use std::io::{Read, Write};
use tauri::{AppHandle, Emitter, Manager};
use super::super::OutgoingMedia;

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

    let (canonical_path, file_size) = validate_media_payload(&payload)?;

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
                "originalPath": canonical_path.clone().map(|p| p.to_string_lossy().to_string()),
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
            let _ = internal_db_save_message(&db_state, db_msg.clone()).await;
            let _ = app.emit("msg://added", db_msg);
        });
    }

    let recipients = vec![payload.recipient.clone()];
    spawn_transfer_task(app, payload, msg_id.clone(), recipients, transfer_id, timestamp, file_size, canonical_path, false);

    Ok(serde_json::to_value(&db_msg).unwrap())
}

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

    let (canonical_path, file_size) = validate_media_payload(&payload)?;

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
                "originalPath": canonical_path.clone().map(|p| p.to_string_lossy().to_string()),
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

    let mut recipients = payload.group_members.clone().ok_or("Group members missing")?;
    recipients.retain(|r| r != &own_id);

    spawn_transfer_task(app, payload, msg_id.clone(), recipients, transfer_id, timestamp, file_size, canonical_path, true);

    Ok(serde_json::to_value(&db_msg).unwrap())
}

fn validate_media_payload(payload: &OutgoingMedia) -> Result<(Option<std::path::PathBuf>, u64), String> {
    if let Some(p) = &payload.file_path {
        let path_buf = std::path::PathBuf::from(p);
        let canonical_path = std::fs::canonicalize(&path_buf)
            .map_err(|_| "Access denied: Invalid or inaccessible file path".to_string())?;

        let metadata = std::fs::metadata(&canonical_path).map_err(|e| e.to_string())?;
        if metadata.len() > 256 * 1024 * 1024u64 {
            return Err("File too large. Maximum size is 256MB.".to_string());
        }
        Ok((Some(canonical_path), metadata.len()))
    } else if let Some(ref d) = payload.file_data {
        if d.len() as u64 > 256 * 1024 * 1024u64 {
            return Err("File too large. Maximum size is 256MB.".to_string());
        }
        Ok((None, d.len() as u64))
    } else {
        Err("No file path or data provided".into())
    }
}

fn spawn_transfer_task(
    app: AppHandle,
    payload: OutgoingMedia,
    msg_id: String,
    recipients: Vec<String>,
    transfer_id: u32,
    timestamp: i64,
    file_size: u64,
    canonical_path: Option<std::path::PathBuf>,
    is_group: bool,
) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let db_state = app.state::<DbState>();
            let net_state = app.state::<NetworkState>();

            let net_key = XChaCha20Poly1305::generate_key(&mut OsRng);
            {
                if let Ok(mut active) = net_state.active_outgoing_transfers.lock() {
                    active.insert(
                        transfer_id,
                        OutgoingTransferInfo {
                            file_path: canonical_path.clone().unwrap_or_default(),
                            transit_key: net_key.into(),
                        },
                    );
                }
            }

            let vault_key_bytes = db_state.media_key.lock().unwrap().clone().unwrap();
            let vault_key = ChaKey::from_slice(&vault_key_bytes);
            let vault_cipher = XChaCha20Poly1305::new(vault_key);

            let media_dir = get_media_dir(&app, &db_state).unwrap();
            let vault_path = media_dir.join(&msg_id);
            let mut vault_file = std::fs::File::create(&vault_path).unwrap();

            let key_b64 = base64::engine::general_purpose::STANDARD.encode(net_key);

            // 1. Announcements
            let mut announcement = serde_json::json!({
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
            if is_group {
                if let Some(obj) = announcement.as_object_mut() {
                    obj.insert("isGroup".to_string(), json!(true));
                    obj.insert("groupId".to_string(), json!(payload.recipient));
                    obj.insert("groupName".to_string(), json!(payload.group_name));
                }
            }

            for recipient in &recipients {
                if let Ok(encrypted) = internal_signal_encrypt(
                    app.clone(),
                    &net_state,
                    recipient,
                    announcement.to_string(),
                )
                .await
                {
                    let routing_hash = recipient.split('.').next().unwrap_or(recipient).to_string();
                    let _ = internal_send_to_network(
                        app.clone(),
                        &net_state,
                        Some(routing_hash),
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

            let mut reader: Box<dyn std::io::Read + Send> = if let Some(ref p) = canonical_path {
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
                                json!({ "msg_id": msg_id.clone(), "error": format!("Disk read error: {}", e) }),
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
                let _ = vault_file
                    .write_all(&v_nonce)
                    .and_then(|_| vault_file.write_all(&v_cipher));

                let transit_cipher = XChaCha20Poly1305::new(ChaKey::from_slice(&net_key));
                let t_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                let t_cipher = transit_cipher.encrypt(&t_nonce, chunk).unwrap();
                let mut packet = Vec::with_capacity(t_cipher.len() + 24);
                packet.extend_from_slice(&t_nonce);
                packet.extend_from_slice(&t_cipher);

                for recipient in &recipients {
                    let routing_hash_str =
                        recipient.split('.').next().unwrap_or(recipient).to_string();
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
                        true,
                        false,
                    )
                    .await;
                }
                fragment_index += 1;
            }
            let _ = vault_file.sync_all();

            if let Some(thumb_b64) = &payload.thumbnail {
                if let Ok(thumb_bytes) = base64::engine::general_purpose::STANDARD.decode(thumb_b64)
                {
                    let thumb_path = media_dir.join(format!("{}_thumb", msg_id));
                    let v_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                    if let Ok(v_cipher) = vault_cipher.encrypt(&v_nonce, thumb_bytes.as_slice()) {
                        if let Ok(mut f) = std::fs::File::create(&thumb_path) {
                            let _ = f
                                .write_all(&v_nonce)
                                .and_then(|_| f.write_all(&v_cipher))
                                .and_then(|_| f.sync_all());
                        }
                    }
                }
            }

            for recipient in &recipients {
                if let Ok(encrypted) = internal_signal_encrypt(
                    app.clone(),
                    &net_state,
                    recipient,
                    announcement.to_string(),
                )
                .await
                {
                    let routing_hash = recipient.split('.').next().unwrap_or(recipient).to_string();
                    let _ = internal_send_to_network(
                        app.clone(),
                        &net_state,
                        Some(routing_hash),
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

            // 5. Success Updates
            let final_attachment_obj = json!({
                "fileName": payload.file_name,
                "fileType": payload.file_type,
                "size": file_size,
                "duration": payload.duration,
                "thumbnail": payload.thumbnail,
                "originalPath": canonical_path.as_ref().map(|p| p.to_string_lossy().to_string()),
                "transferId": transfer_id,
                "vaultPath": vault_path.to_string_lossy().to_string()
            });

            if let Ok(conn) = db_state.get_conn() {
                let _ = conn.execute(
                    "UPDATE messages SET status = 'sent', attachment_json = ?2 WHERE id = ?1",
                    rusqlite::params![msg_id, final_attachment_obj.to_string()],
                );
            }
            let _ = app.emit("msg://status", json!({
                "id": msg_id, "status": "sent", "chatAddress": payload.recipient, "attachment": final_attachment_obj
            }));

            if let Ok(mut active) = net_state.active_outgoing_transfers.lock() {
                active.remove(&transfer_id);
            }
        });
    });
}
