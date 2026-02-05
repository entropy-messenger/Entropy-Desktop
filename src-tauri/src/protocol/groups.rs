use serde_json::json;
use rusqlite::Connection;
use crate::protocol::types::GroupState;

pub fn create_group_sender_key() -> String {
    "sender_key".to_string()
}

pub fn create_group_distribution_message(state: &GroupState) -> Result<serde_json::Value, String> {
    Ok(json!({
        "type": "group_sender_key_distribution",
        "group_id": state.group_id,
    }))
}

pub fn group_encrypt(
    _conn: &Connection,
    _state: &mut GroupState,
    plaintext: &str
) -> Result<serde_json::Value, String> {
    Ok(json!({
        "body": plaintext,
    }))
}

pub fn group_decrypt(
    _state: &mut GroupState,
    _sender_hash: &str,
    msg_obj: &serde_json::Value
) -> Result<String, String> {
    let body = msg_obj["body"].as_str().ok_or("No body")?;
    Ok(body.to_string())
}
