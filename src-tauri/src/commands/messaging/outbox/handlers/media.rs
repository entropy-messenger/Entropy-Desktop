use super::super::OutgoingMedia;
use crate::app_state::{DbState, NetworkState};
use crate::commands::{
    DbMessage, get_media_dir, internal_db_save_message, internal_request, internal_send_to_network,
    internal_signal_encrypt,
};
use base64::Engine;
use chacha20poly1305::{
    Key as ChaKey, XChaCha20Poly1305,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use serde_json::json;
use std::io::Read;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncRead;
use tokio_util::io::ReaderStream;

pub fn process_outgoing_media(
    app: AppHandle,
    payload: OutgoingMedia,
) -> Result<serde_json::Value, String> {
    let msg_id = uuid::Uuid::new_v4().to_string();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("Clock error: {}", e))?
        .as_millis() as i64;

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
                "isDownloaded": true,
            })
            .to_string(),
        ),
        is_starred: false,
        is_group: false,
        reply_to_json: payload
            .reply_to
            .as_ref()
            .map(|r| serde_json::to_string(&r).unwrap_or_default()),
        reactions_json: None,
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
    spawn_upload_task(
        app,
        payload,
        recipients,
        msg_id,
        timestamp,
        canonical_path,
        file_size,
    );

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
                "isDownloaded": true,
            })
            .to_string(),
        ),
        is_starred: false,
        is_group: true,
        reply_to_json: payload
            .reply_to
            .as_ref()
            .map(|r| serde_json::to_string(&r).unwrap_or_default()),
        reactions_json: None,
    };

    internal_db_save_message(&db_state, db_msg.clone()).await?;
    let _ = app.emit("msg://added", db_msg.clone());

    let mut recipients = payload
        .group_members
        .clone()
        .ok_or("Group members missing")?;
    recipients.retain(|r| r != &own_id);

    spawn_upload_task(
        app,
        payload,
        recipients,
        msg_id,
        timestamp,
        canonical_path,
        file_size,
    );

    Ok(serde_json::to_value(&db_msg).unwrap())
}

fn validate_media_payload(
    payload: &OutgoingMedia,
) -> Result<(Option<std::path::PathBuf>, u64), String> {
    if let Some(p) = &payload.file_path {
        let path_buf = std::path::PathBuf::from(p);
        let canonical_path = std::fs::canonicalize(&path_buf)
            .map_err(|_| "Access denied: Invalid or inaccessible file path".to_string())?;

        let metadata = std::fs::metadata(&canonical_path).map_err(|e| e.to_string())?;
        if metadata.len() > 268_435_456u64 {
            return Err("File too large. Maximum size is 256MB.".to_string());
        }
        Ok((Some(canonical_path), metadata.len()))
    } else if let Some(ref d) = payload.file_data {
        if d.len() as u64 > 268_435_456u64 {
            return Err("File too large. Maximum size is 256MB.".to_string());
        }
        Ok((None, d.len() as u64))
    } else {
        Err("No file path or data provided".into())
    }
}

