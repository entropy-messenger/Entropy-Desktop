use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use futures_util::StreamExt;
use serde_json::json;
use std::io::Write;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::Instant;

use crate::app_state::{DbState, NetworkState};
use crate::commands::get_media_dir;

#[tauri::command]
pub async fn download_media(app: AppHandle, msg_id: String) -> Result<(), String> {
    let db_state = app.state::<DbState>();
    let net_state = app.state::<NetworkState>();
    let media_dir = get_media_dir(&app, &db_state)?;
    let vault_path = media_dir.join(&msg_id);

    const BLOCK_SIZE: u64 = 8_388_608;
    const ENC_BLOCK_SIZE: u64 = 8_388_648;
    let (download_url, key_b64, plain_size): (String, String, u64) = {
        let conn = db_state.get_conn()?;
        let attachment_str: String = conn
            .query_row(
                "SELECT attachment_json FROM messages WHERE id = ?1",
                rusqlite::params![msg_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Message not found: {}", e))?;

        let att: serde_json::Value = serde_json::from_str(&attachment_str)
            .map_err(|e| format!("Invalid attachment: {}", e))?;

        let url = att["download_url"]
            .as_str()
            .ok_or("Missing download_url")?
            .to_string();
        let key = att["key"]
            .as_str()
            .ok_or("Missing transit key")?
            .to_string();
        let size = att["size"].as_u64().unwrap_or(0);
        (url, key, size)
    };

    let key_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        key_b64.as_bytes(),
    )
    .map_err(|e| format!("Invalid key: {}", e))?;

    let mut client_builder = reqwest::Client::builder();
    if let Ok(proxy_lock) = net_state.proxy_url.lock() {
        if let Some(ref proxy_url) = *proxy_lock {
            if let Ok(p) = reqwest::Proxy::all(proxy_url) {
                client_builder = client_builder.proxy(p);
            }
        }
    }
    let client = client_builder
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("Download failed: HTTP {}", status));
    }

    // If the relay didn't set Content-Length, fall back to computing encrypted size
    let total = response.content_length().unwrap_or_else(|| {
        if plain_size > 0 {
            ((plain_size + BLOCK_SIZE - 1) / BLOCK_SIZE) * ENC_BLOCK_SIZE
        } else {
            0
        }
    });

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

    // 3. Stream the body — decrypt block by block, emit progress.
    //    Partial vault file cleaned up on any error.
    let enc_block_size: usize = ENC_BLOCK_SIZE as usize;
    let vault_write_result = async {
        let mut vault_file = std::fs::File::create(&vault_path)
            .map_err(|e| format!("Failed to create vault file: {}", e))?;
        let mut stream = response.bytes_stream();
        let mut buf: Vec<u8> = Vec::with_capacity(enc_block_size);
        let mut received: u64 = 0;
        let mut last_emit = Instant::now();

        while let Some(item) = stream.next().await {
            let chunk = item.map_err(|e| format!("Stream error: {}", e))?;
            received += chunk.len() as u64;
            buf.extend_from_slice(&chunk);

            if last_emit.elapsed() >= Duration::from_millis(500) || received >= total {
                let _ = app.emit(
                    "media-dl-progress",
                    json!({"msg_id": msg_id, "received": received, "total": total}),
                );
                last_emit = Instant::now();
            }

            while buf.len() >= enc_block_size {
                let block = buf.drain(..enc_block_size).collect::<Vec<_>>();
                let nonce = XNonce::from_slice(&block[..24]);
                let ciphertext = &block[24..];
                let ptext = transit_cipher
                    .decrypt(nonce, ciphertext)
                    .map_err(|_| "Decryption failed — media may be corrupted")?;

                let v_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                let v_cipher = vault_cipher
                    .encrypt(&v_nonce, ptext.as_slice())
                    .map_err(|e| format!("Vault encryption failed: {}", e))?;
                vault_file
                    .write_all(&v_nonce)
                    .and_then(|_| vault_file.write_all(&v_cipher))
                    .map_err(|e| format!("Failed to write vault file: {}", e))?;
            }
        }

        if !buf.is_empty() && buf.len() >= 40 {
            let nonce = XNonce::from_slice(&buf[..24]);
            let ciphertext = &buf[24..];
            if let Ok(ptext) = transit_cipher.decrypt(nonce, ciphertext) {
                let v_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                if let Ok(v_cipher) = vault_cipher.encrypt(&v_nonce, ptext.as_slice()) {
                    let _ = vault_file.write_all(&v_nonce);
                    let _ = vault_file.write_all(&v_cipher);
                }
            }
        }

        vault_file
            .sync_all()
            .map_err(|e| format!("Failed to sync vault file: {}", e))?;

        Ok::<_, String>(())
    }
    .await;

    if vault_write_result.is_err() {
        let _ = std::fs::remove_file(&vault_path);
        return vault_write_result;
    }

    let mut chat_address = String::new();
    if let Ok(conn) = db_state.get_conn() {
        if let Ok(addr) = conn.query_row::<String, _, _>(
            "SELECT chat_address FROM messages WHERE id = ?1",
            rusqlite::params![msg_id],
            |row| row.get(0),
        ) {
            chat_address = addr;
        }

        if let Ok(attachment_str) = conn.query_row::<String, _, _>(
            "SELECT attachment_json FROM messages WHERE id = ?1",
            rusqlite::params![msg_id],
            |row| row.get(0),
        ) {
            if let Ok(mut att) = serde_json::from_str::<serde_json::Value>(&attachment_str) {
                if let Some(obj) = att.as_object_mut() {
                    obj.insert(
                        "vaultPath".to_string(),
                        json!(vault_path.to_string_lossy().to_string()),
                    );
                    obj.insert("isDownloaded".to_string(), json!(true));
                    let _ = conn.execute(
                        "UPDATE messages SET attachment_json = ?1 WHERE id = ?2",
                        rusqlite::params![att.to_string(), msg_id],
                    );
                }
            }
        }
    }

    let _ = app.emit(
        "media-dl-progress",
        json!({
            "msg_id": msg_id,
            "received": total,
            "total": total
        }),
    );

    let _ = app.emit(
        "msg://status",
        json!({
            "id": msg_id,
            "status": "delivered",
            "chatAddress": chat_address,
            "attachment": {
                "isDownloaded": true,
                "vaultPath": vault_path.to_string_lossy().to_string()
            }
        }),
    );

    Ok(())
}
