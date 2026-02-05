pub mod types;
pub mod groups;
pub mod media;
pub mod utils;

pub use types::*;
pub use groups::*;
pub use media::*;
pub use utils::*;

use rusqlite::{params, Connection};

pub fn establish_outbound_session(
    conn: &Connection,
    remote_hash: &str,
    _bundle: &serde_json::Value
) -> Result<(), String> {
    let state = SessionState {
        is_verified: false,
    };
    state.save_to_db(conn, remote_hash)?;
    Ok(())
}

pub fn ratchet_encrypt(
    _conn: &Connection,
    _remote_hash: &str,
    plaintext: &str
) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "type": 1,
        "body": plaintext,
    }))
}

pub fn ratchet_decrypt(
    _conn: &Connection,
    _remote_hash: &str,
    msg_obj: &serde_json::Value
) -> Result<String, String> {
    let body = msg_obj["body"].as_str().ok_or("Missing body")?;
    Ok(body.to_string())
}

pub fn save_pending_message(conn: &Connection, msg: &PendingMessage) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO pending_messages (id, recipient_hash, body, timestamp, retries) VALUES (?1, ?2, ?3, ?4, ?5);",
        params![msg.id, msg.recipient_hash, msg.body, msg.timestamp, msg.retries],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_pending_messages(conn: &Connection) -> Result<Vec<PendingMessage>, String> {
    let mut stmt = conn.prepare("SELECT id, recipient_hash, body, timestamp, retries FROM pending_messages;").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(PendingMessage {
            id: row.get(0)?,
            recipient_hash: row.get(1)?,
            body: row.get(2)?,
            timestamp: row.get(3)?,
            retries: row.get(4)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut msgs = Vec::new();
    for row in rows {
        msgs.push(row.map_err(|e| e.to_string())?);
    }
    Ok(msgs)
}

pub fn remove_pending_message(conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM pending_messages WHERE id = ?1;", [id]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn verify_session(
    conn: &Connection,
    remote_hash: &str,
    is_verified: bool
) -> Result<(), String> {
    let mut state = SessionState::load_from_db(conn, remote_hash)?.unwrap_or_default();
    state.is_verified = is_verified;
    state.save_to_db(conn, remote_hash)?;
    Ok(())
}

pub fn save_decrypted_message(
    conn: &rusqlite::Connection,
    peer_hash: &str,
    msg: &serde_json::Value
) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO messages (id, peer_hash, timestamp, content, sender_hash, type, is_mine, status, reply_to_id, attachment_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            msg["id"].as_str().ok_or("Missing id")?,
            peer_hash,
            msg["timestamp"].as_u64().unwrap_or(0),
            msg["content"].as_str().unwrap_or(""),
            msg["senderHash"].as_str().unwrap_or(""),
            msg["type"].as_str().unwrap_or("text"),
            if msg["isMine"].as_bool().unwrap_or(false) { 1 } else { 0 },
            msg["status"].as_str().unwrap_or("sent"),
            msg["replyTo"]["id"].as_str(),
            msg["attachment"].as_object().map(|_| serde_json::to_string(&msg["attachment"]).unwrap())
        ]
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn search_messages(
    conn: &rusqlite::Connection,
    query: &str
) -> Result<Vec<serde_json::Value>, String> {
    let mut stmt = conn.prepare(
        "SELECT id, peer_hash, timestamp, content, sender_hash, type, is_mine, status, reply_to_id, attachment_json
         FROM messages WHERE content LIKE ?1 ORDER BY timestamp DESC LIMIT 100"
    ).map_err(|e| e.to_string())?;

    let rows = stmt.query_map([format!("%{}%", query)], |row| {
        let id: String = row.get(0)?;
        let peer_hash: String = row.get(1)?;
        let timestamp: u64 = row.get(2)?;
        let content: String = row.get(3)?;
        let sender_hash: String = row.get(4)?;
        let msg_type: String = row.get(5)?;
        let is_mine: bool = row.get::<_, i32>(6)? == 1;
        let status: String = row.get(7)?;
        let reply_to_id: Option<String> = row.get(8)?;
        let attachment_json: Option<String> = row.get(9)?;
        
        let attachment: Option<serde_json::Value> = attachment_json.and_then(|s| serde_json::from_str(&s).ok());

        Ok(serde_json::json!({
            "id": id,
            "peerHash": peer_hash,
            "timestamp": timestamp,
            "content": content,
            "senderHash": sender_hash,
            "type": msg_type,
            "isMine": is_mine,
            "status": status,
            "replyTo": reply_to_id.map(|id| serde_json::json!({ "id": id })),
            "attachment": attachment
        }))
    }).map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| e.to_string())?);
    }
    Ok(results)
}
