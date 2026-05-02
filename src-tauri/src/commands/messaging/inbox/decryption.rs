use crate::app_state::NetworkState;
use crate::signal_store::SqliteSignalStore;
use libsignal_protocol::{
    CiphertextMessage, DeviceId, ProtocolAddress, message_decrypt,
};
use tauri::AppHandle;
use rand::{SeedableRng, rngs::StdRng};

pub async fn internal_signal_decrypt(
    app: AppHandle,
    net_state: &NetworkState,
    sender: &str,
    payload: &[u8],
) -> Result<Option<String>, String> {
    if payload.len() < 1 {
        return Ok(None);
    }

    let message_type = payload[0];
    let message_body = &payload[1..];

    let ciphertext = match message_type {
        2 => CiphertextMessage::PreKeySignalMessage(
            libsignal_protocol::PreKeySignalMessage::try_from(message_body)
                .map_err(|e| format!("Invalid PreKeySignalMessage: {}", e))?,
        ),
        3 => CiphertextMessage::SignalMessage(
            libsignal_protocol::SignalMessage::try_from(message_body)
                .map_err(|e| format!("Invalid SignalMessage: {}", e))?,
        ),
        _ => return Err(format!("Unsupported signal message type: {}", message_type)),
    };

    let remote_address = ProtocolAddress::new(
        sender.to_string(),
        DeviceId::try_from(1u32).expect("valid ID"),
    );

    let own_hash = net_state
        .identity_hash
        .lock()
        .map_err(|_| "Net lock poisoned")?
        .clone()
        .ok_or("Identity not established")?;
    let local_address = ProtocolAddress::new(
        own_hash,
        DeviceId::try_from(1u32).expect("valid ID"),
    );

    let app_clone = app.clone();
    let remote_addr_sync = remote_address.clone();
    let local_addr_sync = local_address.clone();
    
    let res = tauri::async_runtime::spawn_blocking(move || {
        let mut store = SqliteSignalStore::new(app_clone);
        let mut rng = StdRng::from_os_rng();
        tauri::async_runtime::block_on(async {
            message_decrypt(
                &ciphertext,
                &remote_addr_sync,
                &local_addr_sync,
                &mut store.clone(),
                &mut store.clone(),
                &mut store.clone(),
                &store.clone(),
                &mut store,
                &mut rng,
            )
            .await
        })
    })
    .await
    .map_err(|e| e.to_string())?;

    match res {
        Ok(plaintext) => {
            let s = String::from_utf8(plaintext).map_err(|e| format!("Invalid UTF-8: {}", e))?;
            Ok(Some(s))
        }
        Err(e) => Err(format!("Signal decryption failed: {}", e)),
    }
}
