use rusqlite::Connection;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message;

pub struct DbState {
    pub conn: Mutex<Option<Connection>>,
}

pub struct NetworkState {
    pub sender: Mutex<Option<mpsc::UnboundedSender<Message>>>,
}
