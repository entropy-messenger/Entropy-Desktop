use crate::app_state::{DbState, NetworkState, PendingMediaMetadata};
use crate::commands::messaging::inbox::internal_send_volatile;
use crate::commands::{
    DbMessage, get_media_dir, internal_db_save_message, internal_signal_encrypt,
};
use base64::Engine;
use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use rusqlite::params;
use serde_json::json;
use std::io::{Read, Write};
use tauri::{AppHandle, Emitter, Manager};

pub async fn handle_media_msg(
    app: AppHandle,
    sender: String,
    decrypted_json: serde_json::Value,
    net_state: &NetworkState,
) -> Result<(), String> {
    let raw_msg_id = decrypted_json["id"].as_str().ok_or("Missing msg id")?;
    let msg_id = raw_msg_id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>();
    if msg_id.is_empty() {
        return Err("Invalid message ID".into());
    }

    let bundle = decrypted_json["bundle"].clone();
    let inner_transfer_id = decrypted_json["transfer_id"]
        .as_u64()
        .ok_or("Missing transfer id")? as u32;

    let size = decrypted_json["size"].as_u64().ok_or("Missing size")?;
    if size > 256 * 1024 * 1024u64 {
        return Err("File metadata exceeds size limit".into());
    }
    let m_type = decrypted_json["msg_type"]
        .as_str()
        .ok_or("Missing msg_type")?
        .to_string();
    let duration = decrypted_json["duration"].as_f64().unwrap_or(0.0);
    let timestamp = decrypted_json["timestamp"].as_i64().unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    });

    let db_state = app.state::<DbState>();
    let media_dir = get_media_dir(&app, &db_state)?;
    let final_file_path = media_dir.join(&msg_id);

    let temp_filename = format!("transfer_{}_{}_media.bin", sender, inner_transfer_id);
    let temp_path = media_dir.join(&temp_filename);
    let key_str = bundle["key"].as_str().unwrap_or_default().to_string();

    // 1. Handle thumbnail saving to vault
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

    // Buffer metadata so fragments arriving later can trigger the bridge
    {
        let mut links = net_state
            .pending_media_links
            .lock()
            .map_err(|_| "Network state poisoned")?;
        let transfer_key = format!("{}:{}", sender, inner_transfer_id);
        links.insert(
            transfer_key,
            PendingMediaMetadata {
                id: msg_id.clone(),
                key: key_str.clone(),
            },
        );
    }

    if temp_path.exists() {
        // Decrypt and save media arriving before metadata (Streaming O(1) RAM)
        if let Ok(key_bytes) = base64::engine::general_purpose::STANDARD.decode(&key_str) {
            let transit_key = Key::from_slice(&key_bytes);
            let transit_cipher = XChaCha20Poly1305::new(transit_key);

            let vault_key_bytes = {
                let lock = db_state
                    .media_key
                    .lock()
                    .map_err(|_| "Media key lock poisoned")?;
                lock.clone().ok_or("Media key not initialized")?
            };
            let vault_key = Key::from_slice(&vault_key_bytes);
            let vault_cipher = XChaCha20Poly1305::new(vault_key);

            let vault_path = media_dir.join(&msg_id);

            // 2. Check for missing fragments (Selective Repeat)
            let mut missing = Vec::new();
            {
                let assemblers = net_state
                    .media_assembler
                    .lock()
                    .map_err(|_| "Network state poisoned")?;
                let transfer_key = format!("{}:{}:2", sender, inner_transfer_id);
                if let Some(assembler) = assemblers.get(&transfer_key) {
                    for (idx, received) in assembler.received_chunks.iter().enumerate() {
                        if !received {
                            missing.push(idx as u32);
                        }
                    }
                }
            }

            if !missing.is_empty() {
                // Request resend via background signal
                let resend_req = serde_json::json!({
                    "type": "media_resend_request",
                    "transfer_id": inner_transfer_id,
                    "indices": missing,
                    "msg_id": msg_id.clone()
                });

                if let Ok(encrypted) =
                    internal_signal_encrypt(app.clone(), net_state, &sender, resend_req.to_string())
                        .await
                {
                    let _ = crate::commands::network::transit::internal_send_to_network(
                        app.clone(),
                        net_state,
                        Some(sender.clone()),
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
                return Ok(());
            }

            // All fragments present - consume the metadata and bridge now
            {
                let mut links = net_state
                    .pending_media_links
                    .lock()
                    .map_err(|_| "Network state poisoned")?;
                let transfer_key = format!("{}:{}", sender, inner_transfer_id);
                links.remove(&transfer_key);
            }

            if let Err(e) = internal_vault_bridge(
                &app,
                &temp_path,
                &vault_path,
                &transit_cipher,
                &vault_cipher,
                inner_transfer_id,
                &sender,
            ) {
                let _ = app.emit(
                    "network-bin-error",
                    serde_json::json!({
                        "msg_id": msg_id.clone(),
                        "error": e
                    }),
                );
            } else {
                let _ = std::fs::remove_file(&temp_path);
                let _ = app.emit(
                    "network-bin-complete",
                    serde_json::json!({
                        "sender": sender,
                        "transfer_id": inner_transfer_id,
                        "msg_id": Some(msg_id.clone())
                    }),
                );
            }
        }
    }

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
        attachment_json: {
            let expected_hash = bundle["sha256"].as_str().unwrap_or_default();
            if expected_hash.is_empty() {
                Some(
                    json!({
                        "fileName": bundle["file_name"],
                        "fileType": bundle["file_type"],
                        "size": size,
                        "duration": duration,
                        "thumbnail": decrypted_json["thumbnail"],
                        "bundle": bundle,
                        "transferId": inner_transfer_id
                    })
                    .to_string(),
                )
            } else {
                Some(
                    json!({
                        "fileName": bundle["file_name"],
                        "fileType": bundle["file_type"],
                        "size": size,
                        "duration": duration,
                        "thumbnail": decrypted_json["thumbnail"],
                        "bundle": bundle,
                        "transferId": inner_transfer_id,
                        "vaultPath": final_file_path.to_string_lossy().to_string()
                    })
                    .to_string(),
                )
            }
        },
        is_starred: false,
        is_group,
        reply_to_json: decrypted_json["replyTo"]
            .as_object()
            .map(|r| serde_json::to_string(r).unwrap_or_default()),
    };

    // Auto-create/rename chat for media too
    if is_group {
        let conn = db_state.get_conn()?;
        let _ = conn.execute(
            "INSERT INTO chats (address, is_group, alias) VALUES (?1, 1, ?2)
             ON CONFLICT(address) DO UPDATE SET 
                 alias = CASE WHEN excluded.alias IS NOT NULL THEN excluded.alias ELSE alias END,
                 is_group = 1",
            params![chat_address, group_name],
        );

        // Keep the group membership in sync
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

    // enforce 1:1 delivery receipts
    if !is_group {
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
    Ok(())
}

pub async fn handle_media_completion(
    app: AppHandle,
    sender: String,
    transfer_id: u32,
    net_state: &NetworkState,
) -> Result<(), String> {
    let db_state = app.state::<DbState>();
    let link_key = format!("{}:{}", sender, transfer_id);
    let media_dir = get_media_dir(&app, &db_state)?;
    let temp_filename = format!("transfer_{}_{}_media.bin", sender, transfer_id);
    let temp_path = media_dir.join(&temp_filename);

    let meta = {
        let mut links = net_state
            .pending_media_links
            .lock()
            .map_err(|_| "Network state poisoned")?;
        links.remove(&link_key)
    };

    if let Some(m) = meta {
        // Vault decryption bridge (Streaming O(1) RAM)
        // Offload to blocking thread pool to avoid starving the websocket processing loop
        let app_clone = app.clone();
        let sender_clone = sender.clone();
        let m_clone = m.clone();
        let temp_path_clone = temp_path.clone();

        let vault_key_bytes = {
            let lock = db_state
                .media_key
                .lock()
                .map_err(|_| "Media key lock poisoned")?;
            lock.clone().ok_or("Media key not initialized")?
        };

        let transit_key_bytes = base64::engine::general_purpose::STANDARD
            .decode(&m_clone.key)
            .map_err(|_| "Invalid transit key format")?;

        tokio::task::spawn_blocking(move || {
            let vault_key = Key::from_slice(&vault_key_bytes);
            let vault_cipher = XChaCha20Poly1305::new(vault_key);
            let transit_key = Key::from_slice(&transit_key_bytes);
            let transit_cipher = XChaCha20Poly1305::new(transit_key);

            let media_dir = get_media_dir(&app_clone, &app_clone.state::<DbState>())?;
            let vault_path = media_dir.join(&m_clone.id);

            if let Err(e) = internal_vault_bridge(
                &app_clone,
                &temp_path_clone,
                &vault_path,
                &transit_cipher,
                &vault_cipher,
                transfer_id,
                &sender_clone,
            ) {
                let _ = app_clone.emit(
                    "network-bin-error",
                    serde_json::json!({
                        "msg_id": m_clone.id,
                        "error": e
                    }),
                );
            } else {
                let _ = std::fs::remove_file(&temp_path_clone);
                let _ = app_clone.emit(
                    "network-bin-complete",
                    serde_json::json!({
                        "sender": sender_clone,
                        "transfer_id": transfer_id,
                        "msg_id": Some(m_clone.id)
                    }),
                );
            }
            Ok::<(), String>(())
        });
    }
    Ok(())
}

pub fn handle_vault_retry_bridge(app: AppHandle, msg_id: String) -> Result<(), String> {
    let db_state = app.state::<DbState>();
    let net_state = app.state::<NetworkState>();

    // 1. Get message from DB to find transfer info
    let (attachment_json, sender_hash) = {
        let conn = db_state.get_conn()?;
        conn.query_row(
            "SELECT attachment_json, sender_hash FROM messages WHERE id = ?1",
            rusqlite::params![msg_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|e| format!("Message not found: {}", e))?
    };

    let attachment: serde_json::Value =
        serde_json::from_str(&attachment_json).map_err(|e| format!("Invalid attachment: {}", e))?;
    let transfer_id = attachment["transferId"]
        .as_u64()
        .ok_or("No transferId found")? as u32;
    let sender = sender_hash;

    // 2. Locate temp file
    let media_dir = get_media_dir(&app, &db_state)?;
    let temp_filename = format!("transfer_{}_{}_media.bin", sender, transfer_id);
    let temp_path = media_dir.join(&temp_filename);

    if !temp_path.exists() {
        return Err("Temporary media file not found. It may have been cleared.".into());
    }

    // 3. Setup keys
    let vault_key_bytes = {
        let lock = db_state.media_key.lock().unwrap();
        lock.clone().ok_or("Vault locked")?
    };
    let vault_key = Key::from_slice(&vault_key_bytes);
    let vault_cipher = XChaCha20Poly1305::new(vault_key);

    let transfer_key = format!("{}:{}", sender, transfer_id);
    let transit_key_str = {
        let lock = net_state
            .pending_media_links
            .lock()
            .map_err(|_| "State poisoned")?;
        lock.get(&transfer_key)
            .cloned()
            .ok_or("Transfer metadata expired. You may need to ask the sender to resend.")?
            .key
    };
    let transit_key_bytes = base64::engine::general_purpose::STANDARD
        .decode(&transit_key_str)
        .map_err(|_| "Invalid transit key format")?;
    let transit_key = Key::from_slice(&transit_key_bytes);
    let transit_cipher = XChaCha20Poly1305::new(transit_key);

    let vault_path = media_dir.join(&msg_id);

    // 4. Run bridge
    internal_vault_bridge(
        &app,
        &temp_path,
        &vault_path,
        &transit_cipher,
        &vault_cipher,
        transfer_id,
        &sender,
    )?;

    // 5. Cleanup and notify
    let _ = std::fs::remove_file(&temp_path);

    // Update DB status to 'sent' (received/complete for incoming)
    {
        let conn = db_state.get_conn()?;
        conn.execute(
            "UPDATE messages SET status = 'sent', error = NULL WHERE id = ?1",
            rusqlite::params![msg_id],
        )
        .map_err(|e| e.to_string())?;
    }

    let _ = app.emit(
        "network-bin-complete",
        serde_json::json!({
            "sender": sender,
            "transfer_id": transfer_id,
            "msg_id": Some(msg_id)
        }),
    );

    Ok(())
}

pub fn internal_vault_bridge(
    app: &tauri::AppHandle,
    src_path: &std::path::Path,
    vault_path: &std::path::Path,
    transit_cipher: &XChaCha20Poly1305,
    vault_cipher: &XChaCha20Poly1305,
    transfer_id: u32,
    sender: &str,
) -> Result<(), String> {
    let mut src =
        std::fs::File::open(src_path).map_err(|e| format!("Failed to open temp file: {}", e))?;
    let mut dst = std::fs::File::create(vault_path)
        .map_err(|e| format!("Failed to create vault file: {}", e))?;

    let file_size = src.metadata().map(|m| m.len()).unwrap_or(0);
    let total_blocks = (file_size as f64 / 1319.0).ceil() as u64;
    let progress_step = (total_blocks / 10).max(1);

    let mut block_buf = [0u8; 1319];
    let mut block_count = 0;

    loop {
        let mut n = 0;
        while n < 1319 {
            match src.read(&mut block_buf[n..]) {
                Ok(0) => break,
                Ok(read) => n += read,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("Read error during bridge: {}", e)),
            }
        }
        if n == 0 {
            break;
        }

        let chunk = &block_buf[..n];
        if chunk.len() > 40 {
            let nonce = XNonce::from_slice(&chunk[..24]);
            let ciphertext = &chunk[24..];
            let ptext = transit_cipher
                .decrypt(nonce, ciphertext)
                .map_err(|_| "Decryption failed during bridge - potential corruption")?;

            let v_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
            let v_cipher = vault_cipher
                .encrypt(&v_nonce, ptext.as_slice())
                .map_err(|e| format!("Vault encryption failed: {}", e))?;

            let mut retries = 0;
            loop {
                match dst
                    .write_all(&v_nonce)
                    .and_then(|_| dst.write_all(&v_cipher))
                {
                    Ok(_) => break,
                    Err(e) if retries < 3 => {
                        retries += 1;
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    Err(e) => return Err(format!("Write error during bridge: {}", e)),
                }
            }

            block_count += 1;
            if block_count % progress_step == 0 || block_count == total_blocks {
                let _ = app.emit(
                    "network-bin-progress",
                    json!({
                        "transfer_id": transfer_id,
                        "current": block_count,
                        "total": total_blocks,
                        "sender": sender
                    }),
                );
            }
        } else {
            break;
        }
    }

    let mut retries = 0;
    loop {
        match dst.sync_all() {
            Ok(_) => break,
            Err(e) if retries < 3 => {
                retries += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(format!("Final sync failed: {}", e)),
        }
    }
    Ok(())
}
