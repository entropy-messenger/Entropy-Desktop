use crate::app_state::{DbState, NetworkState};
use aes_gcm::{
    Aes256Gcm,
    aead::{KeyInit, OsRng},
};
use argon2::{
    Argon2, Params,
    password_hash::{PasswordHasher, SaltString},
};
use hex;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, State};

const MIGRATIONS: &[&str] = &[
    // Version 1: Initial Schema (Consolidated base tables, indexes, and FTS triggers)
    "
    /* Core Infrastructure */
    CREATE TABLE IF NOT EXISTS kv_store (
        key TEXT PRIMARY KEY,
        value TEXT
    );

    CREATE TABLE IF NOT EXISTS pending_outbox (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        msg_id TEXT,
        msg_type TEXT,
        content BLOB,
        timestamp INTEGER
    );

    /* Signal Protocol State */
    CREATE TABLE IF NOT EXISTS signal_identity (
        id INTEGER PRIMARY KEY CHECK (id = 0),
        registration_id INTEGER,
        public_key BLOB,
        private_key BLOB,
        session_token TEXT
    );

    CREATE TABLE IF NOT EXISTS signal_pre_keys (
        key_id INTEGER PRIMARY KEY,
        key_data BLOB
    );

    CREATE TABLE IF NOT EXISTS signal_signed_pre_keys (
        key_id INTEGER PRIMARY KEY,
        key_data BLOB,
        signature BLOB,
        timestamp INTEGER
    );

    CREATE TABLE IF NOT EXISTS signal_sessions (
        address TEXT PRIMARY KEY,
        session_data BLOB
    );

    CREATE TABLE IF NOT EXISTS signal_identities_remote (
        address TEXT PRIMARY KEY,
        public_key BLOB NOT NULL,
        trust_level INTEGER DEFAULT 1
    );

    CREATE TABLE IF NOT EXISTS signal_kyber_pre_keys (
        key_id INTEGER PRIMARY KEY,
        key_data BLOB NOT NULL
    );

    CREATE TABLE IF NOT EXISTS signal_kyber_base_keys_seen (
        kyber_prekey_id INTEGER NOT NULL,
        ec_prekey_id INTEGER NOT NULL,
        base_key BLOB NOT NULL,
        PRIMARY KEY (kyber_prekey_id, ec_prekey_id, base_key)
    );

    /* Chat & Messaging Entities */
    CREATE TABLE IF NOT EXISTS contacts (
        hash TEXT PRIMARY KEY,
        alias TEXT,
        global_nickname TEXT,
        is_blocked INTEGER DEFAULT 0,
        trust_level INTEGER DEFAULT 1
    );

    CREATE TABLE IF NOT EXISTS chats (
        address TEXT PRIMARY KEY,
        is_group INTEGER DEFAULT 0,
        alias TEXT,
        global_nickname TEXT,
        last_msg TEXT,
        last_timestamp INTEGER,
        last_sender_hash TEXT,
        last_status TEXT,
        unread_count INTEGER DEFAULT 0,
        is_archived INTEGER DEFAULT 0,
        is_pinned INTEGER DEFAULT 0,
        trust_level INTEGER DEFAULT 1,
        is_blocked INTEGER DEFAULT 0,
        is_active INTEGER DEFAULT 1
    );

    CREATE TABLE IF NOT EXISTS messages (
        id TEXT PRIMARY KEY,
        chat_address TEXT,
        sender_hash TEXT,
        content TEXT,
        timestamp INTEGER,
        type TEXT,
        status TEXT,
        attachment_json TEXT,
        is_group INTEGER DEFAULT 0,
        is_starred INTEGER DEFAULT 0,
        is_pinned INTEGER DEFAULT 0,
        reply_to_json TEXT,
        FOREIGN KEY(chat_address) REFERENCES chats(address)
    );

    CREATE TABLE IF NOT EXISTS chat_members (
        chat_address TEXT,
        member_hash TEXT,
        PRIMARY KEY (chat_address, member_hash),
        FOREIGN KEY(chat_address) REFERENCES chats(address)
    );

    /* Performance Optimization: Indexes */
    CREATE INDEX IF NOT EXISTS idx_messages_chat_addr ON messages(chat_address, timestamp);
    CREATE INDEX IF NOT EXISTS idx_chats_last_ts ON chats(last_timestamp);
    CREATE INDEX IF NOT EXISTS idx_members_hash ON chat_members(member_hash);

    /* Full-Text Search (FTS5) Engine */
    CREATE VIRTUAL TABLE IF NOT EXISTS message_search USING fts5(
        message_id UNINDEXED,
        content,
        chat_address UNINDEXED,
        content='messages',
        content_rowid='rowid'
    );

    /* FTS Synchronization Triggers */
    CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
        INSERT INTO message_search(rowid, message_id, content, chat_address) 
        VALUES (new.rowid, new.id, new.content, new.chat_address);
        
        UPDATE chats SET 
            last_msg = SUBSTR(new.content, 1, 100),
            last_timestamp = new.timestamp,
            last_sender_hash = new.sender_hash,
            last_status = new.status
        WHERE address = new.chat_address 
        AND (last_timestamp IS NULL OR new.timestamp >= last_timestamp);
    END;

    CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE OF status ON messages BEGIN
        UPDATE chats SET last_status = new.status
        WHERE LOWER(address) = LOWER(new.chat_address)
        AND last_timestamp = new.timestamp;
    END;

    CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
        INSERT INTO message_search(message_search, rowid, message_id, content, chat_address) 
        VALUES('delete', old.rowid, old.id, old.content, old.chat_address);
    END;
    ",
];

