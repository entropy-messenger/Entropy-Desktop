
use rusqlite::{params, Connection};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;
use futures_util::{Stream, Sink, SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use sha2::{Sha256, Digest};
use tokio_socks::tcp::Socks5Stream;
use url::Url;
use tokio::time::Duration;
use tracing;
use serde_json::{json, Value};
use base64::Engine;
use tokio_tungstenite::tungstenite::Utf8Bytes;
use serde::{Serialize, Deserialize};
use crate::app_state::{DbState, NetworkState, AudioState, PacedMessage};
use crate::signal_store::SqliteSignalStore;
use crate::security::noise::TrafficNormalizer;
use libsignal_protocol::{
    IdentityKey, IdentityKeyPair, PreKeyBundle, ProtocolAddress,
    SignalProtocolError, PreKeyRecord, SignedPreKeyRecord, KyberPreKeyRecord,
    PreKeyId, SignedPreKeyId, KyberPreKeyId, Timestamp, kem, GenericSignedPreKey, KeyPair,
    CiphertextMessage, CiphertextMessageType, message_encrypt, message_decrypt, process_prekey_bundle,
    DeviceId, IdentityKeyStore, PreKeyStore, SignedPreKeyStore, KyberPreKeyStore
};
use rand::SeedableRng;
use rand::rngs::StdRng;
use hex;
use std::io::{Read, Write};
use walkdir::WalkDir;
use zip::write::FileOptions;
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce
};
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2, Params,
};
// Removed unused BigUint

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DbMessage {
    pub id: String,
    pub chat_address: String,
    pub sender_hash: String,
    pub content: String,
    pub timestamp: i64,
    pub r#type: String,
    pub status: String,
    pub attachment_json: Option<String>,
    #[serde(default)]
    pub is_starred: bool,
    #[serde(default)]
    pub is_group: bool,
    pub reply_to_json: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DbChat {
    pub address: String,
    #[serde(default)]
    pub is_group: bool,
    pub alias: Option<String>,
    pub last_msg: Option<String>,
    pub last_timestamp: Option<i64>,
    pub last_sender_hash: Option<String>,
    pub last_status: Option<String>,
    #[serde(default)]
    pub unread_count: i32,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub trust_level: i32,
    #[serde(default)]
    pub is_blocked: bool,
    pub members: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbContact {
    pub hash: String,
    pub alias: Option<String>,
    pub is_blocked: bool,
    pub trust_level: i32,
}

const PACKET_SIZE: usize = 1400; // MTU-Safe (Fits in single 1500 MTU frame)



async fn send_paced_json(app: &tauri::AppHandle, val: serde_json::Value) -> Result<(), String> {
    let json_str = serde_json::to_string(&val).unwrap();
    let raw_len = json_str.len();

    let net_state = app.state::<NetworkState>();
    let tx_lock = net_state.sender.lock().unwrap();
    let tx = tx_lock.as_ref().ok_or("Network not connected")?;

    if raw_len > 1200 {
        // Chunk it (Type 0x00)
        let data_bytes = json_str.into_bytes();
        let chunks = (data_bytes.len() as f32 / 1200.0).ceil() as usize;
        let transfer_id: u32 = rand::random();
        let zero_hash = vec![0u8; 64];

        for i in 0..chunks {
            let start = i * 1200;
            let end = std::cmp::min(start + 1200, data_bytes.len());
            let chunk_data = &data_bytes[start..end];
            let mut envelope = Vec::with_capacity(1400);
            envelope.extend_from_slice(&zero_hash);
            envelope.push(0x00); 
            envelope.extend_from_slice(&transfer_id.to_be_bytes());
            envelope.extend_from_slice(&(i as u32).to_be_bytes());
            envelope.extend_from_slice(&(chunks as u32).to_be_bytes());
            envelope.extend_from_slice(&(chunk_data.len() as u32).to_be_bytes());
            envelope.extend_from_slice(chunk_data);
            tx.send(PacedMessage { msg: Message::Binary(envelope.into()), is_media: false }).map_err(|e| e.to_string())?;
        }
    } else {
        tx.send(PacedMessage { msg: Message::Text(Utf8Bytes::from(json_str)), is_media: false }).map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn internal_request(
    state: &NetworkState,
    msg_type: &str,
    payload: serde_json::Value
) -> Result<serde_json::Value, String> {
    let req_id = uuid::Uuid::new_v4().to_string();
    let mut full_payload = payload.clone();
    full_payload["type"] = serde_json::Value::String(msg_type.to_string());
    full_payload["req_id"] = serde_json::Value::String(req_id.clone());

    let (tx, rx) = tokio::sync::oneshot::channel();
    {
        let mut channels = state.response_channels.lock().unwrap();
        channels.insert(req_id.clone(), tx);
    }

    {
        let sender_lock = state.sender.lock().unwrap();
        if let Some(ws_tx) = &*sender_lock {
            let text = full_payload.to_string();
            if text.len() > 1200 {
                // Chunked control request
                let data_bytes = text.into_bytes();
                let total_len = data_bytes.len();
                let chunk_capacity = 1319; 
                let chunks = (total_len as f64 / chunk_capacity as f64).ceil() as usize;
                let transfer_id: u32 = rand::random();
                let zero_hash = vec![0u8; 64];
                println!("[Net] TX Internal Request (Chunked): raw_total={} total_chunks={} tid={}", total_len, chunks, transfer_id);
                for i in 0..chunks {
                    let start = i * chunk_capacity;
                    let end = std::cmp::min(start + chunk_capacity, total_len);
                    let chunk_data = &data_bytes[start..end];
                    let mut env = Vec::with_capacity(PACKET_SIZE);
                    env.extend_from_slice(&zero_hash);
                    env.push(0x00);
                    env.extend_from_slice(&transfer_id.to_be_bytes());
                    env.extend_from_slice(&(i as u32).to_be_bytes());
                    env.extend_from_slice(&(chunks as u32).to_be_bytes());
                    env.extend_from_slice(&(chunk_data.len() as u32).to_be_bytes());
                    env.extend_from_slice(chunk_data);
                    let _ = ws_tx.send(PacedMessage { msg: Message::Binary(env.into()), is_media: false });
                }
            } else {
                let raw_len = text.len();
                println!("[Net] TX Internal Request (Single): raw_size={} target=PACKET_SIZE", raw_len);
                let _ = ws_tx.send(PacedMessage {
                    msg: Message::Text(Utf8Bytes::from(text)),
                    is_media: false,
                });
            }
        } else {
            let mut channels = state.response_channels.lock().unwrap();
            channels.remove(&req_id);
            return Err("Not connected to network".into());
        }
    }

    match tokio::time::timeout(Duration::from_secs(10), rx).await {
        Ok(Ok(res)) => Ok(res),
        _ => {
            let mut channels = state.response_channels.lock().unwrap();
            channels.remove(&req_id);
            Err("Request timed out".into())
        }
    }
}

fn internal_decode_key(s: &str) -> Result<Vec<u8>, String> {
    // Priority 1: Check if it's pure HEX (0-9, a-f) and even length
    if (s.len() == 64 || s.len() == 66) && s.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(b) = hex::decode(s) {
            return Ok(b);
        }
    }
    // Priority 2: Try Base64 (Standard Signal/Relay Format)
    if let Ok(b) = base64::engine::general_purpose::STANDARD.decode(s) {
        return Ok(b);
    }
    // Verify with secondary derivation
    hex::decode(s).map_err(|e| format!("Failed to decode key as Base64 or Hex: {} -> {}", s, e))
}

async fn internal_establish_session_logic(
    app: AppHandle,
    remote_hash: &str,
    bundle: serde_json::Value
) -> Result<(), String> {
    let mut store = SqliteSignalStore::new(app.clone());
    let address = ProtocolAddress::new(remote_hash.to_string(), DeviceId::try_from(1u32).expect("valid ID"));

    let registration_id = bundle["registrationId"].as_u64().ok_or("Missing registrationId")? as u32;
    let identity_key_hex = bundle["identityKey"].as_str().ok_or("Missing identityKey")?;
    let mut identity_key_bytes = internal_decode_key(identity_key_hex)?;

    println!("[Signal] Establishing session for {}. IdentityKeyLen={}", remote_hash, identity_key_bytes.len());

    // --- IDENTITY CHANGE DETECTION ---
    // Check if we already have a different identity key for this address
    let mut existing_trust = 1; // Default to Trusted
    {
        let state = app.state::<DbState>();
        let lock = state.conn.lock().unwrap();
        if let Some(conn) = lock.as_ref() {
            let res: Result<(Vec<u8>, i32), _> = conn.query_row(
                "SELECT public_key, trust_level FROM signal_identities_remote WHERE address = ?1",
                params![address.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?))
            );

            if let Ok((old_key, old_trust)) = res {
                // If keys are different, reset trust to 0 (Untrusted)
                // Note: stripping prefix for comparison
                let old_key_stripped = if old_key.len() == 33 && old_key[0] == 0x05 { &old_key[1..] } else { &old_key };
                let new_key_stripped = &identity_key_bytes;
                
                if old_key_stripped != new_key_stripped {
                    println!("[Signal] IDENTITY CHANGED for {}! Resetting trust to 0.", remote_hash);
                    existing_trust = 0;
                    
                    // Update contacts table immediately for UI
                    let _ = conn.execute(
                        "UPDATE contacts SET trust_level = 0 WHERE hash = ?1",
                        params![remote_hash]
                    );
                } else {
                    existing_trust = old_trust;
                }
            }
        }
    }

    // Standard Signal Identity Keys are 33 bytes (type 0x05 + 32-byte public key).
    // The Entropy relay stores them stripped (32 bytes) to match derivation of identity_hash.
    if identity_key_bytes.len() == 32 {
        let mut new_bytes = Vec::with_capacity(33);
        new_bytes.push(0x05);
        new_bytes.extend_from_slice(&identity_key_bytes);
        identity_key_bytes = new_bytes;
    }
    let identity_key = IdentityKey::decode(&identity_key_bytes).map_err(|e| e.to_string())?;

    let (pre_key_id, pre_key_pub) = {
        let pre_key_obj = bundle.get("preKey")
            .or_else(|| bundle.get("preKeys").and_then(|pk| pk.as_array()?.first()))
            .ok_or("Missing preKey object")?;
        let id = PreKeyId::from(pre_key_obj["id"].as_u64().ok_or("Missing preKey id")? as u32);
        let pub_str = pre_key_obj["publicKey"].as_str().ok_or("Missing preKey publicKey")?;
        let mut pub_bytes = internal_decode_key(pub_str)?;
        println!("[Signal]   PreKey {} Len={}", u32::from(id), pub_bytes.len());
        // Relay stores keys as raw 32 bytes — prepend Curve25519 type prefix if missing
        if pub_bytes.len() == 32 {
            let mut prefixed = Vec::with_capacity(33);
            prefixed.push(0x05);
            prefixed.extend_from_slice(&pub_bytes);
            pub_bytes = prefixed;
        }
        let pub_key = libsignal_protocol::PublicKey::deserialize(&pub_bytes).map_err(|e| e.to_string())?;
        (id, pub_key)
    };

    let signed_pre_key_id = SignedPreKeyId::from(bundle["signedPreKey"]["id"].as_u64().ok_or("Missing signedPreKey id")? as u32);
    let s_pub_str = bundle["signedPreKey"]["publicKey"].as_str().ok_or("Missing signedPreKey publicKey")?;
    let mut signed_pre_key_pub_bytes = internal_decode_key(s_pub_str)?;
    println!("[Signal]   SignedPreKey {} Len={}", u32::from(signed_pre_key_id), signed_pre_key_pub_bytes.len());
    // Relay stores keys as raw 32 bytes — prepend Curve25519 type prefix if missing
    if signed_pre_key_pub_bytes.len() == 32 {
        let mut prefixed = Vec::with_capacity(33);
        prefixed.push(0x05);
        prefixed.extend_from_slice(&signed_pre_key_pub_bytes);
        signed_pre_key_pub_bytes = prefixed;
    }
    let signed_pre_key_pub = libsignal_protocol::PublicKey::deserialize(&signed_pre_key_pub_bytes).map_err(|e| e.to_string())?;
    let s_sig_str = bundle["signedPreKey"]["signature"].as_str().ok_or("Missing signedPreKey signature")?;
    let signed_pre_key_sig = if let Ok(b) = base64::engine::general_purpose::STANDARD.decode(s_sig_str) { b } else { hex::decode(s_sig_str).map_err(|e| e.to_string())? };

    let kyber_pre_key_id = KyberPreKeyId::from(bundle["kyberPreKey"]["id"].as_u64().ok_or("Missing kyberPreKey id")? as u32);
    let k_pub_str = bundle["kyberPreKey"]["publicKey"].as_str().ok_or("Missing kyberPreKey publicKey")?;
    let kyber_pre_key_pub_bytes = internal_decode_key(k_pub_str)?;
    println!("[Signal]   KyberPreKey {} Len={}", u32::from(kyber_pre_key_id), kyber_pre_key_pub_bytes.len());
    let kyber_pre_key_pub = kem::PublicKey::deserialize(&kyber_pre_key_pub_bytes).map_err(|e| e.to_string())?;
    let k_sig_str = bundle["kyberPreKey"]["signature"].as_str().ok_or("Missing kyberPreKey signature")?;
    let kyber_pre_key_sig = if let Ok(b) = base64::engine::general_purpose::STANDARD.decode(k_sig_str) { b } else { hex::decode(k_sig_str).map_err(|e| e.to_string())? };

    let prekey_bundle = PreKeyBundle::new(
        registration_id,
        DeviceId::try_from(1u32).expect("valid ID"),
        Some((pre_key_id, pre_key_pub)),
        signed_pre_key_id,
        signed_pre_key_pub,
        signed_pre_key_sig,
        kyber_pre_key_id,
        kyber_pre_key_pub,
        kyber_pre_key_sig,
        identity_key.clone(),
    ).map_err(|e| e.to_string())?;

    // CRITICAL: Cryptographic Pinning - Verify that the bundle owner is legitimate.
    // We hash the raw 32-byte key (stripping 0x05 prefix if present) to match signal_init derivation.
    let mut hasher = Sha256::new();
    let ser = identity_key.serialize();
    let to_hash = if ser.len() == 33 && ser[0] == 0x05 { &ser[1..] } else { ser.as_ref() };
    hasher.update(to_hash);
    let id_hash = hex::encode(hasher.finalize());
    if id_hash != remote_hash.to_lowercase() {
        return Err(format!("Identity Mismatch! Bundle for {} belongs to a different key: {}", remote_hash, id_hash));
    }

    let mut rng = StdRng::from_os_rng();
    process_prekey_bundle(
        &address,
        &mut store.clone(),
        &mut store,
        &prekey_bundle,
        std::time::SystemTime::now(),
        &mut rng,
    ).await.map_err(|e| e.to_string())?;

    // Update the trust level in the remote identities table (if it was reset or found)
    {
        let state = app.state::<DbState>();
        let lock = state.conn.lock().unwrap();
        if let Some(conn) = lock.as_ref() {
            let _ = conn.execute(
                "UPDATE signal_identities_remote SET trust_level = ?1 WHERE address = ?2",
                params![existing_trust, address.to_string()]
            );
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn start_native_recording(app: AppHandle, state: State<'_, AudioState>) -> Result<String, String> {
    let mut recorder = state.recorder.lock().unwrap();
    recorder.start_recording(app)
}

#[tauri::command]
pub async fn stop_native_recording(state: State<'_, AudioState>) -> Result<Vec<u8>, String> {
    let mut recorder = state.recorder.lock().unwrap();
    recorder.stop_recording()
}

pub fn get_db_filename() -> String {
    if let Ok(profile) = std::env::var("ENTROPY_PROFILE") {
        if !profile.is_empty() {
             return format!("entropy_{}.db", profile);
        }
    }
    "entropy.db".to_string()
}

pub async fn internal_mine_pow(seed: String, difficulty: u32, _context: String, modulus: Option<String>) -> serde_json::Value {
    let n_str = modulus.as_ref().map(|s| s.to_string()).unwrap_or_else(|| "16924353219721975706619304977087776638210692887418153614822570947993460098757637997153620390534205323940422136903855515357288961635893026503845398062994157546242993897432842505612884614045940034466012450686593767189610225378750810792439341873585245840091628083670434049768166724299902688993164080731321559365156036266700853190146043193271501897793442680973988812797807962731521024848426255262545103363066538288771520973709300521207461949980255896180578618344539304776270176040513674389484251916722619230508579403099751552290930600171147372478499901544032334923289379116695422056004175570276337468297686269307727794059".to_string());
    
    let seed_clone = seed.clone();
    // Offload CPU-heavy VDF calculation to a thread pool to prevent async runtime starvation/UI lag
    let result_hex = tauri::async_runtime::spawn_blocking(move || {
        let n = num_bigint::BigUint::parse_bytes(n_str.as_bytes(), 10).expect("Valid modulus");
        let x_bytes = hex::decode(&seed_clone).unwrap_or_default();
        let mut x = num_bigint::BigUint::from_bytes_be(&x_bytes) % &n;
        
        // Exact 1:1 VDF logic (y = x^(2^T) mod N)
        for _ in 0..difficulty {
            x = (&x * &x) % &n;
        }
        hex::encode(x.to_bytes_be())
    }).await.unwrap_or_default();
    
    serde_json::json!({
        "seed": seed,
        "nonce": result_hex,
        "difficulty": difficulty,
        "modulus": modulus.unwrap_or_default()
    })
}

#[tauri::command]
pub async fn crypto_mine_pow(seed: String, difficulty: u32, context: Option<String>, modulus: Option<String>) -> Result<serde_json::Value, String> {
    let ctx = context.unwrap_or_default();
    Ok(internal_mine_pow(seed, difficulty, ctx, modulus).await)
}

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
pub fn signal_decrypt_media(data: Vec<u8>, bundle: serde_json::Value) -> Result<Vec<u8>, String> {
    let key_b64 = bundle.get("key").and_then(|k| k.as_str()).ok_or("No decryption key in bundle")?;
    let ciphertext_hex = hex::encode(data);
    crypto_decrypt_media(ciphertext_hex, key_b64.to_string())
}

#[tauri::command]
pub fn crypto_decrypt_media(ciphertext_hex: String, key_b64: String) -> Result<Vec<u8>, String> {
    let combined = hex::decode(ciphertext_hex).map_err(|e| e.to_string())?;
    if combined.len() < 12 {
        return Err("Ciphertext too short".into());
    }

    let key_bytes = base64::engine::general_purpose::STANDARD.decode(key_b64).map_err(|e| e.to_string())?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let nonce = Nonce::from_slice(&combined[..12]);
    let ciphertext = &combined[12..];

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    Ok(plaintext)
}

#[tauri::command]
pub fn vault_exists(app: AppHandle) -> bool {
    if let Ok(app_data_dir) = app.path().app_data_dir() {
        return app_data_dir.join(get_db_filename()).exists();
    }
    false
}

#[tauri::command]
pub async fn init_vault(app: tauri::AppHandle, state: State<'_, DbState>, passphrase: String) -> Result<(), String> {

    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    
    if !app_data_dir.exists() {
        std::fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;
    }

    let db_path = app_data_dir.join(get_db_filename());
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
        | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;

    let attempts_file = app_data_dir.join("login_attempts.dat");
    let mut attempts = 0;
    if attempts_file.exists() {
        if let Ok(s) = std::fs::read_to_string(&attempts_file) {
            attempts = s.trim().parse().unwrap_or(0);
        }
    }

    // PANIC MODE CHECK
    let panic_file = app_data_dir.join("panic.dat");
    if panic_file.exists() {
        if let Ok(stored_hash) = std::fs::read_to_string(&panic_file) {
            let mut hasher = Sha256::new();
            hasher.update(passphrase.clone());
            let input_hash = hex::encode(hasher.finalize());
            
            if input_hash == stored_hash.trim() {
                 let filename = get_db_filename();
                 let _ = std::fs::remove_file(app_data_dir.join(&filename));
                 let _ = std::fs::remove_file(app_data_dir.join(format!("{}-wal", filename)));
                 let _ = std::fs::remove_file(app_data_dir.join(format!("{}-shm", filename)));
                 let _ = std::fs::remove_dir_all(app_data_dir.join("media"));
                 let _ = std::fs::remove_file(&attempts_file);
                 println!("[!] Panic password triggered. Wiping and restarting...");
                 app.restart();
            }
        }
    }

    if attempts >= 10 {
         // Nuclear reset logic inline
         let filename = get_db_filename();
         let _ = std::fs::remove_file(app_data_dir.join(&filename));
         let _ = std::fs::remove_file(app_data_dir.join(format!("{}-wal", filename)));
         let _ = std::fs::remove_file(app_data_dir.join(format!("{}-shm", filename)));
         let _ = std::fs::remove_dir_all(app_data_dir.join("media"));
          let _ = std::fs::remove_file(&attempts_file);
          println!("[!] Max attempts reached. Wiping and restarting...");
          app.restart();
    }

    let conn_res = Connection::open_with_flags(&db_path, flags);
    
    // If connection opens, try to set key and query. If fail, increment attempts.
    let conn = match conn_res {
        Ok(c) => c,
        Err(e) => return Err(e.to_string()),
    };

    if !passphrase.is_empty() {
        // Argon2id is intentionally slow — offload to blocking thread pool to keep UI responsive
        let derived_key_hex = tauri::async_runtime::spawn_blocking(move || {
            let salt = SaltString::from_b64("ZW50cm9weV9zYWx0XzI1Ng").unwrap(); // "entropy_salt_256"
            let argon2 = Argon2::new(
                argon2::Algorithm::Argon2id,
                argon2::Version::V0x13,
                Params::new(65536, 3, 4, Some(32)).unwrap(),
            );
            let password_hash = argon2.hash_password(passphrase.as_bytes(), &salt)
                .map_err(|e| format!("Argon2 hash failed: {}", e))?;
            let derived_key = password_hash.hash.unwrap();
            Ok::<String, String>(hex::encode(derived_key.as_ref()))
        }).await.map_err(|e| e.to_string())??;

        let key_query = format!("PRAGMA key = \"x'{}'\";", derived_key_hex);
        let _ = conn.execute_batch(&key_query);
    }
    
    // Test if key is correct by reading user_version
    if let Err(_) = conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(())) {
         attempts += 1;
         let _ = std::fs::write(&attempts_file, attempts.to_string());
         return Err(format!("Incorrect password. Attempt {}/10", attempts));
    }

    // Success - reset attempts
    if attempts > 0 {
        let _ = std::fs::remove_file(attempts_file);
    }

    // Enable WAL mode for better concurrency
    let _ = conn.execute("PRAGMA journal_mode=WAL;", []);

    // Init basic table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS kv_store (
            key TEXT PRIMARY KEY,
            value TEXT
        )",
        [],
    ).map_err(|e: rusqlite::Error| e.to_string())?;
    
    conn.execute(
        "CREATE TABLE IF NOT EXISTS pending_outbox (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            msg_type TEXT,
            content BLOB,
            timestamp INTEGER
        )",
        [],
    ).map_err(|e: rusqlite::Error| e.to_string())?;

    // Signal Protocol Tables
    conn.execute(
        "CREATE TABLE IF NOT EXISTS signal_identity (
            id INTEGER PRIMARY KEY CHECK (id = 0),
            registration_id INTEGER,
            public_key BLOB,
            private_key BLOB
        )",
        [],
    ).map_err(|e| format!("Failed to create signal_identity: {}", e))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS signal_pre_keys (
            key_id INTEGER PRIMARY KEY,
            key_data BLOB
        )",
        [],
    ).map_err(|e| format!("Failed to create signal_pre_keys: {}", e))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS signal_signed_pre_keys (
            key_id INTEGER PRIMARY KEY,
            key_data BLOB,
            signature BLOB,
            timestamp INTEGER
        )",
        [],
    ).map_err(|e| format!("Failed to create signal_signed_pre_keys: {}", e))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS signal_sessions (
            address TEXT PRIMARY KEY,
            session_data BLOB
        )",
        [],
    ).map_err(|e| format!("Failed to create signal_sessions: {}", e))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS signal_identities_remote (
            address TEXT PRIMARY KEY,
            public_key BLOB NOT NULL,
            trust_level INTEGER DEFAULT 1
        );",
        [],
    ).map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS signal_kyber_pre_keys (
            key_id INTEGER PRIMARY KEY,
            key_data BLOB NOT NULL
        );",
        [],
    ).map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS signal_kyber_base_keys_seen (
            kyber_prekey_id INTEGER NOT NULL,
            ec_prekey_id INTEGER NOT NULL,
            base_key BLOB NOT NULL,
            PRIMARY KEY (kyber_prekey_id, ec_prekey_id, base_key)
        );",
        [],
    ).map_err(|e| e.to_string())?;

    // Entity Tables
    conn.execute(
        "CREATE TABLE IF NOT EXISTS contacts (
            hash TEXT PRIMARY KEY,
            alias TEXT,
            is_blocked INTEGER DEFAULT 0,
            trust_level INTEGER DEFAULT 1
        )",
        [],
    ).map_err(|e| format!("Failed to create contacts: {}", e))?;

    // SELF-HEALING MIGRATION: 0 was accidentally used as default, which means mismatch. Reset to 1 (Trusted).
    let _ = conn.execute("UPDATE contacts SET trust_level = 1 WHERE trust_level = 0", []);
    let _ = conn.execute("UPDATE signal_identities_remote SET trust_level = 1 WHERE trust_level = 0", []);


    conn.execute(
        "CREATE TABLE IF NOT EXISTS chats (
            address TEXT PRIMARY KEY,
            is_group INTEGER DEFAULT 0,
            alias TEXT,
            last_msg TEXT,
            last_timestamp INTEGER,
            last_sender_hash TEXT,
            last_status TEXT,
            unread_count INTEGER DEFAULT 0,
            is_archived INTEGER DEFAULT 0
        )",
        [],
    ).map_err(|e| format!("Failed to create chats: {}", e))?;

    // Phase 2 Migrations: Ensure all required columns exist in chats for sidebar metadata
    let _ = conn.execute("ALTER TABLE chats ADD COLUMN last_msg TEXT", []);
    let _ = conn.execute("ALTER TABLE chats ADD COLUMN last_timestamp INTEGER", []);
    let _ = conn.execute("ALTER TABLE chats ADD COLUMN last_sender_hash TEXT", []);
    let _ = conn.execute("ALTER TABLE chats ADD COLUMN last_status TEXT", []);

    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            chat_address TEXT,
            sender_hash TEXT,
            content TEXT,
            timestamp INTEGER,
            type TEXT,
            status TEXT,
            attachment_json TEXT,
            is_group INTEGER DEFAULT 0,
            is_starred INTEGER DEFAULT 0,
            is_pinned INTEGER DEFAULT 0,
            reply_to_json TEXT,
            FOREIGN KEY(chat_address) REFERENCES chats(address)
        )",
        [],
    ).map_err(|e| format!("Failed to create messages: {}", e))?;

    // Migration
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN reply_to_json TEXT;", []);

    // Phase 3 Metadata Reconciliation: Pull latest message for each chat into the sidebar record if missing
    // Guaranteed to work now that messages table exists.
    let _ = conn.execute(
        "UPDATE chats 
         SET last_msg = (SELECT SUBSTR(content, 1, 100) FROM messages WHERE chat_address = chats.address ORDER BY timestamp DESC LIMIT 1),
             last_timestamp = (SELECT timestamp FROM messages WHERE chat_address = chats.address ORDER BY timestamp DESC LIMIT 1),
             last_sender_hash = (SELECT sender_hash FROM messages WHERE chat_address = chats.address ORDER BY timestamp DESC LIMIT 1),
             last_status = (SELECT status FROM messages WHERE chat_address = chats.address ORDER BY timestamp DESC LIMIT 1)
         WHERE last_msg IS NULL OR last_timestamp IS NULL",
        []
    ).map_err(|e| format!("Reconciliation failed: {}", e))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS chat_members (
            chat_address TEXT,
            member_hash TEXT,
            PRIMARY KEY (chat_address, member_hash),
            FOREIGN KEY(chat_address) REFERENCES chats(address)
        )",
        [],
    ).map_err(|e| format!("Failed to create chat_members: {}", e))?;

    // 🚀 Performance: Indexes for fast message retrieval and sidebar sorting
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_chat_addr ON messages(chat_address, timestamp)",
        [],
    ).map_err(|e| format!("Failed to create idx_messages_chat_addr: {}", e))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chats_last_ts ON chats(last_timestamp)",
        [],
    ).map_err(|e| format!("Failed to create idx_chats_last_ts: {}", e))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_members_hash ON chat_members(member_hash)",
        [],
    ).map_err(|e| format!("Failed to create idx_members_hash: {}", e))?;

    // 🔍 FTS5 for instant message search (Virtual Table)
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS message_search USING fts5(
            message_id UNINDEXED,
            content,
            chat_address UNINDEXED,
            content='messages',
            content_rowid='rowid'
        )",
        [],
    ).map_err(|e| format!("Failed to create message_search FTS5: {}", e))?;

    // Trigger to keep FTS index in sync with base table
    conn.execute("CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
        INSERT INTO message_search(rowid, message_id, content, chat_address) 
        VALUES (new.rowid, new.id, new.content, new.chat_address);
    END;", []).map_err(|e| format!("Failed to create FTS insert trigger: {}", e))?;

    conn.execute("CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
        INSERT INTO message_search(message_search, rowid, message_id, content, chat_address) 
        VALUES('delete', old.rowid, old.id, old.content, old.chat_address);
    END;", []).map_err(|e| format!("Failed to create FTS delete trigger: {}", e))?;

    conn.execute("CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
        INSERT INTO message_search(message_search, rowid, message_id, content, chat_address) 
        VALUES('delete', old.rowid, old.id, old.content, old.chat_address);
        INSERT INTO message_search(rowid, message_id, content, chat_address) 
        VALUES (new.rowid, new.id, new.content, new.chat_address);
    END;", []).map_err(|e| format!("Failed to create FTS update trigger: {}", e))?;
    
    // Phase 3 & 4 Migrations: Add is_pinned and is_starred columns if missing
    let _ = conn.execute("ALTER TABLE chats ADD COLUMN is_pinned INTEGER DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN is_starred INTEGER DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN is_group INTEGER DEFAULT 0", []);

    // Media Encryption Key Initialization
    let media_key = {
        let mut stmt = conn.prepare("SELECT value FROM kv_store WHERE key = '_internal_media_key'").map_err(|e| e.to_string())?;
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let hex_key: String = row.get(0).map_err(|e| e.to_string())?;
            hex::decode(hex_key).map_err(|e| e.to_string())?
        } else {
            let key = Aes256Gcm::generate_key(&mut OsRng);
            let hex_key = hex::encode(key);
            conn.execute("INSERT INTO kv_store (key, value) VALUES ('_internal_media_key', ?1)", [&hex_key])
                .map_err(|e| e.to_string())?;
            key.to_vec()
        }
    };

    let mut db_conn = state.conn.lock().unwrap();
    *db_conn = Some(conn);
    let mut state_key = state.media_key.lock().unwrap();
    *state_key = Some(media_key);
    Ok(())
}

fn get_media_dir(app: &tauri::AppHandle, state: &State<'_, DbState>) -> Result<std::path::PathBuf, String> {
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let profile = state.profile.lock().unwrap();
    let media_dir = app_dir.join("media").join(&*profile);
    if !media_dir.exists() {
        std::fs::create_dir_all(&media_dir).map_err(|e| e.to_string())?;
    }
    Ok(media_dir)
}

#[tauri::command]
pub fn set_panic_password(app: tauri::AppHandle, password: String) -> Result<(), String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(password);
    let hash = hex::encode(hasher.finalize());
    
    std::fs::write(app_data_dir.join("panic.dat"), hash).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn vault_save(state: State<'_, DbState>, key: String, value: String) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        conn.execute(
            "INSERT OR REPLACE INTO kv_store (key, value) VALUES (?1, ?2);",
            [key, value],
        )
        .map_err(|e: rusqlite::Error| e.to_string())?;
        Ok(())
    } else {
        Err("Database not initialized".to_string())
    }
}

