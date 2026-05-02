use crate::app_state::DbState;
use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use tauri::{Manager, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

    loop {
        let mut n = 0;
        while n < block_size_enc {
            match src_file.read(&mut buf[n..]).await {
                Ok(0) => break,
                Ok(read) => n += read,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("Read error during export: {}", e)),
            }
        }

        if n == 0 {
            break;
        }

        if n < 40 {
            return Err("Corrupted media block (too small)".to_string());
        }

        let nonce = XNonce::from_slice(&buf[..24]);
        let ciphertext = &buf[24..n];

        match cipher.decrypt(nonce, ciphertext) {
            Ok(ptext) => {
                dst_file
                    .write_all(&ptext)
                    .await
                    .map_err(|e| format!("Write error during export: {}", e))?;
            }
            Err(_) => {
                return Err("Decryption failed - possibly wrong key or corruption".to_string());
            }
        }
    }

    dst_file
        .sync_all()
        .await
        .map_err(|e| format!("Failed to sync export file: {}", e))?;

    Ok(())
}
