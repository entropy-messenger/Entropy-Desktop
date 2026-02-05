use serde::{Serialize, Deserialize};
use rusqlite::{params, Connection};
use ed25519_dalek::{SigningKey, Signer};

#[derive(Serialize, Deserialize, Clone)]
pub struct ProtocolIdentity {
    pub registration_id: u32,
    pub alias: String,
    pub public_key: Vec<u8>,
    pub private_key: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SessionState {
    pub is_verified: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MediaKeyBundle {
    pub file_name: String,
    pub file_type: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GroupState {
    pub group_id: String,
    pub members: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PendingMessage {
    pub id: String,
    pub recipient_hash: String,
    pub body: String, 
    pub timestamp: u64,
    pub retries: u32,
}

pub fn init_database(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS vault (key TEXT PRIMARY KEY, value TEXT);",
        [],
    ).map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS pending_messages (id TEXT PRIMARY KEY, recipient_hash TEXT, body TEXT, timestamp INTEGER, retries INTEGER);",
        [],
    ).map_err(|e| e.to_string())?;
    
    conn.execute(
        "CREATE TABLE IF NOT EXISTS groups (group_id TEXT PRIMARY KEY, state TEXT);",
        [],
    ).map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            peer_hash TEXT,
            timestamp INTEGER,
            content TEXT,
            sender_hash TEXT,
            type TEXT,
            is_mine INTEGER,
            status TEXT,
            reply_to_id TEXT,
            attachment_json TEXT
        );",
        [],
    ).map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_peer ON messages(peer_hash);",
        [],
    ).map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS blobs (id TEXT PRIMARY KEY, data BLOB);",
        [],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

impl ProtocolIdentity {
    pub fn save_to_db(&self, conn: &Connection) -> Result<(), String> {
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO vault (key, value) VALUES ('protocol_identity', ?1);",
            params![json],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_from_db(conn: &Connection) -> Result<Option<Self>, String> {
        let mut stmt = conn.prepare("SELECT value FROM vault WHERE key = 'protocol_identity';").map_err(|e| e.to_string())?;
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let json: String = row.get(0).map_err(|e| e.to_string())?;
            let identity: ProtocolIdentity = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            Ok(Some(identity))
        } else {
            Ok(None)
        }
    }

    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, String> {
        let signing_key = SigningKey::from_bytes(
            self.private_key.as_slice().try_into().map_err(|_| "Invalid private key")?
        );
        let signature = signing_key.sign(message);
        Ok(signature.to_bytes().to_vec())
    }
}

impl SessionState {
    pub fn save_to_db(&self, conn: &Connection, peer_hash: &str) -> Result<(), String> {
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO vault (key, value) VALUES (?1, ?2);",
            params![format!("session_{}", peer_hash), json],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_from_db(conn: &Connection, peer_hash: &str) -> Result<Option<Self>, String> {
        let mut stmt = conn.prepare("SELECT value FROM vault WHERE key = ?1;").map_err(|e| e.to_string())?;
        let mut rows = stmt.query([format!("session_{}", peer_hash)]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let json: String = row.get(0).map_err(|e| e.to_string())?;
            let state: SessionState = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }
}

impl GroupState {
    pub fn save_to_db(&self, conn: &Connection) -> Result<(), String> {
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO groups (group_id, state) VALUES (?1, ?2);",
            params![self.group_id, json],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_from_db(conn: &Connection, group_id: &str) -> Result<Option<Self>, String> {
        let mut stmt = conn.prepare("SELECT state FROM groups WHERE group_id = ?1;").map_err(|e| e.to_string())?;
        let mut rows = stmt.query([group_id]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let json: String = row.get(0).map_err(|e| e.to_string())?;
            let state: GroupState = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }
}

pub fn generate_new_identity() -> ProtocolIdentity {
    let mut rng = rand::thread_rng();
    use rand::RngCore;
    
    let mut secret = [0u8; 32];
    rng.fill_bytes(&mut secret);
    let signing_key = SigningKey::from_bytes(&secret);
    let public_key = signing_key.verifying_key();
    
    ProtocolIdentity {
        registration_id: (rng.next_u32() % 16383) + 1,
        alias: "User".to_string(),
        public_key: public_key.to_bytes().to_vec(),
        private_key: signing_key.to_bytes().to_vec(),
    }
}