#[tauri::command]
pub fn vault_load(state: State<'_, DbState>, key: String) -> Result<Option<String>, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let mut stmt = conn
            .prepare("SELECT value FROM kv_store WHERE key = ?1;")
            .map_err(|e: rusqlite::Error| e.to_string())?;
        let mut rows = stmt.query([key]).map_err(|e: rusqlite::Error| e.to_string())?;

        if let Some(row) = rows.next().map_err(|e: rusqlite::Error| e.to_string())? {
            Ok(Some(row.get::<_, String>(0).map_err(|e: rusqlite::Error| e.to_string())?))
        } else {
            Ok(None)
        }
    } else {
        Err("Database not initialized".to_string())
    }
}



// --- RELATIONAL DB COMMANDS ---

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReplyTo {
    pub id: String,
    pub content: String,
    pub sender_hash: Option<String>,
    pub sender_alias: Option<String>,
    pub r#type: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingText {
    pub recipient: String,
    pub content: String,
    pub reply_to: Option<ReplyTo>,
    #[serde(rename = "isGroup", default)]
    pub is_group: bool,
    pub group_members: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingMedia {
    pub recipient: String,
    pub file_path: Option<String>,
    pub file_data: Option<Vec<u8>>,
    pub file_name: String,
    pub file_type: String,
    pub msg_type: String, // "file" or "voice_note"
    pub is_group: bool,
    pub group_members: Option<Vec<String>>,
    pub reply_to: Option<ReplyTo>,
}

#[tauri::command]
pub fn process_outgoing_text(
    app: AppHandle,
    payload: OutgoingText,
) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let db_state = app.state::<DbState>();
            let net_state = app.state::<NetworkState>();

            if payload.content.chars().count() > 16000 {
                return Err("Message too long (max 16000 characters)".into());
            }

            let msg_id = uuid::Uuid::new_v4().to_string();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let own_id = {
                let id_lock = net_state.identity_hash.lock().unwrap();
                id_lock.clone().ok_or("Not authenticated")?
            };

            // 1. Prepare E2EE payload
            let signal_payload = serde_json::json!({
                "type": "text_msg",
                "content": payload.content,
                "id": msg_id,
                "replyTo": payload.reply_to,
                "timestamp": timestamp,
                "isGroup": payload.is_group,
            });

            // 2. Encrypt using internal logic
            let ciphertext_obj = internal_signal_encrypt(
                app.clone(), 
                &net_state, 
                &payload.recipient, 
                signal_payload.to_string()
            ).await?;

            // 3. Save to Database
            let db_msg = DbMessage {
                id: msg_id.clone(),
                chat_address: payload.recipient.clone(),
                sender_hash: own_id.clone(),
                content: payload.content.clone(),
                timestamp,
                r#type: "text".to_string(),
                status: "sent".to_string(),
                attachment_json: None,
                is_starred: false,
                is_group: payload.is_group,
                reply_to_json: payload.reply_to.as_ref().map(|r| serde_json::to_string(&r).unwrap_or_default()),
            };
            internal_db_save_message(&db_state, db_msg.clone()).await?;

            // 4. Send to Network (Direct Binary per Member)
            if payload.is_group {
                let members = payload.group_members.ok_or("Group members missing")?;
                let routing_hash_list: Vec<String> = members.iter()
                    .filter(|&m| m != &own_id)
                    .map(|m| m.split('.').next().unwrap_or(m).to_string())
                    .collect();

                let payload_str = ciphertext_obj.to_string();
                let payload_bytes = payload_str.into_bytes();

                for hash in routing_hash_list {
                    internal_send_to_network(app.clone(), &net_state, Some(hash), None, Some(payload_bytes.clone()), true, false).await?;
                }
            } else {
                // Dispatch as a UNIVERSAL BINARY packet (Type 0x01)
                let routing_hash = payload.recipient.split('.').next().unwrap_or(&payload.recipient);
                let payload_str = ciphertext_obj.to_string();
                let payload_bytes = payload_str.into_bytes();
                
                // Send as binary, and 'is_media' = false (type 0x01) ensures Signal decryption path
                internal_send_to_network(app.clone(), &net_state, Some(routing_hash.to_string()), None, Some(payload_bytes), true, false).await?;
            }

            // 5. Emit event for UI refresh
            let final_json = serde_json::to_value(&db_msg).map_err(|e| e.to_string())?;
            app.emit("msg://added", final_json.clone()).map_err(|e| e.to_string())?;

            Ok(final_json)
        })
    }).join().map_err(|_| "Thread panicked".to_string())?
}

