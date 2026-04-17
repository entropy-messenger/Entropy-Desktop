use base64::Engine;
use libsignal_protocol::{
    CiphertextMessage, CiphertextMessageType, DeviceId, GenericSignedPreKey, IdentityKey,
    IdentityKeyPair, IdentityKeyStore, KeyPair, KyberPreKeyId, KyberPreKeyRecord, KyberPreKeyStore,
    PreKeyBundle, PreKeyId, PreKeyRecord, PreKeyStore, ProtocolAddress, SessionStore,
    SignalProtocolError, SignedPreKeyId, SignedPreKeyRecord, SignedPreKeyStore, Timestamp, kem,
    message_encrypt, process_prekey_bundle,
};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::app_state::{DbState, NetworkState};
use crate::commands::internal_request;
use crate::signal_store::SqliteSignalStore;

pub(crate) async fn internal_signal_encrypt(
    app: AppHandle,
    net_state: &NetworkState,
    remote_hash: &str,
    message: String,
) -> Result<serde_json::Value, String> {
    let mut store = SqliteSignalStore::new(app.clone());
    let address = ProtocolAddress::new(
        remote_hash.to_string(),
        DeviceId::try_from(1u32).expect("valid ID"),
    );

    let own_hash = net_state
        .identity_hash
        .lock()
        .map_err(|_| "Net lock poisoned")?
        .clone()
        .ok_or("Identity not established")?;
    let own_address = ProtocolAddress::new(
        own_hash.clone(),
        DeviceId::try_from(1u32).expect("valid ID"),
    );

    let res: Result<CiphertextMessage, SignalProtocolError> = {
        let mut rng = StdRng::from_os_rng();

        // Strictly require a PQXDH session
        if let Ok(Some(_)) = store.load_session(&address).await {
            message_encrypt(
                message.as_bytes(),
                &address,
                &own_address,
                &mut store.clone(),
                &mut store,
                std::time::SystemTime::now(),
                &mut rng,
            )
            .await
        } else {
            Err(SignalProtocolError::InvalidState(
                "SessionStore",
                "Session not found".into(),
            ))
        }
    };

    match res {
        Ok(ciphertext) => {
            let (type_val, body) = match ciphertext {
                CiphertextMessage::SignalMessage(m) => {
                    (CiphertextMessageType::Whisper, m.serialized().to_vec())
                }
                CiphertextMessage::PreKeySignalMessage(m) => {
                    (CiphertextMessageType::PreKey, m.serialized().to_vec())
                }
                _ => return Err("Unsupported ciphertext type".into()),
            };
            Ok(json!({
                "type": type_val as u8,
                "body": base64::engine::general_purpose::STANDARD.encode(body),
                "is_signal": true
            }))
        }
        Err(e)
            if e.to_string().to_lowercase().contains("session")
                || e.to_string().to_lowercase().contains("not found") =>
        {
            let response = internal_request(
                net_state,
                "fetch_key",
                json!({
                    "target_hash": remote_hash,
                    "initiator_hash": own_hash
                }),
            )
            .await?;

            if !response["found"].as_bool().unwrap_or(false) {
                return Err(format!("Peer {} not found on server", remote_hash));
            }

            let bundle = if let Some(bundles_val) = response.get("bundles") {
                if let Some(bundles_obj) = bundles_val.as_object() {
                    bundles_obj
                        .get(remote_hash)
                        .cloned()
                        .unwrap_or(serde_json::Value::Null)
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

            let mut store = SqliteSignalStore::new(app.clone());
            let mut rng = StdRng::from_os_rng();
            let ciphertext = message_encrypt(
                message.as_bytes(),
                &address,
                &own_address,
                &mut store.clone(),
                &mut store,
                std::time::SystemTime::now(),
                &mut rng,
            )
            .await
            .map_err(|e: SignalProtocolError| e.to_string())?;

            let (type_val, body) = match ciphertext {
                CiphertextMessage::SignalMessage(m) => {
                    (CiphertextMessageType::Whisper, m.serialized().to_vec())
                }
                CiphertextMessage::PreKeySignalMessage(m) => {
                    (CiphertextMessageType::PreKey, m.serialized().to_vec())
                }
                _ => return Err("Unsupported ciphertext type".into()),
            };
            Ok(json!({
                "type": type_val as u8,
                "body": base64::engine::general_purpose::STANDARD.encode(body),
                "is_signal": true
            }))
        }
        Err(e) => Err::<serde_json::Value, String>(e.to_string()),
    }
}

pub fn signal_get_bundle(
    handle: tauri::AppHandle,
    count: Option<u32>,
) -> Result<serde_json::Value, String> {
    let key_count = count.unwrap_or(100).min(200);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let mut store = SqliteSignalStore::new(handle.clone());
            let mut rng = StdRng::from_os_rng();

            let identity_key_pair = store.get_identity_key_pair().await.map_err(|e: SignalProtocolError| e.to_string())?;
            let registration_id: u32 = store.get_local_registration_id().await.map_err(|e: SignalProtocolError| e.to_string())?;

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

            let signed_pre_key_id = SignedPreKeyId::from(rand::random::<u32>() & 0x7FFFFFFF);
            let signed_pre_key_pair = KeyPair::generate(&mut rng);
            let timestamp = Timestamp::from_epoch_millis(
                std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
            );
            let signature = identity_key_pair.private_key().calculate_signature(&signed_pre_key_pair.public_key.serialize(), &mut rng)
                .map_err(|e| e.to_string())?;
            let signed_pre_key_record = SignedPreKeyRecord::new(signed_pre_key_id, timestamp, &signed_pre_key_pair, &signature);
            store.save_signed_pre_key(signed_pre_key_id, &signed_pre_key_record).await.map_err(|e: SignalProtocolError| e.to_string())?;

            let mut kyber_pre_keys_json = Vec::new();
            for _ in 0..key_count {
                let id = KyberPreKeyId::from(rand::random::<u32>() & 0x7FFFFFFF);
                let record = KyberPreKeyRecord::generate(kem::KeyType::Kyber1024, id, identity_key_pair.private_key())
                    .map_err(|e: SignalProtocolError| e.to_string())?;
                store.save_kyber_pre_key(id, &record).await.map_err(|e: SignalProtocolError| e.to_string())?;
                kyber_pre_keys_json.push(serde_json::json!({
                    "id": u32::from(id),
                    "publicKey": hex::encode(record.public_key().map_err(|e: SignalProtocolError| e.to_string())?.serialize()),
                    "signature": hex::encode(record.signature().map_err(|e: SignalProtocolError| e.to_string())?)
                }));
            }

            // Also keep the single "last resort" kyberPreKey for compatibility with older establishments
            let last_resort_id = KyberPreKeyId::from(rand::random::<u32>() & 0x7FFFFFFF);
            let last_resort_record = KyberPreKeyRecord::generate(kem::KeyType::Kyber1024, last_resort_id, identity_key_pair.private_key())
                .map_err(|e: SignalProtocolError| e.to_string())?;
            store.save_kyber_pre_key(last_resort_id, &last_resort_record).await.map_err(|e: SignalProtocolError| e.to_string())?;

            Ok(serde_json::json!({
                "registrationId": registration_id,
                "identityKey": hex::encode(identity_key_pair.identity_key().serialize()),
                "preKeys": pre_keys_json,
                "kyberPreKeys": kyber_pre_keys_json,
                "signedPreKey": {
                    "id": u32::from(signed_pre_key_id),
                    "publicKey": hex::encode(signed_pre_key_pair.public_key.serialize()),
                    "signature": hex::encode(signature)
                },
                "kyberPreKey": {
                    "id": u32::from(last_resort_id),
                    "publicKey": hex::encode(last_resort_record.public_key().map_err(|e: SignalProtocolError| e.to_string())?.serialize()),
                    "signature": hex::encode(last_resort_record.signature().map_err(|e: SignalProtocolError| e.to_string())?)
                }
            }))
        })
    }).join().map_err(|_| "Thread panicked".to_string())?
}

