use crate::app_state::{DbState, NetworkState};
use crate::commands::{internal_send_to_network, internal_signal_encrypt};
use serde_json::json;
use tauri::{AppHandle, State};

pub async fn send_typing_status(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    is_typing: bool,
) -> Result<(), String> {
    {
        if let Ok(conn) = db_state.get_conn() {
            let is_group = conn
                .query_row(
                    "SELECT is_group FROM chats WHERE address = ?1",
                    [&peer_hash],
                    |r: &rusqlite::Row| r.get::<_, i32>(0),
                )
                .unwrap_or(0)
                != 0;
            if is_group {
                return Ok(());
            }
        }
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let message =
        json!({ "type": "typing", "isTyping": is_typing, "timestamp": timestamp }).to_string();
    if let Ok(encrypted) =
        internal_signal_encrypt(app.clone(), &net_state, &peer_hash, message).await
    {
        let _ = internal_send_to_network(
            app.clone(),
            &net_state,
            Some(peer_hash.clone()),
            None,
            None,
            Some(encrypted.to_string().into_bytes()),
            true,
            false,
            None,
            true,
        )
        .await;
    }
    Ok(())
}

pub async fn send_receipt(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    msg_ids: Vec<String>,
    status: String,
) -> Result<(), String> {
    if let Ok(conn) = db_state.get_conn() {
        let is_group = conn
            .query_row(
                "SELECT is_group FROM chats WHERE address = ?1",
                [&peer_hash],
                |r: &rusqlite::Row| r.get::<_, i32>(0),
                )
            .unwrap_or(0)
            != 0;
        if is_group {
            return Ok(());
        }
    }

    let message = json!({ "type": "receipt", "msgIds": msg_ids, "status": status }).to_string();
    if let Ok(encrypted) =
        internal_signal_encrypt(app.clone(), &net_state, &peer_hash, message).await
    {
        let _ = internal_send_to_network(
            app.clone(),
            &net_state,
            Some(peer_hash.clone()),
            None,
            None,
            Some(encrypted.to_string().into_bytes()),
            true,
            false,
            None,
            true,
        )
        .await;
    }
    Ok(())
}

pub async fn send_profile_update(
    app: AppHandle,
    _db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    alias: Option<String>,
) -> Result<(), String> {
    let message = json!({
        "type": "profile_update",
        "alias": alias,
        "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }).to_string();

    if let Ok(encrypted) =
        internal_signal_encrypt(app.clone(), &net_state, &peer_hash, message).await
    {
        let payload_bytes = encrypted.to_string().into_bytes();
        let _ = internal_send_to_network(
            app.clone(),
            &net_state,
            Some(peer_hash.clone()),
            None,
            None,
            Some(payload_bytes),
            true,
            false,
            None,
            false,
        )
        .await;
    }
    Ok(())
}