#[tauri::command]
pub fn process_outgoing_media(
    app: AppHandle,
    payload: OutgoingMedia,
) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let db_state = app.state::<DbState>();
            let net_state = app.state::<NetworkState>();

            let msg_id = uuid::Uuid::new_v4().to_string();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            
            // 1. Get raw data with strict 5MB limit
            let data = if let Some(p) = &payload.file_path {
                let metadata = std::fs::metadata(p).map_err(|e| e.to_string())?;
                if metadata.len() > 5 * 1024 * 1024 {
                    return Err("File too large. Maximum size is 5MB.".to_string());
                }
                let mut file = std::fs::File::open(p).map_err(|e| e.to_string())?;
                let mut d = Vec::new();
                file.read_to_end(&mut d).map_err(|e| e.to_string())?;
                d
            } else if let Some(d) = payload.file_data {
                if d.len() > 5 * 1024 * 1024 {
                    return Err("File too large. Maximum size is 5MB.".to_string());
                }
                d
            } else {
                return Err("No data provided".into());
            };

            // 2. Encrypt for peer (AES-GCM-256)
            let key = Aes256Gcm::generate_key(&mut OsRng);
            let cipher = Aes256Gcm::new(&key);
            let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
            let ciphertext = cipher.encrypt(&nonce, data.as_ref()).map_err(|e| e.to_string())?;
            
            let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
            combined.extend_from_slice(&nonce);
            combined.extend_from_slice(&ciphertext);
            let _ciphertext_b64 = base64::engine::general_purpose::STANDARD.encode(&combined);
            let key_b64 = base64::engine::general_purpose::STANDARD.encode(key);

            // 3. Save to Local Vault (encrypted with local media_key)
            let local_file_path = {
                 let local_key_bytes = {
                    let lock = db_state.media_key.lock().unwrap();
                    lock.clone().ok_or("Media key not initialized")?
                };
                let local_key = Key::<Aes256Gcm>::from_slice(&local_key_bytes);
                let local_cipher = Aes256Gcm::new(local_key);
                let local_nonce = Aes256Gcm::generate_nonce(&mut OsRng);
                let local_ciphertext = local_cipher.encrypt(&local_nonce, combined.as_ref()).map_err(|e| e.to_string())?;
                let mut final_blob = local_nonce.to_vec();
                final_blob.extend(local_ciphertext);
                
                let media_dir = get_media_dir(&app, &db_state)?;
                let file_path = media_dir.join(&msg_id);
                let mut f = std::fs::File::create(&file_path).map_err(|e| e.to_string())?;
                f.write_all(&final_blob).map_err(|e| e.to_string())?;
                file_path.to_string_lossy().to_string()
            };

            // 4. Construct Signal-layer payload (Fragmentation-only)
            let transfer_id: u32 = rand::random();
            
            let bundle = json!({
                "type": "signal_media",
                "key": key_b64,
                "file_name": payload.file_name,
                "file_type": payload.file_type,
                "file_size": data.len()
            });

            let content_obj = json!({
                "type": "file",
                "id": msg_id.clone(),
                "bundle": bundle,
                "data": serde_json::Value::Null,
                "transfer_id": transfer_id,
                "size": data.len(),
                "msg_type": payload.msg_type,
                "replyTo": payload.reply_to
            });

            // 5. Send to Network
            let members = if payload.is_group {
                payload.group_members.ok_or("Group members missing")?
            } else {
                vec![payload.recipient.clone()]
            };

            for member in members {
                let own_hash = {
                    let lock = net_state.identity_hash.lock().unwrap();
                    lock.clone().unwrap_or_default()
                };
                if member == own_hash { continue; }

                // A. Send Metadata (Signal-Encrypted JSON Type 0x01)
                let encrypted = internal_signal_encrypt(app.clone(), &net_state, &member, content_obj.to_string()).await?;
                let routing_hash = member.split('.').next().unwrap_or(&member);

                // Type 0x01: Metadata
                internal_send_to_network(app.clone(), &net_state, Some(routing_hash.to_string()), None, Some(encrypted.to_string().into_bytes()), true, false).await?;

                // B. Send binary fragments (Type 0x02)
                internal_send_to_network(
                    app.clone(), 
                    &net_state, 
                    Some(routing_hash.to_string()), 
                    None, 
                    Some(combined.clone()), 
                    true, 
                    true // is_media enables fragmentation logic with transfer_id
                ).await?;
            }

            // 6. DB Save
            let db_msg = DbMessage {
                id: msg_id.clone(),
                chat_address: payload.recipient.clone(),
                sender_hash: {
                    let lock = net_state.identity_hash.lock().unwrap();
                    lock.clone().unwrap_or_default()
                },
                content: if payload.msg_type == "voice_note" || payload.file_name == "voice_note.wav" { 
                    "Voice Note".to_string() 
                } else { 
                    format!("File: {}", payload.file_name) 
                },
                timestamp,
                r#type: payload.msg_type.clone(),
                status: "sent".to_string(),
                attachment_json: Some(json!({
                    "fileName": payload.file_name,
                    "fileType": payload.file_type,
                    "size": data.len(),
                    "bundle": bundle,
                    "data": if data.len() < 1024 * 1024 { Some(base64::engine::general_purpose::STANDARD.encode(&data)) } else { None },
                    "originalPath": payload.file_path,
                    "vaultPath": local_file_path
                }).to_string()),
                is_starred: false,
                is_group: payload.is_group,
                reply_to_json: payload.reply_to.as_ref().map(|r| serde_json::to_string(&r).unwrap_or_default()),
            };
            internal_db_save_message(&db_state, db_msg.clone()).await?;

            // 7. UI Emit
            let final_json = serde_json::to_value(&db_msg).map_err(|e| e.to_string())?;
            app.emit("msg://added", final_json.clone()).map_err(|e| e.to_string())?;

            Ok(final_json)
        })
    }).join().map_err(|_| "Thread panicked".to_string())?
}

#[tauri::command]
pub async fn db_save_message(state: tauri::State<'_, DbState>, msg: DbMessage) -> Result<(), String> {
    internal_db_save_message(&state, msg).await
}

