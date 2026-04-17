use super::pacing::PACKET_SIZE;
use crate::app_state::{NetworkState, PacedMessage};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Utf8Bytes;
use tokio_tungstenite::tungstenite::protocol::Message;

pub(crate) async fn internal_request(
    state: &NetworkState,
    msg_type: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let req_id = uuid::Uuid::new_v4().to_string();
    let mut full_payload = payload.clone();
    full_payload["type"] = serde_json::Value::String(msg_type.to_string());
    full_payload["req_id"] = serde_json::Value::String(req_id.clone());

    let (tx, rx) = tokio::sync::oneshot::channel();
    {
        let mut channels = state
            .response_channels
            .lock()
            .map_err(|_| "Network state poisoned")?;
        channels.insert(req_id.clone(), tx);
    }

    {
        let sender_lock = state.sender.lock().map_err(|_| "Network state poisoned")?;
        if let Some(ws_tx) = &*sender_lock {
            let text = full_payload.to_string();
            if text.len() > 1200 {
                let data_bytes = text.into_bytes();
                let total_len = data_bytes.len();
                let chunk_capacity = 1319;
                let chunks = (total_len as f64 / chunk_capacity as f64).ceil() as usize;
                let transfer_id: u32 = rand::random();
                let zero_hash = vec![0u8; 64];
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
                    let _ = ws_tx.send(PacedMessage {
                        msg: Message::Binary(env.into()),
                    });
                }
            } else {
                let _ = ws_tx.send(PacedMessage {
                    msg: Message::Text(Utf8Bytes::from(text)),
                });
            }
        } else {
            let mut channels = state
                .response_channels
                .lock()
                .map_err(|_| "Response channels poisoned")?;
            channels.remove(&req_id);
            return Err("Not connected to network".into());
        }
    }

    match tokio::time::timeout(Duration::from_secs(10), rx).await {
        Ok(Ok(res)) => Ok(res),
        _ => {
            let mut channels = state
                .response_channels
                .lock()
                .map_err(|_| "Response channels poisoned")?;
            channels.remove(&req_id);
            Err("Request timed out".into())
        }
    }
}
