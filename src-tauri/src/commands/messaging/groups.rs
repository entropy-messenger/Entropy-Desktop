use crate::app_state::{DbState, NetworkState};
use crate::commands::{
    DbChat, DbMessage, internal_db_save_message, internal_db_upsert_chat, internal_send_to_network,
    internal_signal_encrypt,
};
use hex;
use rand;
use rusqlite::params;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};
use uuid;

#[tauri::command]
pub async fn create_group(
    app: AppHandle,
    db_state: State<'_, DbState>,
    state: State<'_, NetworkState>,
    name: String,
    members: Vec<String>
) -> Result<String, String> {
    let group_id = uuid::Uuid::new_v4().to_string();
    let id_hash = state
        .identity_hash
        .lock()
        .map_err(|_| "Network state poisoned")?
        .clone()
        .ok_or("No identity")?;

            let mut all_members = members
                .iter()
                .map(|m| m.to_lowercase())
                .collect::<Vec<String>>();
            if !all_members.contains(&id_hash.to_lowercase()) {
                all_members.push(id_hash.to_lowercase());
            }
            all_members.sort();
            all_members.dedup();

            if all_members.len() > 16 {
                return Err("Group too large (max 16 members)".into());
            }

            let chat = DbChat {
                address: group_id.clone(),
                is_group: true,
                alias: Some(name.clone()),
                global_nickname: None,
                last_msg: None,
                last_timestamp: None,
                unread_count: 0,
                is_archived: false,
                is_pinned: false,
                members: Some(all_members.clone()),
                trust_level: 1,
                is_blocked: false,
                last_sender_hash: None,
                last_status: None,
                is_active: true,
            };
            internal_db_upsert_chat(&db_state, chat).await?;

            // group seed
            let dist_msg = hex::encode(rand::random::<[u8; 16]>());
            let invite = json!({
                "type": "group_invite",
                "groupId": group_id,
                "name": name,
                "members": &all_members,
                "newMembers": members, // In create_group, the input members are the 'new' ones
                "distribution": dist_msg
            });

            // adder status message
            for m in &all_members {
                if m == &id_hash {
                    continue;
                }
                let sys_id = uuid::Uuid::new_v4().to_string();
                let sys_ts = chrono::Utc::now().timestamp_millis();
                let sys_msg = DbMessage {
                    id: sys_id,
                    chat_address: group_id.clone(),
                    sender_hash: id_hash.clone(),
                    content: format!("You added {}", if m.len() > 8 { &m[0..8] } else { m }),
                    timestamp: sys_ts,
                    r#type: "system".to_string(),
                    status: "delivered".to_string(),
                    attachment_json: None,
                    is_starred: false,
                    is_group: true,
                    reply_to_json: None,
                };
                let _ = internal_db_save_message(&db_state, sys_msg.clone()).await;
                let _ = app.emit("msg://added", json!(sys_msg));
            }

            // multicast invite
            let invite_str = invite.to_string();
            for member in members {
                if member == id_hash {
                    continue;
                }
                if let Ok(ciphertext) =
                    internal_signal_encrypt(app.clone(), &state, &member, invite_str.clone()).await
                {
                    let _ = internal_send_to_network(
                        app.clone(),
                        &state,
                        Some(member),
                        None,
                        None,
                        Some(ciphertext.to_string().into_bytes()),
                        true,
                        false,
                        None,
                        false,
                    )
                    .await;
                }
            }

            Ok(group_id)
}

#[tauri::command]
pub async fn add_to_group(
    app: AppHandle,
    db_state: State<'_, DbState>,
    state: State<'_, NetworkState>,
    group_id: String,
    new_members: Vec<String>,
) -> Result<(), String> {
    let id_hash = state.identity_hash.lock().map_err(|_| "Network state poisoned")?.clone().ok_or("No identity")?;

            let current_members = {
                let mut m = Vec::new();
                let conn = db_state.get_conn()?;
                let mut stmt = conn.prepare("SELECT member_hash FROM chat_members WHERE chat_address = ?1")
                    .map_err(|e| e.to_string())?;
                let rows = stmt.query_map([&group_id], |row| row.get::<_, String>(0))
                    .map_err(|e| e.to_string())?;
                for ma in rows.flatten() { m.push(ma); }
                m
            };

            let mut all_members = current_members.clone();
            for nm in &new_members {
                if !all_members.contains(nm) { all_members.push(nm.clone()); }
            }

            if all_members.len() > 16 {
                return Err("Group reached its limit (max 16 members)".into());
            }

            // commit members
            let _ = internal_db_save_members(&db_state, &group_id, all_members.clone()).await;

            // member distribution
            let dist_msg = hex::encode(rand::random::<[u8; 16]>());
            let group_name = {
                let conn = db_state.get_conn()?;
                conn.query_row("SELECT alias FROM chats WHERE address = ?1", params![group_id], |r| r.get::<_, Option<String>>(0)).ok().flatten()
            }.unwrap_or_else(|| "Group".to_string());

            let invite = json!({ "type": "group_invite", "groupId": group_id, "name": group_name, "members": &all_members, "newMembers": &new_members, "distribution": dist_msg });
            let update = json!({ "type": "group_update", "groupId": group_id, "members": &all_members, "newMembers": &new_members });

            // transition status message
            for m in &new_members {
                if current_members.contains(m) {
                    continue;
                }
                let sys_id = uuid::Uuid::new_v4().to_string();
                let sys_ts = chrono::Utc::now().timestamp_millis();
                let sys_msg = DbMessage {
                    id: sys_id,
                    chat_address: group_id.clone(),
                    sender_hash: id_hash.clone(),
                    content: format!("You added {}", if m.len() > 8 { &m[0..8] } else { m }),
                    timestamp: sys_ts,
                    r#type: "system".to_string(),
                    status: "delivered".to_string(),
                    attachment_json: None,
                    is_starred: false,
                    is_group: true,
                    reply_to_json: None,
                };
                let _ = internal_db_save_message(&db_state, sys_msg.clone()).await;
                let _ = app.emit("msg://added", json!(sys_msg));
            }

            let invite_str = invite.to_string();
            let update_str = update.to_string();

            // Invite new ones
            for member in &new_members {
                if let Ok(ciphertext) = internal_signal_encrypt(app.clone(), &state, member, invite_str.clone()).await {
                    let _ = internal_send_to_network(app.clone(), &state, Some(member.clone()), None, None, Some(ciphertext.to_string().into_bytes()), true, false, None, false).await;
                }
            }
            // update existing members
            for member in &current_members {
                if member == &id_hash || new_members.contains(member) { continue; }
                if let Ok(ciphertext) = internal_signal_encrypt(app.clone(), &state, member, update_str.clone()).await {
                    let _ = internal_send_to_network(app.clone(), &state, Some(member.clone()), None, None, Some(ciphertext.to_string().into_bytes()), true, false, None, false).await;
                }
            }

            // trigger UI state sync
            let _ = app.emit("msg://group_update", json!({ "groupId": group_id, "members": all_members, "name": group_name }));

            Ok(())
}

