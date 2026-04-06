use crate::app_state::DbState;
use crate::commands::{vault_delete_media, DbChat, DbContact, DbMessage};
use rusqlite::params;
use serde_json::{json, Value};
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn db_save_message(state: State<'_, DbState>, msg: DbMessage) -> Result<(), String> {
    internal_db_save_message(&state, msg).await
}

#[tauri::command]
pub async fn db_get_contacts(state: State<'_, DbState>) -> Result<Vec<DbContact>, String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    let mut stmt = conn
        .prepare("SELECT hash, alias, is_blocked, trust_level FROM contacts")
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(DbContact {
                hash: row.get(0)?,
                alias: row.get(1)?,
                is_blocked: row.get::<_, i32>(2)? != 0,
                trust_level: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut contacts = Vec::new();
    for r in rows {
        contacts.push(r.map_err(|e| e.to_string())?);
    }
    Ok(contacts)
}

#[tauri::command]
pub async fn db_set_contact_blocked(
    state: State<'_, DbState>,
    hash: String,
    is_blocked: bool,
) -> Result<(), String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    conn.execute(
        "INSERT INTO contacts (hash, is_blocked) VALUES (?1, ?2)
         ON CONFLICT(hash) DO UPDATE SET is_blocked = excluded.is_blocked",
        params![hash, is_blocked as i32],
    )
    .map_err(|e| format!("Failed to update block status: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn db_set_contact_nickname(
    state: State<'_, DbState>,
    hash: String,
    alias: Option<String>,
) -> Result<(), String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    conn.execute(
        "INSERT INTO contacts (hash, alias) VALUES (?1, ?2)
         ON CONFLICT(hash) DO UPDATE SET alias = excluded.alias",
        params![hash, alias],
    )
    .map_err(|e| format!("Failed to update alias: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn db_get_messages(
    state: State<'_, DbState>,
    chat_address: String,
    limit: u32,
    offset: u32,
    include_attachments: bool,
) -> Result<Vec<DbMessage>, String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    let sql = if include_attachments {
        "SELECT id, chat_address, sender_hash, content, timestamp, type, status, attachment_json, is_starred, is_group, reply_to_json
         FROM messages WHERE chat_address = ?1 ORDER BY timestamp DESC LIMIT ?2 OFFSET ?3"
    } else {
        "SELECT id, chat_address, sender_hash, content, timestamp, type, status, NULL, is_starred, is_group, reply_to_json
         FROM messages WHERE chat_address = ?1 ORDER BY timestamp DESC LIMIT ?2 OFFSET ?3"
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![chat_address, limit, offset], |row| {
            Ok(DbMessage {
                id: row.get(0)?,
                chat_address: row.get(1)?,
                sender_hash: row.get(2)?,
                content: row.get(3)?,
                timestamp: row.get(4)?,
                r#type: row.get(5)?,
                status: row.get(6)?,
                attachment_json: row.get(7)?,
                is_starred: row.get::<_, i32>(8)? != 0,
                is_group: row.get::<_, i32>(9)? != 0,
                reply_to_json: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut msgs = Vec::new();
    for r in rows {
        msgs.push(r.map_err(|e| e.to_string())?);
    }
    msgs.reverse();
    Ok(msgs)
}

#[tauri::command]
pub async fn db_search_messages(
    state: State<'_, DbState>,
    query: String,
) -> Result<Vec<DbMessage>, String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    let mut stmt = conn.prepare(
        "SELECT m.id, m.chat_address, m.sender_hash, m.content, m.timestamp, m.type, m.status, NULL, m.is_starred, m.is_group, m.reply_to_json
         FROM message_search ms
         JOIN messages m ON ms.rowid = m.rowid
         WHERE message_search MATCH ?1
         ORDER BY m.timestamp DESC LIMIT 100"
    ).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![query], |row| {
            Ok(DbMessage {
                id: row.get(0)?,
                chat_address: row.get(1)?,
                sender_hash: row.get(2)?,
                content: row.get(3)?,
                timestamp: row.get(4)?,
                r#type: row.get(5)?,
                status: row.get(6)?,
                attachment_json: row.get(7)?,
                is_starred: row.get::<_, i32>(8)? != 0,
                is_group: row.get::<_, i32>(9)? != 0,
                reply_to_json: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut msgs = Vec::new();
    for r in rows {
        msgs.push(r.map_err(|e| e.to_string())?);
    }
    Ok(msgs)
}

#[tauri::command]
pub async fn db_update_messages(
    state: State<'_, DbState>,
    ids: Vec<String>,
    status: Option<String>,
    is_starred: Option<bool>,
    attachment_json: Option<String>,
) -> Result<(), String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    for id in ids {
        if let Some(s) = &status {
            conn.execute(
                "UPDATE messages SET status = ?1 WHERE id = ?2",
                params![s, id],
            )
            .map_err(|e| e.to_string())?;

            let actual_chat: Option<String> = conn
                .query_row(
                    "SELECT chat_address FROM messages WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .ok();

            if let Some(addr) = actual_chat {
                let _ = conn.execute(
                    "UPDATE chats SET last_status = ?1 WHERE address = ?2",
                    params![s, addr],
                );
            }
        }

        if let Some(starred) = is_starred {
            conn.execute(
                "UPDATE messages SET is_starred = ?1 WHERE id = ?2",
                params![starred as i32, id],
            )
            .map_err(|e| e.to_string())?;
        }

        if let Some(json) = &attachment_json {
            conn.execute(
                "UPDATE messages SET attachment_json = ?1 WHERE id = ?2",
                params![json, id],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn db_upsert_chat(state: State<'_, DbState>, chat: DbChat) -> Result<(), String> {
    internal_db_upsert_chat(&state, chat).await
}

#[tauri::command]
pub async fn db_get_chats(state: State<'_, DbState>) -> Result<Vec<DbChat>, String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    let mut stmt = conn.prepare(
        "SELECT 
            c.address, c.is_group, c.alias, c.last_msg, c.last_timestamp, 
            c.unread_count, c.is_archived, c.last_sender_hash, c.last_status, c.is_pinned,
            COALESCE((SELECT trust_level FROM signal_identities_remote WHERE address LIKE c.address || ':%' LIMIT 1), 1) as trust_level,
            COALESCE((SELECT is_blocked FROM contacts WHERE hash = c.address), 0) != 0 as is_blocked,
            c.is_active
        FROM chats c
        WHERE c.is_active != 0"
    ).map_err(|e| e.to_string())?;

    let chat_rows = stmt
        .query_map([], |row| {
            Ok(DbChat {
                address: row.get(0)?,
                is_group: row.get::<_, i32>(1)? != 0,
                alias: row.get(2)?,
                last_msg: row.get(3)?,
                last_timestamp: row.get(4)?,
                unread_count: row.get(5)?,
                is_archived: row.get::<_, i32>(6)? != 0,
                last_sender_hash: row.get(7)?,
                last_status: row.get(8)?,
                is_pinned: row.get::<_, i32>(9)? != 0,
                trust_level: row.get(10)?,
                is_blocked: row.get(11)?,
                is_active: row.get::<_, i32>(12)? != 0,
                members: None,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut chats = Vec::new();
    for r in chat_rows {
        let mut chat = r.map_err(|e| e.to_string())?;

        let mut m_stmt = conn
            .prepare("SELECT member_hash FROM chat_members WHERE chat_address = ?1")
            .map_err(|e| e.to_string())?;
        let m_rows = m_stmt
            .query_map([&chat.address], |m_row| m_row.get(0))
            .map_err(|e| e.to_string())?;
        let mut members = Vec::new();
        for mr in m_rows {
            members.push(mr.map_err(|e| e.to_string())?);
        }

        if !members.is_empty() {
            chat.members = Some(members);
        }
        chats.push(chat);
    }
    Ok(chats)
}

#[tauri::command]
pub async fn db_delete_messages(state: State<'_, DbState>, ids: Vec<String>) -> Result<(), String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    for id in ids {
        let _ = conn.execute("DELETE FROM pending_outbox WHERE msg_id = ?1", params![id]);
        conn.execute("DELETE FROM messages WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete message {}: {}", id, e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn db_set_chat_archived(
    state: State<'_, DbState>,
    address: String,
    is_archived: bool,
) -> Result<(), String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    conn.execute(
        "UPDATE chats SET is_archived = ?1 WHERE address = ?2",
        params![is_archived as i32, address],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn db_set_chat_pinned(
    state: State<'_, DbState>,
    address: String,
    is_pinned: bool,
) -> Result<(), String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    conn.execute(
        "UPDATE chats SET is_pinned = ?1 WHERE address = ?2",
        params![is_pinned as i32, address],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn db_delete_chat(
    app: AppHandle,
    state: State<'_, DbState>,
    address: String,
) -> Result<(), String> {
    let mut conn_lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = conn_lock.as_mut().ok_or("Database not initialized")?;

    let mut stmt = conn
        .prepare("SELECT id FROM messages WHERE chat_address = ?")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([&address], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;

    for id in rows.flatten() {
        let id_clone = id.clone();
        let app_h = app.clone();
        tokio::spawn(async move {
            let _ = vault_delete_media(app_h, id_clone).await;
        });
    }

    conn.execute("DELETE FROM messages WHERE chat_address = ?", [&address])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM chats WHERE address = ?", [&address])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn db_get_starred_messages(
    state: tauri::State<'_, DbState>,
) -> Result<Vec<Value>, String> {
    let conn_lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = conn_lock.as_ref().ok_or("Database not initialized")?;
    let mut stmt = conn.prepare(
        "SELECT id, chat_address, sender_hash, content, timestamp, type, status, attachment_json, is_starred, reply_to_json 
         FROM messages WHERE is_starred = 1 ORDER BY timestamp ASC"
    ).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "chatAddress": row.get::<_, String>(1)?,
                "senderHash": row.get::<_, String>(2)?,
                "content": row.get::<_, String>(3)?,
                "timestamp": row.get::<_, i64>(4)?,
                "type": row.get::<_, String>(5)?,
                "status": row.get::<_, String>(6)?,
                "attachmentJson": row.get::<_, Option<String>>(7)?,
                "isStarred": row.get::<_, i32>(8)? != 0,
                "replyToJson": row.get::<_, Option<String>>(9)?,
            }))
        })
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| e.to_string())?);
    }
    Ok(results)
}

pub async fn internal_db_save_message(state: &DbState, msg: DbMessage) -> Result<(), String> {
    let lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = lock.as_ref().ok_or("Database not initialized")?;

    conn.execute(
        "INSERT OR IGNORE INTO chats (address, is_group, alias, unread_count, is_archived) 
         VALUES (?1, ?2, ?3, 0, 0)",
        params![
            msg.chat_address,
            (msg.chat_address.len() < 40) as i32,
            &msg.chat_address[0..8.min(msg.chat_address.len())]
        ],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT OR IGNORE INTO messages (id, chat_address, sender_hash, content, timestamp, type, status, attachment_json, is_group, is_starred, reply_to_json) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            msg.id, 
            msg.chat_address, 
            msg.sender_hash, 
            msg.content, 
            msg.timestamp, 
            msg.r#type, 
            msg.status, 
            msg.attachment_json,
            msg.is_group as i32,
            msg.is_starred as i32,
            msg.reply_to_json,
        ],
    ).map_err(|e| e.to_string())?;

    if msg.status != "sending" {
        conn.execute(
            "UPDATE messages SET status = ?1 
             WHERE id = ?2 AND (
                (status = 'sent' AND (?1 = 'delivered' OR ?1 = 'read')) OR
                (status = 'delivered' AND ?1 = 'read') OR
                (status = 'sending')
             )",
            params![msg.status, msg.id],
        )
        .map_err(|e| e.to_string())?;

        if let Some(json) = msg.attachment_json {
            conn.execute(
                "UPDATE messages SET attachment_json = ?1 WHERE id = ?2",
                params![json, msg.id],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    conn.execute(
        "UPDATE chats SET last_msg = ?1, last_timestamp = ?2, last_sender_hash = ?3, last_status = ?4 
         WHERE LOWER(address) = LOWER(?5) AND (last_timestamp IS NULL OR ?2 > last_timestamp OR (?2 = last_timestamp AND last_status = 'sending'))",
        params![msg.content.chars().take(100).collect::<String>(), msg.timestamp, msg.sender_hash, msg.status, msg.chat_address],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn internal_db_upsert_chat(state: &DbState, chat: DbChat) -> Result<(), String> {
    let mut conn_lock = state
        .conn
        .lock()
        .map_err(|_| "Database connection lock poisoned")?;
    let conn = conn_lock.as_mut().ok_or("Database not initialized")?;

    conn.execute(
        "INSERT INTO chats (address, is_group, alias, last_msg, last_timestamp, unread_count, is_archived, is_pinned, trust_level, is_blocked, last_sender_hash, last_status, is_active)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(address) DO UPDATE SET
            alias = excluded.alias,
            is_group = excluded.is_group,
            last_msg = excluded.last_msg,
            last_timestamp = excluded.last_timestamp,
            last_sender_hash = excluded.last_sender_hash,
            last_status = excluded.last_status,
            unread_count = excluded.unread_count,
            is_archived = excluded.is_archived,
            is_pinned = excluded.is_pinned,
            trust_level = excluded.trust_level,
            is_blocked = excluded.is_blocked,
            is_active = excluded.is_active",
        params![
            chat.address, 
            chat.is_group as i32, 
            chat.alias, 
            chat.last_msg, 
            chat.last_timestamp, 
            chat.unread_count, 
            chat.is_archived as i32, 
            chat.is_pinned as i32, 
            chat.trust_level, 
            chat.is_blocked as i32,
            chat.last_sender_hash,
            chat.last_status,
            chat.is_active as i32,
        ],
    ).map_err(|e| e.to_string())?;

    if let Some(members) = chat.members {
        let _ = conn.execute(
            "DELETE FROM chat_members WHERE chat_address = ?1",
            params![chat.address],
        );
        for m in members {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO chat_members (chat_address, member_hash) VALUES (?1, ?2)",
                params![chat.address, m],
            );
        }
    }

    Ok(())
}
