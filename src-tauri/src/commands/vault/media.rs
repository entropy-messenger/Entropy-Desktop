use crate::app_state::DbState;
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use base64::Engine;
use sha2::{Digest, Sha256};
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

#[tauri::command]
pub async fn vault_save_media(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: String,
    data: Vec<u8>,
) -> Result<String, String> {
    let key_bytes = {
        let lock = state
            .media_key
            .lock()
            .map_err(|_| "Media key lock poisoned")?;
        lock.clone().ok_or("Media key not initialized")?
    };

    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, data.as_ref())
        .map_err(|e| e.to_string())?;

    let mut final_blob = nonce.to_vec();
    final_blob.extend(ciphertext);

    let media_dir = get_media_dir(&app, &state)?;
    let file_path = media_dir.join(&id);
    let mut file = std::fs::File::create(&file_path).map_err(|e| e.to_string())?;
    file.write_all(&final_blob).map_err(|e| e.to_string())?;

    Ok(file_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn vault_load_media(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<u8>, String> {
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

    if buffer.len() < 12 {
        return Err("File too short (corrupted)".to_string());
    }

    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&buffer[0..12]);
    let ciphertext = &buffer[12..];

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    Ok(plaintext)
}

#[tauri::command]
pub async fn vault_delete_media(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<DbState>();
    let media_dir = get_media_dir(&app, &state)?;
    let safe_id = id.replace("/", "").replace("..", "");
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
    let media_dir = get_media_dir(&app, &state)?;

    if std::path::Path::new(&src_path).starts_with(&media_dir) {
        let key_bytes = {
            let lock = state
                .media_key
                .lock()
                .map_err(|_| "Media key lock poisoned")?;
            lock.clone().ok_or("Media key not initialized")?
        };
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);

        let mut file = std::fs::File::open(&src_path)
            .map_err(|e| format!("Failed to open source file: {}", e))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .map_err(|e| format!("Failed to read source file: {}", e))?;

        if buffer.len() < 12 {
            return Err("Source file too short (corrupted)".to_string());
        }

        let nonce = Nonce::from_slice(&buffer[0..12]);
        let ciphertext = &buffer[12..];
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("Decryption failed during export: {}", e))?;

        let mut out_file = std::fs::File::create(&target_path)
            .map_err(|e| format!("Failed to create target file: {}", e))?;
        out_file
            .write_all(&plaintext)
            .map_err(|e| format!("Failed to write to target file: {}", e))?;
    } else {
        std::fs::copy(&src_path, &target_path)
            .map_err(|e| format!("Failed to copy file: {}", e))?;
    }

    Ok(())
}

// --- CONSOLIDATED MEDIA ENCRYPTION (ON-DEMAND) ---

#[tauri::command]
pub fn crypto_sha256(data: Vec<u8>) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(hex::encode(hasher.finalize()))
}

#[tauri::command]
pub fn crypto_encrypt_media(data: Vec<u8>) -> Result<serde_json::Value, String> {
    let key = Aes256Gcm::generate_key(&mut OsRng);
    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, data.as_ref())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);

    let key_b64 = base64::engine::general_purpose::STANDARD.encode(key);

    Ok(serde_json::json!({
        "ciphertext": hex::encode(combined),
        "key": key_b64
    }))
}

#[tauri::command]
pub async fn crypto_encrypt_file(path: String) -> Result<serde_json::Value, String> {
    let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|e| e.to_string())?;

    let key = Aes256Gcm::generate_key(&mut OsRng);
    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, data.as_ref())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);

    let key_b64 = base64::engine::general_purpose::STANDARD.encode(key);

    Ok(serde_json::json!({
        "ciphertext": hex::encode(combined),
        "key": key_b64,
        "file_size": data.len()
    }))
}

#[tauri::command]
pub fn crypto_decrypt_media(ciphertext_hex: String, key_b64: String) -> Result<Vec<u8>, String> {
    let combined = hex::decode(ciphertext_hex).map_err(|e| e.to_string())?;
    if combined.len() < 12 {
        return Err("Ciphertext too short".into());
    }

    let key_bytes = base64::engine::general_purpose::STANDARD
        .decode(key_b64)
        .map_err(|e| e.to_string())?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let nonce = Nonce::from_slice(&combined[..12]);
    let ciphertext = &combined[12..];

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    Ok(plaintext)
}