pub async fn internal_db_save_message(state: &DbState, msg: DbMessage) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    // ... (rest of implementation remains same) ...

    // Ensure the parent chat record exists before saving the message
    // This prevents foreign key violations and "Ghost Chats"
    conn.execute(
        "INSERT OR IGNORE INTO chats (address, is_group, alias, unread_count, is_archived) 
         VALUES (?1, ?2, ?3, 0, 0)",
        // UUIDs (Groups) are 36 chars. SHA256 hashes (1:1) are 64 chars.
        params![msg.chat_address, (msg.chat_address.len() < 40) as i32, &msg.chat_address[0..8.min(msg.chat_address.len())]],
    ).map_err(|e| e.to_string())?;

    // Use INSERT OR IGNORE to prevent overwriting an existing message that might have a more advanced status (like 'read')
    conn.execute(
        "INSERT OR IGNORE INTO messages (id, chat_address, sender_hash, content, timestamp, type, status, attachment_json, is_group, is_starred, reply_to_json) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            msg.id, 
            msg.chat_address, 
            msg.sender_hash, 
            msg.content, 
            msg.timestamp, 
            msg.r#type, 
            msg.status, 
            msg.attachment_json,
            msg.is_group as i32,
            msg.is_starred as i32,
            msg.reply_to_json,
        ],
    ).map_err(|e| e.to_string())?;

    // If it was ignored, it means it already exists. We should only update the status if the new status is "better"
    // Status priority logic: read (3) > delivered (2) > sent (1) > sending (0)
    if msg.status != "sending" {
         conn.execute(
            "UPDATE messages SET status = ?1 
             WHERE id = ?2 AND (
                (status = 'sent' AND (?1 = 'delivered' OR ?1 = 'read')) OR
                (status = 'delivered' AND ?1 = 'read') OR
                (status = 'sending')
             )",
            params![msg.status, msg.id],
        ).map_err(|e| e.to_string())?;

        // Update attachment if provided (e.g. adding encryption bundle after processing)
        if let Some(json) = msg.attachment_json {
            conn.execute(
                "UPDATE messages SET attachment_json = ?1 WHERE id = ?2",
                params![json, msg.id],
            ).map_err(|e| e.to_string())?;
        }
    }

    // Auto-update the chat's last message info
    conn.execute(
        "UPDATE chats SET last_msg = ?1, last_timestamp = ?2, last_sender_hash = ?3, last_status = ?4 
         WHERE address = ?5 AND (last_timestamp IS NULL OR ?2 >= last_timestamp)",
        params![msg.content.chars().take(100).collect::<String>(), msg.timestamp, msg.sender_hash, msg.status, msg.chat_address],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn db_get_messages(state: State<'_, DbState>, chat_address: String, limit: u32, offset: u32, include_attachments: bool) -> Result<Vec<DbMessage>, String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    let sql = if include_attachments {
        "SELECT id, chat_address, sender_hash, content, timestamp, type, status, attachment_json, is_starred, is_group, reply_to_json
         FROM messages WHERE chat_address = ?1 ORDER BY timestamp DESC LIMIT ?2 OFFSET ?3"
    } else {
        "SELECT id, chat_address, sender_hash, content, timestamp, type, status, NULL, is_starred, is_group, reply_to_json
         FROM messages WHERE chat_address = ?1 ORDER BY timestamp DESC LIMIT ?2 OFFSET ?3"
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let rows = stmt.query_map(params![chat_address, limit, offset], |row| {
        Ok(DbMessage {
            id: row.get(0)?,
            chat_address: row.get(1)?,
            sender_hash: row.get(2)?,
            content: row.get(3)?,
            timestamp: row.get(4)?,
            r#type: row.get(5)?,
            status: row.get(6)?,
            attachment_json: row.get(7)?,
            is_starred: row.get::<_, i32>(8)? != 0,
            is_group: row.get::<_, i32>(9)? != 0,
            reply_to_json: row.get(10)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut msgs = Vec::new();
    for r in rows {
        msgs.push(r.map_err(|e| e.to_string())?);
    }
    // Return in chronological order for UI
    msgs.reverse();
    Ok(msgs)
}

#[tauri::command]
pub async fn db_search_messages(state: State<'_, DbState>, query: String) -> Result<Vec<DbMessage>, String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    // Search using FTS5 virtual table
    let mut stmt = conn.prepare(
        "SELECT m.id, m.chat_address, m.sender_hash, m.content, m.timestamp, m.type, m.status, NULL, m.is_starred, m.is_group, m.reply_to_json
         FROM message_search ms
         JOIN messages m ON ms.rowid = m.rowid
         WHERE message_search MATCH ?1
         ORDER BY m.timestamp DESC LIMIT 100"
    ).map_err(|e| e.to_string())?;

    let rows = stmt.query_map(params![query], |row| {
        Ok(DbMessage {
            id: row.get(0)?,
            chat_address: row.get(1)?,
            sender_hash: row.get(2)?,
            content: row.get(3)?,
            timestamp: row.get(4)?,
            r#type: row.get(5)?,
            status: row.get(6)?,
            attachment_json: row.get(7)?,
            is_starred: row.get::<_, i32>(8)? != 0,
            is_group: row.get::<_, i32>(9)? != 0,
            reply_to_json: row.get(10)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut msgs = Vec::new();
    for r in rows {
        msgs.push(r.map_err(|e| e.to_string())?);
    }
    Ok(msgs)
}

#[tauri::command]
pub async fn db_update_messages_status(state: State<'_, DbState>, _chat_address: String, ids: Vec<String>, status: String) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    for id in ids {
        // First, find which chat this message belongs to (important for groups)
        let actual_chat: Option<String> = conn.query_row(
            "SELECT chat_address FROM messages WHERE id = ?1",
            params![id],
            |row| row.get(0)
        ).ok();

        conn.execute(
            "UPDATE messages SET status = ?1 WHERE id = ?2",
            params![status, id],
        ).map_err(|e| e.to_string())?;

        // If we found the chat, update its sidebar preview status
        if let Some(addr) = actual_chat {
            let _ = conn.execute(
                "UPDATE chats SET last_status = ?1 WHERE address = ?2",
                params![status, addr],
            );
        }
    }

    Ok(())
}

pub async fn internal_db_upsert_chat(db_state: &DbState, chat: DbChat) -> Result<(), String> {
    let lock = db_state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    conn.execute(
        "INSERT OR REPLACE INTO chats (address, is_group, alias, last_msg, last_timestamp, last_sender_hash, last_status, unread_count, is_archived, is_pinned) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            chat.address, 
            chat.is_group as i32, 
            chat.alias, 
            chat.last_msg, 
            chat.last_timestamp, 
            chat.last_sender_hash,
            chat.last_status,
            chat.unread_count, 
            chat.is_archived as i32,
            chat.is_pinned as i32,
        ],
    ).map_err(|e| e.to_string())?;

    // Handle members if it's a group
    if let Some(members) = chat.members {
        conn.execute("DELETE FROM chat_members WHERE chat_address = ?1", [chat.address.clone()]).map_err(|e| e.to_string())?;
        for m in members {
            conn.execute(
                "INSERT OR IGNORE INTO chat_members (chat_address, member_hash) VALUES (?1, ?2)",
                [chat.address.clone(), m],
            ).map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn db_upsert_chat(state: State<'_, DbState>, chat: DbChat) -> Result<(), String> {
    internal_db_upsert_chat(&state, chat).await
}

#[tauri::command]
pub async fn db_get_chats(state: State<'_, DbState>) -> Result<Vec<DbChat>, String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    let mut stmt = conn.prepare(
        "SELECT 
            c.address, c.is_group, c.alias, c.last_msg, c.last_timestamp, 
            c.unread_count, c.is_archived, c.last_sender_hash, c.last_status, c.is_pinned,
            COALESCE((SELECT trust_level FROM signal_identities_remote WHERE address LIKE c.address || ':%' LIMIT 1), 1) as trust_level,
            COALESCE((SELECT is_blocked FROM contacts WHERE hash = c.address), 0) != 0 as is_blocked
        FROM chats c"
    ).map_err(|e| e.to_string())?;
    
    let chat_rows = stmt.query_map([], |row| {
        Ok(DbChat {
            address: row.get(0)?,
            is_group: row.get::<_, i32>(1)? != 0,
            alias: row.get(2)?,
            last_msg: row.get(3)?,
            last_timestamp: row.get(4)?,
            unread_count: row.get(5)?,
            is_archived: row.get::<_, i32>(6)? != 0,
            last_sender_hash: row.get(7)?,
            last_status: row.get(8)?,
            is_pinned: row.get::<_, i32>(9)? != 0,
            trust_level: row.get(10)?,
            is_blocked: row.get(11)?,
            members: None, // Will fill below
        })
    }).map_err(|e| e.to_string())?;

    let mut chats = Vec::new();
    for r in chat_rows {
        let mut chat = r.map_err(|e| e.to_string())?;
        
        // Fetch members for this chat
        let mut m_stmt = conn.prepare("SELECT member_hash FROM chat_members WHERE chat_address = ?1").map_err(|e| e.to_string())?;
        let m_rows = m_stmt.query_map([&chat.address], |m_row| m_row.get(0)).map_err(|e| e.to_string())?;
        let mut members = Vec::new();
        for mr in m_rows {
            members.push(mr.map_err(|e| e.to_string())?);
        }

        if !members.is_empty() {
            chat.members = Some(members);
        }
        chats.push(chat);
    }
    Ok(chats)
}

#[tauri::command]
pub async fn db_get_contacts(state: State<'_, DbState>) -> Result<Vec<DbContact>, String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    let mut stmt = conn.prepare("SELECT hash, alias, is_blocked, trust_level FROM contacts").map_err(|e| e.to_string())?;

    let rows = stmt.query_map([], |row| {
        Ok(DbContact {
            hash: row.get(0)?,
            alias: row.get(1)?,
            is_blocked: row.get::<_, i32>(2)? != 0,
            trust_level: row.get(3)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut contacts = Vec::new();
    for r in rows {
        contacts.push(r.map_err(|e| e.to_string())?);
    }
    Ok(contacts)
}

#[tauri::command]
pub async fn db_set_contact_blocked(state: State<'_, DbState>, hash: String, is_blocked: bool) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    // UPSERT: Insert if missing, or update if exists
    conn.execute(
        "INSERT INTO contacts (hash, is_blocked) VALUES (?1, ?2)
         ON CONFLICT(hash) DO UPDATE SET is_blocked = excluded.is_blocked",
        params![hash, is_blocked as i32],
    ).map_err(|e| format!("Failed to update block status: {}", e))?;

    Ok(())
}


#[tauri::command]
pub async fn db_set_contact_nickname(state: State<'_, DbState>, hash: String, alias: Option<String>) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    conn.execute(
        "INSERT INTO contacts (hash, alias) VALUES (?1, ?2)
         ON CONFLICT(hash) DO UPDATE SET alias = excluded.alias",
        params![hash, alias],
    ).map_err(|e| format!("Failed to update alias: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn db_delete_messages(state: State<'_, DbState>, ids: Vec<String>) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    for id in ids {
        conn.execute("DELETE FROM messages WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete message {}: {}", id, e))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn disconnect_network(state: State<'_, NetworkState>) -> Result<(), String> {
    *state.is_enabled.lock().unwrap() = false;
    
    let mut sender_lock = state.sender.lock().unwrap();
    *sender_lock = None;
    
    let mut cancel_lock = state.cancel.lock().unwrap();
    if let Some(token) = cancel_lock.take() {
        token.cancel();
    }
    
    let mut queue_lock = state.queue.lock().unwrap();
    queue_lock.clear();
    
    Ok(())
}

async fn internal_connect(
    app: AppHandle,
    state: &NetworkState,
    url_str: String,
    proxy_url: Option<String>,
    token: tokio_util::sync::CancellationToken
) -> Result<(), String> {
    let url = Url::parse(&url_str).map_err(|e| e.to_string())?;
    let host = url.host_str().ok_or("Invalid host")?;
    let port = url.port_or_known_default().ok_or("Invalid port")?;

    let (mut write, mut read) = if let Some(p_url) = proxy_url {
        let proxy_uri = Url::parse(&p_url).map_err(|e| format!("Invalid proxy URL: {}", e))?;
        let proxy_host = proxy_uri.host_str().unwrap_or("127.0.0.1");
        let proxy_port = proxy_uri.port().unwrap_or(9050);
        
        let socket = Socks5Stream::connect((proxy_host, proxy_port), (host, port))
            .await
            .map_err(|e| format!("Proxy connection failed: {}", e))?;
            
        let (stream, _) = tokio_tungstenite::client_async_tls(&url_str, socket)
            .await
            .map_err(|e| format!("WebSocket over proxy failed: {}", e))?;
            
        let (w, r) = stream.split();
        (
            Box::new(w) as Box<dyn Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Send + Unpin>,
            Box::new(r) as Box<dyn Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Send + Unpin>
        )
    } else {
        let (stream, _) = connect_async(&url_str).await.map_err(|e| e.to_string())?;
        let (w, r) = stream.split();
        (
            Box::new(w) as Box<dyn Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Send + Unpin>,
            Box::new(r) as Box<dyn Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Send + Unpin>
        )
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<PacedMessage>();

    {
        let mut sender = state.sender.lock().unwrap();
        *sender = Some(tx.clone());
    }
    
    let app_handle = app.clone();
    let write_token = token.clone();
    
    tokio::spawn(async move {
        let mut next_dummy_sleep = Box::pin(tokio::time::sleep(Duration::from_millis(rand::random::<u64>() % 9000 + 1000)));

        loop {
            tokio::select! {
                _ = write_token.cancelled() => break,
                Some(paced) = rx.recv() => {
                    let mut msg_to_send = paced.msg;
                    match &mut msg_to_send {
                        Message::Text(text) => {
                            if !paced.is_media && text.len() != PACKET_SIZE {
                                let mut final_json: String = text.to_string();
                                TrafficNormalizer::pad_json_str(&mut final_json, PACKET_SIZE);
                                msg_to_send = Message::Text(Utf8Bytes::from(final_json));
                            }
                        },
                        Message::Binary(data) => {
                            if data.len() != PACKET_SIZE {
                                let mut data_vec = data.to_vec();
                                TrafficNormalizer::pad_binary(&mut data_vec, PACKET_SIZE);
                                msg_to_send = Message::Binary(data_vec.into());
                            }
                        },
                        _ => {}
                    }
                    let frame_len = match &msg_to_send {
                        Message::Text(t) => t.len(),
                        Message::Binary(b) => b.len(),
                        _ => 0
                    };
                    println!("[Net] RX Background Paced send: framesize={}", frame_len);
                    if let Err(_) = write.send(msg_to_send).await { break; }
                }
                _ = &mut next_dummy_sleep => {
                    let mut dummy_vec = vec![0u8; PACKET_SIZE];
                    dummy_vec[0] = 0x03; // Type 0x03 Binary Dummy
                    TrafficNormalizer::pad_binary(&mut dummy_vec, PACKET_SIZE);
                    
                    println!("[Net] TX Dummy Pacing: Binary Type 0x03 (1400B)");
                    if let Err(_) = write.send(Message::Binary(dummy_vec.into())).await { break; }
                    
                    next_dummy_sleep = Box::pin(tokio::time::sleep(Duration::from_millis(rand::random::<u64>() % 9000 + 1000)));
                }
            }
        }
        {
            let state = app_handle.state::<NetworkState>();
            let mut s = state.sender.lock().unwrap();
            *s = None;
        }
        let _ = app_handle.emit("network-status", "disconnected");
    });
    
    let app_read = app.clone();
    let read_token = token;
    tokio::spawn(async move {
        // 🦾 AGGRESSIVE SESSION RESUMPTION: Skip PoW if we have a valid token
        let id_hash = app_read.state::<NetworkState>().identity_hash.lock().unwrap().clone();
        let session_token = app_read.state::<NetworkState>().session_token.lock().unwrap().clone();
        
        if let (Some(id), Some(token)) = (id_hash.clone(), session_token) {
            if let Some(tx) = &*app_read.state::<NetworkState>().sender.lock().unwrap() {
                let payload = json!({ "identity_hash": id, "session_token": token });
                let auth_req = json!({"type": "auth", "payload": payload});
                println!("[Net] Resuming session with token for {}...", id);
                let _ = tx.send(PacedMessage { msg: Message::Text(Utf8Bytes::from(auth_req.to_string())), is_media: false });
            }
        } else if let Some(id) = id_hash {
            if let Some(tx) = &*app_read.state::<NetworkState>().sender.lock().unwrap() {
                let challenge_req = json!({"type": "pow_challenge", "identity_hash": id, "id": "auto_challenge"});
                println!("[Net] No session token found. Requesting PoW challenge for {}...", id);
                let _ = tx.send(PacedMessage { msg: Message::Text(Utf8Bytes::from(challenge_req.to_string())), is_media: false });
            }
        }

        loop {
            tokio::select! {
                _ = read_token.cancelled() => break,
                res = read.next() => {
                    match res {
                        Some(Ok(msg)) => {
                            match msg {
                                Message::Text(text) => {
                                    let text_str = text.to_string();
                                    let mut handled = false;
                                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text_str) {
                                        if let Some(req_id) = val.get("req_id").and_then(|r| r.as_str()) {
                                            if let Some(tx) = app_read.state::<NetworkState>().response_channels.lock().unwrap().remove(req_id) {
                                                let _ = tx.send(val.clone());
                                                handled = true;
                                            }
                                        }
                                        if let Some(msg_type) = val.get("type").and_then(|t| t.as_str()) {
                                            match msg_type {
                                                "auth_success" => {
                                                    let net_state = app_read.state::<NetworkState>();
                                                    *net_state.is_authenticated.lock().unwrap() = true;
                                                    
                                                    if let Some(token) = val.get("session_token").and_then(|t| t.as_str()) {
                                                        *net_state.session_token.lock().unwrap() = Some(token.to_string());
                                                        
                                                        // 🔐 ETERNAL SESSION: Persist new token back to the encrypted vault's KV store
                                                        if let Some(id_hash) = net_state.identity_hash.lock().unwrap().as_ref() {
                                                            let key = format!("entropy_meta_{}", id_hash);
                                                            let token_str = token.to_string();
                                                            
                                                            // We must update the JSON structure used by the frontend
                                                            let app_inner = app_read.clone();
                                                            tokio::spawn(async move {
                                                                let db_state = app_inner.state::<DbState>();
                                                                let lock = db_state.conn.lock().unwrap();
                                                                if let Some(conn) = lock.as_ref() {
                                                                    let stmt = conn.prepare("SELECT value FROM kv_store WHERE key = ?1").ok();
                                                                    if let Some(mut s) = stmt {
                                                                        let existing_json: Option<String> = s.query_row(params![&key], |r| r.get(0)).ok();
                                                                        let mut meta: Value = if let Some(j) = existing_json {
                                                                            serde_json::from_str(&j).unwrap_or(json!({}))
                                                                        } else {
                                                                            json!({})
                                                                        };
                                                                        
                                                                        meta["sessionToken"] = json!(token_str);
                                                                        let updated_json = meta.to_string();
                                                                        let _ = conn.execute(
                                                                            "INSERT OR REPLACE INTO kv_store (key, value) VALUES (?1, ?2)",
                                                                            params![&key, &updated_json]
                                                                        );
                                                                    }
                                                                }
                                                            });
                                                        }
                                                    }
                                                    
                                                    // Signal Hardening: Auto-replenish keys if they drop below threshold
                                                    let count = val.get("otk_count").and_then(|c| c.as_u64()).unwrap_or(0);
                                                    if count < 50 {
                                                        let mut refill_lock = net_state.is_refilling.lock().unwrap();
                                                        if !*refill_lock {
                                                            *refill_lock = true;
                                                            let delta = 100_u32.saturating_sub(count as u32);
                                                            if delta > 0 {
                                                                println!("[Signal] One-time prekeys are low ({}). Triggering smart refill (delta={})...", count, delta);
                                                                let app_sync = app_read.clone();
                                                                tokio::spawn(async move {
                                                                    let _ = signal_sync_keys(app_sync.clone(), Some(delta));
                                                                    *app_sync.state::<NetworkState>().is_refilling.lock().unwrap() = false;
                                                                });
                                                            } else {
                                                                *refill_lock = false;
                                                            }
                                                        }
                                                    }
                                                    
                                                    let _ = app_read.emit("network-status", json!({ "status": "authenticated", "token": net_state.session_token.lock().unwrap().clone() }));
                                                    handled = true;
                                                },
                                                "keys_low" => {
                                                    let count = val.get("count").and_then(|c| c.as_u64()).unwrap_or(0);
                                                    let net_state = app_read.state::<NetworkState>();
                                                    let mut refill_lock = net_state.is_refilling.lock().unwrap();
                                                    if !*refill_lock {
                                                        *refill_lock = true;
                                                        let delta = 100_u32.saturating_sub(count as u32);
                                                        if delta > 0 {
                                                            tracing::warn!("[Signal] MOTK Pool Alert! Only {} keys remaining. Triggering smart refill (delta={})...", count, delta);
                                                            let app_sync = app_read.clone();
                                                            tokio::spawn(async move {
                                                                let _ = signal_sync_keys(app_sync.clone(), Some(delta));
                                                                *app_sync.state::<NetworkState>().is_refilling.lock().unwrap() = false;
                                                            });
                                                        } else {
                                                            *refill_lock = false;
                                                        }
                                                    }
                                                    handled = true;
                                                },
                                                "delivery_error" => {
                                                    if let Some(t) = val.get("target").and_then(|t| t.as_str()) {
                                                        app_read.state::<NetworkState>().halted_targets.lock().unwrap().insert(t.to_string());
                                                    }
                                                    let _ = app_read.emit("network-warning", json!({ "type": val.get("reason"), "target": val.get("target") }));
                                                    handled = true;
                                                },
                                                "pow_challenge_res" => {
                                                    if val.get("req_id").is_none() {
                                                        let seed = val.get("seed").and_then(|s| s.as_str()).map(|s| s.to_string());
                                                        let diff = val.get("difficulty").and_then(|d| d.as_u64()).map(|d| d as u32);
                                                        let id = app_read.state::<NetworkState>().identity_hash.lock().unwrap().clone();
                                                        let modulus = val.get("modulus").and_then(|m| m.as_str()).map(|s| s.to_string());
                                                        
                                                        if let (Some(s), Some(d), Some(i)) = (seed, diff, id) {
                                                            let app_inner = app_read.clone();
                                                            
                                                            // 🦾 Session Resumption: Try using token to bypass PoW
                                                            let existing_token = app_inner.state::<NetworkState>().session_token.lock().unwrap().clone();
                                                            if let Some(token) = existing_token {
                                                                let app_inner = app_read.clone();
                                                                tokio::spawn(async move {
                                                                    let payload = json!({ "identity_hash": i, "session_token": token });
                                                                    let auth_val = json!({"type": "auth", "payload": payload});
                                                                    let _ = send_paced_json(&app_inner, auth_val).await;
                                                                });
                                                            } else {
                                                                // No token available -> Mining PoW
                                                                tokio::spawn(async move {
                                                                    let result = internal_mine_pow(s.clone(), d, i.clone(), modulus).await;
                                                                    let app_sig = app_inner.clone();
                                                                    let sig_res = tauri::async_runtime::spawn_blocking(move || {
                                                                        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                                                                        rt.block_on(async move { SqliteSignalStore::new(app_sig).get_identity_key_pair().await })
                                                                    }).await.map_err(|e| e.to_string());
                                                                    
                                                                    let mut payload = json!({"identity_hash": i, "seed": result["seed"], "nonce": result["nonce"], "modulus": result["modulus"]});
                                                                    if let Ok(Ok(kp)) = sig_res {
                                                                        let mut rng = rand::rngs::StdRng::from_os_rng();
                                                                        let seed_bytes = hex::decode(&s).unwrap_or_else(|_| s.as_bytes().to_vec());
                                                                        if let Ok(sig) = kp.private_key().calculate_signature(&seed_bytes, &mut rng) {
                                                                            payload["signature"] = json!(hex::encode(sig));
                                                                            let mut pk = kp.identity_key().serialize().to_vec();
                                                                            if pk.len() == 33 && pk[0] == 0x05 { pk.remove(0); }
                                                                            payload["public_key"] = json!(hex::encode(pk));
                                                                        }
                                                                    }
                                                                    let auth_val = json!({"type": "auth", "payload": payload});
                                                                    let _ = send_paced_json(&app_inner, auth_val).await;
                                                                });
                                                            }
                                                        }
                                                        handled = true;
                                                    }
                                                },
                                                "error" => {
                                                    if let Some(code) = val.get("code").and_then(|c| c.as_str()) {
                                                        if code == "auth_failed" {
                                                            *app_read.state::<NetworkState>().is_authenticated.lock().unwrap() = false;
                                                            let _ = app_read.emit("network-status", "auth_failed");
                                                            handled = true;
                                                        }
                                                    }
                                                },
                                                _ => {}
                                            }
                                        }
                                    }
                                    if !handled { let _ = app_read.emit("network-msg", text_str); }
                                }
                                Message::Binary(bin) => {
                                    let app_recv = app_read.clone();
                                    let bin_vec = bin.to_vec();
                                    std::thread::spawn(move || {
                                        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                                        rt.block_on(async move { let _ = process_incoming_binary(app_recv, bin_vec, None).await; });
                                    });
                                }
                                Message::Close(_) => break,
                                _ => {}
                            }
                        }
                        Some(Err(_)) => break,
                        None => break,
                    }
                }
            }
        }
        *app_read.state::<NetworkState>().is_authenticated.lock().unwrap() = false;
        {
            let state = app_read.state::<NetworkState>();
            let mut s = state.sender.lock().unwrap();
            *s = None;
        }
        let _ = app_read.emit("network-status", "disconnected");
    });

    Ok(())
}

async fn run_connection_loop(app: AppHandle) {
    let mut retry_count = 0;
    let backoff = [1, 2, 4, 8, 15, 30, 60];
    loop {
        let (enabled, url, proxy_url, token) = {
            let state = app.state::<NetworkState>();
            let enabled = *state.is_enabled.lock().unwrap();
            let url = state.url.lock().unwrap().clone();
            let proxy_url = state.proxy_url.lock().unwrap().clone();
            let token = state.cancel.lock().unwrap().clone();
            (enabled, url, proxy_url, token)
        };
        if !enabled || token.is_none() { break; }
        let token = token.unwrap();
        if token.is_cancelled() { break; }
        if let Some(target_url) = url {
            println!("[Network] Attempting reconnection: attempt {}", retry_count + 1);
            let state = app.state::<NetworkState>();
            if let Err(e) = internal_connect(app.clone(), &state, target_url, proxy_url, token.clone()).await {
                eprintln!("[Network] Connection error: {}", e);
            } else {
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    if token.is_cancelled() || !*state.is_enabled.lock().unwrap() { break; }
                    let s = state.sender.lock().unwrap();
                    if s.is_none() { break; }
                }
                retry_count = 0;
            }
        }
        if !*app.state::<NetworkState>().is_enabled.lock().unwrap() || token.is_cancelled() { break; }
        let delay = backoff[retry_count.min(backoff.len() - 1)];
        println!("[Network] Next retry in {}s", delay);
        tokio::select! {
            _ = token.cancelled() => break,
            _ = tokio::time::sleep(Duration::from_secs(delay)) => { retry_count += 1; }
        }
    }
}

#[tauri::command]
pub async fn connect_network(
    app: tauri::AppHandle, 
    state: State<'_, NetworkState>, 
    url: String,
    proxy_url: Option<String>,
    id_hash: Option<String>,
    session_token: Option<String>
) -> Result<(), String> {
    {
        *state.is_enabled.lock().unwrap() = true;
        *state.url.lock().unwrap() = Some(url.clone());
        *state.proxy_url.lock().unwrap() = proxy_url.clone();
        *state.identity_hash.lock().unwrap() = id_hash.clone();
        *state.session_token.lock().unwrap() = session_token.clone();
    }

    // Trigger the background reconnection loop
    let app_handle = app.clone();
    {
        let mut cancel_lock = state.cancel.lock().unwrap();
        if let Some(t) = cancel_lock.take() {
            t.cancel();
        }
        let t = tokio_util::sync::CancellationToken::new();
        *cancel_lock = Some(t.clone());
    }

    tokio::spawn(async move {
        run_connection_loop(app_handle).await;
    });

    // Background Cleanup Loop for Media Fragments
    let app_cleanup = app.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let state = app_cleanup.state::<NetworkState>();
            let mut assembler = state.media_assembler.lock().unwrap();
            let now = std::time::Instant::now();
            let mut removed_count = 0;
            assembler.retain(|_, buffer| {
                if now.duration_since(buffer.last_activity).as_secs() > 60 {
                    removed_count += 1;
                    false
                } else {
                    true
                }
            });
            if removed_count > 0 {
                tracing::warn!("[Network] Cleaned up {} stale media transfer(s)", removed_count);
            }
            if !*state.is_enabled.lock().unwrap() { break; }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn send_to_network(
    app: AppHandle,
    state: tauri::State<'_, NetworkState>,
    routing_hash: Option<String>,
    msg: Option<String>,
    data: Option<Vec<u8>>,
    is_binary: bool,
    is_media: bool
) -> Result<(), String> {
    internal_send_to_network(app, &state, routing_hash, msg, data, is_binary, is_media).await
}

pub async fn internal_send_to_network(
    app: AppHandle,
    state: &NetworkState, 
    target_hash: Option<String>,
    msg: Option<String>, 
    data: Option<Vec<u8>>,
    is_binary: bool,
    is_media: bool
) -> Result<(), String> {
    let is_connected = state.sender.lock().unwrap().is_some();

    if is_connected {
        if is_binary {
            let sender_lock = state.sender.lock().unwrap();
            let tx = sender_lock.as_ref().unwrap();
            let bytes = if let Some(d) = data {
                d
            } else if let Some(m) = msg {
                if let Ok(b) = hex::decode(&m) { b } else { m.into_bytes() }
            } else {
                return Err("Missing binary data".into());
            };

            if !bytes.is_empty() {
                // Determine Routing Hash and Payload Data
                let (hash_bytes, data_bytes) = if let Some(h) = target_hash {
                    // Explicit target hash provided (Normal Flow)
                    let mut h_padded = vec![0u8; 64];
                    let h_bytes = h.as_bytes();
                    let len = std::cmp::min(h_bytes.len(), 64);
                    h_padded[..len].copy_from_slice(&h_bytes[..len]);
                    (h_padded, bytes)
                } else {
                    // Control Transfer context (already binary wrapped/padded)
                    (vec![0u8; 64], bytes)
                };

                let total_len = data_bytes.len();
                let chunk_capacity = 1319; 
                let transfer_id: u32 = rand::random();

                if is_media {
                    let chunks = (total_len as f64 / chunk_capacity as f64).ceil() as usize;
                    let target_hash_str = hex::encode(&hash_bytes);
                    println!("[Media] Starting Paced Fragmented Send: size={} chunks={} tid={}", total_len, chunks, transfer_id);

                    for i in 0..chunks {
                        if state.halted_targets.lock().unwrap().contains(&target_hash_str) { break; }

                        let start = i * chunk_capacity;
                        let end = std::cmp::min(start + chunk_capacity, total_len);
                        let chunk_data = &data_bytes[start..end];
                        
                        let mut envelope = Vec::with_capacity(1400);
                        envelope.extend_from_slice(&hash_bytes); 
                        envelope.push(0x02); 
                        envelope.extend_from_slice(&transfer_id.to_be_bytes());
                        envelope.extend_from_slice(&(i as u32).to_be_bytes());
                        envelope.extend_from_slice(&(chunks as u32).to_be_bytes());
                        envelope.extend_from_slice(&(chunk_data.len() as u32).to_be_bytes());
                        envelope.extend_from_slice(chunk_data);
                        
                        tx.send(PacedMessage { msg: Message::Binary(envelope.into()), is_media: true }).map_err(|e| e.to_string())?;
                    }
                } else {
                    let chunks = (total_len as f64 / chunk_capacity as f64).ceil() as usize;
                    println!("[Net] Starting Relay Transfer (Type 0x01): size={} total_chunks={} tid={}", total_len, chunks, transfer_id);

                    for i in 0..chunks {
                        let start = i * chunk_capacity;
                        let end = std::cmp::min(start + chunk_capacity, total_len);
                        let chunk_data = &data_bytes[start..end];
                        
                        let mut envelope = Vec::with_capacity(1400);
                        envelope.extend_from_slice(&hash_bytes); 
                        envelope.push(0x01); 
                        envelope.extend_from_slice(&transfer_id.to_be_bytes());
                        envelope.extend_from_slice(&(i as u32).to_be_bytes());
                        envelope.extend_from_slice(&(chunks as u32).to_be_bytes());
                        envelope.extend_from_slice(&(chunk_data.len() as u32).to_be_bytes());
                        envelope.extend_from_slice(chunk_data);
                        
                        tx.send(PacedMessage { msg: Message::Binary(envelope.into()), is_media: false }).map_err(|e| e.to_string())?;
                    }
                }
            }
        } else {
            let actual_msg = msg.ok_or("Missing message text")?;
            let val: serde_json::Value = serde_json::from_str(&actual_msg).map_err(|e| e.to_string())?;
            send_paced_json(&app, val).await?;
        }
        Ok(())
    } else {
        // Queue in persistent outbox if disconnected
        let db_lock = app.state::<DbState>();
        let conn_lock = db_lock.conn.lock().unwrap();
        if let Some(conn) = conn_lock.as_ref() {
            let (msg_type, content) = if is_binary {
                let bytes = if let Some(d) = data {
                    d
                } else if let Some(m) = msg {
                    if let Ok(b) = hex::decode(&m) { b } else { m.into_bytes() }
                } else {
                    Vec::new()
                };
                ("binary", bytes)
            } else {
                ("text", msg.unwrap_or_default().into_bytes())
            };
            
            let _ = conn.execute(
                "INSERT INTO pending_outbox (msg_type, content, timestamp) VALUES (?1, ?2, ?3)",
                rusqlite::params![msg_type, content, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()]
            );
        }
        
        Err("Network not connected. Message queued in outbox.".to_string())
    }
}

#[tauri::command]
pub async fn flush_outbox(
    app: AppHandle,
    state: State<'_, NetworkState>
) -> Result<(), String> {
    let sender_lock = state.sender.lock().unwrap();
    if let Some(tx) = &*sender_lock {
        let db_state = app.state::<DbState>();
        let db_lock = db_state.conn.lock().unwrap();
        if let Some(conn) = db_lock.as_ref() {
            let mut stmt = conn.prepare("SELECT id, msg_type, content FROM pending_outbox ORDER BY timestamp ASC").map_err(|e| e.to_string())?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?
                ))
            }).map_err(|e| e.to_string())?;

            for row in rows {
                if let Ok((id, msg_type, content)) = row {
                    let is_media = msg_type == "binary";
                    let msg = if msg_type == "text" {
                        Message::Text(Utf8Bytes::from(String::from_utf8_lossy(&content).to_string()))
                    } else {
                        Message::Binary(content.into())
                    };
                    let _ = tx.send(PacedMessage { msg, is_media });
                    let _ = conn.execute("DELETE FROM pending_outbox WHERE id = ?", [id]);
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn nuclear_reset(app: tauri::AppHandle, state: State<'_, DbState>) -> Result<(), String> {
    {
        let mut conn = state.conn.lock().unwrap();
        if let Some(c) = conn.take() {
            let _ = c.close(); 
        }
        *conn = None;
    }

    let filename = get_db_filename();
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    
    let db_path = app_dir.join(&filename);
    let wal_path = app_dir.join(format!("{}-wal", filename));
    let shm_path = app_dir.join(format!("{}-shm", filename));
    let media_dir = app_dir.join("media");

    if db_path.exists() {
        std::fs::remove_file(&db_path).map_err(|e| e.to_string())?;
    }
    if wal_path.exists() {
        let _ = std::fs::remove_file(wal_path);
    }
    if shm_path.exists() {
        let _ = std::fs::remove_file(shm_path);
    }
    if media_dir.exists() {
        let _ = std::fs::remove_dir_all(media_dir);
    }

    println!("[!] Nuclear reset initiated. Restarting application...");
    app.restart(); 
}

#[tauri::command]
pub async fn save_file(app: tauri::AppHandle, data: Vec<u8>, filename: String) -> Result<(), String> {
    use std::io::Write;
    
    let download_dir = app.path().download_dir().unwrap_or_else(|_| std::env::temp_dir());
    
    let safe_filename = std::path::Path::new(&filename)
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("download"));
    
    let target_path = download_dir.join(safe_filename);
    
    println!("[*] Saving file to: {:?}", target_path);
    
    let mut file = std::fs::File::create(&target_path).map_err(|e| e.to_string())?;
    file.write_all(&data).map_err(|e| e.to_string())?;
    
    Ok(())
}

#[tauri::command]
pub fn show_in_folder(path: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        // Try to use DBus to highlight the file
        let _ = Command::new("dbus-send")
            .args(&[
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:file://{}", path),
                "string:",
            ])
            .spawn();
        
        // Standard behavior: open the folder
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = Command::new("xdg-open")
                .arg(parent)
                .spawn();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn vault_save_media(app: tauri::AppHandle, state: State<'_, DbState>, id: String, data: Vec<u8>) -> Result<String, String> {
    // 1. Get Key from state
    let key_bytes = {
        let lock = state.media_key.lock().unwrap();
        lock.clone().ok_or("Media key not initialized")?
    };

    // 2. Encrypt Data
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 96-bits; unique per message
    let ciphertext = cipher.encrypt(&nonce, data.as_ref()).map_err(|e| e.to_string())?;
    
    // Store as Nonce + Ciphertext
    let mut final_blob = nonce.to_vec();
    final_blob.extend(ciphertext);

    // 3. Save to Disk
    let media_dir = get_media_dir(&app, &state)?;
    let file_path = media_dir.join(&id);
    let mut file = std::fs::File::create(&file_path).map_err(|e| e.to_string())?;
    file.write_all(&final_blob).map_err(|e| e.to_string())?;
    
    Ok(file_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn vault_load_media(app: tauri::AppHandle, state: State<'_, DbState>, id: String) -> Result<Vec<u8>, String> {
    // 1. Get Key from state
    let key_bytes = {
        let lock = state.media_key.lock().unwrap();
        lock.clone().ok_or("Media key not initialized")?
    };

    // 2. Load File
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

    // 3. Decrypt
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&buffer[0..12]);
    let ciphertext = &buffer[12..];
    
    let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|e| format!("Decryption failed: {}", e))?;
    
    Ok(plaintext)
}

#[tauri::command]
pub async fn vault_delete_media(app:  tauri::AppHandle, id: String) -> Result<(), String> {
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
pub async fn export_database(app: tauri::AppHandle, state: State<'_, DbState>, target_path: String) -> Result<(), String> {
    // 1. Checkpoint WAL to main DB file
    {
        let conn_guard = state.conn.lock().unwrap();
        if let Some(conn) = conn_guard.as_ref() {
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(|e| format!("Failed to checkpoint DB: {}", e))?;
        }
    }

    let filename = get_db_filename();
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let src_path = app_dir.join(&filename);
    
    if !src_path.exists() {
        return Err("Database file not found".to_string());
    }

    // CRITICAL: Prevent arbitrary file write/overwrite outside of intended scope
    let path_obj = std::path::Path::new(&target_path);
    let extension = path_obj.extension().and_then(|e| e.to_str()).unwrap_or("");
    if extension != "entropy" && extension != "zip" {
        return Err("Invalid export location: Backup must have .entropy or .zip extension".into());
    }

    // 2. Prepare Zip
    let file = std::fs::File::create(&target_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    // 3. Add Database File (Streaming copy to prevent memory exhaustion)
    let mut f = std::fs::File::open(&src_path).map_err(|e| format!("Failed to open DB for export: {}", e))?;
    zip.start_file(filename, options).map_err(|e| e.to_string())?;
    std::io::copy(&mut f, &mut zip).map_err(|e| format!("Failed to stream DB to zip: {}", e))?;

    // 4. Add Media Folder Recursively
    let media_path = app_dir.join("media");
    if media_path.exists() {
        let walker = WalkDir::new(&media_path).into_iter();
        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = path.strip_prefix(&app_dir)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned();

            if path.is_file() {
                zip.start_file(name, options).map_err(|e| e.to_string())?;
                let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
                let mut buffer = Vec::new();
                f.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
                zip.write_all(&buffer).map_err(|e| e.to_string())?;
            } else if !name.is_empty() {
                 zip.add_directory(name, options).map_err(|e| e.to_string())?;
            }
        }
    }
    
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn import_database(app: tauri::AppHandle, state: State<'_, DbState>, src_path: String) -> Result<(), String> {
    // 1. Close current connection deeply
    {
        let mut conn = state.conn.lock().unwrap();
        *conn = None;
        drop(conn); // Release lock before filesystem ops
    }

    // CRITICAL: Prevent arbitrary system file deletion or unauthorized imports
    let backup_path = std::path::Path::new(&src_path);
    let extension = backup_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if extension != "entropy" && extension != "zip" {
         return Err("Invalid backup file: Must be a .entropy or .zip archive".into());
    }
    
    if !backup_path.exists() {
        return Err("Selected backup file does not exist".to_string());
    }

    let filename = get_db_filename();
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let dest_path = app_dir.join(&filename);
    let wal_path = app_dir.join(format!("{}-wal", filename));
    let shm_path = app_dir.join(format!("{}-shm", filename));

    // 2. Clean up ALL local data
    if dest_path.exists() {
        std::fs::remove_file(&dest_path).map_err(|e| e.to_string())?;
    }
    if wal_path.exists() {
        let _ = std::fs::remove_file(wal_path);
    }
    if shm_path.exists() {
        let _ = std::fs::remove_file(shm_path);
    }

    let media_path = app_dir.join("media");
    if media_path.exists() {
        std::fs::remove_dir_all(&media_path).map_err(|e| e.to_string())?;
    }

    // 3. Unzip Backup
    let file = std::fs::File::open(&src_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        
        // Sanitize path against Zip Slip
        let outpath = match file.enclosed_name() {
            Some(path) => app_dir.join(path),
            None => continue,
        };

        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
                }
            }
            let mut outfile = std::fs::File::create(&outpath).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
        }
    }

    // 4. Re-initialize database state
    println!("[Import] Restore complete. Re-opening connection...");
    let new_conn = rusqlite::Connection::open(&dest_path).map_err(|e| format!("Failed to re-open DB: {}", e))?;
    let mut state_lock = state.conn.lock().unwrap();
    *state_lock = Some(new_conn);
    
    Ok(())
}

#[tauri::command]
pub async fn signal_init(handle: tauri::AppHandle) -> Result<String, String> {
    let handle_clone = handle.clone();
    let result: Result<String, String> = tauri::async_runtime::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let store = SqliteSignalStore::new(handle_clone.clone());
            
            // Check if identity already exists
            if let Ok(kp) = store.get_identity_key_pair().await {
                let mut pub_key = kp.identity_key().serialize().to_vec();
                if pub_key.len() == 33 && pub_key[0] == 0x05 {
                    pub_key.remove(0);
                }
                return Ok::<String, String>(hex::encode(pub_key));
            }

            let mut rng = StdRng::from_os_rng();
            let identity_key_pair = IdentityKeyPair::generate(&mut rng);
            let registration_id: u32 = rand::random::<u32>() & 0x3FFF;

            let mut pub_bytes = identity_key_pair.identity_key().serialize().to_vec();
            let priv_bytes = identity_key_pair.private_key().serialize();

            let db_state = handle_clone.state::<DbState>();
            let db_lock = db_state.conn.lock().unwrap();
            let conn = db_lock.as_ref().ok_or("Database not initialized")?;

            conn.execute(
                "INSERT OR REPLACE INTO signal_identity (id, registration_id, public_key, private_key) VALUES (0, ?1, ?2, ?3)",
                params![registration_id, &pub_bytes[..], &priv_bytes[..]],
            ).map_err(|e: rusqlite::Error| e.to_string())?;

            if pub_bytes.len() == 33 && pub_bytes[0] == 0x05 {
                pub_bytes.remove(0);
            }
            Ok::<String, String>(hex::encode(pub_bytes))
        })
    }).await.map_err(|e| e.to_string())?;
    
    let pub_key_hex = result?;
    
    // Update NetworkState with the identity hash so autonomous handshake works
    let state = handle.state::<NetworkState>();
    let pub_bytes = hex::decode(&pub_key_hex).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&pub_bytes);
    let id_hash = hex::encode(hasher.finalize());
    
    let mut hash_lock = state.identity_hash.lock().unwrap();
    *hash_lock = Some(id_hash);
    
    Ok(pub_key_hex)
}

pub fn signal_get_bundle(handle: tauri::AppHandle, count: Option<u32>) -> Result<serde_json::Value, String> {
    let key_count = count.unwrap_or(100).min(200); // Limit to 200 max to prevent server abuse
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let mut store = SqliteSignalStore::new(handle.clone());
            let mut rng = StdRng::from_os_rng();
            
            let identity_key_pair = store.get_identity_key_pair().await.map_err(|e: SignalProtocolError| e.to_string())?;
            let registration_id: u32 = store.get_local_registration_id().await.map_err(|e: SignalProtocolError| e.to_string())?;

            println!("[Signal] Creating bundle for local user (keys_requested={}). RegistrationId: {}", key_count, registration_id);

            // Generate X25519 PreKey Batch (Smart Top-up)
            let mut pre_keys_json = Vec::new();
            for _ in 0..key_count {
                let id = PreKeyId::from(rand::random::<u32>() & 0x7FFFFFFF);
                let pair = KeyPair::generate(&mut rng);
                let record = PreKeyRecord::new(id, &pair);
                store.save_pre_key(id, &record).await.map_err(|e: SignalProtocolError| e.to_string())?;
                pre_keys_json.push(serde_json::json!({
                    "id": u32::from(id),
                    "publicKey": hex::encode(pair.public_key.serialize())
                }));
            }

            // Generate Signed PreKey
            let signed_pre_key_id = SignedPreKeyId::from(rand::random::<u32>() & 0x7FFFFFFF);
            let signed_pre_key_pair = KeyPair::generate(&mut rng);
            let timestamp = Timestamp::from_epoch_millis(
                std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64
            );
            let signature = identity_key_pair.private_key().calculate_signature(&signed_pre_key_pair.public_key.serialize(), &mut rng)
                .map_err(|e| e.to_string())?;
            let signed_pre_key_record = SignedPreKeyRecord::new(signed_pre_key_id, timestamp, &signed_pre_key_pair, &signature);
            println!("[Signal] Generated new SignedPreKey: {}", u32::from(signed_pre_key_id));
            store.save_signed_pre_key(signed_pre_key_id, &signed_pre_key_record).await.map_err(|e: SignalProtocolError| e.to_string())?;

            // Generate Kyber PreKey
            let kyber_pre_key_id = KyberPreKeyId::from(rand::random::<u32>() & 0x7FFFFFFF);
            let kyber_pre_key_record = KyberPreKeyRecord::generate(kem::KeyType::Kyber1024, kyber_pre_key_id, identity_key_pair.private_key())
                .map_err(|e: SignalProtocolError| e.to_string())?;
            println!("[Signal] Generated new KyberPreKey: {}", u32::from(kyber_pre_key_id));
            store.save_kyber_pre_key(kyber_pre_key_id, &kyber_pre_key_record).await.map_err(|e: SignalProtocolError| e.to_string())?;

            Ok(serde_json::json!({
                "registrationId": registration_id,
                "identityKey": hex::encode(identity_key_pair.identity_key().serialize()),
                "preKeys": pre_keys_json,
                "signedPreKey": {
                    "id": u32::from(signed_pre_key_id),
                    "publicKey": hex::encode(signed_pre_key_pair.public_key.serialize()),
                    "signature": hex::encode(signature)
                },
                "kyberPreKey": {
                    "id": u32::from(kyber_pre_key_id),
                    "publicKey": hex::encode(kyber_pre_key_record.public_key().map_err(|e: SignalProtocolError| e.to_string())?.serialize()),
                    "signature": hex::encode(kyber_pre_key_record.signature().map_err(|e: SignalProtocolError| e.to_string())?)
                }
            }))
        })
    }).join().map_err(|_| "Thread panicked".to_string())?
}

#[tauri::command]
pub fn signal_sync_keys(
    handle: AppHandle,
    count: Option<u32>
) -> Result<(), String> {
    let handle_clone = handle.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let state = handle_clone.state::<NetworkState>();
            
            // 1. Generate/Get Bundle with precise top-up count
            let raw_bundle = signal_get_bundle(handle_clone.clone(), count).map_err(|e| e.to_string())?;
            
            let id_hash = {
                let lock = state.identity_hash.lock().unwrap();
                lock.clone().ok_or("No identity hash in network state")?
            };

            // 2. Prepare Base64 bundle for server 
            let mut ik_bytes = hex::decode(raw_bundle["identityKey"].as_str().unwrap()).unwrap();
            if ik_bytes.len() == 33 && ik_bytes[0] == 0x05 {
                ik_bytes.remove(0);
            }
            let bundle = json!({
                "identity_hash": id_hash,
                "registrationId": raw_bundle["registrationId"],
                "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_bytes),
                "signedPreKey": {
                    "id": raw_bundle["signedPreKey"]["id"],
                    "publicKey": base64::engine::general_purpose::STANDARD.encode(hex::decode(raw_bundle["signedPreKey"]["publicKey"].as_str().unwrap()).unwrap()),
                    "signature": base64::engine::general_purpose::STANDARD.encode(hex::decode(raw_bundle["signedPreKey"]["signature"].as_str().unwrap()).unwrap()),
                },
                "preKeys": raw_bundle["preKeys"],
                "kyberPreKey": {
                    "id": raw_bundle["kyberPreKey"]["id"],
                    "publicKey": base64::engine::general_purpose::STANDARD.encode(hex::decode(raw_bundle["kyberPreKey"]["publicKey"].as_str().unwrap()).unwrap()),
                    "signature": base64::engine::general_purpose::STANDARD.encode(hex::decode(raw_bundle["kyberPreKey"]["signature"].as_str().unwrap()).unwrap())
                }
            });

            // 3. ZERO-POW: Skip challenge/mining, go straight to signing the ID hash
            let store = SqliteSignalStore::new(handle_clone.clone());
            let kp = store.get_identity_key_pair().await.map_err(|e: SignalProtocolError| e.to_string())?;
            let mut rng = rand::rngs::StdRng::from_os_rng();
            
            // Signature is over the identity_hash to prove intent
            let sig = kp.private_key().calculate_signature(id_hash.as_bytes(), &mut rng)
                .map_err(|e| e.to_string())?;

            // 4. Upload Keys with ownership proof
            let mut final_upload = bundle;
            let mut pk_bytes = kp.identity_key().serialize().to_vec();
            if pk_bytes.len() == 33 && pk_bytes[0] == 0x05 {
                pk_bytes.remove(0);
            }
            final_upload["identityKey"] = json!(hex::encode(&pk_bytes));
            final_upload["signature"] = json!(hex::encode(&sig));
            
            let final_str = final_upload.to_string();
            let delta = count.unwrap_or(100);
            println!("[Signal] Syncing keys (delta={}, size={} bytes)...", delta, final_str.len());
            
            let response = internal_request(&state, "keys_upload", final_upload).await?;
            if response["status"].as_str() == Some("success") {
                println!("[Signal] Keys synced successfully ({} added).", delta);
                Ok(())
            } else {
                let err = response["error"].as_str().unwrap_or("Unknown upload error");
                Err(format!("Key upload failed: {}", err))
            }
        })
    }).join().map_err(|_| "Thread panicked".to_string())?
}

#[tauri::command]
pub fn signal_encrypt(
    handle: tauri::AppHandle,
    remote_hash: String,
    message: String,
) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let _db_state = handle.state::<DbState>();
            let _net_state = handle.state::<NetworkState>();
            internal_signal_encrypt(handle.clone(), &_net_state, &remote_hash, message).await
        })
    }).join().map_err(|_| "Thread panicked".to_string())?
}
#[tauri::command]
pub fn signal_sign_message(handle: tauri::AppHandle, message: String) -> Result<String, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let store = SqliteSignalStore::new(handle.clone());
            let kp = store.get_identity_key_pair().await.map_err(|e| e.to_string())?;
            
            let mut rng = rand::rngs::StdRng::from_os_rng();
            let sig = kp.private_key().calculate_signature(message.as_bytes(), &mut rng)
                .map_err(|e| e.to_string())?;
            
            Ok(base64::engine::general_purpose::STANDARD.encode(sig))
        })
    }).join().map_err(|_| "Thread panicked".to_string())?
}

