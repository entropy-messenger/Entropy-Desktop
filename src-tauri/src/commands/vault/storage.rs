use crate::app_state::DbState;
use tauri::State;

#[tauri::command]
pub fn vault_save(state: State<'_, DbState>, key: String, value: String) -> Result<(), String> {
    let conn = state.get_conn()?;
    conn.execute(
        "INSERT OR REPLACE INTO kv_store (key, value) VALUES (?1, ?2);",
        [key, value],
    )
    .map_err(|e: rusqlite::Error| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn vault_load(state: State<'_, DbState>, key: String) -> Result<Option<String>, String> {
    let conn = state.get_conn()?;
    let mut stmt = conn
        .prepare("SELECT value FROM kv_store WHERE key = ?1;")
        .map_err(|e: rusqlite::Error| e.to_string())?;
    let mut rows = stmt
        .query([key])
        .map_err(|e: rusqlite::Error| e.to_string())?;

    if let Some(row) = rows.next().map_err(|e: rusqlite::Error| e.to_string())? {
        Ok(Some(
            row.get::<_, String>(0)
                .map_err(|e: rusqlite::Error| e.to_string())?,
        ))
    } else {
        Ok(None)
    }
}
