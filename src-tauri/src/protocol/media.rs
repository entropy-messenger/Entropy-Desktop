use sha2::{Sha256, Digest};
use aes_gcm::{Aes256Gcm, Nonce, aead::{Aead, KeyInit}};
use rand::{RngCore, thread_rng};
use crate::protocol::types::MediaKeyBundle;
use crate::protocol::utils::{encode_b64, decode_b64};
use rusqlite::Connection;

pub fn encrypt_media(
    _conn: &Connection,
    data: &[u8],
    file_name: &str,
    file_type: &str
) -> Result<(Vec<u8>, MediaKeyBundle), String> {
    let mut rng = thread_rng();
    let mut key_bytes = [0u8; 32];
    rng.fill_bytes(&mut key_bytes);
    
    let cipher = Aes256Gcm::new_from_slice(&key_bytes).map_err(|e| e.to_string())?;
    let mut nonce_bytes = [0u8; 12];
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    let ciphertext = cipher.encrypt(nonce, data).map_err(|e| e.to_string())?;
    
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();

    let bundle = MediaKeyBundle {
        key: encode_b64(&key_bytes),
        nonce: encode_b64(&nonce_bytes),
        digest: encode_b64(&digest),
        file_name: file_name.to_string(),
        file_type: file_type.to_string()
    };

    Ok((ciphertext, bundle))
}

pub fn decrypt_media(
    _conn: &Connection,
    ciphertext: &[u8],
    bundle: &MediaKeyBundle
) -> Result<Vec<u8>, String> {
    let key_bytes = decode_b64(&bundle.key)?;
    let nonce_bytes = decode_b64(&bundle.nonce)?;
    
    let cipher = Aes256Gcm::new_from_slice(&key_bytes).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let pt = cipher.decrypt(nonce, ciphertext).map_err(|e| e.to_string())?;
    
    let mut hasher = Sha256::new();
    hasher.update(&pt);
    let digest = hasher.finalize();

    if encode_b64(&digest) != bundle.digest {
        return Err("Media digest mismatch".to_string());
    }

    Ok(pt)
}
