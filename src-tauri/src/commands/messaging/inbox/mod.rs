use crate::app_state::{DbState, NetworkState};
pub mod decrypt;
pub mod handlers;
pub mod reassembler;
use crate::commands::internal_send_to_network;
use base64::Engine;
use rusqlite::params;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

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
        return Ok(());
    }

    if payload[0] == 0x03 {
        return Ok(());
    }

    let header_bytes = &trimmed[0..64];
    let header_str = String::from_utf8_lossy(header_bytes).to_string();
    let sender = override_sender
        .unwrap_or_else(|| header_str.trim().to_string())
        .to_lowercase();

    if !sender.is_empty()
        && let Ok(conn) = db_state.get_conn()
    {
        let is_blocked = conn
            .query_row(
                "SELECT is_blocked FROM contacts WHERE hash = ?1",
                params![sender],
                |row: &rusqlite::Row| row.get::<_, i32>(0),
            )
            .unwrap_or(0)
            != 0;

        if is_blocked {
            return Ok(());
        }
    }

    let body_data = &trimmed[64..];
    if body_data.is_empty() {
        return Ok(());
    }

    let frame_type = body_data[0];
    let payload_data = &body_data[1..];

    if frame_type == 0x01 || frame_type == 0x02 || frame_type == 0x04 {
        if payload_data.len() < 16 {
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

        if total > 250_000 {
            return Err("Payload exceeds limit".into());
        }

        if raw_chunk_data.len() < chunk_len {
            return Err("Fragment data too short".into());
        }
        let chunk_data = &raw_chunk_data[..chunk_len];

        let (is_complete, complete_data) = reassembler::internal_process_fragments(
            app.clone(),
            &net_state,
            &sender,
            reassembler::FragmentHeader {
                frame_type,
                transfer_id,
                index,
                total,
            },
            chunk_data,
        )
        .await?;

        if is_complete {
            if frame_type == 0x01 || frame_type == 0x04 {
                let complete_data = complete_data.ok_or("Failed to load reassembled data")?;
                let envelope: serde_json::Value = serde_json::from_slice(&complete_data)
                    .map_err(|e| format!("Failed to parse message envelope: {}", e))?;

                let msg_type = envelope["type"].as_u64().unwrap_or(1) as u8;
                let body_b64 = envelope["body"].as_str().ok_or("Missing envelope body")?;
                let body_bytes = base64::engine::general_purpose::STANDARD
                    .decode(body_b64)
                    .map_err(|e| e.to_string())?;

                match decrypt::internal_signal_decrypt(app.clone(), &sender, msg_type, &body_bytes)
                    .await
                {
                    Ok(decrypted_str) => {
                        let decrypted_json: serde_json::Value =
                            serde_json::from_str(&decrypted_str).map_err(|e| e.to_string())?;

                        // Check if chat is active for group messages (except invites)
                        if let Some(p_type) = decrypted_json["type"].as_str()
                            && p_type != "group_invite"
                            && let Some(gid) = decrypted_json["groupId"].as_str()
                            && let Ok(conn) = db_state.get_conn()
                        {
                            let is_active: i32 = conn
                                .query_row(
                                    "SELECT is_active FROM chats WHERE address = ?1",
                                    params![gid],
                                    |r| r.get(0),
                                )
                                .unwrap_or(1);
                            if is_active == 0 {
                                return Ok(());
                            }
                        }

                        let p_type = decrypted_json["type"]
                            .as_str()
                            .ok_or("Missing message type")?;
                        match p_type {
                            "media_resend_request" => {
                                let transfer_id = decrypted_json["transfer_id"]
                                    .as_u64()
                                    .ok_or("Missing transfer_id")?
                                    as u32;
                                let indices: Vec<u32> = decrypted_json["indices"]
                                    .as_array()
                                    .ok_or("Missing indices")?
                                    .iter()
                                    .filter_map(|v| v.as_u64().map(|i| i as u32))
                                    .collect();
                                let info = {
                                    let active =
                                        net_state.active_outgoing_transfers.lock().unwrap();
                                    active.get(&transfer_id).cloned()
                                };
                                if let Some(info) = info {
                                    let app_clone = app.clone();
                                    let recipient = sender.clone();
                                    tokio::spawn(async move {
                                        let net_state = app_clone.state::<NetworkState>();
                                        if let Ok(mut file) = std::fs::File::open(&info.file_path) {
                                            let file_size =
                                                file.metadata().map(|m| m.len()).unwrap_or(0);
                                            let total_fragments =
                                                (file_size as f64 / 1279.0).ceil() as u32;
                                            let mut routing_hash = [0u8; 64];
                                            let r_bytes = recipient.as_bytes();
                                            let r_len = std::cmp::min(r_bytes.len(), 64);
                                            routing_hash[..r_len]
                                                .copy_from_slice(&r_bytes[..r_len]);

                                            for idx in indices {
                                                let mut buffer = vec![0u8; 1279];
                                                let offset = (idx as u64) * 1279;
                                                use std::io::{Read, Seek, SeekFrom};
                                                if file.seek(SeekFrom::Start(offset)).is_ok() {
                                                    let n = file.read(&mut buffer).unwrap_or(0);
                                                    if n > 0 {
                                                        let chunk = &buffer[..n];

                                                        use chacha20poly1305::{
                                                            Key as ChaKey, XChaCha20Poly1305,
                                                            aead::{
                                                                Aead, AeadCore, KeyInit, OsRng,
                                                            },
                                                        };
                                                        let transit_cipher = XChaCha20Poly1305::new(
                                                            ChaKey::from_slice(&info.transit_key),
                                                        );
                                                        let t_nonce =
                                                            XChaCha20Poly1305::generate_nonce(
                                                                &mut OsRng,
                                                            );
                                                        let t_cipher = transit_cipher
                                                            .encrypt(&t_nonce, chunk)
                                                            .unwrap();

                                                        let mut packet =
                                                            Vec::with_capacity(t_cipher.len() + 24);
                                                        packet.extend_from_slice(&t_nonce);
                                                        packet.extend_from_slice(&t_cipher);

                                                        let _ = crate::commands::network::transit::internal_dispatch_fragment(
                                                            app_clone.clone(), &net_state, routing_hash, None, transfer_id, idx, total_fragments, &packet, true, true, true
                                                        ).await;
                                                    }
                                                }
                                            }
                                        }
                                    });
                                }
                                return Ok(());
                            }
                            "group_invite" => {
                                handlers::groups::handle_group_invite(
                                    app.clone(),
                                    sender.clone(),
                                    decrypted_json,
                                    &own_hash,
                                )
                                .await?
                            }
                            "group_leave" => {
                                handlers::groups::handle_group_leave(
                                    app.clone(),
                                    sender.clone(),
                                    decrypted_json,
                                )
                                .await?
                            }
                            "group_update" => {
                                handlers::groups::handle_group_update(
                                    app.clone(),
                                    sender.clone(),
                                    decrypted_json,
                                    &own_hash,
                                )
                                .await?
                            }
                            "text_msg" => {
                                handlers::text::handle_text_msg(
                                    app.clone(),
                                    sender.clone(),
                                    decrypted_json,
                                )
                                .await?
                            }
                            "receipt" => {
                                handlers::status::handle_receipt(
                                    app.clone(),
                                    sender.clone(),
                                    decrypted_json,
                                )
                                .await?
                            }
                            "typing" => {
                                handlers::status::handle_typing(
                                    app.clone(),
                                    sender.clone(),
                                    decrypted_json,
                                )
                                .await?
                            }
                            "profile_update" => {
                                handlers::status::handle_profile_update(
                                    app.clone(),
                                    sender.clone(),
                                    decrypted_json,
                                )
                                .await?
                            }
                            "file" | "media" => {
                                handlers::media::handle_media_msg(
                                    app.clone(),
                                    sender.clone(),
                                    decrypted_json,
                                    &net_state,
                                )
                                .await?
                            }
                            _ => {
                                app.emit("msg://decrypted", json!({ "sender": sender, "type": p_type, "payload": decrypted_json })).map_err(|e: tauri::Error| e.to_string())?;
                            }
                        }
                    }
                    Err(e) => return Err(e),
                }
            } else if frame_type == 0x02 {
                handlers::media::handle_media_completion(
                    app.clone(),
                    sender.clone(),
                    transfer_id,
                    &net_state,
                )
                .await?;
            }
        }
    }

    Ok(())
}

pub async fn internal_send_volatile(
    app: AppHandle,
    net_state: &NetworkState,
    to: &str,
    payload: serde_json::Value,
) -> Result<(), String> {
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

#[tauri::command]
pub async fn vault_retry_bridge(app: tauri::AppHandle, msg_id: String) -> Result<(), String> {
    handlers::media::handle_vault_retry_bridge(app, msg_id)
}
