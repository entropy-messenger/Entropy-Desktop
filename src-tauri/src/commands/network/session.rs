use futures_util::{Sink, SinkExt, Stream, StreamExt};
use libsignal_protocol::{IdentityKeyPair, IdentityKeyStore};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;
use tokio_socks::tcp::Socks5Stream;
use tokio_tungstenite::tungstenite::Utf8Bytes;
use tokio_tungstenite::tungstenite::protocol::Message;
use url::Url;

use crate::app_state::{DbState, NetworkState, PacedMessage};
use crate::commands::messaging::inbox::process_incoming_binary;
use crate::commands::pow::internal_mine_pow;
use crate::commands::signal::signal_sync_keys;
use crate::noise::TrafficNormalizer;
use crate::signal_store::SqliteSignalStore;

use super::pacing::{PACKET_SIZE, send_paced_json};
use super::transit::flush_outbox;
const RELAY_URL: &str = "ws://localhost:8080/ws";

#[tauri::command]
pub async fn revoke_session_token(
    app: AppHandle,
    state: State<'_, NetworkState>,
) -> Result<(), String> {
    let id_hash = state
        .identity_hash
        .lock()
        .map_err(|_| "State poisoned")?
        .clone();

    let tx = {
        let sender_lock = state.sender.lock().map_err(|_| "State poisoned")?;
        sender_lock.clone()
    };

    if let (Some(_id), Some(tx)) = (id_hash, tx) {
        let revoke_req = json!({"type": "session_revoke", "req_id": "revoke_op"});
        let _ = tx
            .send(PacedMessage {
                msg: Message::Text(Utf8Bytes::from(revoke_req.to_string())),
            })
            .await;
    }

    // Always clear local even if server message fails to send
    if let Ok(mut l) = state.session_token.lock() {
        *l = None;
    }
    if let Ok(mut l) = state.is_authenticated.lock() {
        *l = false;
    }

    let app_inner = app.clone();
    tokio::task::spawn_local(async move {
        let _ = SqliteSignalStore::new(app_inner)
            .set_session_token(None)
            .await;
    });

    let _ = app.emit("network-status", "auth_failed");
    Ok(())
}

#[tauri::command]
pub async fn disconnect_network(state: State<'_, NetworkState>) -> Result<(), String> {
    if let Ok(mut l) = state.is_enabled.lock() {
        *l = false;
    }
    if let Ok(mut l) = state.sender.lock() {
        *l = None;
    }
    if let Ok(mut l) = state.cancel.lock()
        && let Some(token) = l.take()
    {
        token.cancel();
    }
    if let Ok(mut l) = state.queue.lock() {
        l.clear();
    }
    Ok(())
}

