//! Binary Reassembly and Inbound Message Processing
//!
//! This module handles the reassembly of fragmented binary payloads received from the network.
//! Payloads are processed through an asynchronous media assembler that enforces:
//! - Transfer ID (TID) isolation.
//! - Strict index-to-total validation to prevent buffer overflows.
//! - Resource limits (max 80000 fragments per transfer) to mitigate memory exhaustion.
//!
//! Once reassembled, payloads are passed to the Signal decryption pipeline where session
//! state is updated and messages are persisted to the encrypted local vault.

use crate::app_state::{DbState, NetworkState};
use crate::commands::*;
use crate::signal_store::SqliteSignalStore;
use base64::Engine;
use libsignal_protocol::{
    CiphertextMessage, CiphertextMessageType, DeviceId, ProtocolAddress, SignalProtocolError,
    message_decrypt,
};
use rand::SeedableRng;
use rand::rngs::StdRng;
use rusqlite::params;
use serde_json::json;
use std::io::Write;
use tauri::{AppHandle, Emitter, Manager};

#[tauri::command]
pub fn signal_decrypt_media(data: Vec<u8>, bundle: serde_json::Value) -> Result<Vec<u8>, String> {
    let key_b64 = bundle
        .get("key")
        .and_then(|k| k.as_str())
        .ok_or("No decryption key in bundle")?;
    crypto_decrypt_media(data, key_b64.to_string())
}

async fn internal_signal_decrypt(
    app: AppHandle,
    remote_hash: &str,
    message_type: u8,
    message_body: &[u8],
) -> Result<String, String> {
    let mut store = SqliteSignalStore::new(app.clone());
    let address = ProtocolAddress::new(
        remote_hash.to_string(),
        DeviceId::try_from(1u32).expect("valid ID"),
    );

    let mut rng = StdRng::from_os_rng();

    let ciphertext_type =
        CiphertextMessageType::try_from(message_type).map_err(|_| "Invalid message type")?;

    let ciphertext = match ciphertext_type {
        CiphertextMessageType::Whisper => CiphertextMessage::SignalMessage(
            libsignal_protocol::SignalMessage::try_from(message_body)
                .map_err(|e: SignalProtocolError| e.to_string())?,
        ),
        CiphertextMessageType::PreKey => CiphertextMessage::PreKeySignalMessage(
            libsignal_protocol::PreKeySignalMessage::try_from(message_body)
                .map_err(|e: SignalProtocolError| e.to_string())?,
        ),
        _ => return Err("Unsupported ciphertext type".into()),
    };

    let own_hash = {
        let ns = app.state::<NetworkState>();
        let lock = ns.identity_hash.lock().map_err(|_| "Net lock poisoned")?;
        lock.clone().ok_or("Local identity not found")?
    };
    let own_address = ProtocolAddress::new(own_hash, DeviceId::try_from(1u32).expect("valid ID"));

    let ptext = message_decrypt(
        &ciphertext,
        &address,
        &own_address,
        &mut store.clone(),
        &mut store.clone(),
        &mut store.clone(),
        &store.clone(),
        &mut store,
        &mut rng,
    )
    .await
    .map_err(|e: SignalProtocolError| e.to_string())?;

    String::from_utf8(ptext).map_err(|e| e.to_string())
}

