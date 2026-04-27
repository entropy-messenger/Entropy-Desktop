use crate::app_state::DbState;
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    XChaCha20Poly1305, Key, XNonce,
};
use base64::Engine;
use std::io::{Read, Write};
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
pub async fn vault_load_media(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<u8>, String> {
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

    let media_dir = get_media_dir(&app, &state)?;
    let file_path = media_dir.join(&id);

    if !file_path.exists() {
        return Err("Media file not found".to_string());
    }

    let mut file = std::fs::File::open(&file_path).map_err(|e| e.to_string())?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;

    let key = Key::from_slice(&key_bytes);
    let cipher = XChaCha20Poly1305::new(key);
    
    let mut plaintext = Vec::new();
    let mut offset = 0;
    
    // Decrypt chunked AEAD
    println!("[VAULT] Loading media ID: {}. Key fingerprint: {}", id, hex::encode(&key_bytes[..4]));
    while offset + 24 < buffer.len() {
        let nonce = XNonce::from_slice(&buffer[offset..offset+24]);
        offset += 24;
        
        // We need to know the chunk size. 
        // In our outbox, each chunk was 1279 bytes -> ciphertext was 1279 + 16 = 1295 bytes.
        // Total block size = 24 + 1295 = 1319 bytes.
        let chunk_cipher_len = 1295; 
        let end = std::cmp::min(offset + chunk_cipher_len, buffer.len());
        let ciphertext = &buffer[offset..end];
        offset = end;

        let cipher = XChaCha20Poly1305::new(Key::from_slice(&key_bytes));
        let chunk_plain = cipher.decrypt(nonce, ciphertext).map_err(|e| {
            let err = format!("Vault chunk decryption failed: {}", e);
            println!("[VAULT-ERROR] {}", err);
            err
        })?;
        plaintext.extend(chunk_plain);
    }

    Ok(plaintext)
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
pub async fn db_export_media(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    src_path: String,
    target_path: String,
) -> Result<(), String> {
    let _media_dir = get_media_dir(&app, &state)?;
    let src_path_abs = std::path::PathBuf::from(&src_path);

    if src_path_abs.exists() {
        let key_bytes = {
            let lock = state
                .media_key
                .lock()
                .map_err(|_| "Media key lock poisoned")?;
            lock.clone().ok_or("Media key not initialized")?
        };
        let key = Key::from_slice(&key_bytes);
        let cipher = XChaCha20Poly1305::new(key);

        let mut file = std::fs::File::open(&src_path_abs)
            .map_err(|e| format!("Failed to open source file: {}", e))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .map_err(|e| format!("Failed to read source file: {}", e))?;

        let mut plaintext = Vec::new();
        let mut offset = 0;
        
        while offset + 24 < buffer.len() {
            let nonce = XNonce::from_slice(&buffer[offset..offset+24]);
            offset += 24;
            
            let chunk_cipher_len = 1295; 
            let end = std::cmp::min(offset + chunk_cipher_len, buffer.len());
            let ciphertext = &buffer[offset..end];
            offset = end;

            let chunk_plain = cipher.decrypt(nonce, ciphertext).map_err(|e| format!("Vault export chunk decryption failed: {}", e))?;
            plaintext.extend(chunk_plain);
        }

        let target_path_abs = std::path::PathBuf::from(&target_path);
        std::fs::write(target_path_abs, plaintext).map_err(|e| e.to_string())?;
    } else {
        // Refuse to copy files from outside the media vault
        return Err("Export denied: Source path is outside the allowed media vault".into());
    }

    Ok(())
}

// Logic for media encryption is handled in process_outgoing_media in outbox.rs

pub fn crypto_decrypt_media(combined: Vec<u8>, key_b64: String) -> Result<Vec<u8>, String> {
    let key_bytes = base64::engine::general_purpose::STANDARD
        .decode(key_b64)
        .map_err(|e| e.to_string())?;
    let key = Key::from_slice(&key_bytes);
    let cipher = XChaCha20Poly1305::new(key);

    let mut plaintext = Vec::new();
    let mut offset = 0;
    
    // Decrypt chunked AEAD
    while offset + 24 < combined.len() {
        let nonce = XNonce::from_slice(&combined[offset..offset+24]);
        offset += 24;
        
        let chunk_cipher_len = 1295; 
        let end = std::cmp::min(offset + chunk_cipher_len, combined.len());
        let ciphertext = &combined[offset..end];
        offset = end;

        let chunk_plain = cipher.decrypt(nonce, ciphertext).map_err(|e| format!("Network chunk decryption failed: {}", e))?;
        plaintext.extend(chunk_plain);
    }

    Ok(plaintext)
}