#[tauri::command]
pub async fn signal_get_peer_identity(state: tauri::State<'_, DbState>, address: String) -> Result<Option<(Vec<u8>, i32)>, String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    // address is "peer_hash:1"
    let mut stmt = conn.prepare("SELECT public_key, trust_level FROM signal_identities_remote WHERE address = ?1")
        .map_err(|e| e.to_string())?;
    
    let mut rows = stmt.query(params![address])
        .map_err(|e| e.to_string())?;

    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let pub_key: Vec<u8> = row.get(0).map_err(|e| e.to_string())?;
        let trust: i32 = row.get(1).map_err(|e| e.to_string())?;
        Ok(Some((pub_key, trust)))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn signal_set_peer_trust(state: tauri::State<'_, DbState>, address: String, trust_level: i32) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    // 1. Update Signal Identity Store (Standard Signal address format)
    let signal_addr = if !address.contains(':') { format!("{}:1", address) } else { address.clone() };
    conn.execute(
        "UPDATE signal_identities_remote SET trust_level = ?1 WHERE address = ?2",
        params![trust_level, signal_addr],
    ).map_err(|e| e.to_string())?;

    // 2. Sync to Contacts Table (UI address format)
    let contact_hash = address.split(':').next().unwrap_or(&address);
    conn.execute(
        "UPDATE contacts SET trust_level = ?1 WHERE hash = ?2",
        params![trust_level, contact_hash],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn signal_get_own_identity(state: tauri::State<'_, DbState>) -> Result<Vec<u8>, String> {
    let lock = state.conn.lock().unwrap();
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    let pub_key: Vec<u8> = conn.query_row(
        "SELECT public_key FROM signal_identity LIMIT 1",
        [],
        |row| row.get(0)
    ).map_err(|e| e.to_string())?;

    Ok(pub_key)
}

#[tauri::command]
pub async fn signal_get_identity_hash(
    db_state: tauri::State<'_, DbState>, 
    net_state: tauri::State<'_, crate::app_state::NetworkState>
) -> Result<String, String> {
    // Check cache first
    {
        let lock = net_state.identity_hash.lock().unwrap();
        if let Some(hash) = lock.as_ref() {
            return Ok(hash.clone());
        }
    }

    // Procedural Key Derivation
    let mut pub_key = signal_get_own_identity(db_state).await?;
    if pub_key.len() == 33 && pub_key[0] == 0x05 {
        pub_key.remove(0);
    }
    let mut hasher = Sha256::new();
    hasher.update(&pub_key);
    let hash = hex::encode(hasher.finalize());
    
    // Update cache
    {
        let mut lock = net_state.identity_hash.lock().unwrap();
        *lock = Some(hash.clone());
    }
    
    Ok(hash)
}

#[tauri::command]
pub async fn signal_get_fingerprint(
    db_state: tauri::State<'_, DbState>, 
    net_state: tauri::State<'_, crate::app_state::NetworkState>,
    remote_hash: String
) -> Result<serde_json::Value, String> {
    let own_id_bytes = signal_get_own_identity(db_state.clone()).await?;
    
    // address is "remote_hash:1"
    let peer_data = signal_get_peer_identity(db_state.clone(), format!("{}:1", remote_hash)).await?;

    let (peer_id_bytes, trust_level) = match peer_data {
        Some(data) => data,
        None => return Err("Peer identity not found".into()),
    };

    let own_hash = signal_get_identity_hash(db_state, net_state).await?;

    let mut combined = Vec::with_capacity(own_id_bytes.len() + peer_id_bytes.len());
    if remote_hash < own_hash {
        combined.extend_from_slice(&peer_id_bytes);
        combined.extend_from_slice(&own_id_bytes);
    } else {
        combined.extend_from_slice(&own_id_bytes);
        combined.extend_from_slice(&peer_id_bytes);
    }

    let mut hasher = Sha256::new();
    hasher.update(&combined);
    let hash_result = hasher.finalize();

    // Fast numeric formatting for the 60-digit fingerprint (Matching Signal Standard)
    let mut digits = String::with_capacity(72); // 60 digits + 11 spaces + 1 newline
    for i in 0..12 {
        let val = ((hash_result[i * 2] as u32) << 8) | (hash_result[i * 2 + 1] as u32);
        // % 100000 ensures 5 digits
        let block_val = val % 100000;
        
        // Manual zero padding for performance (avoiding slow format! recursion)
        if block_val < 10000 { digits.push('0'); }
        if block_val < 1000  { digits.push('0'); }
        if block_val < 100   { digits.push('0'); }
        if block_val < 10    { digits.push('0'); }
        digits.push_str(&block_val.to_string());
        
        if i == 5 {
            digits.push('\n');
        } else if i < 11 {
            digits.push(' ');
        }
    }

    Ok(json!({
        "digits": digits,
        "trustLevel": trust_level
    }))
}

#[tauri::command]
pub async fn db_set_chat_archived(state: tauri::State<'_, DbState>, address: String, is_archived: bool) -> Result<(), String> {
    let conn_lock = state.conn.lock().unwrap();
    let conn = conn_lock.as_ref().ok_or("Database not initialized")?;
    conn.execute(
        "UPDATE chats SET is_archived = ? WHERE address = ?",
        (if is_archived { 1 } else { 0 }, address),
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn db_set_chat_pinned(state: tauri::State<'_, DbState>, address: String, is_pinned: bool) -> Result<(), String> {
    let conn_lock = state.conn.lock().unwrap();
    let conn = conn_lock.as_ref().ok_or("Database not initialized")?;
    conn.execute(
        "UPDATE chats SET is_pinned = ? WHERE address = ?",
        (if is_pinned { 1 } else { 0 }, address),
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn db_set_message_starred(state: tauri::State<'_, DbState>, id: String, is_starred: bool) -> Result<(), String> {
    let conn_lock = state.conn.lock().unwrap();
    let conn = conn_lock.as_ref().ok_or("Database not initialized")?;
    conn.execute(
        "UPDATE messages SET is_starred = ? WHERE id = ?",
        (if is_starred { 1 } else { 0 }, id),
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn db_delete_chat(
    app: AppHandle,
    state: State<'_, DbState>,
    address: String
) -> Result<(), String> {
    let mut conn_lock = state.conn.lock().unwrap();
    let conn = conn_lock.as_mut().ok_or("Database not initialized")?;
    
    // 1. Fetch message IDs to clean media
    let mut stmt = conn.prepare("SELECT id FROM messages WHERE chat_address = ?")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([&address], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    
        for id in rows.flatten() {
            let id_clone = id.clone();
            let app_h = app.clone();
            tokio::spawn(async move {
                let _ = vault_delete_media(app_h, id_clone).await;
            });
        }

    // 2. Wipe everything from Disk
    conn.execute("DELETE FROM messages WHERE chat_address = ?", [&address])
        .map_err(|e| e.to_string())?;
        
    conn.execute("DELETE FROM chats WHERE address = ?", [&address])
        .map_err(|e| e.to_string())?;
        
    Ok(())
}

#[tauri::command]
pub fn register_nickname(
    handle: tauri::AppHandle,
    nickname: String
) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let state = handle.state::<NetworkState>();
            let id_hash = {
                let lock = state.identity_hash.lock().unwrap();
                lock.clone().ok_or("No identity hash in network state")?
            };

            println!("[Identity] Requesting Global Nickname '{}' for ID {}", nickname, id_hash);
            
            // 🛡️ SIGNATURE REQUIRED: Proving ownership of the hash without PoW
            let store = SqliteSignalStore::new(handle.clone());
            let kp = store.get_identity_key_pair().await.map_err(|e| e.to_string())?;
            let mut rng = rand::rngs::StdRng::from_os_rng();
            
            let payload = format!("NICKNAME_REGISTER:{}", nickname);
            let sig = kp.private_key().calculate_signature(payload.as_bytes(), &mut rng)
                .map_err(|e| e.to_string())?;
                
            let mut pk_bytes = kp.identity_key().serialize().to_vec();
            if pk_bytes.len() == 33 && pk_bytes[0] == 0x05 { pk_bytes.remove(0); }

            let res = internal_request(&state, "nickname_register", json!({
                "identity_hash": id_hash,
                "nickname": nickname,
                "public_key": hex::encode(&pk_bytes),
                "signature": hex::encode(&sig)
            })).await?;
            
            Ok(res)
        })
    }).join().map_err(|_| "Thread panic during nickname registration".to_string())?
}

#[tauri::command]
pub fn burn_account(
    handle: tauri::AppHandle
) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let state = handle.state::<NetworkState>();
            let id_hash = {
                let lock = state.identity_hash.lock().unwrap();
                lock.clone().ok_or("No identity hash in network state")?
            };

            println!("[Identity] Requesting Nuclear Burn for ID {}", id_hash);
            
            // 🚀 SIGNATURE REQUIRED: Server needs proof of ownership for Nuke
            let store = SqliteSignalStore::new(handle.clone());
            let kp = store.get_identity_key_pair().await.map_err(|e| e.to_string())?;
            let mut rng = rand::rngs::StdRng::from_os_rng();
            
            let payload = format!("BURN_ACCOUNT:{}", id_hash);
            let sig = kp.private_key().calculate_signature(payload.as_bytes(), &mut rng)
                .map_err(|e| e.to_string())?;
                
            let mut pk_bytes = kp.identity_key().serialize().to_vec();
            if pk_bytes.len() == 33 && pk_bytes[0] == 0x05 { pk_bytes.remove(0); }

            // Call the relay's account_burn
            let res = internal_request(&state, "account_burn", json!({
                "identity_hash": id_hash,
                "public_key": hex::encode(&pk_bytes),
                "signature": hex::encode(&sig)
            })).await?;
            
            Ok(res)
        })
    }).join().map_err(|_| "Thread panic during account burn".to_string())?
}