#[tauri::command]
pub async fn update_group_name(
    app: AppHandle,
    db_state: State<'_, DbState>,
    state: State<'_, NetworkState>,
    group_id: String,
    new_name: String
) -> Result<(), String> {
    let id_hash = state.identity_hash.lock().map_err(|_| "Network state poisoned")?.clone().ok_or("No identity")?;

            let members = {
                let mut m = Vec::new();
                let conn = db_state.get_conn()?;
                let _ = conn.execute("UPDATE chats SET alias = ?1 WHERE address = ?2", params![new_name, group_id]);
                let mut stmt = conn.prepare("SELECT member_hash FROM chat_members WHERE chat_address = ?1")
                    .map_err(|e| e.to_string())?;
                let rows = stmt.query_map([&group_id], |row| row.get::<_, String>(0))
                    .map_err(|e| e.to_string())?;
                for ma in rows.flatten() { m.push(ma); }
                m
            };

            let update = json!({ "type": "group_update", "groupId": group_id, "name": new_name, "members": &members });
            let update_str = update.to_string();

            for member in &members {
                if member == &id_hash { continue; }
                if let Ok(ciphertext) =
                    internal_signal_encrypt(app.clone(), &state, member, update_str.clone()).await
                {
                    let _ = internal_send_to_network(app.clone(), &state, Some(member.clone()), None, None, Some(ciphertext.to_string().into_bytes()), true, false, None, false).await;
                }
            }

            // trigger UI state sync
            let _ = app.emit("msg://group_update", json!({ "groupId": group_id, "name": new_name, "members": members }));

            Ok(())
}

#[tauri::command]
pub async fn leave_group(
    app: AppHandle,
    db_state: State<'_, DbState>,
    state: State<'_, NetworkState>,
    group_id: String
) -> Result<(), String> {
    let id_hash = state
        .identity_hash
        .lock()
        .map_err(|_| "Network state poisoned")?
        .clone()
        .ok_or("No identity")?;

            let members = {
                let mut m = Vec::new();
                let conn = db_state.get_conn()?;
                let mut stmt = conn
                    .prepare("SELECT member_hash FROM chat_members WHERE chat_address = ?1")
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([&group_id], |row| row.get::<_, String>(0))
                    .map_err(|e| e.to_string())?;
                for ma in rows.flatten() {
                    m.push(ma);
                }
                m
            };

            let payload = json!({ "type": "group_leave", "groupId": group_id, "member": id_hash });
            let payload_str = payload.to_string();

            for member in members {
                if member == id_hash {
                    continue;
                }
                if let Ok(ciphertext) =
                    internal_signal_encrypt(app.clone(), &state, &member, payload_str.clone()).await
                {
                    let _ = internal_send_to_network(
                        app.clone(),
                        &state,
                        Some(member),
                        None,
                        None,
                        Some(ciphertext.to_string().into_bytes()),
                        true,
                        false,
                        None,
                        false,
                    )
                    .await;
                }
            }

            // local cleanup
            {
                let conn = db_state.get_conn()?;
                let _ = conn.execute(
                    "UPDATE chats SET is_active = 0 WHERE address = ?1",
                    [&group_id],
                );
                let _ = conn.execute(
                    "DELETE FROM chat_members WHERE chat_address = ?1",
                    [&group_id],
                );
                let _ = conn.execute("DELETE FROM messages WHERE chat_address = ?1", [&group_id]);
            }

            Ok(())
}

pub async fn internal_db_save_members(
    state: &DbState,
    chat_address: &str,
    members: Vec<String>,
) -> Result<(), String> {
    let conn = state.get_conn()?;

    let _ = conn.execute(
        "DELETE FROM chat_members WHERE chat_address = ?1",
        params![chat_address],
    );
    for m in members {
        let _ = conn.execute(
            "INSERT OR IGNORE INTO chat_members (chat_address, member_hash) VALUES (?1, ?2)",
            params![chat_address, m],
        );
    }
    Ok(())
}