pub(crate) async fn internal_establish_network(
    app: AppHandle,
    url_str: String,
    proxy_url: Option<String>,
    token: tokio_util::sync::CancellationToken,
) -> Result<(), String> {
    let url = Url::parse(&url_str).map_err(|e| e.to_string())?;
    let host = url.host_str().ok_or("Invalid host")?;
    let port = url.port_or_known_default().ok_or("Invalid port")?;

    let _ = app.emit("network-status", "connecting");

    let mut config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
    config.max_frame_size = Some(256 * 1024);
    config.max_message_size = Some(256 * 1024);
    config.accept_unmasked_frames = false;

    let (mut write, mut read) = if let Some(p_url) = proxy_url {
        let proxy_uri = Url::parse(&p_url).map_err(|e| format!("Invalid proxy URL: {}", e))?;
        let proxy_host = proxy_uri.host_str().unwrap_or("127.0.0.1");
        let proxy_port = proxy_uri.port().unwrap_or(9050);
        let socket = Socks5Stream::connect((proxy_host, proxy_port), (host, port))
            .await
            .map_err(|e| format!("Proxy connection failed: {}", e))?;
        let (stream, _) =
            tokio_tungstenite::client_async_tls_with_config(&url_str, socket, Some(config), None)
                .await
                .map_err(|e| format!("WebSocket over proxy failed: {}", e))?;
        let (w, r) = stream.split();
        (
            Box::new(w)
                as Box<
                    dyn Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Send + Unpin,
                >,
            Box::new(r)
                as Box<
                    dyn Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
                        + Send
                        + Unpin,
                >,
        )
    } else {
        let (stream, _) =
            tokio_tungstenite::connect_async_with_config(&url_str, Some(config), false)
                .await
                .map_err(|e| e.to_string())?;
        let (w, r) = stream.split();
        (
            Box::new(w)
                as Box<
                    dyn Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Send + Unpin,
                >,
            Box::new(r)
                as Box<
                    dyn Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
                        + Send
                        + Unpin,
                >,
        )
    };

    let (tx, rx) = mpsc::channel::<PacedMessage>(100);
    let (bin_tx, mut bin_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let net_state_setup = app.state::<NetworkState>();
    if let Ok(mut l) = net_state_setup.sender.lock() {
        *l = Some(tx);
    }
    if let Ok(mut l) = net_state_setup.binary_receiver.lock() {
        *l = Some(bin_tx);
    }

    let app_recv_sequencer = app.clone();
    tokio::task::spawn_local(async move {
        while let Some(bin_vec) = bin_rx.recv().await {
            let _ = process_incoming_binary(app_recv_sequencer.clone(), bin_vec, None).await;
        }
    });

    let write_token = token.clone();
    tokio::task::spawn_local(async move {
        let mut rx = rx;
        let mut next_dummy_sleep = Box::pin(tokio::time::sleep(Duration::from_millis(
            rand::random::<u64>() % 9000 + 1000,
        )));
        loop {
            tokio::select! {
                _ = write_token.cancelled() => break,
                Some(paced) = rx.recv() => {
                    let mut msg_to_send = paced.msg;
                    match &mut msg_to_send {
                        Message::Text(text) => {
                            let mut final_json: String = text.to_string();
                            TrafficNormalizer::pad_json_str(&mut final_json, PACKET_SIZE);
                            msg_to_send = Message::Text(Utf8Bytes::from(final_json));
                        },
                        Message::Binary(data) => {
                            let mut data_vec = data.to_vec();
                            TrafficNormalizer::pad_binary(&mut data_vec, PACKET_SIZE);
                            msg_to_send = Message::Binary(data_vec.into());
                        },
                        _ => {}
                    }
                    if write.send(msg_to_send).await.is_err() { break; }
                }
                _ = &mut next_dummy_sleep => {
                    let mut dummy_vec = vec![0u8; PACKET_SIZE];
                    dummy_vec[0] = 0x03;
                    if write.send(Message::Binary(dummy_vec.into())).await.is_err() { break; }
                    next_dummy_sleep = Box::pin(tokio::time::sleep(Duration::from_millis(rand::random::<u64>() % 9000 + 1000)));
                }
            }
        }
    });

    let id_hash = app
        .state::<NetworkState>()
        .identity_hash
        .lock()
        .map_err(|_| "Network state poisoned")?
        .clone();
    let session_token_lock = app
        .state::<NetworkState>()
        .session_token
        .lock()
        .map_err(|_| "Network state poisoned")?
        .clone();

    let tx_auth = {
        if let Ok(sender_lock) = app.state::<NetworkState>().sender.lock() {
            sender_lock.clone()
        } else {
            None
        }
    };

    if let (Some(id), Some(token_val)) = (id_hash.clone(), session_token_lock) {
        if let Some(tx) = tx_auth {
            let payload = json!({ "identity_hash": id, "session_token": token_val });
            let auth_req = json!({"type": "auth", "payload": payload});
            let _ = tx
                .send(PacedMessage {
                    msg: Message::Text(Utf8Bytes::from(auth_req.to_string())),
                })
                .await;
        }
    } else if let Some(id) = id_hash
        && let Some(tx) = tx_auth
    {
        let challenge_req =
            json!({"type": "pow_challenge", "identity_hash": id, "req_id": "auto_challenge"});
        let _ = tx
            .send(PacedMessage {
                msg: Message::Text(Utf8Bytes::from(challenge_req.to_string())),
            })
            .await;
    }

    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            res = tokio::time::timeout(Duration::from_secs(60), read.next()) => {
                match res {
                    Ok(Some(Ok(msg))) => {
                        match msg {
                            Message::Text(text) => {
                                let text_str = text.to_string();
                                let mut handled = false;
                                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text_str) {
                                    if let Some(req_id) = val.get("req_id").and_then(|r| r.as_str()) {
                                        let net_state = app.state::<NetworkState>();
                                        if let Ok(mut channels) = net_state.response_channels.lock()
                                            && let Some(tx) = channels.remove(req_id)
                                        {
                                            let _ = tx.send(val.clone());
                                            handled = true;
                                        }
                                    }
                                    if let Some(msg_type) = val.get("type").and_then(|t| t.as_str()) {
                                        match msg_type {
                                            "auth_success" => {
                                                let net_state = app.state::<NetworkState>();
                                                if let Ok(mut l) = net_state.is_authenticated.lock() { *l = true; }
                                                if let Some(token_val) = val.get("session_token").and_then(|t| t.as_str()) {
                                                    if let Ok(mut l) = net_state.session_token.lock() { *l = Some(token_val.to_string()); }
                                                    let app_token = app.clone();
                                                    let token_str = token_val.to_string();
                                                    tokio::task::spawn_local(async move {
                                                        let _ = SqliteSignalStore::new(app_token).set_session_token(Some(token_str)).await;
                                                    });
                                                }
                                                let count = val.get("otk_count").and_then(|c| c.as_u64()).unwrap_or(0);
                                                if count < 50 && let Ok(mut refill_lock) = net_state.is_refilling.lock() && !*refill_lock {
                                                    *refill_lock = true;
                                                    let delta = 100_u32.saturating_sub(count as u32);
                                                    if delta > 0 {
                                                        let app_sync = app.clone();
                                                        tokio::task::spawn_local(async move {
                                                            let _ = signal_sync_keys(app_sync.clone(), Some(delta)).await;
                                                            if let Ok(mut l) = app_sync.state::<NetworkState>().is_refilling.lock() { *l = false; }
                                                        });
                                                    } else { *refill_lock = false; }
                                                }
                                                let app_flush = app.clone();
                                                tokio::task::spawn_local(async move {
                                                    let _ = flush_outbox(app_flush.clone(), app_flush.state::<NetworkState>()).await;
                                                });
                                                let _ = app.emit("network-status", "authenticated");
                                                handled = true;
                                            },
                                            "keys_low" | "otk_low" => {
                                                let count = val.get("count").or_else(|| val.get("otk_count")).and_then(|c| c.as_u64()).unwrap_or(0);
                                                let net_state = app.state::<NetworkState>();
                                                if let Ok(mut refill_lock) = net_state.is_refilling.lock() && !*refill_lock {
                                                    *refill_lock = true;
                                                    let delta = 100_u32.saturating_sub(count as u32);
                                                    if delta > 0 {
                                                        let app_sync = app.clone();
                                                        tokio::task::spawn_local(async move {
                                                            let _ = signal_sync_keys(app_sync.clone(), Some(delta)).await;
                                                            if let Ok(mut l) = app_sync.state::<NetworkState>().is_refilling.lock() { *l = false; }
                                                        });
                                                    } else { *refill_lock = false; }
                                                }
                                                handled = true;
                                            },
                                            "relay_success" | "delivery_status" | "delivery_error" => {
                                                let tid = val.get("transfer_id").and_then(|v| v.as_u64()).map(|v| v as u32);
                                                let reason = val.get("reason").and_then(|r| r.as_str()).unwrap_or("");
                                                let target_peer = val.get("target").and_then(|t| t.as_str()).unwrap_or("");
                                                if let Ok(mut pending) = app.state::<NetworkState>().pending_transfers.lock() {
                                                    let id_found = if let Some(transfer_id) = tid { pending.remove(&transfer_id) } else { None };
                                                    if let Some(id) = id_found {
                                                        let status = if msg_type == "delivery_error" {
                                                            if reason == "media_offline" { "offline" } else { "failed" }
                                                        } else { "sent" };
                                                        let db_state = app.state::<DbState>();
                                                        if let Ok(conn) = db_state.get_conn() {
                                                            let chat_info: Option<(String, String)> = conn.query_row(
                                                                "SELECT chat_address, status FROM messages WHERE LOWER(id) = LOWER(?1)",
                                                                [&id],
                                                                |r| Ok((r.get(0)?, r.get(1)?))
                                                            ).ok();
                                                            if let Some((addr, current_status)) = chat_info
                                                                && (current_status == "pending" || current_status == "sending") {
                                                                    let _ = conn.execute("UPDATE messages SET status = ?1 WHERE LOWER(id) = LOWER(?2)", [status, &id]);
                                                                    let _ = conn.execute("UPDATE chats SET last_status = ?1 WHERE LOWER(address) = LOWER(?2)", [status, &addr]);
                                                                    let _ = app.emit("msg://status", serde_json::json!({ "id": id, "status": status, "chat_address": addr }));
                                                            }
                                                        }
                                                    }
                                                }
                                                if msg_type == "delivery_error" {
                                                     if !target_peer.is_empty() && reason != "media_offline" && let Ok(mut l) = app.state::<NetworkState>().halted_targets.lock() { l.insert(target_peer.to_string()); }
                                                     let _ = app.emit("network-warning", json!({ "type": reason, "target": target_peer }));
                                                }
                                                handled = true;
                                            },
                                            "pow_challenge_res" if val.get("req_id").is_none() || val.get("req_id").and_then(|r| r.as_str()) == Some("auto_challenge") => {
                                                let seed = val.get("seed").and_then(|s| s.as_str()).map(|s| s.to_string());
                                                let diff = val.get("difficulty").and_then(|d| d.as_u64()).map(|d| d as u32);
                                                let id = app.state::<NetworkState>().identity_hash.lock().map(|l| l.clone()).unwrap_or(None);
                                                let modulus = val.get("modulus").and_then(|m| m.as_str()).map(|s| s.to_string());
                                                if let (Some(s), Some(d), Some(i)) = (seed, diff, id) {
                                                    let app_inner = app.clone();
                                                    let existing_token = app_inner.state::<NetworkState>().session_token.lock().map(|l| l.clone()).unwrap_or(None);
                                                    if let Some(token_val) = existing_token {
                                                        tokio::task::spawn_local(async move {
                                                            let payload = json!({ "identity_hash": i, "session_token": token_val });
                                                            let auth_val = json!({"type": "auth", "payload": payload});
                                                            let _ = send_paced_json(&app_inner, auth_val).await;
                                                        });
                                                    } else {
                                                        let jailed = if let Ok(l) = app_inner.state::<NetworkState>().jailed_until.lock() {
                                                            l.as_ref().map(|until| *until > tokio::time::Instant::now()).unwrap_or(false)
                                                        } else { false };

                                                        if jailed {
                                                            let _ = app_inner.emit("network-status", "jailed");
                                                        } else {
                                                            let _ = app_inner.emit("network-status", "mining");
                                                            tokio::task::spawn_local(async move {
                                                                let result = internal_mine_pow(s.clone(), d, i.clone(), modulus).await;
                                                                let sig_res = SqliteSignalStore::new(app_inner.clone()).get_identity_key_pair().await.map_err(|e| e.to_string());
                                                                let mut auth_payload = json!({"identity_hash": i, "seed": result["seed"], "nonce": result["nonce"], "modulus": result["modulus"]});
                                                                if let Ok(kp) = sig_res {
                                                                    let kp: IdentityKeyPair = kp;
                                                                    let mut rng = StdRng::from_os_rng();
                                                                    let seed_bytes = hex::decode(&s).unwrap_or_else(|_| s.as_bytes().to_vec());
                                                                    if let Ok(sig) = kp.private_key().calculate_signature(&seed_bytes, &mut rng) {
                                                                        auth_payload["signature"] = json!(hex::encode(sig));
                                                                        let mut pk = kp.identity_key().serialize().to_vec();
                                                                        if pk.len() == 33 && pk[0] == 0x05 { pk.remove(0); }
                                                                        auth_payload["public_key"] = json!(hex::encode(pk));
                                                                    }
                                                                }
                                                                let auth_val = json!({"type": "auth", "payload": auth_payload});
                                                                let _ = send_paced_json(&app_inner, auth_val).await;
                                                            });
                                                        }
                                                    }
                                                }
                                                handled = true;
                                            },
                                            "error" => {
                                                let error_msg = val.get("error").and_then(|e| e.as_str()).unwrap_or("");
                                                let error_code = val.get("code").and_then(|c| c.as_str()).unwrap_or("");
                                                if error_msg.contains("Jailed") || error_msg.contains("Identity Jailed") {
                                                     let retry_after = val.get("retry_after").and_then(|t| t.as_u64()).unwrap_or(300);
                                                     if let Ok(mut l) = app.state::<NetworkState>().jailed_until.lock() {
                                                         *l = Some(tokio::time::Instant::now() + Duration::from_secs(retry_after));
                                                     }
                                                     let _ = app.emit("network-status", "jailed");
                                                     handled = true;
                                                } else if error_code == "auth_failed" || error_msg.contains("Invalid Token") || error_msg.contains("Handshake failed") || error_msg.contains("Challenge") {
                                                    if let Ok(mut l) = app.state::<NetworkState>().session_token.lock() { *l = None; }
                                                    if let Ok(mut l) = app.state::<NetworkState>().is_authenticated.lock() { *l = false; }
                                                    let app_inner = app.clone();
                                                    tokio::task::spawn_local(async move {
                                                        let _ = SqliteSignalStore::new(app_inner).set_session_token(None).await;
                                                    });
                                                    let _ = app.emit("network-status", "auth_failed");
                                                    return Err("Handshake/Auth failed - forcing reconnect".into());
                                                }
                                            },
                                            _ => {}
                                        }
                                    }
                                }
                                if !handled { let _ = app.emit("network-msg", text_str); }
                            }
                            Message::Binary(bin) => {
                                let _ = process_incoming_binary(app.clone(), bin.to_vec(), None).await;
                            },
                            _ => {}
                        }
                    },
                    Ok(Some(Err(_))) => break,
                    Ok(None) => break,
                    Err(_) => {
                        // Timeout reached - potential stale connection
                        return Err("Network read timeout - potential stale connection".into());
                    }
                }
            }
        }
    }
    if let Ok(mut l) = app.state::<NetworkState>().sender.lock() {
        *l = None;
    }
    if let Ok(mut l) = app.state::<NetworkState>().is_authenticated.lock() {
        *l = false;
    }
    let _ = app.emit("network-status", "disconnected");
    Ok(())
}

