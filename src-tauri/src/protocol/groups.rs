use serde_json::json;

use rusqlite::Connection;
use ed25519_dalek::{PublicKey, SecretKey};
use aes_gcm::{Aes256Gcm, Nonce, aead::{Aead, KeyInit}};
use rand::{RngCore, thread_rng};

use crate::protocol::types::{GroupState, SenderKey};
use crate::protocol::utils::{encode_b64, decode_b64};
use crate::protocol::crypto::{kdf_ck, pad_message, unpad_message};

pub fn create_group_sender_key() -> SenderKey {
    let mut rng = thread_rng();
    let mut ck = [0u8; 32];
    rng.fill_bytes(&mut ck);
    
    let mut sk_bytes = [0u8; 32];
    rng.fill_bytes(&mut sk_bytes);
    let id_secret = SecretKey::from_bytes(&sk_bytes).map_err(|_| "Invalid key size").unwrap_or_else(|_| SecretKey::from_bytes(&[0u8; 32]).unwrap()); 
    let id_public = PublicKey::from(&id_secret);

    SenderKey {
        key_id: rng.next_u32(),
        chain_key: encode_b64(&ck),
        signature_key_private: encode_b64(id_secret.as_bytes()),
        signature_key_public: encode_b64(id_public.as_bytes()),
    }
}

pub fn create_group_distribution_message(state: &GroupState) -> Result<serde_json::Value, String> {
    let sk = state.my_sender_key.as_ref().ok_or("No group sender key")?;
    Ok(json!({
        "type": "group_sender_key_distribution",
        "group_id": state.group_id,
        "key_id": sk.key_id,
        "chain_key": sk.chain_key,
        "signature_key_public": sk.signature_key_public
    }))
}

pub fn group_encrypt(
    _conn: &Connection,
    state: &mut GroupState,
    plaintext: &str
) -> Result<serde_json::Value, String> {
    let sk = state.my_sender_key.as_mut().ok_or("No group sender key")?;
    let cur_ck = decode_b64(&sk.chain_key)?;
    let (next_ck, mk) = kdf_ck(&cur_ck)?;
    sk.chain_key = encode_b64(&next_ck);

    let cipher = Aes256Gcm::new_from_slice(&mk).map_err(|e| e.to_string())?;
    let mut rng = thread_rng();
    let mut nonce_bytes = [0u8; 12];
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    let padded = pad_message(plaintext.as_bytes());
    let ciphertext = cipher.encrypt(nonce, padded.as_slice()).map_err(|e| e.to_string())?;

    Ok(json!({
        "body": encode_b64(&ciphertext),
        "nonce": encode_b64(&nonce_bytes),
        "key_id": sk.key_id
    }))
}

pub fn group_decrypt(
    state: &mut GroupState,
    sender_hash: &str,
    msg_obj: &serde_json::Value
) -> Result<String, String> {
    let sk = state.member_sender_keys.get_mut(sender_hash).ok_or("No sender key for peer in group")?;
    let body_b64 = msg_obj["body"].as_str().ok_or("No body")?;
    let nonce_b64 = msg_obj["nonce"].as_str().ok_or("No nonce")?;
    
    let cur_ck = decode_b64(&sk.chain_key)?;
    let (next_ck, mk) = kdf_ck(&cur_ck)?;
    sk.chain_key = encode_b64(&next_ck);

    let cipher = Aes256Gcm::new_from_slice(&mk).map_err(|e| e.to_string())?;
    let nonce_vec = decode_b64(nonce_b64)?;
    let nonce = Nonce::from_slice(&nonce_vec);
    let body_vec = decode_b64(body_b64)?;
    
    let pt = cipher.decrypt(nonce, body_vec.as_slice()).map_err(|e| e.to_string())?;
    let unpadded = unpad_message(&pt)?;
    
    String::from_utf8(unpadded).map_err(|e| e.to_string())
}