#[tauri::command]
pub fn send_typing_status(
    app: AppHandle,
    _db_state: State<'_, crate::app_state::DbState>,
    net_state: State<'_, crate::app_state::NetworkState>,
    peer_hash: String,
    is_typing: bool
) -> Result<(), String> {
    println!("[Typing] Command called for {} (isTyping={})", peer_hash, is_typing);
    tauri::async_runtime::block_on(async move {
        let message = json!({ "type": "typing", "isTyping": is_typing }).to_string();
        match internal_signal_encrypt(app.clone(), &net_state, &peer_hash, message).await {
            Ok(encrypted) => {
                // Flag as is_binary=true, is_media=false (opaque) to skip JSON check
                let _ = internal_send_to_network(app.clone(), &net_state, Some(peer_hash.clone()), None, Some(encrypted.to_string().into_bytes()), true, false).await;
            },
            Err(e) => println!("[Typing] Encryption FAILED for {}: {}", peer_hash, e),
        }
        Ok(())
    })
}


#[tauri::command]
pub fn send_receipt(
    app: AppHandle,
    _db_state: State<'_, crate::app_state::DbState>,
    net_state: State<'_, crate::app_state::NetworkState>,
    peer_hash: String,
    msg_ids: Vec<String>,
    status: String
) -> Result<(), String> {
    println!("[Receipt] Command called for {} (ids={:?}, status={})", peer_hash, msg_ids, status);
    tauri::async_runtime::block_on(async move {
        let message = json!({ "type": "receipt", "msgIds": msg_ids, "status": status }).to_string();
        match internal_signal_encrypt(app.clone(), &net_state, &peer_hash, message).await {
            Ok(encrypted) => {
                // Flag as is_binary=true, is_media=false (opaque) to skip JSON check
                let _ = internal_send_to_network(app.clone(), &net_state, Some(peer_hash.clone()), None, Some(encrypted.to_string().into_bytes()), true, false).await;
            },
            Err(e) => println!("[Receipt] Encryption FAILED for {}: {}", peer_hash, e),
        }
        Ok(())
    })
}


