use tauri::State;
use crate::protocol;
use crate::app_state::DbState;
use serde_json::Value;

#[tauri::command]
pub fn protocol_establish_session(state: State<'_, DbState>, remote_hash: String, bundle: Value) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        protocol::establish_outbound_session(conn, &remote_hash, &bundle)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_encrypt(state: State<'_, DbState>, remote_hash: String, plaintext: String) -> Result<Value, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        protocol::ratchet_encrypt(conn, &remote_hash, &plaintext)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_decrypt(state: State<'_, DbState>, remote_hash: String, msg_obj: Value) -> Result<String, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        protocol::ratchet_decrypt(conn, &remote_hash, &msg_obj)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_init(state: State<'_, DbState>) -> Result<Value, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let identity = if let Some(identity) = protocol::ProtocolIdentity::load_from_db(conn)? {
            identity
        } else {
            let identity = protocol::generate_new_identity();
            identity.save_to_db(conn)?;
            identity
        };

        Ok(serde_json::json!({
            "registration_id": identity.registration_id,
            "alias": identity.alias,
        }))
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_verify_session(state: State<'_, DbState>, remote_hash: String, verified: bool) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        protocol::verify_session(conn, &remote_hash, verified)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_encrypt_media(state: State<'_, DbState>, data: Vec<u8>, file_name: String, file_type: String) -> Result<Value, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let (ct, bundle) = protocol::encrypt_media(conn, &data, &file_name, &file_type)?;
        Ok(serde_json::json!({
            "ciphertext": hex::encode(ct),
            "bundle": bundle
        }))
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_decrypt_media(state: State<'_, DbState>, hex_data: String, bundle: Value) -> Result<Vec<u8>, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let ct = hex::decode(hex_data).map_err(|e| e.to_string())?;
        let b: protocol::MediaKeyBundle = serde_json::from_value(bundle).map_err(|e| e.to_string())?;
        protocol::decrypt_media(conn, &ct, &b)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_group_init(state: State<'_, DbState>, group_id: String) -> Result<Value, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let gs = protocol::GroupState {
            group_id: group_id.clone(),
            members: vec![]
        };
        gs.save_to_db(conn)?;
        let dist = protocol::create_group_distribution_message(&gs)?;
        Ok(dist)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_group_encrypt(state: State<'_, DbState>, group_id: String, plaintext: String) -> Result<Value, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let mut gs = protocol::GroupState::load_from_db(conn, &group_id)?.ok_or("Group not found")?;
        let res = protocol::group_encrypt(conn, &mut gs, &plaintext)?;
        gs.save_to_db(conn)?;
        Ok(res)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_group_decrypt(state: State<'_, DbState>, group_id: String, sender_hash: String, msg_obj: Value) -> Result<String, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let mut gs = protocol::GroupState::load_from_db(conn, &group_id)?.ok_or("Group not found")?;
        let res = protocol::group_decrypt(&mut gs, &sender_hash, &msg_obj)?;
        gs.save_to_db(conn)?;
        Ok(res)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_get_pending(state: State<'_, DbState>) -> Result<Vec<protocol::PendingMessage>, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        protocol::get_pending_messages(conn)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_remove_pending(state: State<'_, DbState>, id: String) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        protocol::remove_pending_message(conn, &id)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_save_pending(state: State<'_, DbState>, msg: protocol::PendingMessage) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        protocol::save_pending_message(conn, &msg)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_export_vault(app: tauri::AppHandle) -> Result<Vec<u8>, String> {
    let db_path = super::vault::get_profile_db_path(&app)?;
    if !db_path.exists() { return Err("Vault does not exist".to_string()); }
    std::fs::read(db_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn protocol_import_vault(app: tauri::AppHandle, state: State<'_, DbState>, bytes: Vec<u8>) -> Result<(), String> {
    {
        let mut lock = state.conn.lock().unwrap();
        *lock = None;
    }

    let db_path = super::vault::get_profile_db_path(&app)?;
    let app_data_dir = db_path.parent().unwrap();
    if !app_data_dir.exists() {
        std::fs::create_dir_all(app_data_dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(db_path, bytes).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn protocol_save_message(state: State<'_, DbState>, peer_hash: String, msg: Value) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        protocol::save_decrypted_message(conn, &peer_hash, &msg)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_search_messages(state: State<'_, DbState>, query: String) -> Result<Vec<Value>, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        protocol::search_messages(conn, &query)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_sign(state: State<'_, DbState>, message: String) -> Result<String, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let identity = protocol::ProtocolIdentity::load_from_db(conn)?.ok_or("Identity not found")?;
        let sig = identity.sign(message.as_bytes())?;
        Ok(protocol::encode_b64(&sig))
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_get_identity_key(state: State<'_, DbState>) -> Result<String, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let identity = protocol::ProtocolIdentity::load_from_db(conn)?.ok_or("Identity not found")?;
        Ok(protocol::encode_b64(&identity.public_key))
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_blob_put(state: State<'_, DbState>, id: String, data: Vec<u8>) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        conn.execute("INSERT OR REPLACE INTO blobs (id, data) VALUES (?1, ?2)", rusqlite::params![id, data])
            .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_blob_get(state: State<'_, DbState>, id: String) -> Result<Vec<u8>, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let mut stmt = conn.prepare("SELECT data FROM blobs WHERE id = ?1").map_err(|e| e.to_string())?;
        let res = stmt.query_row([id], |row| row.get(0)).map_err(|e| e.to_string())?;
        Ok(res)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn protocol_blob_delete(state: State<'_, DbState>, id: String) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        conn.execute("DELETE FROM blobs WHERE id = ?1", [id]).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Vault not initialized".to_string())
    }
}
