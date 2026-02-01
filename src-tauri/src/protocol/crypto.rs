use sha2::{Sha256, Sha512, Digest};
use ed25519_dalek::{Keypair, Signer, PublicKey, SecretKey};
use x25519_dalek::StaticSecret;
use curve25519_dalek::edwards::CompressedEdwardsY;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use aes_gcm::{Aes256Gcm, Nonce, aead::{Aead, KeyInit}};
use rand::{RngCore, thread_rng};
use rusqlite::Connection;
use crate::protocol::types::ProtocolIdentity;
use crate::protocol::utils::{encode_b64, decode_b64};

pub fn sign_message(conn: &Connection, message: &[u8]) -> Result<String, String> {
    let mut stmt = conn.prepare("SELECT value FROM vault WHERE key = 'protocol_identity';").map_err(|e| e.to_string())?;
    let json: String = stmt.query_row([], |r| r.get(0)).map_err(|e| e.to_string())?;
    let id: ProtocolIdentity = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    
    let sk_bytes = decode_b64(&id.identity_keys.private_key)?;
    let sk = SecretKey::from_bytes(&sk_bytes).map_err(|_| "Invalid private key bytes")?;
    let pk = PublicKey::from(&sk);
    let keypair = Keypair { secret: sk, public: pk };
    
    let signature = keypair.sign(message);
    Ok(encode_b64(&signature.to_bytes()))
}

pub fn kdf_rk(rk: &[u8], dh_out: &[u8]) -> Result<([u8; 32], [u8; 32], [u8; 32]), String> {
    let hk = Hkdf::<Sha256>::new(Some(rk), dh_out);
    let mut okm = [0u8; 96]; 
    hk.expand(b"EntropyV1 Ratchet", &mut okm).map_err(|_| "HKDF Expand failed")?;
    
    let mut new_rk = [0u8; 32];
    let mut new_ck = [0u8; 32];
    let mut new_hk = [0u8; 32];
    new_rk.copy_from_slice(&okm[0..32]);
    new_ck.copy_from_slice(&okm[32..64]);
    new_hk.copy_from_slice(&okm[64..96]);
    
    Ok((new_rk, new_ck, new_hk))
}

pub fn rk_mix_pq(rk: &[u8], pq_secret: &[u8]) -> Result<[u8; 32], String> {
    let hk = Hkdf::<Sha256>::new(Some(rk), pq_secret);
    let mut okm = [0u8; 32];
    hk.expand(b"EntropyV1 PQ Mix", &mut okm).map_err(|_| "PQ Mix failed")?;
    Ok(okm)
}

pub fn kdf_ck(ck: &[u8]) -> Result<([u8; 32], [u8; 32]), String> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(ck).map_err(|e| e.to_string())?;
    mac.update(b"\x01");
    let new_ck_bytes = mac.finalize().into_bytes();
    
    let mut mac2 = <Hmac<Sha256> as Mac>::new_from_slice(ck).map_err(|e| e.to_string())?;
    mac2.update(b"\x02");
    let mk_bytes = mac2.finalize().into_bytes();

    let mut ck_res = [0u8; 32];
    let mut mk_res = [0u8; 32];
    ck_res.copy_from_slice(&new_ck_bytes);
    mk_res.copy_from_slice(&mk_bytes);

    Ok((ck_res, mk_res))
}

pub(crate) fn pad_message(message: &[u8]) -> Vec<u8> {
    let block_size = 512;
    let pad_len = block_size - (message.len() % block_size);
    let mut padded = Vec::with_capacity(message.len() + pad_len);
    padded.extend_from_slice(message);
    
    for _ in 0..pad_len {
        padded.push((pad_len % 256) as u8);
    }
    
    let len_bytes = (pad_len as u16).to_be_bytes();
    padded.push(len_bytes[0]);
    padded.push(len_bytes[1]);
    padded
}