async fn run_connection_loop(app: AppHandle) {
    let mut retry_count = 0;
    let backoff = [1, 2, 4, 8, 15, 30, 60];
    'outer_loop: loop {
        let (enabled, url, proxy_url, token) = {
            let state = app.state::<NetworkState>();
            let enabled = state.is_enabled.lock().map(|l| *l).unwrap_or(false);
            let url = state.url.lock().map(|l| l.clone()).unwrap_or(None);
            let proxy_url = state.proxy_url.lock().map(|l| l.clone()).unwrap_or(None);
            let token = state.cancel.lock().map(|l| l.clone()).unwrap_or(None);
            (enabled, url, proxy_url, token)
        };
        if !enabled {
            break;
        }
        let token_val = match token {
            Some(t) => t,
            None => break,
        };
        if token_val.is_cancelled() {
            break;
        }

        if let Some(target_url) = url {
            // Check if identity is jailed before trying to connect
            let jailed_until = if let Ok(l) = app.state::<NetworkState>().jailed_until.lock() {
                *l
            } else { None };

            if let Some(until) = jailed_until {
                let now = tokio::time::Instant::now();
                if until > now {
                    let _ = app.emit("network-status", "jailed");
                    let sleep_dur = until.duration_since(now);
                    tokio::select! {
                        _ = token_val.cancelled() => break 'outer_loop,
                        _ = tokio::time::sleep(sleep_dur) => {}
                    }
                    // Re-check everything after jail time expires
                    continue;
                }
            }

            let _ = app.emit("network-status", "connecting");
            if internal_establish_network(app.clone(), target_url, proxy_url, token_val.clone())
                .await
                .is_err()
            {
                // Connection error handled by loop
            } else {
                retry_count = 0;
            }
        }

        let is_enabled = app
            .state::<NetworkState>()
            .is_enabled
            .lock()
            .map(|l| *l)
            .unwrap_or(false);
        if !is_enabled || token_val.is_cancelled() {
            break;
        }
        let delay_sec = backoff[retry_count.min(backoff.len() - 1)];
        for s in (1..=delay_sec).rev() {
            let _ = app.emit(
                "network-status",
                json!({ "status": "reconnecting", "seconds": s }),
            );
            tokio::select! {
                _ = token_val.cancelled() => break 'outer_loop,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
        }
        retry_count += 1;
    }
}