pub fn get_db_filename() -> String {
    if let Ok(profile) = std::env::var("ENTROPY_PROFILE")
        && !profile.is_empty()
    {
        return format!("entropy_{}.db", profile);
    }
    "entropy.db".to_string()
}

pub fn get_media_dirname() -> String {
    if let Ok(profile) = std::env::var("ENTROPY_PROFILE")
        && !profile.is_empty()
    {
        return format!("media_{}", profile);
    }
    "media".to_string()
}

#[tauri::command]
pub fn vault_exists(app: AppHandle) -> bool {
    if let Ok(app_data_dir) = app.path().app_data_dir() {
        return app_data_dir.join(get_db_filename()).exists();
    }
    false
}

#[tauri::command]
pub async fn init_vault(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    passphrase: String,
) -> Result<(), String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    if !app_data_dir.exists() {
        std::fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;
    }

    let db_path = app_data_dir.join(get_db_filename());
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
        | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;

    let attempts_file = app_data_dir.join("login_attempts.dat");
    let mut attempts = 0;
    if attempts_file.exists()
        && let Ok(s) = std::fs::read_to_string(&attempts_file)
    {
        attempts = s.trim().parse().unwrap_or(0);
    }

    // PANIC MODE CHECK
    let panic_file = app_data_dir.join("panic.dat");
    if panic_file.exists()
        && let Ok(stored_hash) = std::fs::read_to_string(&panic_file)
    {
        let argon2 = Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            Params::new(65536, 3, 4, Some(32)).unwrap(),
        );

        let salt = SaltString::from_b64("cGFuaWMtc2FsdC12MQ").expect("valid salt");

        let password_hash = argon2
            .hash_password(passphrase.as_bytes(), &salt)
            .map_err(|e| format!("Argon2 hash failed: {}", e))?;
        let input_hash = hex::encode(password_hash.hash.unwrap().as_ref());

        if input_hash == stored_hash.trim() {
            let filename = get_db_filename();
            let _ = std::fs::remove_file(app_data_dir.join(&filename));
            let _ = std::fs::remove_file(app_data_dir.join(format!("{}-wal", filename)));
            let _ = std::fs::remove_file(app_data_dir.join(format!("{}-shm", filename)));
            let _ = std::fs::remove_dir_all(app_data_dir.join(get_media_dirname()));
            let _ = std::fs::remove_file(&attempts_file);
            app.restart();
        }
    }

    if attempts >= 10 {
        // Nuclear reset logic inline
        let filename = get_db_filename();
        let _ = std::fs::remove_file(app_data_dir.join(&filename));
        let _ = std::fs::remove_file(app_data_dir.join(format!("{}-wal", filename)));
        let _ = std::fs::remove_file(app_data_dir.join(format!("{}-shm", filename)));
        let _ = std::fs::remove_dir_all(app_data_dir.join(get_media_dirname()));
        let _ = std::fs::remove_file(&attempts_file);
        app.restart();
    }

    let conn_res = Connection::open_with_flags(&db_path, flags);

    // If connection opens, try to set key and query. If fail, increment attempts.
    let conn = match conn_res {
        Ok(c) => c,
        Err(e) => return Err(e.to_string()),
    };

    if !passphrase.is_empty() {
        // Load or Generate a unique random salt
        let salt_path = app_data_dir.join("vault.salt");
        let salt_string = if salt_path.exists() {
            std::fs::read_to_string(&salt_path)
                .map_err(|e| format!("Failed to read salt: {}", e))?
        } else {
            let new_salt = SaltString::generate(&mut OsRng);
            let s = new_salt.as_str().to_string();
            std::fs::write(&salt_path, &s).map_err(|e| format!("Failed to save salt: {}", e))?;
            s
        };

        let derived_key_hex = tauri::async_runtime::spawn_blocking(move || {
            let salt =
                SaltString::from_b64(&salt_string).map_err(|e| format!("Salt error: {}", e))?;
            let argon2 = Argon2::new(
                argon2::Algorithm::Argon2id,
                argon2::Version::V0x13,
                Params::new(65536, 3, 4, Some(32))
                    .map_err(|e| format!("Argon2 params error: {}", e))?,
            );
            let password_hash = argon2
                .hash_password(passphrase.as_bytes(), &salt)
                .map_err(|e| format!("Argon2 hash failed: {}", e))?;
            let derived_key = password_hash
                .hash
                .ok_or_else(|| "Argon2 key derivation failed to return hash".to_string())?;
            Ok::<String, String>(hex::encode(derived_key.as_ref()))
        })
        .await
        .map_err(|e| e.to_string())??;

        let key_query = format!("PRAGMA key = \"x'{}'\";", derived_key_hex);
        let _ = conn.execute_batch(&key_query);
    }

    // Test if key is correct by reading user_version
    if conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .is_err()
    {
        attempts += 1;
        let _ = std::fs::write(&attempts_file, attempts.to_string());
        return Err(format!("Incorrect password. Attempt {}/10", attempts));
    }

    // Success - reset attempts
    if attempts > 0 {
        let _ = std::fs::remove_file(attempts_file);
    }

    let _ = conn.execute("PRAGMA journal_mode=WAL;", []);

    // --- DATABASE MIGRATIONS ---
    let current_version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| format!("Schema version check failed: {}", e))?;

    let target_version = MIGRATIONS.len() as i32;

    if current_version < target_version {
        for (idx, migration_sql) in MIGRATIONS.iter().enumerate() {
            let migration_ver = (idx + 1) as i32;
            if migration_ver > current_version {
                conn.execute_batch(migration_sql)
                    .map_err(|e| format!("Migration to v{} failed: {}", migration_ver, e))?;

                conn.execute(&format!("PRAGMA user_version = {}", migration_ver), [])
                    .map_err(|e| {
                        format!("Failed to update user_version to v{}: {}", migration_ver, e)
                    })?;
            }
        }
    }

    // Media Encryption Key Initialization
    let media_key = {
        let mut stmt = conn
            .prepare("SELECT value FROM kv_store WHERE key = '_internal_media_key'")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let hex_key: String = row.get(0).map_err(|e| e.to_string())?;
            hex::decode(hex_key).map_err(|e| e.to_string())?
        } else {
            let key = Aes256Gcm::generate_key(&mut OsRng);
            let hex_key = hex::encode(key);
            conn.execute(
                "INSERT INTO kv_store (key, value) VALUES ('_internal_media_key', ?1)",
                [&hex_key],
            )
            .map_err(|e| e.to_string())?;
            key.to_vec()
        }
    };

    {
        let mut db_conn = state
            .conn
            .lock()
            .map_err(|_| "Database connection lock poisoned")?;
        *db_conn = Some(conn);
    }

    {
        let mut state_key = state
            .media_key
            .lock()
            .map_err(|_| "Media key lock poisoned")?;
        *state_key = Some(media_key);
    }

    //  Restore the secure session from the vault synchronously
    let pub_key_opt: Option<Vec<u8>> = {
        let lock = state
            .conn
            .lock()
            .map_err(|_| "Database connection lock poisoned")?;
        if let Some(c) = lock.as_ref() {
            c.query_row("SELECT public_key FROM signal_identity LIMIT 1", [], |r| {
                r.get(0)
            })
            .ok()
        } else {
            None
        }
    };

    let token_opt: Option<String> = {
        let lock = state
            .conn
            .lock()
            .map_err(|_| "Database connection lock poisoned")?;
        if let Some(c) = lock.as_ref() {
            c.query_row(
                "SELECT session_token FROM signal_identity LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok()
            .flatten()
        } else {
            None
        }
    };

    if let Some(mut pk) = pub_key_opt {
        if pk.len() == 33 && pk[0] == 0x05 {
            pk.remove(0);
        }
        let id_hash = hex::encode(Sha256::digest(&pk));
        if let Ok(mut l) = app.state::<NetworkState>().identity_hash.lock() {
            *l = Some(id_hash);
        }

        if let Some(t) = token_opt
            && let Ok(mut l) = app.state::<NetworkState>().session_token.lock()
        {
            *l = Some(t);
        }
    }

    Ok(())
}

