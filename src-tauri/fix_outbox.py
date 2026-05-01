import sys

def process_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()

    # Find start and end of process_outgoing_media
    start_idx = content.find('#[tauri::command]\npub fn process_outgoing_media')
    if start_idx == -1:
        print("Could not find process_outgoing_media")
        sys.exit(1)
        
    # Find the end of the function (the closing brace at column 0)
    end_idx = content.find('\n}\n', start_idx)
    if end_idx == -1:
        end_idx = content.find('\n}', start_idx)
        
    old_func = content[start_idx:end_idx+2]
    
    new_func = """#[tauri::command]
pub async fn process_outgoing_media(
    app: tauri::AppHandle,
    payload: OutgoingMedia,
) -> Result<serde_json::Value, String> {
    let db_state = app.state::<crate::app_state::DbState>();
    let net_state = app.state::<crate::app_state::NetworkState>();

    let msg_id = uuid::Uuid::new_v4().to_string();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("Clock error: {}", e))?
        .as_millis() as i64;

    // Validate path
    let canonical_path_opt = if let Some(p) = &payload.file_path {
        let path_buf = std::path::PathBuf::from(p);
        let canonical_path = std::fs::canonicalize(&path_buf)
            .map_err(|_| "Access denied: Invalid or inaccessible file path".to_string())?;
        if canonical_path.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(false)
            || canonical_path.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.') && c.as_os_str() != ".")
        {
            return Err("Access denied: Cannot send hidden files".into());
        }
        let metadata = std::fs::metadata(&canonical_path).map_err(|e| e.to_string())?;
        if metadata.len() > 10737418240 { return Err("File too large. Maximum size is 10GB.".to_string()); }
        Some(canonical_path)
    } else if let Some(ref d) = payload.file_data {
        if d.len() > 10737418240 { return Err("File too large.".to_string()); }
        None
    } else {
        return Err("No file path or data provided".into());
    };

    let file_size = if let Some(ref p) = canonical_path_opt {
        std::fs::metadata(p).map_err(|e| e.to_string())?.len()
    } else if let Some(ref d) = payload.file_data {
        d.len() as u64
    } else {
        return Err("No data source".into());
    };

    let transfer_id: u32 = rand::random();
    let net_chunk_capacity = 1279; 
    let total_fragments = (file_size as f64 / net_chunk_capacity as f64).ceil() as u32;

    use sha2::{Sha256, Digest};
    use chacha20poly1305::aead::OsRng;
    use chacha20poly1305::{XChaCha20Poly1305, Key as ChaKey, KeyInit};
    let net_key = XChaCha20Poly1305::generate_key(&mut OsRng);
    let vault_key_bytes = {
        let lock = db_state.media_key.lock().map_err(|_| "State poisoned")?;
        lock.clone().ok_or("Media key not initialized")?
    };

    // Hash file in background to not block async runtime
    let payload_clone = payload.clone();
    let canonical_path_clone = canonical_path_opt.clone();
    let file_hash = tokio::task::spawn_blocking(move || {
        let mut hasher = Sha256::new();
        let mut reader: Box<dyn std::io::Read> = if let Some(ref p) = canonical_path_clone {
            if let Ok(f) = std::fs::File::open(p) { Box::new(std::io::BufReader::new(f)) } else { return Err("Read error".to_string()); }
        } else if let Some(ref d) = payload_clone.file_data {
            Box::new(std::io::Cursor::new(d.clone()))
        } else { return Err("No data".to_string()); };
        
        let mut buf = vec![0u8; 65536];
        loop {
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
        }
        Ok::<String, String>(hex::encode(hasher.finalize()))
    }).await.map_err(|_| "Hash panicked")??;

    let media_dir = crate::commands::vault::get_media_dir(&app, &db_state)?;
    let vault_path = media_dir.join(&msg_id);
    let saved_vault_path = vault_path.to_string_lossy().to_string();

    let routing_hash = payload.recipient.split('.').next().unwrap_or(&payload.recipient).to_string();
    let key_b64 = base64::engine::general_purpose::STANDARD.encode(&net_key);

    let metadata_json = serde_json::json!({
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
            "sha256": file_hash
        }
    });

    let own_id = net_state.identity_hash.lock().map_err(|_| "Network state poisoned")?.clone().unwrap_or_default();
    let db_msg = DbMessage {
        id: msg_id.clone(),
        chat_address: payload.recipient.clone(),
        sender_hash: own_id.clone(),
        content: if payload.msg_type.as_deref() == Some("voice_note") { "Voice Note".to_string() } else { format!("File: {}", payload.file_name.as_deref().unwrap_or("Media")) },
        timestamp,
        r#type: payload.msg_type.clone().unwrap_or_else(|| "file".to_string()),
        status: "sending".to_string(),
        attachment_json: Some(serde_json::json!({
            "fileName": payload.file_name,
            "fileType": payload.file_type,
            "size": file_size,
            "duration": payload.duration,
            "thumbnail": payload.thumbnail,
            "transferId": transfer_id,
            "vaultPath": saved_vault_path
        }).to_string()),
        is_starred: false,
        is_group: false,
        reply_to_json: payload.reply_to.as_ref().map(|r| serde_json::to_string(&r).unwrap_or_default()),
    };

    crate::commands::messaging::outbox::internal_db_save_message(&db_state, db_msg.clone()).await?;
    let mut final_json = serde_json::to_value(&db_msg).map_err(|e| e.to_string())?;
    if let Some(obj) = final_json.as_object_mut() { obj.insert("chatAlias".to_string(), serde_json::json!(null)); }
    
    use tauri::Manager;
    app.emit("msg://added", final_json.clone()).map_err(|e| e.to_string())?;

    // Send Metadata FIRST
    let encrypted_meta = crate::commands::messaging::inbox::internal_signal_encrypt(
        app.clone(), &net_state, &payload.recipient, metadata_json.to_string()
    ).await?;
    let _ = crate::commands::network::transit::internal_send_to_network(
        app.clone(), &net_state, Some(routing_hash.clone()), Some(msg_id.clone()), None,
        Some(encrypted_meta.to_string().into_bytes()), true, false, Some(transfer_id), true,
    ).await;

    // Background task for binary dispatch
    let app_clone = app.clone();
    let recipient_clone = payload.recipient.clone();
    let file_data_clone = payload.file_data.clone();
    
    tauri::async_runtime::spawn(async move {
        let db_state = app_clone.state::<crate::app_state::DbState>();
        let net_state = app_clone.state::<crate::app_state::NetworkState>();
        
        let mut reader: Box<dyn std::io::Read> = if let Some(ref p) = canonical_path_opt {
            if let Ok(f) = std::fs::File::open(p) { Box::new(std::io::BufReader::new(f)) } else { return; }
        } else if let Some(ref d) = file_data_clone {
            Box::new(std::io::Cursor::new(d.clone()))
        } else { return; };

        let mut vault_file = if let Ok(f) = std::fs::File::create(&vault_path) { f } else { return; };

        let mut fragment_index = 0;
        let batch_size = 32;
        loop {
            let mut batch = Vec::with_capacity(batch_size);
            for _ in 0..batch_size {
                let mut chunk_buf = vec![0u8; 1279];
                let mut n = 0;
                while n < 1279 {
                    match reader.read(&mut chunk_buf[n..]) {
                        Ok(0) => break,
                        Ok(read) => n += read,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                if n == 0 { break; }
                batch.push(if n < 1279 { chunk_buf[..n].to_vec() } else { chunk_buf });
            }
            if batch.is_empty() { break; }

            let mut tasks = Vec::with_capacity(batch.len());
            for chunk in batch {
                let v_net_key = net_key.clone();
                let v_vault_key_bytes = vault_key_bytes.clone();
                tasks.push(tokio::task::spawn_blocking(move || {
                    let vault_cipher = XChaCha20Poly1305::new(ChaKey::from_slice(&v_vault_key_bytes));
                    let net_cipher = XChaCha20Poly1305::new(&v_net_key);
                    let v_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                    let v_enc = vault_cipher.encrypt(&v_nonce, chunk.as_slice()).unwrap_or_default();
                    let n_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                    let n_enc = net_cipher.encrypt(&n_nonce, chunk.as_slice()).unwrap_or_default();
                    (v_nonce.to_vec(), v_enc, n_nonce.to_vec(), n_enc)
                }));
            }

            let results = futures_util::future::join_all(tasks).await;
            for res in results {
                if let Ok((v_nonce, v_enc, n_nonce, n_enc)) = res {
                    use std::io::Write;
                    let _ = vault_file.write_all(&v_nonce);
                    let _ = vault_file.write_all(&v_enc);
                    
                    let mut packet_data = Vec::with_capacity(n_nonce.len() + n_enc.len());
                    packet_data.extend_from_slice(&n_nonce);
                    packet_data.extend_from_slice(&n_enc);
                    
                    let _ = crate::commands::network::transit::internal_dispatch_fragment(
                        app_clone.clone(), &net_state, &routing_hash, Some(msg_id.clone()),
                        transfer_id, fragment_index, total_fragments, &packet_data, true, false,
                    ).await;
                    
                    if fragment_index % 500 == 0 {
                        let _ = app_clone.emit("transfer://progress", serde_json::json!({
                            "transferId": transfer_id,
                            "current": fragment_index,
                            "total": total_fragments,
                            "direction": "upload"
                        }));
                    }
                    fragment_index += 1;
                }
            }
        }
        use std::io::Write;
        let _ = vault_file.sync_all();

        // Update DB status to sent
        if let Ok(lock) = db_state.conn.lock() {
            if let Some(conn) = lock.as_ref() {
                let _ = conn.execute("UPDATE messages SET status = 'sent' WHERE id = ?1", rusqlite::params![msg_id]);
                let _ = conn.execute("UPDATE chats SET last_status = 'sent' WHERE LOWER(address) = LOWER(?1)", rusqlite::params![recipient_clone]);
            }
        }
        let _ = app_clone.emit("msg://status", serde_json::json!({ "id": msg_id, "status": "sent", "chatAddress": recipient_clone }));
    });

    Ok(final_json)
}"""
    
    new_content = content.replace(old_func, new_func)
    with open(filepath, 'w') as f:
        f.write(new_content)
    print("Done")

process_file('src/commands/messaging/outbox.rs')
