//! Transit Layer: Fragmentation and Network Dispatch
//!
//! The Transit Layer is responsible for the reliable delivery of binary payloads 
//! across the peer-to-peer network. Core responsibilities:
//! - Fragmentation: Dividing large E2EE payloads into 1319-byte data chunks.
//! - Network Framing: Encapsulating fragments with 64-byte padded recipient headers.
//! - Padding: Enforcing a uniform 1400-byte packet size to neutralize side-channel analysis.
//! - Offline Queuing: Persisting fragments in the local DB for delivery during network events.
//! - Dummy Pacing: Intermittent injection of dummy traffic to mask usage patterns.

use super::pacing::{send_paced_json, PACKET_SIZE};
use crate::app_state::{DbState, NetworkState, PacedMessage};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::tungstenite::Utf8Bytes;


pub async fn internal_send_to_network(
    app: AppHandle,
    state: &NetworkState,
    target_hash: Option<String>,
    msg_id: Option<String>,
    msg: Option<String>,
    data: Option<Vec<u8>>,
    is_binary: bool,
    is_media: bool,
    transfer_id_override: Option<u32>,
    is_volatile: bool,
) -> Result<(), String> {
    let mut paced_messages = Vec::new();

    if is_binary {
        let bytes = if let Some(d) = data {
            d
        } else if let Some(ref m) = msg {
            if let Ok(b) = hex::decode(m) {
                b
            } else {
                m.clone().into_bytes()
            }
        } else {
            return Err("Missing binary data".into());
        };

        if !bytes.is_empty() {
            let (hash_bytes, data_bytes) = if let Some(h) = target_hash {
                let mut h_padded = vec![0u8; 64];
                let h_bytes = h.as_bytes();
                let len = std::cmp::min(h_bytes.len(), 64);
                h_padded[..len].copy_from_slice(&h_bytes[..len]);
                (h_padded, bytes)
            } else {
                (vec![0u8; 64], bytes)
            };

            let total_len = data_bytes.len();
            // fragmentation: split binary payload into manageable chunks for pacing
            let chunk_capacity = 1319;
            let transfer_id: u32 = transfer_id_override.unwrap_or_else(rand::random);
            let chunks = (total_len as f64 / chunk_capacity as f64).ceil() as usize;

            for i in 0..chunks {
                let target_hash_str = hex::encode(&hash_bytes);
                if state
                    .halted_targets
                    .lock()
                    .map_err(|_| "Network state poisoned")?
                    .contains(&target_hash_str)
                {
                    break;
                }

                let start = i * chunk_capacity;
                let end = std::cmp::min(start + chunk_capacity, total_len);
                let chunk_data = &data_bytes[start..end];

                let mut envelope = Vec::with_capacity(PACKET_SIZE);
                envelope.extend_from_slice(&hash_bytes);
                if is_media {
                    envelope.push(0x02);
                } else if is_volatile {
                    envelope.push(0x04);
                } else {
                    envelope.push(0x01);
                }
                envelope.extend_from_slice(&transfer_id.to_be_bytes());
                envelope.extend_from_slice(&(i as u32).to_be_bytes());
                envelope.extend_from_slice(&(chunks as u32).to_be_bytes());
                envelope.extend_from_slice(&(chunk_data.len() as u32).to_be_bytes());
                envelope.extend_from_slice(chunk_data);

                if let Some(ref id) = msg_id {
                    state
                        .pending_transfers
                        .lock()
                        .map_err(|_| "Network state poisoned")?
                        .insert(transfer_id, id.clone());
                }
                paced_messages.push(PacedMessage {
                    msg: Message::Binary(envelope.into()),
                });
            }
        }
    }

    let is_connected = state
        .sender
        .lock()
        .map_err(|_| "Network state poisoned")?
        .is_some();

    if is_connected {
        if is_binary {
            let sender_lock = state.sender.lock().map_err(|_| "Network state poisoned")?;
            if let Some(tx) = sender_lock.as_ref() {
                for pm in paced_messages {
                    tx.send(pm).map_err(|e| e.to_string())?;
                }
            }
        } else {
            let actual_msg = msg.ok_or("Missing message text")?;
            let val: serde_json::Value =
                serde_json::from_str(&actual_msg).map_err(|e| e.to_string())?;
            send_paced_json(&app, val).await?;
        }
        Ok(())
    } else {
        // offline handling: queue fragments in database for delayed delivery
        if is_binary {
            let db_lock = app.state::<DbState>();
            let conn_lock = db_lock
                .conn
                .lock()
                .map_err(|_| "Database connection lock poisoned")?;
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            if let Some(conn) = conn_lock.as_ref() {
                let _ = conn.execute("BEGIN TRANSACTION", []);
                if let Ok(mut stmt) = conn.prepare("INSERT INTO pending_outbox (msg_id, msg_type, content, timestamp) VALUES (?1, ?2, ?3, ?4)") {
                    for pm in paced_messages {
                        let content = match &pm.msg {
                            Message::Binary(b) => b.to_vec(),
                            _ => vec![],
                        };
                        let _ = stmt.execute(rusqlite::params![msg_id.clone(), "binary", content, timestamp]);
                    }
                }
                let _ = conn.execute("COMMIT", []);
            }
            if let Some(id) = msg_id {
                let db_lock = app.state::<DbState>();
                let conn_lock = db_lock
                    .conn
                    .lock()
                    .map_err(|_| "Database connection lock poisoned")?;
                let mut chat_address: Option<String> = None;
                if let Some(conn) = conn_lock.as_ref() {
                    chat_address = conn
                        .query_row(
                            "SELECT chat_address FROM messages WHERE id = ?",
                            [&id],
                            |r| r.get(0),
                        )
                        .ok();
                    let _ = conn.execute(
                        "UPDATE messages SET status = 'pending' WHERE id = ?",
                        [id.clone()],
                    );
                    if let Some(ref addr) = chat_address {
                        let _ = conn.execute(
                            "UPDATE chats SET last_status = 'pending' WHERE address = ?",
                            [addr],
                        );
                    }
                }
                app.emit(
                    "msg://status",
                    json!({ "id": id, "status": "pending", "chat_address": chat_address }),
                )
                .map_err(|e| e.to_string())?;
            }
        }
        Err("Network not connected. Message queued in outbox.".to_string())
    }
}

