use rusqlite::Connection;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message;

use std::collections::VecDeque;

pub struct DbState {
    pub conn: Mutex<Option<Connection>>,
    pub media_key: Mutex<Option<Vec<u8>>>,
    pub profile: Mutex<String>,
}

pub struct PacedMessage {
    pub msg: Message,
}

pub struct FragmentBuffer {
    pub total: u32,
    pub chunks: std::collections::HashMap<u32, Vec<u8>>,
    pub last_activity: std::time::Instant,
}

pub struct PendingMediaMetadata {
    pub id: String,
    pub key: String,
}

pub struct NetworkState {
    pub is_enabled: Mutex<bool>,
    pub url: Mutex<Option<String>>,
    pub proxy_url: Mutex<Option<String>>,
    pub queue: Mutex<VecDeque<PacedMessage>>,
    pub sender: Mutex<Option<mpsc::UnboundedSender<PacedMessage>>>,
    pub cancel: Mutex<Option<tokio_util::sync::CancellationToken>>,
    pub response_channels:
        Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<serde_json::Value>>>,
    pub is_authenticated: Mutex<bool>,
    pub identity_hash: Mutex<Option<String>>,
    pub session_token: Mutex<Option<String>>,
    pub halted_targets: Mutex<std::collections::HashSet<String>>,
    pub media_assembler: Mutex<std::collections::HashMap<String, FragmentBuffer>>,
    pub pending_media_links: Mutex<std::collections::HashMap<String, PendingMediaMetadata>>, // transfer_key -> (msg_id, dec_key)
    pub binary_receiver: Mutex<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    pub is_refilling: Mutex<bool>,
    pub pending_transfers: Mutex<std::collections::HashMap<u32, String>>,
}
