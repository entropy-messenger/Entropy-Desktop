use crate::app_state::DbState;
use crate::commands::{DbChat, DbMessage, internal_db_save_message, internal_db_upsert_chat};
use rusqlite::params;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

pub async fn handle_group_invite(
    app: AppHandle,
    sender: String,
    decrypted_json: serde_json::Value,
    own_hash: &str,
) -> Result<(), String> {
    let gid = decrypted_json["groupId"]
        .as_str()
        .ok_or("Missing groupId")?
        .to_string();
    let name = decrypted_json["name"]
        .as_str()
        .ok_or("Missing group name")?
        .to_string();
    let members = decrypted_json["members"]
        .as_array()
        .map(|m| {
            m.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let chat = DbChat {
        address: gid.clone(),
        is_group: true,
        alias: Some(name.clone()),
        global_nickname: None,
        last_msg: Some(format!("Added to {}", name)),
        last_timestamp: Some(chrono::Utc::now().timestamp_millis()),
        unread_count: 1,
        is_archived: false,
        is_pinned: false,
        members: Some(members.clone()),
        trust_level: 0,
        is_blocked: false,
        last_sender_hash: Some(sender.clone()),
        last_status: Some("delivered".to_string()),
        is_active: true,
    };
    let db_state = app.state::<DbState>();
    internal_db_upsert_chat(&db_state, chat.clone()).await?;

    app.emit(
        "msg://group_update",
        json!({
            "groupId": gid.clone(),
            "name": name.clone(),
            "members": members.clone(),
        }),
    )
    .ok();

    let mut handled_me = false;
    if let Some(new_m_list) = decrypted_json["newMembers"].as_array() {
        for nm_val in new_m_list {
            if let Some(nm) = nm_val.as_str() {
                let sys_id = uuid::Uuid::new_v4().to_string();
                let sys_ts = chrono::Utc::now().timestamp_millis();
                let content = if nm == own_hash {
                    handled_me = true;
                    format!(
                        "You were added to the group by {}",
                        &sender[0..8.min(sender.len())]
                    )
                } else {
                    format!(
                        "{} added {}",
                        &sender[0..8.min(sender.len())],
                        &nm[0..8.min(nm.len())]
                    )
                };
                let sys_msg = DbMessage {
                    id: sys_id,
                    chat_address: gid.clone(),
                    sender_hash: sender.clone(),
                    content,
                    timestamp: sys_ts,
                    r#type: "system".to_string(),
                    status: "delivered".to_string(),
                    attachment_json: None,
                    is_starred: false,
                    is_group: true,
                    reply_to_json: None,
                };
                if internal_db_save_message(&db_state, sys_msg.clone())
                    .await
                    .is_ok()
                {
                    let _ = app.emit("msg://added", json!(sys_msg));
                }
            }
        }
    }

    if !handled_me {
        let sys_id = uuid::Uuid::new_v4().to_string();
        let sys_ts = chrono::Utc::now().timestamp_millis();
        let sys_msg = DbMessage {
            id: sys_id,
            chat_address: gid.clone(),
            sender_hash: sender.clone(),
            content: format!(
                "You were added to the group by {}",
                &sender[0..8.min(sender.len())]
            ),
            timestamp: sys_ts,
            r#type: "system".to_string(),
            status: "delivered".to_string(),
            attachment_json: None,
            is_starred: false,
            is_group: true,
            reply_to_json: None,
        };
        internal_db_save_message(&db_state, sys_msg.clone()).await?;
        app.emit("msg://added", json!(sys_msg))
            .map_err(|e: tauri::Error| e.to_string())?;
    }

    app.emit(
        "msg://invite",
        json!({
            "groupId": gid,
            "name": name,
            "members": members,
            "lastMsg": format!("Added to {}", name),
            "lastTimestamp": chrono::Utc::now().timestamp_millis()
        }),
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    Ok(())
}

pub async fn handle_group_leave(
    app: AppHandle,
    sender: String,
    decrypted_json: serde_json::Value,
) -> Result<(), String> {
    let gid = decrypted_json["groupId"]
        .as_str()
        .ok_or("Missing groupId")?
        .to_string();
    let leaver = decrypted_json["member"]
        .as_str()
        .ok_or("Missing member")?
        .to_string();
    let db_state = app.state::<DbState>();

    let msg_id = uuid::Uuid::new_v4().to_string();
    let timestamp = chrono::Utc::now().timestamp_millis();
    let sys_msg = DbMessage {
        id: msg_id,
        chat_address: gid.clone(),
        sender_hash: sender.clone(),
        content: format!("{} left the group", &leaver[0..8]),
        timestamp,
        r#type: "system".to_string(),
        status: "delivered".to_string(),
        attachment_json: None,
        is_starred: false,
        is_group: true,
        reply_to_json: None,
    };
    internal_db_save_message(&db_state, sys_msg.clone()).await?;
    app.emit("msg://added", json!(sys_msg))
        .map_err(|e: tauri::Error| e.to_string())?;

    app.emit(
        "msg://group_leave",
        json!({ "groupId": gid, "member": leaver }),
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    Ok(())
}

pub async fn handle_group_update(
    app: AppHandle,
    sender: String,
    decrypted_json: serde_json::Value,
    own_hash: &str,
) -> Result<(), String> {
    let gid = decrypted_json["groupId"]
        .as_str()
        .ok_or("Missing groupId")?
        .to_string();
    let group_name = decrypted_json["name"].as_str();
    let db_state = app.state::<DbState>();

    if let Some(new_name) = group_name {
        let sys_id = uuid::Uuid::new_v4().to_string();
        let sys_ts = chrono::Utc::now().timestamp_millis();
        let sys_msg = DbMessage {
            id: sys_id,
            chat_address: gid.clone(),
            sender_hash: sender.clone(),
            content: format!(
                "{} changed the group name to \"{}\"",
                &sender[0..8],
                new_name
            ),
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

    {
        let mut system_messages = Vec::new();
        let m_strings: Vec<String> = if let Some(members_val) = decrypted_json["members"].as_array()
        {
            members_val
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        } else {
            Vec::new()
        };

        if let Ok(conn) = db_state.get_conn() {
            // Detection of changes in group membership
            if let Some(new_members) = decrypted_json["newMembers"].as_array() {
                for nm_val in new_members {
                    if let Some(m) = nm_val.as_str() {
                        if m == own_hash {
                            continue;
                        }
                        let content = if m == sender {
                            format!("{} joined the group", &m[0..8.min(m.len())])
                        } else {
                            format!(
                                "{} added {}",
                                &sender[0..8.min(sender.len())],
                                &m[0..8.min(m.len())]
                            )
                        };
                        system_messages.push(content);
                    }
                }
            } else if !m_strings.is_empty() {
                let mut current_m = Vec::new();
                if let Ok(mut stmt) =
                    conn.prepare("SELECT member_hash FROM chat_members WHERE chat_address = ?1")
                    && let Ok(rows) =
                        stmt.query_map(params![&gid], |row: &rusqlite::Row| row.get::<_, String>(0))
                {
                    for m in rows.flatten() {
                        current_m.push(m);
                    }
                }
                for m in &m_strings {
                    if !current_m.contains(m) && m != own_hash {
                        let content = if m == &sender {
                            format!("{} joined the group", &m[0..8.min(m.len())])
                        } else {
                            format!(
                                "{} added {}",
                                &sender[0..8.min(sender.len())],
                                &m[0..8.min(m.len())]
                            )
                        };
                        system_messages.push(content);
                    }
                }
            }

            if !m_strings.is_empty() {
                let _ = conn.execute(
                    "DELETE FROM chat_members WHERE chat_address = ?1",
                    params![gid],
                );
                for m in m_strings {
                    let _ = conn.execute("INSERT OR IGNORE INTO chat_members (chat_address, member_hash) VALUES (?1, ?2)", params![gid, m]);
                }
            }
            if let Some(name) = group_name {
                let _ = conn.execute(
                    "UPDATE chats SET alias = ?1 WHERE address = ?2",
                    params![name, gid],
                );
            }
        }

        for content in system_messages {
            let sys_id = uuid::Uuid::new_v4().to_string();
            let sys_ts = chrono::Utc::now().timestamp_millis();
            let sys_msg = DbMessage {
                id: sys_id,
                chat_address: gid.clone(),
                sender_hash: sender.clone(),
                content,
                timestamp: sys_ts,
                r#type: "system".to_string(),
                status: "delivered".to_string(),
                attachment_json: None,
                is_starred: false,
                is_group: true,
                reply_to_json: None,
            };
            if internal_db_save_message(&db_state, sys_msg.clone())
                .await
                .is_ok()
            {
                let _ = app.emit("msg://added", json!(sys_msg));
            }
        }
    }

    app.emit(
        "msg://group_update",
        json!({ "groupId": gid, "name": group_name }),
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    Ok(())
}
