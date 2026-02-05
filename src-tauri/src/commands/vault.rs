use rusqlite::Connection;
use tauri::{Manager, State};
use crate::protocol;
use crate::app_state::DbState;
use std::collections::HashMap;

pub fn get_profile_db_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let profile = std::env::var("ENTROPY_PROFILE").unwrap_or_default();
    
    if profile.is_empty() {
        Ok(app_data_dir.join("vault.db"))
    } else {
        Ok(app_data_dir.join(format!("vault_{}.db", profile)))
    }
}

fn get_profile_secret_path(app: &tauri::AppHandle, key: &str) -> Result<std::path::PathBuf, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let profile = std::env::var("ENTROPY_PROFILE").unwrap_or_default();
    
    if profile.is_empty() {
        Ok(app_data_dir.join(format!("{}.secret", key)))
    } else {
        Ok(app_data_dir.join(format!("{}_{}.secret", key, profile)))
    }
}

#[tauri::command]
pub fn store_secret(app: tauri::AppHandle, key: String, value: String) -> Result<(), String> {
    let secret_path = get_profile_secret_path(&app, &key)?;
    let app_data_dir = secret_path.parent().unwrap();
    if !app_data_dir.exists() {
        std::fs::create_dir_all(app_data_dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(secret_path, value).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_secret(app: tauri::AppHandle, key: String) -> Result<String, String> {
    let secret_path = get_profile_secret_path(&app, &key)?;
    if secret_path.exists() {
        return std::fs::read_to_string(secret_path).map_err(|e| e.to_string());
    }
    Err("Secret not found".to_string())
}

#[tauri::command]
pub fn init_vault(app: tauri::AppHandle, state: State<'_, DbState>, _passphrase: String) -> Result<(), String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    if !app_data_dir.exists() {
        std::fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;
    }

    let db_path = get_profile_db_path(&app)?;

    {
        let mut conn_lock = state.conn.lock().unwrap();
        if let Some(conn) = conn_lock.as_ref() {
            if conn.pragma_update(None, "journal_mode", &"WAL").is_ok() {
                return Ok(());
            }
        }
        *conn_lock = None;
    }

    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    let _ = conn.pragma_update(None, "journal_mode", &"WAL");
    protocol::init_database(&conn)?;

    let mut db_conn = state.conn.lock().unwrap();
    *db_conn = Some(conn);
    Ok(())
}

#[tauri::command]
pub fn clear_vault(state: State<'_, DbState>) -> Result<(), String> {
    let conn_lock = state.conn.lock().unwrap();
    if let Some(conn) = conn_lock.as_ref() {
        conn.execute("DELETE FROM vault;", []).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn vault_save(state: State<'_, DbState>, key: String, value: String) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        conn.execute(
            "INSERT OR REPLACE INTO vault (key, value) VALUES (?1, ?2);",
            [key, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn vault_load(state: State<'_, DbState>, key: String) -> Result<Option<String>, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let mut stmt = conn
            .prepare("SELECT value FROM vault WHERE key = ?1;")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([key]).map_err(|e| e.to_string())?;

        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            Ok(Some(row.get(0).map_err(|e| e.to_string())?))
        } else {
            Ok(None)
        }
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn dump_vault(state: State<'_, DbState>) -> Result<HashMap<String, String>, String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        let mut stmt = conn.prepare("SELECT key, value FROM vault;").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }).map_err(|e| e.to_string())?;

        let mut data = HashMap::new();
        for row in rows {
            let (k, v) = row.map_err(|e| e.to_string())?;
            data.insert(k, v);
        }
        Ok(data)
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn restore_vault(state: State<'_, DbState>, data: HashMap<String, String>) -> Result<(), String> {
    let lock = state.conn.lock().unwrap();
    if let Some(conn) = lock.as_ref() {
        for (k, v) in data {
            conn.execute(
                "INSERT OR REPLACE INTO vault (key, value) VALUES (?1, ?2);",
                [&k, &v],
            ).map_err(|e| e.to_string())?;
        }
        Ok(())
    } else {
        Err("Vault not initialized".to_string())
    }
}

#[tauri::command]
pub fn nuclear_reset(app: tauri::AppHandle, state: State<'_, DbState>) -> Result<(), String> {
    if let Ok(mut conn) = state.conn.lock() {
        *conn = None;
    }

    let db_path = get_profile_db_path(&app)?;
    let salt_path = get_profile_secret_path(&app, "entropy_vault_salt")?;
    
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(salt_path);
    
    Ok(())
}