pub async fn flush_outbox(app: AppHandle, state: State<'_, NetworkState>) -> Result<(), String> {
    let sender_lock = state.sender.lock().map_err(|_| "Network state poisoned")?;
    if let Some(tx) = &*sender_lock {
        let mut msg_batch = Vec::new();

        { // Scope to minimize DB lock duration
            let db_state = app.state::<DbState>();
            let db_lock = db_state.conn.lock().map_err(|_| "Database connection lock poisoned")?;
            if let Some(conn) = db_lock.as_ref() {
                if let Ok(mut stmt) = conn.prepare("SELECT id, msg_type, content, msg_id FROM pending_outbox ORDER BY rowid ASC") {
                    if let Ok(rows) = stmt.query_map([], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Vec<u8>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                        ))
                    }) {
                        for row in rows.flatten() {
                            msg_batch.push(row);
                        }
                    }
                }
            }
        }

        let mut ids_to_delete = Vec::new();
        for (id, msg_type, content, _msg_id) in msg_batch {
            let msg = if msg_type == "text" {
                Message::Text(Utf8Bytes::from(String::from_utf8_lossy(&content).to_string()))
            } else {
                Message::Binary(content.into())
            };
            let _ = tx.send(PacedMessage { msg });
            ids_to_delete.push(id);
        }

        if !ids_to_delete.is_empty() {
            let db_state = app.state::<DbState>();
            if let Ok(db_lock) = db_state.conn.lock() {
                if let Some(conn) = db_lock.as_ref() {
                    let _ = conn.execute("BEGIN TRANSACTION", []);
                    if let Ok(mut stmt) = conn.prepare("DELETE FROM pending_outbox WHERE id = ?") {
                        for id in ids_to_delete {
                            let _ = stmt.execute([id]);
                        }
                    }
                    let _ = conn.execute("COMMIT", []);
                }
            };
        }
    }
    Ok(())
}