#[tauri::command]
pub fn set_panic_password(app: tauri::AppHandle, password: String) -> Result<(), String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        Params::new(65536, 3, 4, Some(32)).expect("valid params"),
    );
    let salt = SaltString::from_b64("cGFuaWMtc2FsdC12MQ").expect("valid salt");

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| format!("Argon2 hash failed: {}", e))?;
    let hash = hex::encode(password_hash.hash.unwrap().as_ref());

    std::fs::write(app_data_dir.join("panic.dat"), hash).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn reset_database(app: tauri::AppHandle, state: State<'_, DbState>) -> Result<(), String> {
    {
        let mut conn = state
            .conn
            .lock()
            .map_err(|_| "Database connection lock poisoned")?;
        if let Some(c) = conn.take() {
            let _ = c.close();
        }
        *conn = None;
    }

    let filename = get_db_filename();
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    let db_path = app_dir.join(&filename);
    let wal_path = app_dir.join(format!("{}-wal", filename));
    let shm_path = app_dir.join(format!("{}-shm", filename));
    let media_dir_name = get_media_dirname();
    let media_dir = app_dir.join(&media_dir_name);

    if db_path.exists() {
        std::fs::remove_file(&db_path).map_err(|e| e.to_string())?;
    }
    if wal_path.exists() {
        let _ = std::fs::remove_file(wal_path);
    }
    if shm_path.exists() {
        let _ = std::fs::remove_file(shm_path);
    }
    if media_dir.exists() {
        let _ = std::fs::remove_dir_all(media_dir);
    }

    app.restart();
    #[allow(unreachable_code)]
    Ok(())
}
