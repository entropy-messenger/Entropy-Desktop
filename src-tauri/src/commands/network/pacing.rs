use crate::app_state::{NetworkState, PacedMessage};
use tauri::Manager;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::tungstenite::Utf8Bytes;

pub const PACKET_SIZE: usize = 1400;

pub async fn send_paced_json(app: &tauri::AppHandle, val: serde_json::Value) -> Result<(), String> {
    let json_str = serde_json::to_string(&val).map_err(|e| e.to_string())?;
    let raw_len = json_str.len();

    let net_state = app.state::<NetworkState>();
    let tx_lock = net_state
        .sender
        .lock()
        .map_err(|_| "Network state poisoned")?;
    let tx = tx_lock.as_ref().ok_or("Network not connected")?;

    if raw_len > 1200 {
        let data_bytes = json_str.into_bytes();
        let chunks = (data_bytes.len() as f32 / 1200.0).ceil() as usize;
        let transfer_id: u32 = rand::random();
        let zero_hash = vec![0u8; 64];

        for i in 0..chunks {
            let start = i * 1200;
            let end = std::cmp::min(start + 1200, data_bytes.len());
            let chunk_data = &data_bytes[start..end];
            let mut envelope = Vec::with_capacity(PACKET_SIZE);
            envelope.extend_from_slice(&zero_hash);
            envelope.push(0x00);
            envelope.extend_from_slice(&transfer_id.to_be_bytes());
            envelope.extend_from_slice(&(i as u32).to_be_bytes());
            envelope.extend_from_slice(&(chunks as u32).to_be_bytes());
            envelope.extend_from_slice(&(chunk_data.len() as u32).to_be_bytes());
            envelope.extend_from_slice(chunk_data);
            tx.send(PacedMessage {
                msg: Message::Binary(envelope.into()),
            })
            .map_err(|e| e.to_string())?;
        }
    } else {
        tx.send(PacedMessage {
            msg: Message::Text(Utf8Bytes::from(json_str)),
        })
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