fn internal_decode_key(s: &str) -> Result<Vec<u8>, String> {
    if (s.len() == 64 || s.len() == 66)
        && s.chars().all(|c| c.is_ascii_hexdigit())
        && let Ok(b) = hex::decode(s)
    {
        return Ok(b);
    }
    if let Ok(b) = base64::engine::general_purpose::STANDARD.decode(s) {
        return Ok(b);
    }
    hex::decode(s).map_err(|e| format!("Failed to decode key as Base64 or Hex: {} -> {}", s, e))
}

pub(crate) async fn internal_establish_session_logic(
    app: AppHandle,
    remote_hash: &str,
    bundle: serde_json::Value,
) -> Result<(), String> {
    let mut store = SqliteSignalStore::new(app.clone());
    let address = ProtocolAddress::new(
        remote_hash.to_string(),
        DeviceId::try_from(1u32).expect("valid ID"),
    );

    let registration_id = bundle["registrationId"]
        .as_u64()
        .ok_or("Missing registrationId")? as u32;
    let identity_key_hex = bundle["identityKey"]
        .as_str()
        .ok_or("Missing identityKey")?;
    let mut identity_key_bytes = internal_decode_key(identity_key_hex)?;

    let mut existing_trust = 1;
    {
        let state = app.state::<DbState>();
        let lock = state
            .conn
            .lock()
            .map_err(|_| "Database connection lock poisoned")?;
        if let Some(conn) = lock.as_ref() {
            let res: Result<(Vec<u8>, i32), _> = conn.query_row(
                "SELECT public_key, trust_level FROM signal_identities_remote WHERE address = ?1",
                rusqlite::params![address.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            );

            if let Ok((old_key, old_trust)) = res {
                let old_key_stripped = if old_key.len() == 33 && old_key[0] == 0x05 {
                    &old_key[1..]
                } else {
                    &old_key
                };
                let new_key_stripped = &identity_key_bytes;

                if old_key_stripped != new_key_stripped {
                    existing_trust = 0;
                    let _ = conn.execute(
                        "UPDATE contacts SET trust_level = 0 WHERE hash = ?1",
                        rusqlite::params![remote_hash],
                    );
                } else {
                    existing_trust = old_trust;
                }
            }
        }
    }

    if identity_key_bytes.len() == 32 {
        let mut new_bytes = Vec::with_capacity(33);
        new_bytes.push(0x05);
        new_bytes.extend_from_slice(&identity_key_bytes);
        identity_key_bytes = new_bytes;
    }
    let identity_key = IdentityKey::decode(&identity_key_bytes).map_err(|e| e.to_string())?;

    let (pre_key_id, pre_key_pub) = {
        let pre_key_obj = bundle
            .get("preKey")
            .or_else(|| bundle.get("preKeys").and_then(|pk| pk.as_array()?.first()))
            .ok_or("Missing preKey object")?;
        let id = PreKeyId::from(pre_key_obj["id"].as_u64().ok_or("Missing preKey id")? as u32);
        let pub_str = pre_key_obj["publicKey"]
            .as_str()
            .ok_or("Missing preKey publicKey")?;
        let mut pub_bytes = internal_decode_key(pub_str)?;
        if pub_bytes.len() == 32 {
            let mut prefixed = Vec::with_capacity(33);
            prefixed.push(0x05);
            prefixed.extend_from_slice(&pub_bytes);
            pub_bytes = prefixed;
        }
        let pub_key =
            libsignal_protocol::PublicKey::deserialize(&pub_bytes).map_err(|e| e.to_string())?;
        (id, pub_key)
    };

    let signed_pre_key_id = SignedPreKeyId::from(
        bundle["signedPreKey"]["id"]
            .as_u64()
            .ok_or("Missing signedPreKey id")? as u32,
    );
    let s_pub_str = bundle["signedPreKey"]["publicKey"]
        .as_str()
        .ok_or("Missing signedPreKey publicKey")?;
    let mut signed_pre_key_pub_bytes = internal_decode_key(s_pub_str)?;
    if signed_pre_key_pub_bytes.len() == 32 {
        let mut prefixed = Vec::with_capacity(33);
        prefixed.push(0x05);
        prefixed.extend_from_slice(&signed_pre_key_pub_bytes);
        signed_pre_key_pub_bytes = prefixed;
    }
    let signed_pre_key_pub = libsignal_protocol::PublicKey::deserialize(&signed_pre_key_pub_bytes)
        .map_err(|e| e.to_string())?;
    let s_sig_str = bundle["signedPreKey"]["signature"]
        .as_str()
        .ok_or("Missing signedPreKey signature")?;
    let signed_pre_key_sig =
        if let Ok(b) = base64::engine::general_purpose::STANDARD.decode(s_sig_str) {
            b
        } else {
            hex::decode(s_sig_str).map_err(|e| e.to_string())?
        };

    let kyber_pre_key_id = KyberPreKeyId::from(
        bundle["kyberPreKey"]["id"]
            .as_u64()
            .ok_or("Missing kyberPreKey id")? as u32,
    );
    let k_pub_str = bundle["kyberPreKey"]["publicKey"]
        .as_str()
        .ok_or("Missing kyberPreKey publicKey")?;
    let kyber_pre_key_pub_bytes = internal_decode_key(k_pub_str)?;
    let kyber_pre_key_pub =
        kem::PublicKey::deserialize(&kyber_pre_key_pub_bytes).map_err(|e| e.to_string())?;
    let k_sig_str = bundle["kyberPreKey"]["signature"]
        .as_str()
        .ok_or("Missing kyberPreKey signature")?;
    let kyber_pre_key_sig =
        if let Ok(b) = base64::engine::general_purpose::STANDARD.decode(k_sig_str) {
            b
        } else {
            hex::decode(k_sig_str).map_err(|e| e.to_string())?
        };

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
        identity_key,
    )
    .map_err(|e| e.to_string())?;

    let mut hasher = Sha256::new();
    let ser = identity_key.serialize();
    let to_hash = if ser.len() == 33 && ser[0] == 0x05 {
        &ser[1..]
    } else {
        ser.as_ref()
    };
    hasher.update(to_hash);
    let id_hash = hex::encode(hasher.finalize());
    if id_hash != remote_hash.to_lowercase() {
        return Err(format!(
            "Identity Mismatch! Bundle for {} belongs to a different key: {}",
            remote_hash, id_hash
        ));
    }

    let mut rng = StdRng::from_os_rng();
    process_prekey_bundle(
        &address,
        &mut store.clone(),
        &mut store,
        &prekey_bundle,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await
    .map_err(|e| e.to_string())?;

    {
        let state = app.state::<DbState>();
        let lock = state
            .conn
            .lock()
            .map_err(|_| "Database connection lock poisoned")?;
        if let Some(conn) = lock.as_ref() {
            let _ = conn.execute(
                "UPDATE signal_identities_remote SET trust_level = ?1 WHERE address = ?2",
                rusqlite::params![existing_trust, address.to_string()],
            );
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn signal_init(handle: tauri::AppHandle) -> Result<String, String> {
    let handle_clone = handle.clone();
    let result: Result<String, String> = tauri::async_runtime::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let store = SqliteSignalStore::new(handle_clone.clone());
            if let Ok(kp) = store.get_identity_key_pair().await {
                let mut pub_key_raw = kp.identity_key().serialize().to_vec();
                if pub_key_raw.len() == 33 && pub_key_raw[0] == 0x05 { pub_key_raw.remove(0); }
                return Ok::<String, String>(hex::encode(pub_key_raw));
            }
            let mut rng = StdRng::from_os_rng();
            let identity_key_pair = IdentityKeyPair::generate(&mut rng);
            let registration_id: u32 = rand::random::<u32>() & 0x3FFF;
            let mut pub_bytes = identity_key_pair.identity_key().serialize().to_vec();
            let priv_bytes = identity_key_pair.private_key().serialize();

            let db_state = handle_clone.state::<DbState>();
            let db_lock = db_state.conn.lock().map_err(|_| "Database connection lock poisoned")?;
            let conn = db_lock.as_ref().ok_or("Database not initialized")?;

            conn.execute(
                "INSERT OR REPLACE INTO signal_identity (id, registration_id, public_key, private_key) VALUES (0, ?1, ?2, ?3)",
                rusqlite::params![registration_id, &pub_bytes[..], &priv_bytes[..]],
            ).map_err(|e: rusqlite::Error| e.to_string())?;

            if pub_bytes.len() == 33 && pub_bytes[0] == 0x05 { pub_bytes.remove(0); }
            Ok::<String, String>(hex::encode(pub_bytes))
        })
    }).await.map_err(|e| e.to_string())?;

    let pub_key_hex = result?;
    let state = handle.state::<NetworkState>();
    let pub_bytes = hex::decode(&pub_key_hex).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&pub_bytes);
    let id_hash = hex::encode(hasher.finalize());
    if let Ok(mut hash_lock) = state.identity_hash.lock() {
        *hash_lock = Some(id_hash);
    }
    Ok(pub_key_hex)
}

