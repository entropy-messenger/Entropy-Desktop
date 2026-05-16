use super::super::OutgoingReaction;
use crate::app_state::{DbState, NetworkState};
use crate::commands::{internal_send_to_network, internal_signal_encrypt};
use rusqlite::params;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

pub async fn process_outgoing_reaction(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    payload: OutgoingReaction,
) -> Result<(), String> {
    let own_id = {
        let lock = net_state
            .identity_hash
            .lock()
            .map_err(|_| "Network state poisoned")?;
        lock.clone().ok_or("Not authenticated")?
    };

    {
        let conn = db_state.get_conn()?;
        let current_json: Option<String> = conn
            .query_row(
                "SELECT reactions_json FROM messages WHERE id = ?1",
                params![payload.target_msg_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        let mut reactions: serde_json::Map<String, serde_json::Value> = current_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        let senders = reactions
            .entry(payload.emoji.clone())
            .or_insert_with(|| json!([]));

        if let Some(arr) = senders.as_array_mut() {
            if let Some(pos) = arr.iter().position(|v| v.as_str() == Some(&own_id)) {
                arr.remove(pos);
            } else {
                arr.push(json!(own_id.clone()));
            }
            if arr.is_empty() {
                reactions.remove(&payload.emoji);
            }
        }

        let new_json = serde_json::to_string(&reactions).unwrap_or_default();
        conn.execute(
            "UPDATE messages SET reactions_json = ?1 WHERE id = ?2",
            params![new_json, payload.target_msg_id],
        )
        .map_err(|e| format!("Failed to save own reaction: {}", e))?;

        app.emit(
            "msg://reaction",
            json!({
                "targetMsgId": payload.target_msg_id,
                "reactions": reactions,
                "senderHash": own_id,
            }),
        )
        .map_err(|e: tauri::Error| e.to_string())?;
    }

    let signal_payload = json!({
        "type": "reaction",
        "targetMsgId": payload.target_msg_id,
        "emoji": payload.emoji,
    });

    let recipients: Vec<String> = if payload.is_group {
        payload.group_members.unwrap_or_default()
    } else {
        vec![payload.recipient.clone()]
    };

    for recipient in &recipients {
        if recipient == &own_id {
            continue;
        }
        let routing_hash = recipient.split('.').next().unwrap_or(recipient).to_string();
        if let Ok(ciphertext_obj) = internal_signal_encrypt(
            app.clone(),
            &net_state,
            recipient,
            signal_payload.to_string(),
        )
        .await
        {
            let payload_bytes = ciphertext_obj.to_string().into_bytes();
            let _ = internal_send_to_network(
                app.clone(),
                &net_state,
                Some(routing_hash),
                None,
                None,
                Some(payload_bytes),
                true,
                None,
                false,
            )
            .await;
        }
    }

    Ok(())
}
