use crate::app_state::NetworkState;
use crate::signal_store::SqliteSignalStore;
use libsignal_protocol::{
    CiphertextMessage, CiphertextMessageType, DeviceId, ProtocolAddress, SignalProtocolError,
    message_decrypt,
};
use rand::SeedableRng;
use rand::rngs::StdRng;
use tauri::{AppHandle, Manager};

pub async fn internal_signal_decrypt(
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

    String::from_utf8(ptext).map_err(|e: std::string::FromUtf8Error| e.to_string())
}