#[tauri::command]
pub fn send_profile_update(
    app: AppHandle,
    _db_state: State<'_, crate::app_state::DbState>,
    net_state: State<'_, crate::app_state::NetworkState>,
    peer_hash: String,
    alias: Option<String>
) -> Result<(), String> {
    tauri::async_runtime::block_on(async move {
        let message = json!({
            "type": "profile_update",
            "alias": alias
        }).to_string();
        if let Ok(encrypted) = internal_signal_encrypt(app.clone(), &net_state, &peer_hash, message).await {
            let _ = internal_send_volatile(app.clone(), &net_state, &peer_hash, encrypted).await;
        }
        Ok(())
    })
}


// Polling-based presence loop was removed as per performance/privacy requirements.

async fn internal_signal_encrypt(
    app: AppHandle,
    net_state: &NetworkState,
    remote_hash: &str,
    message: String
) -> Result<serde_json::Value, String> {
    let mut store = SqliteSignalStore::new(app.clone());
    let address = ProtocolAddress::new(remote_hash.to_string(), DeviceId::try_from(1u32).expect("valid ID"));
    
    // Attempt encryption
    let res: Result<CiphertextMessage, SignalProtocolError> = {
        let mut rng = StdRng::from_os_rng();
        message_encrypt(
            message.as_bytes(),
            &address,
            &mut store.clone(),
            &mut store,
            std::time::SystemTime::now(),
            &mut rng,
        ).await
    };

    match res {
        Ok(ciphertext) => {
            let (type_val, body) = match ciphertext {
                CiphertextMessage::SignalMessage(m) => (CiphertextMessageType::Whisper, m.serialized().to_vec()),
                CiphertextMessage::PreKeySignalMessage(m) => (CiphertextMessageType::PreKey, m.serialized().to_vec()),
                _ => return Err("Unsupported ciphertext type".into()),
            };
            Ok(json!({
                "type": type_val as u8,
                "body": base64::engine::general_purpose::STANDARD.encode(body),
                "is_signal": true
            }))
        }
        Err(e) if e.to_string().contains("session") || e.to_string().contains("not found") => {
            // Smart recovery: Fetch bundle and establish session
            println!("[Signal] Session not found for {}, fetching bundle...", remote_hash);
            
            let response = internal_request(net_state, "fetch_key", json!({ "target_hash": remote_hash })).await?;
            
            if !response["found"].as_bool().unwrap_or(false) {
                return Err(format!("Peer {} not found on server", remote_hash));
            }

            let bundle = if let Some(bundles_val) = response.get("bundles") {
                if let Some(bundles_obj) = bundles_val.as_object() {
                    bundles_obj.get(remote_hash).cloned().unwrap_or(serde_json::Value::Null)
                } else {
                    serde_json::Value::Null
                }
            } else {
                response["bundle"].clone()
            };

            if bundle.is_null() {
                return Err(format!("Bundle for {} is null in response", remote_hash));
            }

            internal_establish_session_logic(app.clone(), remote_hash, bundle).await?;

            println!("[Signal] Session established for {}, retrying encryption...", remote_hash);
            let mut store = SqliteSignalStore::new(app.clone());
            let mut rng = StdRng::from_os_rng();
            let ciphertext = message_encrypt(
                message.as_bytes(),
                &address,
                &mut store.clone(),
                &mut store,
                std::time::SystemTime::now(),
                &mut rng,
            ).await.map_err(|e: SignalProtocolError| e.to_string())?;

            let (type_val, body) = match ciphertext {
                CiphertextMessage::SignalMessage(m) => (CiphertextMessageType::Whisper, m.serialized().to_vec()),
                CiphertextMessage::PreKeySignalMessage(m) => (CiphertextMessageType::PreKey, m.serialized().to_vec()),
                _ => return Err("Unsupported ciphertext type".into()),
            };
            Ok(json!({
                "type": type_val as u8,
                "body": base64::engine::general_purpose::STANDARD.encode(body),
                "is_signal": true
            }))
        }
        Err(e) => Err(e.to_string())
    }
}

// --- HEADLESS RECEIVER LOGIC ---

async fn internal_signal_decrypt(
    app: AppHandle,
    remote_hash: &str,
    message_type: u8,
    message_body: &[u8]
) -> Result<String, String> {
    let mut store = SqliteSignalStore::new(app);
    let address = ProtocolAddress::new(remote_hash.to_string(), DeviceId::try_from(1u32).expect("valid ID"));
    
    println!("[Signal] Decrypting message from {} (type={})", remote_hash, message_type);

    let mut rng = StdRng::from_os_rng();

    let ciphertext_type = CiphertextMessageType::try_from(message_type)
        .map_err(|_| "Invalid message type")?;

    let ciphertext = match ciphertext_type {
        CiphertextMessageType::Whisper => CiphertextMessage::SignalMessage(
            libsignal_protocol::SignalMessage::try_from(message_body).map_err(|e: SignalProtocolError| e.to_string())?
        ),
        CiphertextMessageType::PreKey => CiphertextMessage::PreKeySignalMessage(
            libsignal_protocol::PreKeySignalMessage::try_from(message_body).map_err(|e: SignalProtocolError| e.to_string())?
        ),
        _ => return Err("Unsupported ciphertext type".into()),
    };

    let ptext = message_decrypt(
        &ciphertext,
        &address,
        &mut store.clone(),
        &mut store.clone(),
        &mut store.clone(),
        &store.clone(),
        &mut store,
        &mut rng,
    ).await.map_err(|e: SignalProtocolError| e.to_string())?;

    String::from_utf8(ptext).map_err(|e| e.to_string())
}

