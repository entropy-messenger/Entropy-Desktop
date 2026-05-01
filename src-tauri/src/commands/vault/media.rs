use crate::app_state::DbState;
use chacha20poly1305::{
    Key, XChaCha20Poly1305,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use std::io::Write;
use tauri::{Manager, State};

pub fn get_media_dir(
    app: &tauri::AppHandle,
    state: &State<'_, DbState>,
) -> Result<std::path::PathBuf, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e: tauri::Error| e.to_string())?;
    let profile = state.profile.lock().map_err(|_| "Profile lock poisoned")?;
    let media_dir = app_dir.join("media").join(&*profile);
    if !media_dir.exists() {
        std::fs::create_dir_all(&media_dir).map_err(|e| e.to_string())?;
    }
    Ok(media_dir)
}

// vault_save_media handles encryption now.

#[tauri::command]
pub async fn vault_save_media(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: String,
    data: Vec<u8>,
) -> Result<String, String> {
    let id = id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>();
    if id.is_empty() {
        return Err("Invalid ID".into());
    }
    let key_bytes = {
        let lock = state
            .media_key
            .lock()
            .map_err(|_| "Media key lock poisoned")?;
        lock.clone().ok_or("Media key not initialized")?
    };

    let key = Key::from_slice(&key_bytes);
    let cipher = XChaCha20Poly1305::new(key);

    let mut final_blob = Vec::new();

    // Split data into chunks to match the streaming loader
    for chunk in data.chunks(1279) {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher.encrypt(&nonce, chunk).map_err(|e| e.to_string())?;
        final_blob.extend_from_slice(&nonce);
        final_blob.extend_from_slice(&ciphertext);
    }

    let media_dir = get_media_dir(&app, &state)?;
    let file_path = media_dir.join(&id);
    let mut file = std::fs::File::create(&file_path).map_err(|e| e.to_string())?;
    file.write_all(&final_blob).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;

    Ok(file_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn vault_delete_media(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<DbState>();
    let media_dir = get_media_dir(&app, &state)?;
    let safe_id = id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>();
    if safe_id.is_empty() {
        return Ok(());
    }
    let file_path = media_dir.join(&safe_id);

    if file_path.exists() {
        std::fs::remove_file(file_path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
pub async fn vault_export_media(
    app: tauri::AppHandle,
    state: tauri::State<'_, DbState>,
    id: String,
    target_path: String,
) -> Result<(), String> {
    use chacha20poly1305::{
        Key, XChaCha20Poly1305, XNonce,
        aead::{Aead, KeyInit},
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // 1. Get the media key
    let key_bytes = {
        let lock = state.media_key.lock().map_err(|_| "Vault not open")?;
        lock.clone().ok_or("Vault not open")?
    };
    let key = Key::from_slice(&key_bytes);
    let cipher = XChaCha20Poly1305::new(key);

    // 2. Locate the source file
    let media_dir = get_media_dir(&app, &state)?;
    let src_path = media_dir.join(&id);
    if !src_path.exists() {
        return Err("Source file not found".to_string());
    }

    let mut src_file = tokio::fs::File::open(&src_path)
        .await
        .map_err(|e| e.to_string())?;
    let mut dst_file = tokio::fs::File::create(&target_path)
        .await
        .map_err(|e| e.to_string())?;

    // 3. Streaming Decryption (Zero-RAM Pipeline)
    let block_size_enc = 1319;
    let mut buf = vec![0u8; block_size_enc];

    while let Ok(n) = src_file.read(&mut buf).await {
        if n == 0 {
            break;
        }
        if n < 40 {
            return Err("Corrupted media block".to_string());
        }

        let nonce = XNonce::from_slice(&buf[..24]);
        let ciphertext = &buf[24..n];

        match cipher.decrypt(nonce, ciphertext) {
            Ok(ptext) => {
                dst_file
                    .write_all(&ptext)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Err(_) => {
                return Err("Decryption failed - possibly wrong key or corruption".to_string());
            }
        }
    }

    dst_file.flush().await.map_err(|e| e.to_string())?;

    Ok(())
}
