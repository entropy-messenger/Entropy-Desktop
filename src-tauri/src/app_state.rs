//! Synchronized Application State and Shared Resources
//!
//! This module defines the global state containers used across the Tauri command bridge.
//! Current implementation utilizes thread-safe primitives (Mutex, Arc) to manage:
//! - Persistent database connections (SQLCipher).
//! - Active network handles and pacing channels.
//! - Transient memory buffers for binary reassembly.

use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message;

use r2d2::Pool;
use std::collections::{HashMap, VecDeque};

pub struct FragmentAssembly {
    pub total: u32,
    pub chunks: HashMap<u32, Vec<u8>>,
    pub last_activity: std::time::Instant,
}

impl FragmentAssembly {
    pub fn new(total: u32) -> Self {
        Self {
            total,
            chunks: HashMap::new(),
            last_activity: std::time::Instant::now(),
        }
    }
}

pub struct RusqliteManager {
    pub path: std::path::PathBuf,
    pub flags: rusqlite::OpenFlags,
}

impl r2d2::ManageConnection for RusqliteManager {
    type Connection = rusqlite::Connection;
    type Error = rusqlite::Error;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        rusqlite::Connection::open_with_flags(&self.path, self.flags)
    }

    fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        conn.execute_batch("SELECT 1")
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}

pub struct DbState {
    pub pool: Mutex<Option<Pool<RusqliteManager>>>,
    pub media_key: Mutex<Option<Vec<u8>>>,
    pub profile: Mutex<String>,
    pub media_proxy_port: Mutex<Option<u16>>,
}

#[derive(Debug)]
pub struct SqlCipherCustomizer {
    pub key: String,
}

impl r2d2::CustomizeConnection<rusqlite::Connection, rusqlite::Error> for SqlCipherCustomizer {
    fn on_acquire(&self, conn: &mut rusqlite::Connection) -> Result<(), rusqlite::Error> {
        if !self.key.is_empty() {
            {
                let escaped = self.key.replace("'", "''");
                conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", escaped))?;
            }
        }
        Ok(())
    }
}

impl DbState {
    pub fn get_conn(&self) -> Result<r2d2::PooledConnection<RusqliteManager>, String> {
        let lock = self.pool.lock().map_err(|_| "DB Pool lock poisoned")?;
        let pool = lock
            .as_ref()
            .ok_or("Database not initialized. Please unlock your vault.")?;
        pool.get()
            .map_err(|e| format!("Failed to get DB connection from pool: {}", e))
    }
}

pub struct PacedMessage {
    pub msg: Message,
}

pub struct NetworkState {
    pub is_enabled: Mutex<bool>,
    pub url: Mutex<Option<String>>,
    pub proxy_url: Mutex<Option<String>>,
    pub queue: Mutex<VecDeque<PacedMessage>>,
    pub sender: Mutex<Option<mpsc::Sender<PacedMessage>>>,
    pub cancel: Mutex<Option<tokio_util::sync::CancellationToken>>,
    pub response_channels:
        Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<serde_json::Value>>>,
    pub is_authenticated: Mutex<bool>,
    pub identity_hash: Mutex<Option<String>>,
    pub session_token: Mutex<Option<String>>,
    pub binary_receiver: Mutex<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    pub is_refilling: Mutex<bool>,
    pub jailed_until: Mutex<Option<tokio::time::Instant>>,
    pub pending_transfers: Mutex<std::collections::HashMap<u32, String>>,
    pub fragment_assembler: Mutex<HashMap<String, FragmentAssembly>>,
}