pub async fn process_incoming_binary(
    app: AppHandle,
    payload: Vec<u8>,
    override_sender: Option<String>
) -> Result<(), String> {
    println!("[Network] process_incoming_binary called ({} bytes)", payload.len());
    let db_state = app.state::<DbState>();
    let net_state = app.state::<NetworkState>();

    // CRITICAL: DO NOT trim zeros globally anymore. 
    // Data is random binary and zero bytes are valid.
    // Reassembly will handle the exact byte counts from fragments.
    let trimmed = &payload; 

    if trimmed.len() < 65 {
        return Ok(()); // Invalid
    }

    // FAST-DROP 0x03 (Dummy Pacing from Relay)
    if trimmed[64] == 0x03 {
        return Ok(()); 
    }

    // 1. Extract Sender (Space-padded from server)
    let header_bytes = &trimmed[0..64];
    let header_str = String::from_utf8_lossy(header_bytes).to_string();
    let sender = override_sender.unwrap_or_else(|| header_str.trim().to_string()).to_lowercase();
    
    // 🛑 Block check: Drop binary if sender is in contacts and is_blocked=1
    if !sender.is_empty() {
        let lock = db_state.conn.lock().unwrap();
        if let Some(conn) = lock.as_ref() {
            let is_blocked = conn.query_row(
                "SELECT is_blocked FROM contacts WHERE hash = ?1",
                params![sender],
                |row| row.get::<_, i32>(0)
            ).unwrap_or(0) != 0;
            
            if is_blocked {
                // Return Ok implicitly, dropping the data without further reassembly or decryption
                return Ok(());
            }
        }
    }

    let body_data = &trimmed[64..];
    if body_data.is_empty() { return Ok(()); }
    
    let frame_type = body_data[0];
    let payload_data = &body_data[1..];

    if frame_type == 0x01 || frame_type == 0x02 {
        // Universal Reassembly Flow: [4B TID] [4B Idx] [4B Total] [4B Length] [Data]
        if payload_data.len() < 16 { 
            eprintln!("[Network] Received malformed binary fragment (len={})", payload_data.len());
            return Err("Invalid binary fragment header (too short)".into()); 
        }
        
        let (tid_bytes, rest) = payload_data.split_at(4);
        let (idx_bytes, rest) = rest.split_at(4);
        let (total_bytes, rest) = rest.split_at(4);
        let (len_bytes, raw_chunk_data) = rest.split_at(4);
        
        let transfer_id = u32::from_be_bytes(tid_bytes.try_into().unwrap());
        let index = u32::from_be_bytes(idx_bytes.try_into().unwrap());
        let total = u32::from_be_bytes(total_bytes.try_into().unwrap());
        let chunk_len = u32::from_be_bytes(len_bytes.try_into().unwrap()) as usize;

        // Security: Prevent Fragment Bombing (Limit to ~10MB reassembly)
        if total > 7680 {
            tracing::warn!("[Security] Dropped Oversized Fragmented Payload from peer {} (total={} > max=7680)", sender, total);
            return Err("Payload exceeds peer-to-peer reassembly limit".into());
        }

        if raw_chunk_data.len() < chunk_len {
            eprintln!("[Network] Fragment truncated: expected {} bytes, got {}", chunk_len, raw_chunk_data.len());
            return Err("Fragment data too short".into());
        }
        let chunk_data = &raw_chunk_data[..chunk_len];
        
        let transfer_key = format!("{}:{}", sender, transfer_id);
        
        println!("[Network] Fragment Received: type={:02x} sender={} tid={} idx={} total={}", frame_type, sender, transfer_id, index, total);

        let mut assembler = net_state.media_assembler.lock().unwrap();
        let entry = assembler.entry(transfer_key.clone()).or_insert_with(|| crate::app_state::FragmentBuffer {
            total,
            chunks: std::collections::HashMap::new(),
            last_activity: std::time::Instant::now(),
        });
        
        entry.chunks.insert(index, chunk_data.to_vec());
        entry.last_activity = std::time::Instant::now();
        
        if entry.chunks.len() >= entry.total as usize {
            // Reassembly complete
            let mut complete_data = Vec::new();
            for i in 0..entry.total {
                if let Some(chunk) = entry.chunks.get(&i) {
                    complete_data.extend_from_slice(chunk);
                }
            }
            
            if frame_type == 0x01 {
                // Reassembled a Text/Signal Message
                println!("[Network] Reassembly COMPLETE for Type 0x01 (TID={} size={} bytes)", transfer_id, complete_data.len());
                
                let envelope: serde_json::Value = serde_json::from_slice(&complete_data)
                    .map_err(|e| {
                        eprintln!("[Network] JSON parse FAILED for reassembled envelope: {}", e);
                        format!("Failed to parse reassembled message envelope: {}", e)
                    })?;
                
                let msg_type = envelope["type"].as_u64().unwrap_or(1) as u8;
                let body_b64 = envelope["body"].as_str().ok_or("Missing envelope body")?;
                let body_bytes = base64::engine::general_purpose::STANDARD.decode(body_b64)
                    .map_err(|e| e.to_string())?;

                // 3. Decrypt
                match internal_signal_decrypt(app.clone(), &sender, msg_type, &body_bytes).await {
                    Ok(decrypted_str) => {
                        println!("[Signal] Decryption SUCCESS from {}: {:.50}...", sender, decrypted_str);
                        let decrypted_json: serde_json::Value = serde_json::from_str(&decrypted_str)
                            .map_err(|e| e.to_string())?;

                        // Handle based on payload type
                        let p_type = decrypted_json["type"].as_str().ok_or("Missing message type")?;
                        match p_type {
                            "group_invite" => {
                                let gid = decrypted_json["groupId"].as_str().ok_or("Missing groupId")?.to_string();
                                let name = decrypted_json["name"].as_str().ok_or("Missing group name")?.to_string();
                                let members = decrypted_json["members"].as_array()
                                    .map(|m| m.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<_>>());
                                
                                let chat = DbChat {
                                    address: gid.clone(),
                                    is_group: true,
                                    alias: Some(name.clone()),
                                    last_msg: Some(format!("Group invite: {}", name)),
                                    last_timestamp: Some(chrono::Utc::now().timestamp_millis()),
                                    last_sender_hash: Some(sender.clone()),
                                    last_status: Some("delivered".into()),
                                    unread_count: 1,
                                    is_archived: false,
                                    is_pinned: false,
                                    trust_level: 1,
                                    is_blocked: false,
                                    members,
                                };
                                internal_db_upsert_chat(&db_state, chat.clone()).await?;

                                let sys_msg = DbMessage {
                                    id: decrypted_json["id"].as_str().unwrap_or(&uuid::Uuid::new_v4().to_string()).to_string(),
                                    chat_address: gid.clone(),
                                    sender_hash: sender.clone(),
                                    content: format!("Invited to group: {}", name),
                                    timestamp: chrono::Utc::now().timestamp_millis(),
                                    r#type: "system".to_string(),
                                    status: "delivered".to_string(),
                                    attachment_json: None,
                                    is_starred: false,
                                    is_group: true,
                                    reply_to_json: None,
                                };
                                internal_db_save_message(&db_state, sys_msg.clone()).await?;
                                app.emit("msg://added", json!(sys_msg)).map_err(|e| e.to_string())?;
                                app.emit("msg://invite", json!({ "groupId": gid, "name": name, "members": chat.members })).map_err(|e| e.to_string())?;
                            },
                            "group_leave" => {
                                let gid = decrypted_json["groupId"].as_str().ok_or("Missing groupId")?.to_string();
                                let leaver = decrypted_json["sender"].as_str().unwrap_or(&sender).to_string();
                                
                                // Remove from chat_members table
                                {
                                    let lock = db_state.conn.lock().unwrap();
                                    if let Some(conn) = lock.as_ref() {
                                        let _ = conn.execute("DELETE FROM chat_members WHERE chat_address = ?1 AND member_hash = ?2", params![gid, leaver]);
                                    }
                                }

                                let sys_msg = DbMessage {
                                    id: decrypted_json["id"].as_str().unwrap_or(&uuid::Uuid::new_v4().to_string()).to_string(),
                                    chat_address: gid.clone(),
                                    sender_hash: leaver.clone(),
                                    content: format!("{} left the group", leaver),
                                    timestamp: chrono::Utc::now().timestamp_millis(),
                                    r#type: "system".to_string(),
                                    status: "delivered".to_string(),
                                    attachment_json: None,
                                    is_starred: false,
                                    is_group: true,
                                    reply_to_json: None,
                                };
                                internal_db_save_message(&db_state, sys_msg.clone()).await?;
                                app.emit("msg://added", json!(sys_msg)).map_err(|e| e.to_string())?;
                                app.emit("msg://group_leave", json!({ "groupId": gid, "member": leaver })).map_err(|e| e.to_string())?;
                            },
                            "group_update" => {
                                let gid = decrypted_json["groupId"].as_str().ok_or("Missing groupId")?.to_string();
                                if let Some(members) = decrypted_json["members"].as_array() {
                                    let m_strings: Vec<String> = members.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                                    let lock = db_state.conn.lock().unwrap();
                                    if let Some(conn) = lock.as_ref() {
                                        let _ = conn.execute("DELETE FROM chat_members WHERE chat_address = ?1", params![gid]);
                                        for m in m_strings {
                                            let _ = conn.execute("INSERT OR IGNORE INTO chat_members (chat_address, member_hash) VALUES (?1, ?2)", params![gid, m]);
                                        }
                                    }
                                }
                                app.emit("msg://group_update", json!({ "groupId": gid })).map_err(|e| e.to_string())?;
                            },
                            "text_msg" => {
                                let msg_id = decrypted_json["id"].as_str().ok_or("Missing msg id")?.to_string();
                                let content = decrypted_json["content"].as_str().ok_or("Missing content")?.to_string();
                                let timestamp = decrypted_json["timestamp"].as_i64().ok_or("Missing timestamp")?;

                                let db_msg = DbMessage {
                                    id: msg_id.clone(),
                                    chat_address: sender.clone(),
                                    sender_hash: sender.clone(),
                                    content,
                                    timestamp,
                                    r#type: "text".to_string(),
                                    status: "delivered".to_string(),
                                    attachment_json: None,
                                    is_starred: false,
                                    is_group: decrypted_json["isGroup"].as_bool().unwrap_or(sender.len() < 40),
                                    reply_to_json: decrypted_json["replyTo"].as_object().map(|r| serde_json::to_string(r).unwrap_or_default()),
                                };

                                internal_db_save_message(&db_state, db_msg.clone()).await?;
                                app.emit("msg://added", serde_json::to_value(&db_msg).unwrap()).map_err(|e| e.to_string())?;

                                // Send "delivered" receipt back to peer
                                let receipt_payload = json!({ 
                                    "type": "receipt", 
                                    "msgIds": vec![msg_id], 
                                    "status": "delivered" 
                                });
                                if let Ok(encrypted) = internal_signal_encrypt(app.clone(), &net_state, &sender, receipt_payload.to_string()).await {
                                    let _ = internal_send_to_network(app.clone(), &net_state, Some(sender.clone()), None, Some(encrypted.to_string().into_bytes()), true, false).await;
                                }
                            },
                            "receipt" => {
                                let status = decrypted_json["status"].as_str().ok_or("Missing status")?.to_string();
                                let ids = if let Some(arr) = decrypted_json["msgIds"].as_array() {
                                    arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<_>>()
                                } else {
                                    return Err("Missing or invalid msgIds array in receipt".into());
                                };

                                if !ids.is_empty() {
                                    db_update_messages_status(app.state::<DbState>(), sender.clone(), ids.clone(), status.clone()).await?;
                                    app.emit("msg://status", json!({
                                        "chat_address": sender,
                                        "ids": ids,
                                        "status": status
                                    })).map_err(|e| e.to_string())?;
                                }
                            },
                            "typing" | "profile_update" => {
                                app.emit(&format!("msg://{}", p_type), json!({
                                    "sender": sender,
                                    "payload": decrypted_json
                                })).map_err(|e| e.to_string())?;
                            },
                            "file" | "media" => {
                                let msg_id = decrypted_json["id"].as_str().ok_or("Missing msg id")?.to_string();
                                let bundle = decrypted_json["bundle"].clone();
                                let inner_transfer_id = decrypted_json["transfer_id"].as_u64().ok_or("Missing transfer id")? as u32;
                                
                                let size = decrypted_json["size"].as_u64().ok_or("Missing size")?;
                                let m_type = decrypted_json["msg_type"].as_str().ok_or("Missing msg_type")?.to_string();
                                let timestamp = decrypted_json["timestamp"].as_i64().ok_or("Missing timestamp")?;

                                let media_dir = get_media_dir(&app, &db_state)?;
                                let final_file_path = media_dir.join(&msg_id);

                                // fragmentation flow: Link or Wait
                                let inner_transfer_key = format!("{}:{}", sender, inner_transfer_id);
                                let temp_filename = format!("transfer_{}_{}.bin", sender, inner_transfer_id);
                                let temp_path = media_dir.join(&temp_filename);
                                
                                if temp_path.exists() {
                                    let _ = std::fs::rename(&temp_path, &final_file_path);
                                    println!("[Network] JSON linked to ALREADY REASSEMBLED file: {}", inner_transfer_id);
                                } else {
                                    let mut links = net_state.pending_media_links.lock().unwrap();
                                    links.insert(inner_transfer_key, msg_id.clone());
                                    println!("[Network] JSON stored pending link for transfer: {}", inner_transfer_id);
                                }

                                let db_msg = DbMessage {
                                    id: msg_id.clone(),
                                    chat_address: sender.clone(),
                                    sender_hash: sender.clone(),
                                    content: if m_type == "voice_note" { "Voice Note".to_string() } else { format!("File: {}", bundle["file_name"].as_str().unwrap_or("unnamed")) },
                                    timestamp,
                                    r#type: m_type.clone(),
                                    status: "delivered".to_string(),
                                    attachment_json: Some(json!({
                                        "fileName": bundle["file_name"],
                                        "fileType": bundle["file_type"],
                                        "size": size,
                                        "bundle": bundle,
                                        "vaultPath": final_file_path.to_string_lossy().to_string()
                                    }).to_string()),
                                    is_starred: false,
                                    is_group: bundle["isGroup"].as_bool().unwrap_or(sender.len() > 60),
                                    reply_to_json: decrypted_json["replyTo"].as_object().map(|r| serde_json::to_string(r).unwrap_or_default()),
                                };

                                internal_db_save_message(&db_state, db_msg.clone()).await?;
                                app.emit("msg://added", serde_json::to_value(&db_msg).unwrap()).map_err(|e| e.to_string())?;

                                let receipt_payload = json!({ "type": "receipt", "msgIds": vec![msg_id], "status": "delivered" });
                                if let Ok(encrypted) = internal_signal_encrypt(app.clone(), &net_state, &sender, receipt_payload.to_string()).await {
                                    let _ = internal_send_volatile(app.clone(), &net_state, &sender, encrypted).await;
                                }
                            },
                            _ => {
                                app.emit("msg://decrypted", json!({ "sender": sender, "type": p_type, "payload": decrypted_json })).map_err(|e| e.to_string())?;
                            }
                        }
                    },
                    Err(e) => {
                        eprintln!("[Signal] Decryption FAILED for {}: {}", sender, e);
                        return Err(e);
                    }
                }
            } else if frame_type == 0x02 {
                // Reassembled Media: Save to disk
                let mut final_filename = format!("transfer_{}_{}.bin", sender, transfer_id);
                if let Ok(media_dir) = get_media_dir(&app, &db_state) {
                    {
                        let mut links = net_state.pending_media_links.lock().unwrap();
                        if let Some(msg_id) = links.remove(&transfer_key) {
                            final_filename = msg_id;
                            println!("[Network] Reassembly linked to pending msg_id: {}", final_filename);
                        }
                    }

                    let file_path = media_dir.join(&final_filename);
                    if let Ok(mut f) = std::fs::File::create(&file_path) {
                        let _ = f.write_all(&complete_data);
                        println!("[Network] Media reassembly COMPLETE for TID={} ({} bytes)", transfer_id, complete_data.len());
                        app.emit("network-bin-complete", json!({
                            "sender": sender,
                            "transfer_id": transfer_id,
                            "file_path": file_path.to_string_lossy().to_string(),
                            "msg_id": if final_filename.starts_with("transfer_") { None } else { Some(final_filename) }
                        })).map_err(|e| e.to_string())?;
                    }
                }
            }
            
            // Cleanup reassembly state
            assembler.remove(&transfer_key);
        } else {
            // Fragment received, reassembly in progress
            println!("[Network] Reassembly progress for TID={}: {}/{}", transfer_id, entry.chunks.len(), total);
            app.emit("network-bin-progress", json!({
                "sender": sender,
                "transfer_id": transfer_id,
                "current": entry.chunks.len(),
                "total": total,
                "type": frame_type
            })).map_err(|e| e.to_string())?;
        }
    } else if frame_type == 0x03 {
        // Dummy/Pacing - ignore
    }

    Ok(())
}

async fn internal_send_volatile(app: AppHandle, net_state: &NetworkState, to: &str, payload: serde_json::Value) -> Result<(), String> {
    // Volatile signals (receipts, typing) are served via the Universal Binary Frame Type 0x01.
    // We now send 'volatile' signals (receipts, typing) through the Universal Binary Frame Type 0x01.
    // This provides metadata blindness and consistency.
    let payload_str = payload.to_string();
    let payload_bytes = payload_str.into_bytes();

    let routing_hash = to.split('.').next().unwrap_or(to);
    // Call internal_send_to_network with is_binary=true, is_media=false (Type 0x01)
    internal_send_to_network(app, net_state, Some(routing_hash.to_string()), None, Some(payload_bytes), true, false).await
}
