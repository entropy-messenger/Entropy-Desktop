use crate::app_state::{DbState, NetworkState};
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::State;

#[tauri::command]
pub async fn signal_get_peer_identity(
    state: State<'_, DbState>,
    address: String,
) -> Result<Option<(Vec<u8>, i32)>, String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;
    let mut stmt = conn
        .prepare("SELECT public_key, trust_level FROM signal_identities_remote WHERE address = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query(rusqlite::params![address])
        .map_err(|e| e.to_string())?;
    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        Ok(Some((
            row.get(0).map_err(|e| e.to_string())?,
            row.get(1).map_err(|e| e.to_string())?,
        )))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn signal_set_peer_trust(
    state: State<'_, DbState>,
    address: String,
    trust_level: i32,
) -> Result<(), String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;
    let signal_addr = if !address.contains(':') {
        format!("{}:1", address)
    } else {
        address.clone()
    };
    conn.execute(
        "UPDATE signal_identities_remote SET trust_level = ?1 WHERE address = ?2",
        rusqlite::params![trust_level, signal_addr],
    )
    .map_err(|e| e.to_string())?;
    let contact_hash = address.split(':').next().unwrap_or(&address);
    conn.execute(
        "UPDATE contacts SET trust_level = ?1 WHERE hash = ?2",
        rusqlite::params![trust_level, contact_hash],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn signal_get_own_identity(state: State<'_, DbState>) -> Result<Vec<u8>, String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;
    conn.query_row(
        "SELECT public_key FROM signal_identity LIMIT 1",
        [],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn signal_get_identity_hash(
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
) -> Result<String, String> {
    {
        let lock = net_state
            .identity_hash
            .lock()
            .map_err(|_| "Network state poisoned")?;
        if let Some(hash) = lock.as_ref() {
            return Ok(hash.clone());
        }
    }
    let mut pub_key = signal_get_own_identity(db_state).await?;
    if pub_key.len() == 33 && pub_key[0] == 0x05 {
        pub_key.remove(0);
    }
    let mut hasher = Sha256::new();
    hasher.update(&pub_key);
    let hash = hex::encode(hasher.finalize());
    let mut lock = net_state
        .identity_hash
        .lock()
        .map_err(|_| "Network state poisoned")?;
    *lock = Some(hash.clone());
    Ok(hash)
}

#[tauri::command]
pub async fn signal_get_fingerprint(
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    remote_hash: String,
) -> Result<serde_json::Value, String> {
    let own_id_bytes = signal_get_own_identity(db_state.clone()).await?;
    let peer_data =
        signal_get_peer_identity(db_state.clone(), format!("{}:1", remote_hash)).await?;
    let (peer_id_bytes, trust_level) = peer_data.ok_or("Peer identity not found")?;
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
    let mut digits = String::with_capacity(72);
    for i in 0..12 {
        let val = ((hash_result[i * 2] as u32) << 8) | (hash_result[i * 2 + 1] as u32);
        let block_val = val % 100000;
        if block_val < 10000 {
            digits.push('0');
        }
        if block_val < 1000 {
            digits.push('0');
        }
        if block_val < 100 {
            digits.push('0');
        }
        if block_val < 10 {
            digits.push('0');
        }
        digits.push_str(&block_val.to_string());
        if i == 5 {
            digits.push('\n');
        } else if i < 11 {
            digits.push(' ');
        }
    }
    Ok(json!({ "digits": digits, "trustLevel": trust_level }))
}