#[tauri::command]
pub fn signal_sync_keys(handle: AppHandle, count: Option<u32>) -> Result<(), String> {
    let handle_clone = handle.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let state = handle_clone.state::<NetworkState>();
            let raw_bundle = signal_get_bundle(handle_clone.clone(), count).map_err(|e| e.to_string())?;
            let id_hash = {
                let lock = state.identity_hash.lock().map_err(|_| "Network state poisoned")?;
                lock.clone().ok_or("No identity hash in network state")?
            };
            let mut ik_bytes = hex::decode(raw_bundle["identityKey"].as_str().unwrap_or("")).unwrap_or_default();
            if ik_bytes.len() == 33 && ik_bytes[0] == 0x05 { ik_bytes.remove(0); }
            let bundle = json!({
                "identity_hash": id_hash,
                "registrationId": raw_bundle["registrationId"],
                "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_bytes),
                "signedPreKey": {
                    "id": raw_bundle["signedPreKey"]["id"],
                    "publicKey": base64::engine::general_purpose::STANDARD.encode(hex::decode(raw_bundle["signedPreKey"]["publicKey"].as_str().unwrap_or("")).unwrap_or_default()),
                    "signature": base64::engine::general_purpose::STANDARD.encode(hex::decode(raw_bundle["signedPreKey"]["signature"].as_str().unwrap_or("")).unwrap_or_default()),
                },
                "preKeys": raw_bundle["preKeys"],
                "kyberPreKey": {
                    "id": raw_bundle["kyberPreKey"]["id"],
                    "publicKey": base64::engine::general_purpose::STANDARD.encode(hex::decode(raw_bundle["kyberPreKey"]["publicKey"].as_str().unwrap_or("")).unwrap_or_default()),
                    "signature": base64::engine::general_purpose::STANDARD.encode(hex::decode(raw_bundle["kyberPreKey"]["signature"].as_str().unwrap_or("")).unwrap_or_default())
                }
            });
            let store = SqliteSignalStore::new(handle_clone.clone());
            let kp = store.get_identity_key_pair().await.map_err(|e: SignalProtocolError| e.to_string())?;
            let mut rng = rand::rngs::StdRng::from_os_rng();
            let sig = kp.private_key().calculate_signature(id_hash.as_bytes(), &mut rng).map_err(|e| e.to_string())?;
            let mut final_upload = bundle;
            let mut pk_bytes = kp.identity_key().serialize().to_vec();
            if pk_bytes.len() == 33 && pk_bytes[0] == 0x05 { pk_bytes.remove(0); }
            final_upload["identityKey"] = json!(hex::encode(&pk_bytes));
            final_upload["signature"] = json!(hex::encode(&sig));
            let response = internal_request(&state, "keys_upload", final_upload).await?;
            if response["status"].as_str() == Some("success") { Ok(()) }
            else { Err(format!("Key upload failed: {}", response["error"].as_str().unwrap_or("Unknown"))) }
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
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let _net_state = handle.state::<NetworkState>();
            internal_signal_encrypt(handle.clone(), &_net_state, &remote_hash, message).await
        })
    })
    .join()
    .map_err(|_| "Thread panicked".to_string())?
}

#[tauri::command]
pub fn signal_sign_message(handle: tauri::AppHandle, message: String) -> Result<String, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let store = SqliteSignalStore::new(handle.clone());
            let kp = store
                .get_identity_key_pair()
                .await
                .map_err(|e| e.to_string())?;
            let mut rng = rand::rngs::StdRng::from_os_rng();
            let sig = kp
                .private_key()
                .calculate_signature(message.as_bytes(), &mut rng)
                .map_err(|e| e.to_string())?;
            Ok(base64::engine::general_purpose::STANDARD.encode(sig))
        })
    })
    .join()
    .map_err(|_| "Thread panicked".to_string())?
}