fn spawn_upload_task(
    app: AppHandle,
    payload: OutgoingMedia,
    recipients: Vec<String>,
    msg_id: String,
    timestamp: i64,
    canonical_path: Option<std::path::PathBuf>,
    file_size: u64,
) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let db_state = app.state::<DbState>();
            let net_state = app.state::<NetworkState>();

            // 1. Generate one-time transit key
            let net_key = XChaCha20Poly1305::generate_key(&mut OsRng);
            let transit_cipher = XChaCha20Poly1305::new(ChaKey::from_slice(&net_key));

            // 2. Prepare vault file
            let vault_key_bytes = db_state.media_key.lock().unwrap().clone().unwrap();
            let vault_key = ChaKey::from_slice(&vault_key_bytes);
            let vault_cipher = XChaCha20Poly1305::new(vault_key);
            let media_dir = get_media_dir(&app, &db_state).unwrap();
            let vault_path = media_dir.join(&msg_id);

            // 3. Stream-read source in 8MB blocks.
            //    Each block is encrypted for transit (→temp file) and vault (→vault_file).
            const BLOCK_SIZE: usize = 8_388_608;

            let mut reader: Box<dyn std::io::Read + Send> = if let Some(ref p) = canonical_path {
                Box::new(std::io::BufReader::new(std::fs::File::open(p).unwrap()))
            } else if let Some(ref d) = payload.file_data {
                Box::new(std::io::Cursor::new(d.clone()))
            } else {
                return;
            };

            // Temp file for the transit-encrypted blob (streamed to relay, not held in RAM)
            let transit_temp = media_dir.join(format!(".transit_{}", msg_id));
            let mut transit_file = std::fs::File::create(&transit_temp).unwrap();

            {
                use std::io::Write;
                let mut vault_file = std::fs::File::create(&vault_path).unwrap();
                let mut buffer = vec![0u8; BLOCK_SIZE];

                loop {
                    let mut n = 0;
                    while n < BLOCK_SIZE {
                        match reader.read(&mut buffer[n..]) {
                            Ok(0) => break,
                            Ok(read) => n += read,
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(_) => break,
                        }
                    }
                    if n == 0 {
                        break;
                    }
                    let chunk = &buffer[..n];

                    // Transit encrypt → write to temp file (not held in RAM)
                    let t_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                    let t_cipher = transit_cipher
                        .encrypt(&t_nonce, chunk)
                        .expect("transit encrypt");
                    transit_file.write_all(&t_nonce).unwrap();
                    transit_file.write_all(&t_cipher).unwrap();

                    // Vault encrypt → write to vault file
                    let v_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                    let v_cipher = vault_cipher
                        .encrypt(&v_nonce, chunk)
                        .expect("vault encrypt");
                    let _ = vault_file.write_all(&v_nonce);
                    let _ = vault_file.write_all(&v_cipher);
                }
                let _ = vault_file.sync_all();
            }
            drop(reader);
            transit_file.sync_all().unwrap();
            drop(transit_file);

            // 4. Request upload URL from relay
            let upload_req = json!({
                "type": "request_media_upload",
                "size": file_size,
            });

            let upload_resp =
                internal_request(&net_state, "request_media_upload", upload_req).await;

            if let Err(_) = upload_resp {
                let _ = std::fs::remove_file(&transit_temp);
                return;
            }
            let upload_resp = upload_resp.unwrap();

            if upload_resp.get("type").and_then(|t| t.as_str()) == Some("error") {
                let _ = std::fs::remove_file(&transit_temp);
                let _error_msg = upload_resp.get("error").and_then(|e| e.as_str()).unwrap_or("Upload denied by relay");
                if let Ok(conn) = db_state.get_conn() {
                    let _ = conn.execute(
                        "UPDATE messages SET status = 'failed' WHERE id = ?1",
                        rusqlite::params![msg_id],
                    );
                }
                let _ = app.emit("msg://status", json!({
                    "id": msg_id, "status": "failed", "chatAddress": payload.recipient
                }));
                return;
            }

            let upload_url = upload_resp["upload_url"].as_str().unwrap_or("").to_string();
            let download_url = upload_resp["download_url"]
                .as_str()
                .unwrap_or("")
                .to_string();

            // 6. Upload encrypted blob to relay via HTTP PUT (streamed from temp file, ~64KB RAM)
            let mut client_builder = reqwest::Client::builder();
            if let Ok(proxy_lock) = net_state.proxy_url.lock() {
                if let Some(ref proxy_url) = *proxy_lock {
                    if let Ok(p) = reqwest::Proxy::all(proxy_url) {
                        client_builder = client_builder.proxy(p);
                    }
                }
            }
            let client = client_builder.build().map_err(|e| e.to_string()).unwrap();

            // Wrap the temp file in a progress-tracking AsyncRead that emits transfer://progress
            struct ProgressFile {
                inner: tokio::fs::File,
                bytes_read: u64,
                total: u64,
                last_emit: tokio::time::Instant,
                msg_id: String,
                app: AppHandle,
            }

            impl AsyncRead for ProgressFile {
                fn poll_read(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                    buf: &mut tokio::io::ReadBuf<'_>,
                ) -> Poll<std::io::Result<()>> {
                    let before = buf.filled().len();
                    let result = Pin::new(&mut self.inner).poll_read(cx, buf);
                    if result.is_ready() {
                        let chunk_size = buf.filled().len().saturating_sub(before) as u64;
                        self.bytes_read += chunk_size;
                        if self.last_emit.elapsed() >= Duration::from_millis(500)
                            || self.bytes_read >= self.total
                        {
                            let _ = self.app.emit(
                                "transfer://progress",
                                json!({
                                    "transfer_id": 0,
                                    "current": self.bytes_read,
                                    "total": self.total,
                                    "direction": "upload",
                                    "msgId": self.msg_id,
                                }),
                            );
                            self.last_emit = tokio::time::Instant::now();
                        }
                    }
                    result
                }
            }

            let file_for_upload = tokio::fs::File::open(&transit_temp).await.unwrap();
            let file_size_meta = file_for_upload.metadata().await.unwrap().len();
            let progress_file = ProgressFile {
                inner: file_for_upload,
                bytes_read: 0,
                total: file_size_meta,
                last_emit: tokio::time::Instant::now(),
                msg_id: msg_id.clone(),
                app: app.clone(),
            };
            let upload_stream = ReaderStream::new(progress_file);
            let upload_body = reqwest::Body::wrap_stream(upload_stream);

            let upload_result = client
                .put(&upload_url)
                .header("Content-Type", "application/octet-stream")
                .body(upload_body)
                .send()
                .await;

            // Clean up temp file regardless of upload outcome
            let _ = std::fs::remove_file(&transit_temp);

            if let Err(_) = upload_result {
                return;
            }

            let key_b64 = base64::engine::general_purpose::STANDARD.encode(net_key);

            // 7. Build announcement
            let mut announcement = serde_json::json!({
                "type": "file",
                "id": msg_id.clone(),
                "size": file_size,
                "msg_type": payload.msg_type.as_deref().unwrap_or("file"),
                "duration": payload.duration,
                "thumbnail": payload.thumbnail,
                "replyTo": payload.reply_to,
                "timestamp": timestamp,
                "download_url": download_url,
                "bundle": {
                    "key": key_b64,
                    "file_name": payload.file_name,
                    "file_type": payload.file_type,
                }
            });

            if payload.is_group
                && let Some(obj) = announcement.as_object_mut()
            {
                obj.insert("isGroup".to_string(), json!(true));
                obj.insert("groupId".to_string(), json!(payload.recipient));
                obj.insert("groupName".to_string(), json!(payload.group_name));
            }

            // 8. Send Signal-encrypted announcement to each recipient
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
                        None,
                        false,
                    )
                    .await;
                }
            }

            // 9. Save thumbnail to vault if present
            if let Some(thumb_b64) = &payload.thumbnail
                && let Ok(thumb_bytes) = base64::engine::general_purpose::STANDARD.decode(thumb_b64)
            {
                let vault_key_bytes = db_state.media_key.lock().unwrap().clone().unwrap();
                let vault_key = ChaKey::from_slice(&vault_key_bytes);
                let vault_cipher = XChaCha20Poly1305::new(vault_key);
                let thumb_path = media_dir.join(format!("{}_thumb", msg_id));
                let v_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                if let Ok(v_cipher) = vault_cipher.encrypt(&v_nonce, thumb_bytes.as_slice())
                    && let Ok(mut f) = std::fs::File::create(&thumb_path)
                {
                    use std::io::Write;
                    let _ = f.write_all(&v_nonce);
                    let _ = f.write_all(&v_cipher);
                    let _ = f.sync_all();
                }
            }

            // 10. Update message status
            let final_attachment_obj = json!({
                "fileName": payload.file_name,
                "fileType": payload.file_type,
                "size": file_size,
                "duration": payload.duration,
                "thumbnail": payload.thumbnail,
                "originalPath": canonical_path.as_ref().map(|p| p.to_string_lossy().to_string()),
                "vaultPath": vault_path.to_string_lossy().to_string(),
                "download_url": download_url,
                "isDownloaded": true,
            });

            if let Ok(conn) = db_state.get_conn() {
                let _ = conn.execute(
                    "UPDATE messages SET status = 'sent', attachment_json = ?2 WHERE id = ?1",
                    rusqlite::params![msg_id, final_attachment_obj.to_string()],
                );
            }
            let _ = app.emit(
                "msg://status",
                json!({
                    "id": msg_id, "status": "sent",
                    "chatAddress": payload.recipient,
                    "attachment": final_attachment_obj
                }),
            );
        });
    });
}
