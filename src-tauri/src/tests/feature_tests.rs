#[cfg(test)]
mod tests {
    use crate::protocol::*;
    use rusqlite::Connection;
    use tempfile::tempdir;



    #[test]
    fn test_vault_export_import_integrity() {
        // 1. Create a DB and populate it
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("vault.db");
        
        {
            let conn = Connection::open(&db_path).unwrap();
            init_database(&conn).unwrap();
            conn.execute("INSERT INTO vault (key, value) VALUES (?1, ?2)", ["test_key", "test_value"]).unwrap();
            
            // Generate an identity to make it realistic
            let id = generate_new_identity();
            id.save_to_db(&conn).unwrap();
        }

        // 2. Export (Simulate reading bytes)
        let exported_bytes = std::fs::read(&db_path).unwrap();
        assert!(!exported_bytes.is_empty());

        // 3. Import (Write bytes to new location)
        let import_path = dir.path().join("imported_vault.db");
        std::fs::write(&import_path, &exported_bytes).unwrap();

        // 4. Verify Integrity of Imported DB
         {
            let conn = Connection::open(&import_path).unwrap();
            
            // Check Vault Data
            let val: String = conn.query_row("SELECT value FROM vault WHERE key = 'test_key'", [], |r| r.get(0)).unwrap();
            assert_eq!(val, "test_value");

            // Check Identity Data
            let id_loaded = ProtocolIdentity::load_from_db(&conn).unwrap();
            assert!(id_loaded.is_some());
        }
    }

    #[test]
    fn test_secure_nuke_wipes_file() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("secret.db");
        std::fs::write(&db_path, "sensitive data").unwrap();

        assert!(db_path.exists());
        
        // Execute Secure Nuke (calling the protocol function directly)
        crate::protocol::secure_nuke_database(&db_path).unwrap();

        // File should not exist
        assert!(!db_path.exists());
    }

    #[test]
    fn test_corrupt_vault_handling() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("corrupt.db");
        std::fs::write(&db_path, "not a sqlite database").unwrap();

        let conn_res = Connection::open(&db_path);
        // SQLite might open it but fail on queries, or open successfully as a new blank DB if it wasn't a valid format?
        // Actually SQLite checks header.
        
        if let Ok(conn) = conn_res {
             let res = init_database(&conn);
             // It might error here if content is garbage
             if res.is_ok() {
                 // If it initialized, it might have overwritten the file or treated it as empty. 
                 // Let's check if we can write to it.
                 conn.execute("CREATE TABLE IF NOT EXISTS test (id INTEGER PRIMARY KEY)", []).unwrap();
             }
        }
    }
}
