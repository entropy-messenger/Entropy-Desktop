use crate::app_state::NetworkState;
use crate::commands::internal_request;
use crate::signal_store::SqliteSignalStore;
use libsignal_protocol::IdentityKeyStore;
use rand::SeedableRng;
use serde_json::json;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn register_nickname(handle: AppHandle, nickname: String) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let state = handle.state::<NetworkState>();
            let id_hash = state.identity_hash.lock().map_err(|_| "Network state poisoned")?.clone().ok_or("No identity hash")?;
            let store = SqliteSignalStore::new(handle.clone());
            let kp = store.get_identity_key_pair().await.map_err(|e| e.to_string())?;
            let mut rng = rand::rngs::StdRng::from_os_rng();
            let sig = kp.private_key().calculate_signature(format!("NICKNAME_REGISTER:{}", nickname).as_bytes(), &mut rng).map_err(|e| e.to_string())?;
            let mut pk_bytes = kp.identity_key().serialize().to_vec();
            if pk_bytes.len() == 33 && pk_bytes[0] == 0x05 { pk_bytes.remove(0); }
            internal_request(&state, "nickname_register", json!({ "identity_hash": id_hash, "nickname": nickname, "public_key": hex::encode(&pk_bytes), "signature": hex::encode(&sig) })).await
        })
    }).join().map_err(|_| "Thread panic".to_string())?
}

#[tauri::command]
pub fn nickname_lookup(handle: AppHandle, name: String) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let state = handle.state::<NetworkState>();
            let id_hash = state
                .identity_hash
                .lock()
                .map_err(|_| "Network state poisoned")?
                .clone()
                .ok_or("No identity hash")?;
            let store = SqliteSignalStore::new(handle.clone());
            let kp = store
                .get_identity_key_pair()
                .await
                .map_err(|e| e.to_string())?;
            let mut rng = rand::rngs::StdRng::from_os_rng();

            let payload = format!("LOOKUP_NICKNAME:{}", name);
            let sig = kp
                .private_key()
                .calculate_signature(payload.as_bytes(), &mut rng)
                .map_err(|e| e.to_string())?;
            let mut pk_bytes = kp.identity_key().serialize().to_vec();
            if pk_bytes.len() == 33 && pk_bytes[0] == 0x05 {
                pk_bytes.remove(0);
            }

            internal_request(
                &state,
                "nickname_lookup",
                json!({
                    "name": name,
                    "initiator_hash": id_hash,
                    "public_key": hex::encode(&pk_bytes),
                    "signature": hex::encode(&sig)
                }),
            )
            .await
        })
    })
    .join()
    .map_err(|_| "Thread panic".to_string())?
}

#[tauri::command]
pub fn identity_resolve(
    handle: AppHandle,
    identity_hash: String,
) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let state = handle.state::<NetworkState>();
            let id_hash = state
                .identity_hash
                .lock()
                .map_err(|_| "Network state poisoned")?
                .clone()
                .ok_or("No identity hash")?;
            let store = SqliteSignalStore::new(handle.clone());
            let kp = store
                .get_identity_key_pair()
                .await
                .map_err(|e| e.to_string())?;
            let mut rng = rand::rngs::StdRng::from_os_rng();

            let payload = format!("RESOLVE_IDENTITY:{}", identity_hash);
            let sig = kp
                .private_key()
                .calculate_signature(payload.as_bytes(), &mut rng)
                .map_err(|e| e.to_string())?;
            let mut pk_bytes = kp.identity_key().serialize().to_vec();
            if pk_bytes.len() == 33 && pk_bytes[0] == 0x05 {
                pk_bytes.remove(0);
            }

            internal_request(
                &state,
                "identity_resolve",
                json!({
                    "identity_hash": identity_hash,
                    "initiator_hash": id_hash,
                    "public_key": hex::encode(&pk_bytes),
                    "signature": hex::encode(&sig)
                }),
            )
            .await
        })
    })
    .join()
    .map_err(|_| "Thread panic".to_string())?
}

#[tauri::command]
pub fn burn_account(handle: AppHandle) -> Result<serde_json::Value, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let state = handle.state::<NetworkState>();
            let id_hash = state.identity_hash.lock().map_err(|_| "Network state poisoned")?.clone().ok_or("No identity hash")?;
            let store = SqliteSignalStore::new(handle.clone());
            let kp = store.get_identity_key_pair().await.map_err(|e| e.to_string())?;
            let mut rng = rand::rngs::StdRng::from_os_rng();
            let sig = kp.private_key().calculate_signature(format!("BURN_ACCOUNT:{}", id_hash).as_bytes(), &mut rng).map_err(|e| e.to_string())?;
            let mut pk_bytes = kp.identity_key().serialize().to_vec();
            if pk_bytes.len() == 33 && pk_bytes[0] == 0x05 { pk_bytes.remove(0); }
            internal_request(&state, "account_burn", json!({ "identity_hash": id_hash, "public_key": hex::encode(&pk_bytes), "signature": hex::encode(&sig) })).await
        })
    }).join().map_err(|_| "Thread panic".to_string())?
}
