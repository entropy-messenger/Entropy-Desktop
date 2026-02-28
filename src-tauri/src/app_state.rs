use rusqlite::Connection;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::audio::AudioRecorder;

use std::collections::VecDeque;

pub struct DbState {
    pub conn: Mutex<Option<Connection>>,
    pub media_key: Mutex<Option<Vec<u8>>>,
    pub profile: Mutex<String>,
}

pub struct PacedMessage {
    pub msg: Message,
    pub is_media: bool,
}

pub struct NetworkState {
    pub queue: Mutex<VecDeque<PacedMessage>>,
    pub sender: Mutex<Option<mpsc::UnboundedSender<PacedMessage>>>, 
    pub cancel: Mutex<Option<tokio_util::sync::CancellationToken>>,
    pub response_channels: Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<serde_json::Value>>>,
    pub is_authenticated: Mutex<bool>,
    pub identity_hash: Mutex<Option<String>>,
    pub session_token: Mutex<Option<String>>,
}

pub struct AudioState {
    pub recorder: Mutex<AudioRecorder>,
}