pub(crate) fn unpad_message(padded: &[u8]) -> Result<Vec<u8>, String> {
    if padded.len() < 2 { return Err("Message too short".to_string()); }
    
    let last_two = &padded[padded.len()-2..];
    let pad_len = u16::from_be_bytes([last_two[0], last_two[1]]) as usize;
    
    if pad_len == 0 || pad_len > padded.len() {
        return Err("Invalid padding".to_string());
    }
    Ok(padded[..padded.len() - pad_len - 2].to_vec())
}

pub fn encrypt_header(key: &[u8], ratchet_pub: &[u8], n: u32, pn: u32) -> Result<(String, String), String> {
    let header_json = serde_json::json!({
        "ratchet_key": encode_b64(ratchet_pub),
        "n": n,
        "pn": pn
    }).to_string();

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let mut rng = thread_rng();
    let mut nonce_bytes = [0u8; 12];
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    let ciphertext = cipher.encrypt(nonce, header_json.as_bytes()).map_err(|e| e.to_string())?;
    Ok((encode_b64(&ciphertext), encode_b64(&nonce_bytes)))
}

pub fn decrypt_header(key: &[u8], ciphertext: &str, nonce_b64: &str) -> Result<serde_json::Value, String> {
    let ciphertext_bytes = decode_b64(ciphertext)?;
    let nonce_bytes = decode_b64(nonce_b64)?;
    
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    let plaintext = cipher.decrypt(nonce, ciphertext_bytes.as_slice()).map_err(|e| e.to_string())?;
    serde_json::from_slice(&plaintext).map_err(|e| e.to_string())
}

pub fn ed25519_pub_to_x25519(ed_pub: &[u8]) -> Result<[u8; 32], String> {
    if ed_pub.len() != 32 { return Err("Invalid Ed25519 key length".to_string()); }
    let compressed = CompressedEdwardsY::from_slice(ed_pub);
    let ed_point = compressed.decompress().ok_or("Invalid Ed25519 public key (decompression failed)")?;
    let x25519_pub = ed_point.to_montgomery();
    Ok(x25519_pub.to_bytes())
}

pub(crate) fn ed25519_priv_to_x25519(ed_priv_seed: &[u8]) -> Result<StaticSecret, String> {
    if ed_priv_seed.len() != 32 { return Err("Invalid Ed25519 seed length".to_string()); }
    let mut hasher = Sha512::new();
    hasher.update(ed_priv_seed);
    let hash = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&hash[0..32]);
    Ok(StaticSecret::from(bytes))
}

pub fn calculate_safety_number(me_ik: &str, peer_ik: &str) -> Result<String, String> {
    let mut keys = vec![me_ik.to_string(), peer_ik.to_string()];
    keys.sort();
    
    let mut hasher = Sha256::new();
    hasher.update(b"EntropySafetyNumberV1");
    hasher.update(keys[0].as_bytes());
    hasher.update(keys[1].as_bytes());
    
    let hash = hasher.finalize();
    
    let mut result = String::new();
    for chunk in hash.chunks(4) {
        let val = u32::from_be_bytes(<[u8; 4]>::try_from(chunk).unwrap_or([0; 4]));
        result.push_str(&format!("{:05} ", val % 100000));
        if result.len() >= 35 { break; } 
    }
    
    Ok(result.trim().to_string())
}

pub fn mine_pow(seed: &str, difficulty: u32, context: &str) -> Result<(u64, String), String> {
        let mut nonce = 0u64;
        let target_prefix = "0".repeat(difficulty as usize);
        
        loop {
            let mut hasher = Sha256::new();
            hasher.update(seed.as_bytes());
            if !context.is_empty() {
                hasher.update(context.as_bytes());
            }
            hasher.update(nonce.to_string().as_bytes());
            let hash = hex::encode(hasher.finalize());
            
            if hash.starts_with(&target_prefix) {
                return Ok((nonce, hash));
            }
            
            nonce += 1;
            if nonce % 100000 == 0 {
                
            }
        }
    }