pub async fn process_incoming_binary(
    app: AppHandle,
    payload: Vec<u8>,
    override_sender: Option<String>,
) -> Result<(), String> {
    let db_state = app.state::<DbState>();
    let net_state = app.state::<NetworkState>();
    let own_hash = {
        let lock = net_state
            .identity_hash
            .lock()
            .map_err(|_| "Net state poisoned")?;
        lock.clone().ok_or("No identity found")?
    };

    let trimmed = &payload;

    if trimmed.len() < 65 {
        return Ok(()); // Invalid
    }

    // Discard dummy pacing packets from relay
    if payload[0] == 0x03 {
        return Ok(());
    }

    // extract sender hash
    let header_bytes = &trimmed[0..64];
    let header_str = String::from_utf8_lossy(header_bytes).to_string();
    let sender = override_sender
        .unwrap_or_else(|| header_str.trim().to_string())
        .to_lowercase();

    // contact bridge filter
    if !sender.is_empty() {
        let lock = db_state
            .conn
            .lock()
            .map_err(|_| "Database connection lock poisoned")?;
        if let Some(conn) = lock.as_ref() {
            let is_blocked = conn
                .query_row(
                    "SELECT is_blocked FROM contacts WHERE hash = ?1",
                    params![sender],
                    |row| row.get::<_, i32>(0),
                )
                .unwrap_or(0)
                != 0;

            if is_blocked {
                return Ok(());
            }
        }
    }

    let body_data = &trimmed[64..];
    if body_data.is_empty() {
        return Ok(());
    }

    let frame_type = body_data[0];
    let payload_data = &body_data[1..];

    if frame_type == 0x01 || frame_type == 0x02 || frame_type == 0x04 {
        // binary reassembly: header format [TID: 4B][Idx: 4B][Total: 4B][Len: 4B]
        if payload_data.len() < 16 {
            return Err("Invalid binary fragment header (too short)".into());
        }

        let (tid_bytes, rest) = payload_data.split_at(4);
        let (idx_bytes, rest) = rest.split_at(4);
        let (total_bytes, rest) = rest.split_at(4);
        let (len_bytes, raw_chunk_data) = rest.split_at(4);

        let transfer_id = u32::from_be_bytes(
            tid_bytes
                .try_into()
                .map_err(|e: std::array::TryFromSliceError| e.to_string())?,
        );
        let index = u32::from_be_bytes(
            idx_bytes
                .try_into()
                .map_err(|e: std::array::TryFromSliceError| e.to_string())?,
        );
        let total = u32::from_be_bytes(
            total_bytes
                .try_into()
                .map_err(|e: std::array::TryFromSliceError| e.to_string())?,
        );
        let chunk_len = u32::from_be_bytes(
            len_bytes
                .try_into()
                .map_err(|e: std::array::TryFromSliceError| e.to_string())?,
        ) as usize;

        // security: cap maximum fragments per transfer (approx 100MB)
        if total > 80000 {
            return Err("Payload exceeds limit".into());
        }

        if raw_chunk_data.len() < chunk_len {
            return Err("Fragment data too short".into());
        }
        let chunk_data = &raw_chunk_data[..chunk_len];
        let _link_key = format!("{}:{}", sender, transfer_id);
        let assembler_key = format!("{}:{}:{:02x}", sender, transfer_id, frame_type);

        let (is_complete, entry_data, _total_actual, _current_count) = {
            let mut assembler = net_state
                .media_assembler
                .lock()
                .map_err(|_| "Network state poisoned")?;
            let entry = assembler.entry(assembler_key.clone()).or_insert_with(|| {
                crate::app_state::FragmentBuffer {
                    total,
                    chunks: std::collections::HashMap::new(),
                    last_activity: std::time::Instant::now(),
                }
            });

            // security: validate index range and cap hashmap entries per assembler
            if index >= entry.total || entry.chunks.len() >= 76800 {
                return Err("Index out of range".into());
            }

            entry.chunks.insert(index, chunk_data.to_vec());
            entry.last_activity = std::time::Instant::now();

            let complete = entry.chunks.len() >= entry.total as usize;
            let mut data = Vec::new();
            let tot = entry.total;
            let cur = entry.chunks.len() as u32;

            if complete {
                for i in 0..entry.total {
                    if let Some(chunk) = entry.chunks.get(&i) {
                        data.extend_from_slice(chunk);
                    }
                }
                assembler.remove(&assembler_key);
            }

            // Throttled progress emission (every 2% or completion)
            let progress_step = (tot / 50).max(1);
            if cur % progress_step == 0 || cur == tot {
                let _ = app.emit("transfer://progress", json!({
                    "transferId": transfer_id,
                    "sender": sender,
                    "current": cur,
                    "total": tot,
                    "direction": "download"
                }));
            }

            (complete, data, tot, cur)
        };

        if is_complete {
            let complete_data = entry_data;

            if frame_type == 0x01 || frame_type == 0x04 {
                let envelope: serde_json::Value = serde_json::from_slice(&complete_data)
                    .map_err(|e| format!("Failed to parse reassembled message envelope: {}", e))?;

                let msg_type = envelope["type"].as_u64().unwrap_or(1) as u8;
                let body_b64 = envelope["body"].as_str().ok_or("Missing envelope body")?;
                let body_bytes = base64::engine::general_purpose::STANDARD
                    .decode(body_b64)
                    .map_err(|e: base64::DecodeError| e.to_string())?;

                match internal_signal_decrypt(app.clone(), &sender, msg_type, &body_bytes).await {
                    Ok(decrypted_str) => {
                        let decrypted_json: serde_json::Value =
                            serde_json::from_str(&decrypted_str).map_err(|e| e.to_string())?;

                        // check group status
                        if let Some(p_type) = decrypted_json["type"].as_str()
                            && p_type != "group_invite"
                            && let Some(gid) = decrypted_json["groupId"].as_str()
                        {
                            let lock = db_state.conn.lock().map_err(|_| "DB Lock poisoned")?;
                            if let Some(conn) = lock.as_ref() {
                                let is_active: i32 = conn
                                    .query_row(
                                        "SELECT is_active FROM chats WHERE address = ?1",
                                        params![gid],
                                        |r| r.get(0),
                                    )
                                    .unwrap_or(1); // Default to 1 (active) if group not found yet

                                if is_active == 0 {
                                    return Ok(());
                                }
                            }
                        }

                        let p_type = decrypted_json["type"]
                            .as_str()
                            .ok_or("Missing message type")?;
                        match p_type {
                            "group_invite" => {
                                let gid = decrypted_json["groupId"]
                                    .as_str()
                                    .ok_or("Missing groupId")?
                                    .to_string();
                                let name = decrypted_json["name"]
                                    .as_str()
                                    .ok_or("Missing group name")?
                                    .to_string();
                                let members = decrypted_json["members"]
                                    .as_array()
                                    .map(|m| {
                                        m.iter()
                                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default();

                                let chat = DbChat {
                                    address: gid.clone(),
                                    is_group: true,
                                    alias: Some(name.clone()),
                                    global_nickname: None,
                                    last_msg: Some(format!("Added to {}", name)),
                                    last_timestamp: Some(chrono::Utc::now().timestamp_millis()),
                                    unread_count: 1,
                                    is_archived: false,
                                    is_pinned: false,
                                    members: Some(members.clone()),
                                    trust_level: 0,
                                    is_blocked: false,
                                    last_sender_hash: Some(sender.clone()),
                                    last_status: Some("delivered".to_string()),
                                    is_active: true,
                                };
                                let db_state = app.state::<DbState>();
                                internal_db_upsert_chat(&db_state, chat.clone()).await?;

                                app.emit(
                                    "msg://group_update",
                                    json!({
                                        "groupId": gid.clone(),
                                        "name": name.clone(),
                                        "members": members.clone(),
                                    }),
                                )
                                .ok();

                                // batch members
                                let mut handled_me = false;
                                if let Some(new_m_list) = decrypted_json["newMembers"].as_array() {
                                    for nm_val in new_m_list {
                                        if let Some(nm) = nm_val.as_str() {
                                            let sys_id = uuid::Uuid::new_v4().to_string();
                                            let sys_ts = chrono::Utc::now().timestamp_millis();
                                            let content = if nm == own_hash {
                                                handled_me = true;
                                                format!(
                                                    "You were added to the group by {}",
                                                    &sender[0..8.min(sender.len())]
                                                )
                                            } else {
                                                format!(
                                                    "{} added {}",
                                                    &sender[0..8.min(sender.len())],
                                                    &nm[0..8.min(nm.len())]
                                                )
                                            };
                                            let sys_msg = DbMessage {
                                                id: sys_id,
                                                chat_address: gid.clone(),
                                                sender_hash: sender.clone(),
                                                content,
                                                timestamp: sys_ts,
                                                r#type: "system".to_string(),
                                                status: "delivered".to_string(),
                                                attachment_json: None,
                                                is_starred: false,
                                                is_group: true,
                                                reply_to_json: None,
                                            };
                                            let _ = internal_db_save_message(
                                                &db_state,
                                                sys_msg.clone(),
                                            )
                                            .await;
                                            let _ = app.emit("msg://added", json!(sys_msg));
                                        }
                                    }
                                }

                                // fallback
                                if !handled_me {
                                    let sys_id = uuid::Uuid::new_v4().to_string();
                                    let sys_ts = chrono::Utc::now().timestamp_millis();
                                    let sys_msg = DbMessage {
                                        id: sys_id,
                                        chat_address: gid.clone(),
                                        sender_hash: sender.clone(),
                                        content: format!(
                                            "You were added to the group by {}",
                                            &sender[0..8.min(sender.len())]
                                        ),
                                        timestamp: sys_ts,
                                        r#type: "system".to_string(),
                                        status: "delivered".to_string(),
                                        attachment_json: None,
                                        is_starred: false,
                                        is_group: true,
                                        reply_to_json: None,
                                    };
                                    internal_db_save_message(&db_state, sys_msg.clone()).await?;
                                    app.emit("msg://added", json!(sys_msg))
                                        .map_err(|e: tauri::Error| e.to_string())?;
                                }

                                app.emit(
                                    "msg://invite",
                                    json!({
                                        "groupId": gid,
                                        "name": name,
                                        "members": members,
                                        "lastMsg": format!("Added to {}", name),
                                        "lastTimestamp": chrono::Utc::now().timestamp_millis()
                                    }),
                                )
                                .map_err(|e: tauri::Error| e.to_string())?;
                            }
                            "group_leave" => {
                                let gid = decrypted_json["groupId"]
                                    .as_str()
                                    .ok_or("Missing groupId")?
                                    .to_string();
                                let leaver = decrypted_json["member"]
                                    .as_str()
                                    .ok_or("Missing member")?
                                    .to_string();
                                let db_state = app.state::<DbState>();

                                let msg_id = uuid::Uuid::new_v4().to_string();
                                let timestamp = chrono::Utc::now().timestamp_millis();
                                let sys_msg = DbMessage {
                                    id: msg_id,
                                    chat_address: gid.clone(),
                                    sender_hash: sender.clone(),
                                    content: format!("{} left the group", &leaver[0..8]),
                                    timestamp,
                                    r#type: "system".to_string(),
                                    status: "delivered".to_string(),
                                    attachment_json: None,
                                    is_starred: false,
                                    is_group: true,
                                    reply_to_json: None,
                                };
                                internal_db_save_message(&db_state, sys_msg.clone()).await?;
                                app.emit("msg://added", json!(sys_msg))
                                    .map_err(|e: tauri::Error| e.to_string())?;
                                app.emit(
                                    "msg://group_leave",
                                    json!({ "groupId": gid, "member": leaver }),
                                )
                                .map_err(|e: tauri::Error| e.to_string())?;
                            }
                            "group_update" => {
                                let gid = decrypted_json["groupId"]
                                    .as_str()
                                    .ok_or("Missing groupId")?
                                    .to_string();
                                let group_name = decrypted_json["name"].as_str();
                                let db_state = app.state::<DbState>();

                                if let Some(new_name) = group_name {
                                    let sys_id = uuid::Uuid::new_v4().to_string();
                                    let sys_ts = chrono::Utc::now().timestamp_millis();
                                    let sys_msg = DbMessage {
                                        id: sys_id,
                                        chat_address: gid.clone(),
                                        sender_hash: sender.clone(),
                                        content: format!(
                                            "{} changed the group name to \"{}\"",
                                            &sender[0..8],
                                            new_name
                                        ),
                                        timestamp: sys_ts,
                                        r#type: "system".to_string(),
                                        status: "delivered".to_string(),
                                        attachment_json: None,
                                        is_starred: false,
                                        is_group: true,
                                        reply_to_json: None,
                                    };
                                    let _ =
                                        internal_db_save_message(&db_state, sys_msg.clone()).await;
                                    let _ = app.emit("msg://added", json!(sys_msg));
                                }

                                {
                                    let mut system_messages = Vec::new();
                                    let m_strings: Vec<String> = if let Some(members_val) =
                                        decrypted_json["members"].as_array()
                                    {
                                        members_val
                                            .iter()
                                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                            .collect()
                                    } else {
                                        Vec::new()
                                    };

                                    {
                                        let lock = db_state
                                            .conn
                                            .lock()
                                            .map_err(|_| "Database connection lock poisoned")?;
                                        if let Some(conn) = lock.as_ref() {
                                            // Detection of changes in group membership
                                            if let Some(new_members) =
                                                decrypted_json["newMembers"].as_array()
                                            {
                                                for nm_val in new_members {
                                                    if let Some(m) = nm_val.as_str() {
                                                        if m == own_hash {
                                                            continue;
                                                        }
                                                        let content = if m == sender {
                                                            format!(
                                                                "{} joined the group",
                                                                &m[0..8.min(m.len())]
                                                            )
                                                        } else {
                                                            format!(
                                                                "{} added {}",
                                                                &sender[0..8.min(sender.len())],
                                                                &m[0..8.min(m.len())]
                                                            )
                                                        };
                                                        system_messages.push(content);
                                                    }
                                                }
                                            } else if !m_strings.is_empty() {
                                                // Fallback
                                                let mut current_m = Vec::new();
                                                if let Ok(mut stmt) = conn.prepare("SELECT member_hash FROM chat_members WHERE chat_address = ?1")
                                                    && let Ok(rows) = stmt.query_map(params![&gid], |row| row.get::<_, String>(0))
                                                {
                                                    for m in rows.flatten() {
                                                        current_m.push(m);
                                                    }
                                                }
                                                for m in &m_strings {
                                                    if !current_m.contains(m) && m != &own_hash {
                                                        let content = if m == &sender {
                                                            format!(
                                                                "{} joined the group",
                                                                &m[0..8.min(m.len())]
                                                            )
                                                        } else {
                                                            format!(
                                                                "{} added {}",
                                                                &sender[0..8.min(sender.len())],
                                                                &m[0..8.min(m.len())]
                                                            )
                                                        };
                                                        system_messages.push(content);
                                                    }
                                                }
                                            }

                                            // Update DB state
                                            if !m_strings.is_empty() {
                                                let _ = conn.execute("DELETE FROM chat_members WHERE chat_address = ?1", params![gid]);
                                                for m in m_strings {
                                                    let _ = conn.execute("INSERT OR IGNORE INTO chat_members (chat_address, member_hash) VALUES (?1, ?2)", params![gid, m]);
                                                }
                                            }
                                            if let Some(name) = group_name {
                                                let _ = conn.execute("UPDATE chats SET alias = ?1 WHERE address = ?2", params![name, gid]);
                                            }
                                        }
                                    }

                                    for content in system_messages {
                                        let sys_id = uuid::Uuid::new_v4().to_string();
                                        let sys_ts = chrono::Utc::now().timestamp_millis();
                                        let sys_msg = DbMessage {
                                            id: sys_id,
                                            chat_address: gid.clone(),
                                            sender_hash: sender.clone(),
                                            content,
                                            timestamp: sys_ts,
                                            r#type: "system".to_string(),
                                            status: "delivered".to_string(),
                                            attachment_json: None,
                                            is_starred: false,
                                            is_group: true,
                                            reply_to_json: None,
                                        };
                                        if internal_db_save_message(&db_state, sys_msg.clone())
                                            .await
                                            .is_err()
                                        {
                                            // Handle error
                                        }
                                        if app.emit("msg://added", json!(sys_msg)).is_err() {
                                            // Handle error
                                        }
                                    }
                                }

                                app.emit(
                                    "msg://group_update",
                                    json!({ "groupId": gid, "name": group_name }),
                                )
                                .map_err(|e: tauri::Error| e.to_string())?;
                            }
                            "text_msg" => {
                                let msg_id = decrypted_json["id"]
                                    .as_str()
                                    .ok_or("Missing msg id")?
                                    .to_string();
                                let content = decrypted_json["content"]
                                    .as_str()
                                    .ok_or("Missing content")?
                                    .to_string();
                                let timestamp = decrypted_json["timestamp"]
                                    .as_i64()
                                    .ok_or("Missing timestamp")?;

                                let is_group = decrypted_json["isGroup"].as_bool().unwrap_or(false);
                                let group_name =
                                    decrypted_json["groupName"].as_str().map(|s| s.to_string());
                                let chat_address = if is_group {
                                    decrypted_json["groupId"]
                                        .as_str()
                                        .unwrap_or(&sender)
                                        .to_string()
                                } else {
                                    sender.clone()
                                };

                                let db_msg = DbMessage {
                                    id: msg_id.clone(),
                                    chat_address: chat_address.clone(),
                                    sender_hash: sender.clone(),
                                    content,
                                    timestamp,
                                    r#type: "text".to_string(),
                                    status: "delivered".to_string(),
                                    attachment_json: None,
                                    is_starred: false,
                                    is_group,
                                    reply_to_json: decrypted_json["replyTo"]
                                        .as_object()
                                        .map(|r| serde_json::to_string(r).unwrap_or_default()),
                                };

                                let db_state = app.state::<DbState>();

                                // If it's a group and we have a name, ensure the chat record exists with the CORRECT name
                                if is_group {
                                    let lock = db_state
                                        .conn
                                        .lock()
                                        .map_err(|_| "Database connection lock poisoned")?;
                                    if let Some(conn) = lock.as_ref() {
                                        let is_active: i32 = conn
                                            .query_row(
                                                "SELECT is_active FROM chats WHERE address = ?1",
                                                params![chat_address],
                                                |r| r.get(0),
                                            )
                                            .unwrap_or(1);

                                        if is_active == 0 {
                                            return Ok(());
                                        }
                                    }
                                }

                                internal_db_save_message(&db_state, db_msg.clone()).await?;
                                let mut final_json =
                                    serde_json::to_value(&db_msg).map_err(|e| e.to_string())?;
                                if is_group && let Some(obj) = final_json.as_object_mut() {
                                    obj.insert("chatAlias".to_string(), json!(group_name));
                                    if let Some(members) = decrypted_json["groupMembers"].as_array()
                                    {
                                        obj.insert("chatMembers".to_string(), json!(members));
                                    }
                                }
                                app.emit("msg://added", final_json.clone())
                                    .map_err(|e| e.to_string())?;

                                // Automated delivery receipts for 1:1 chats
                                if !is_group {
                                    let receipt_payload = json!({
                                        "type": "receipt",
                                        "msgIds": vec![msg_id],
                                        "status": "delivered"
                                    });
                                    let net_state = app.state::<NetworkState>();
                                    if let Ok(encrypted) = internal_signal_encrypt(
                                        app.clone(),
                                        &net_state,
                                        &sender,
                                        receipt_payload.to_string(),
                                    )
                                    .await
                                    {
                                        let _ = internal_send_volatile(
                                            app.clone(),
                                            &net_state,
                                            &sender,
                                            encrypted,
                                        )
                                        .await;
                                    }
                                }
                            }
                            "receipt" => {
                                if let Some(ids) = decrypted_json["msgIds"].as_array() {
                                    let id_strs: Vec<String> = ids
                                        .iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect();
                                    if let Some(status) = decrypted_json["status"].as_str() {
                                        let _ = db_update_messages(
                                            app.state::<DbState>(),
                                            id_strs.clone(),
                                            Some(status.to_string()),
                                            None,
                                            None,
                                        )
                                        .await;
                                        app.emit("msg://status", json!({ "chat_address": sender, "ids": id_strs, "status": status })).map_err(|e: tauri::Error| e.to_string())?;
                                    }
                                }
                            }
                            "typing" => {
                                app.emit(
                                    "msg://typing",
                                    json!({ "sender": sender, "payload": decrypted_json }),
                                )
                                .map_err(|e: tauri::Error| e.to_string())?;
                            }
                            "profile_update" => {
                                let alias = decrypted_json["alias"].as_str().map(|s| s.to_string());
                                let db_state = app.state::<DbState>();
                                // Update contact nickname in local storage on receipt
                                let _ = db_set_contact_global_nickname(
                                    db_state,
                                    sender.clone(),
                                    alias.clone(),
                                )
                                .await;
                                app.emit(
                                    "contact-update",
                                    json!({ "hash": sender, "alias": alias }),
                                )
                                .map_err(|e: tauri::Error| e.to_string())?;
                            }
                            "file" | "media" => {
                                let raw_msg_id =
                                    decrypted_json["id"].as_str().ok_or("Missing msg id")?;
                                // Sanitize msg_id to mitigate path traversal risks
                                let msg_id = raw_msg_id
                                    .chars()
                                    .filter(|c| c.is_alphanumeric() || *c == '-')
                                    .collect::<String>();
                                if msg_id.is_empty() {
                                    return Err("Invalid message ID".into());
                                }

                                let bundle = decrypted_json["bundle"].clone();
                                let inner_transfer_id = decrypted_json["transfer_id"]
                                    .as_u64()
                                    .ok_or("Missing transfer id")?
                                    as u32;

                                let size = decrypted_json["size"].as_u64().ok_or("Missing size")?;
                                if size > 100 * 1024 * 1024 {
                                    // Rejected oversized file metadata
                                    return Err("File metadata exceeds size limit".into());
                                }
                                let m_type = decrypted_json["msg_type"]
                                    .as_str()
                                    .ok_or("Missing msg_type")?
                                    .to_string();
                                let duration = decrypted_json["duration"].as_f64().unwrap_or(0.0);
                                let timestamp =
                                    decrypted_json["timestamp"].as_i64().unwrap_or_else(|| {
                                        std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .map(|d| d.as_millis() as i64)
                                            .unwrap_or(0)
                                    });

                                let db_state = app.state::<DbState>();
                                let media_dir = get_media_dir(&app, &db_state)?;
                                let final_file_path = media_dir.join(&msg_id);

                                // fragmentation flow: Link or Wait
                                // Both JSON and Binary share a link_key for metadata cross-referencing
                                let inner_transfer_key =
                                    format!("{}:{}", sender, inner_transfer_id);
                                let temp_filename =
                                    format!("transfer_{}_{}.bin", sender, inner_transfer_id);
                                let temp_path = media_dir.join(&temp_filename);
                                let key_str =
                                    bundle["key"].as_str().unwrap_or_default().to_string();

                                if temp_path.exists() {
                                    // Decrypt and save media arriving before metadata
                                    if let Ok(encrypted_bytes) = std::fs::read(&temp_path) {
                                        if let Ok(plaintext) = crypto_decrypt_media(
                                            encrypted_bytes,
                                            key_str.clone(),
                                        ) {
                                            let _ = vault_save_media(
                                                app.clone(),
                                                db_state.clone(),
                                                msg_id.clone(),
                                                plaintext,
                                            )
                                            .await;
                                            let _ = std::fs::remove_file(&temp_path);
                                            let _ = app.emit("network-bin-complete", serde_json::json!({
                                                "sender": sender,
                                                "transfer_id": inner_transfer_id,
                                                "msg_id": Some(msg_id.clone())
                                            }));
                                        } else {
                                            // Decryption failed silently
                                        }
                                    }
                                } else {
                                    // Buffer metadata while waiting for fragments
                                    let mut links = net_state
                                        .pending_media_links
                                        .lock()
                                        .map_err(|_| "Network state poisoned")?;
                                    links.insert(
                                        inner_transfer_key,
                                        crate::app_state::PendingMediaMetadata {
                                            id: msg_id.clone(),
                                            key: key_str,
                                        },
                                    );
                                }

                                let is_group = decrypted_json["isGroup"].as_bool().unwrap_or(false);
                                let group_name =
                                    decrypted_json["groupName"].as_str().map(|s| s.to_string());
                                let chat_address = if is_group {
                                    decrypted_json["groupId"]
                                        .as_str()
                                        .unwrap_or(&sender)
                                        .to_string()
                                } else {
                                    sender.clone()
                                };

                                let db_msg = DbMessage {
                                    id: msg_id.clone(),
                                    chat_address: chat_address.clone(),
                                    sender_hash: sender.clone(),
                                    content: if m_type == "voice_note" { "Voice Note".to_string() } else { format!("File: {}", bundle["file_name"].as_str().unwrap_or("Unnamed File")) },
                                    timestamp,
                                    r#type: m_type.clone(),
                                    status: "delivered".to_string(),
                                    attachment_json: Some(json!({
                                        "fileName": bundle["file_name"],
                                        "fileType": bundle["file_type"],
                                        "size": size,
                                        "duration": duration,
                                        "thumbnail": decrypted_json["thumbnail"],
                                        "bundle": bundle,
                                        "vaultPath": final_file_path.to_string_lossy().to_string()
                                    }).to_string()),
                                    is_starred: false,
                                    is_group,
                                    reply_to_json: decrypted_json["replyTo"].as_object().map(|r| serde_json::to_string(r).unwrap_or_default()),
                                };

                                // Auto-create/rename chat for media too
                                if is_group {
                                    let lock = db_state
                                        .conn
                                        .lock()
                                        .map_err(|_| "Database connection lock poisoned")?;
                                    if let Some(conn) = lock.as_ref() {
                                        // Always update the alias to the one provided in the message metadata
                                        let _ = conn.execute(
                                            "INSERT INTO chats (address, is_group, alias) VALUES (?1, 1, ?2)
                                             ON CONFLICT(address) DO UPDATE SET 
                                                alias = CASE WHEN excluded.alias IS NOT NULL THEN excluded.alias ELSE alias END,
                                                is_group = 1",
                                            params![chat_address, group_name],
                                        );

                                        // Keep the group membership in sync
                                        if let Some(members) =
                                            decrypted_json["groupMembers"].as_array()
                                        {
                                            let m_strings: Vec<String> = members
                                                .iter()
                                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                                .collect();
                                            if !m_strings.is_empty() {
                                                let _ = conn.execute("DELETE FROM chat_members WHERE chat_address = ?1", params![chat_address]);
                                                for m in m_strings {
                                                    let _ = conn.execute("INSERT OR IGNORE INTO chat_members (chat_address, member_hash) VALUES (?1, ?2)", params![chat_address, m]);
                                                }
                                            }
                                        }
                                    }
                                }

                                internal_db_save_message(&db_state, db_msg.clone()).await?;

                                let mut final_json =
                                    serde_json::to_value(&db_msg).map_err(|e| e.to_string())?;
                                if is_group && let Some(obj) = final_json.as_object_mut() {
                                    obj.insert("chatAlias".to_string(), json!(group_name));
                                    if let Some(members) = decrypted_json["groupMembers"].as_array()
                                    {
                                        obj.insert("chatMembers".to_string(), json!(members));
                                    }
                                }
                                app.emit("msg://added", final_json.clone())
                                    .map_err(|e| e.to_string())?;

                                // enforce 1:1 delivery receipts
                                if !is_group {
                                    let receipt_payload = json!({ "type": "receipt", "msgIds": vec![msg_id], "status": "delivered" });
                                    if let Ok(encrypted) = internal_signal_encrypt(
                                        app.clone(),
                                        &net_state,
                                        &sender,
                                        receipt_payload.to_string(),
                                    )
                                    .await
                                    {
                                        let _ = internal_send_volatile(
                                            app.clone(),
                                            &net_state,
                                            &sender,
                                            encrypted,
                                        )
                                        .await;
                                    }
                                }
                            }
                            _ => {
                                app.emit("msg://decrypted", json!({ "sender": sender, "type": p_type, "payload": decrypted_json })).map_err(|e: tauri::Error| e.to_string())?;
                            }
                        }
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            } else if frame_type == 0x02 {
                // media reassembly
                let db_state = app.state::<DbState>();
                let link_key = format!("{}:{}", sender, transfer_id);
                if let Ok(media_dir) = get_media_dir(&app, &db_state) {
                    let meta = {
                        let mut links = net_state
                            .pending_media_links
                            .lock()
                            .map_err(|_| "Network state poisoned")?;
                        links.remove(&link_key)
                    };

                    if let Some(m) = meta {
                        // vault encryption bridge
                        if let Ok(plaintext) =
                            crypto_decrypt_media(complete_data.clone(), m.key)
                        {
                            let _ = vault_save_media(
                                app.clone(),
                                db_state.clone(),
                                m.id.clone(),
                                plaintext,
                            )
                            .await;
                            app.emit(
                                "network-bin-complete",
                                json!({
                                    "sender": sender,
                                    "transfer_id": transfer_id,
                                    "msg_id": Some(m.id)
                                }),
                            )
                            .map_err(|e| e.to_string())?;
                        } else {
                            // Media decryption failed
                        }
                    } else {
                        // Metadata not arrived yet, save raw Encrypted fragments to temp file
                        let temp_filename = format!("transfer_{}_{}.bin", sender, transfer_id);
                        let file_path = media_dir.join(&temp_filename);
                        if let Ok(mut f) = std::fs::File::create(&file_path) {
                            let _ = f.write_all(&complete_data);
                            let _ = f.sync_all();
                            // Media reassembly complete
                        }
                    }
                }
            }
        }
    } else if frame_type == 0x03 {
        // ignore
    }

    Ok(())
}

async fn internal_send_volatile(
    app: AppHandle,
    net_state: &NetworkState,
    to: &str,
    payload: serde_json::Value,
) -> Result<(), String> {
    // reciepts sent with 0x01 so it can be delivered even if offline
    let payload_str = payload.to_string();
    let payload_bytes = payload_str.into_bytes();

    let routing_hash = to.split('.').next().unwrap_or(to);
    internal_send_to_network(
        app,
        net_state,
        Some(routing_hash.to_string()),
        None,
        None,
        Some(payload_bytes),
        true,
        false,
        None,
        false,
    )
    .await
}