#[tauri::command]
pub async fn connect_network(
    app: AppHandle,
    state: State<'_, NetworkState>,
    proxy_url: Option<String>,
    id_hash: Option<String>,
    session_token: Option<String>,
) -> Result<(), String> {
    let (final_id, final_token) = {
        let identity = if id_hash.is_none() {
            let db_state = app.state::<DbState>();
            if let Ok(conn) = db_state.get_conn() {
                let id_res: Option<Vec<u8>> = conn
                    .query_row(
                        "SELECT public_key FROM signal_identity WHERE id = 0",
                        [],
                        |r| r.get(0),
                    )
                    .ok();
                id_res.map(|pk| {
                    let mut ik = pk;
                    if ik.len() == 33 && ik[0] == 0x05 {
                        ik.remove(0);
                    }
                    let mut hasher = Sha256::new();
                    hasher.update(ik);
                    hex::encode(hasher.finalize())
                })
            } else {
                None
            }
        } else {
            id_hash.clone()
        };

        let token = if session_token.is_none() {
            SqliteSignalStore::new(app.clone())
                .get_session_token()
                .await
        } else {
            session_token.clone()
        };
        (identity, token)
    };

    {
        if let Ok(mut l) = state.is_enabled.lock() {
            *l = true;
        }
        if let Ok(mut l) = state.url.lock() {
            *l = Some(RELAY_URL.to_string());
        }
        if let Ok(mut l) = state.proxy_url.lock() {
            *l = proxy_url.clone();
        }
        if let Ok(mut l) = state.identity_hash.lock() {
            *l = final_id;
        }
        if let Ok(mut l) = state.session_token.lock() {
            *l = final_token;
        }
    }

    {
        let mut cancel_lock = state.cancel.lock().map_err(|_| "Network state poisoned")?;
        if let Some(t) = cancel_lock.take() {
            t.cancel();
        }
        let t = tokio_util::sync::CancellationToken::new();
        *cancel_lock = Some(t.clone());
    }

    let app_handle = app.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to build runtime");
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, run_connection_loop(app_handle));
    });

    Ok(())
}
