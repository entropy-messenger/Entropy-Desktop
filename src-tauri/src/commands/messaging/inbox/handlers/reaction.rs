use crate::app_state::DbState;
use rusqlite::params;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};

pub async fn handle_reaction(
    app: AppHandle,
    sender: String,
    decrypted_json: Value,
) -> Result<(), String> {
    let target_msg_id = decrypted_json["targetMsgId"]
        .as_str()
        .ok_or("Missing targetMsgId")?
        .to_string();
    let emoji = decrypted_json["emoji"]
        .as_str()
        .ok_or("Missing emoji")?
        .to_string();

    // Emoji strings should be at most a few bytes (even multi-codepoint sequences like 👨‍👩‍👧‍👦 are < 30 bytes)
    if emoji.is_empty() || emoji.len() > 64 {
        return Ok(());
    }

    let db_state = app.state::<DbState>();
    let conn = db_state.get_conn()?;

    let current_json: Option<String> = conn
        .query_row(
            "SELECT reactions_json FROM messages WHERE id = ?1",
            params![target_msg_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    let mut reactions: serde_json::Map<String, Value> = current_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let senders = reactions
        .entry(emoji.clone())
        .or_insert_with(|| json!([]));

    if let Some(arr) = senders.as_array_mut() {
        if let Some(pos) = arr.iter().position(|v| v.as_str() == Some(&sender)) {
            arr.remove(pos);
        } else {
            arr.push(json!(sender));
        }
        if arr.is_empty() {
            reactions.remove(&emoji);
        }
    }

    let new_reactions_json = serde_json::to_string(&reactions).unwrap_or_default();

    conn.execute(
        "UPDATE messages SET reactions_json = ?1 WHERE id = ?2",
        params![new_reactions_json, target_msg_id],
    )
    .map_err(|e| format!("Failed to persist reaction: {}", e))?;

    // msg://reaction keeps reactions off the chat list — no bubble, no unread bump
    app.emit(
        "msg://reaction",
        json!({
            "targetMsgId": target_msg_id,
            "reactions": reactions,
            "senderHash": sender,
        }),
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    Ok(())
}
