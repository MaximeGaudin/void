use rusqlite::Connection;

pub const SCHEMA_VERSION: i32 = 1;

pub fn run_migrations(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS kb_meta (key TEXT PRIMARY KEY, value TEXT)")?;

    let current_version: i32 = conn
        .query_row(
            "SELECT value FROM kb_meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    if current_version < 1 {
        migrate_v1(conn)?;
    }

    conn.execute(
        "INSERT OR REPLACE INTO kb_meta (key, value) VALUES ('schema_version', ?1)",
        [SCHEMA_VERSION.to_string()],
    )?;

    Ok(())
}

fn migrate_v1(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS kb_documents (
            id          TEXT PRIMARY KEY,
            content     TEXT NOT NULL,
            source_type TEXT NOT NULL,
            source_path TEXT,
            content_hash TEXT NOT NULL,
            expiration  TEXT,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS kb_document_metadata (
            id          INTEGER PRIMARY KEY,
            document_id TEXT NOT NULL REFERENCES kb_documents(id) ON DELETE CASCADE,
            key         TEXT NOT NULL,
            value       TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_kb_doc_meta ON kb_document_metadata(document_id);

        CREATE TABLE IF NOT EXISTS kb_chunks (
            id          INTEGER PRIMARY KEY,
            document_id TEXT NOT NULL REFERENCES kb_documents(id) ON DELETE CASCADE,
            content     TEXT NOT NULL,
            chunk_index INTEGER NOT NULL,
            start_byte  INTEGER NOT NULL,
            end_byte    INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_kb_chunks_doc ON kb_chunks(document_id);

        CREATE VIRTUAL TABLE IF NOT EXISTS kb_vec_chunks USING vec0(
            chunk_id INTEGER PRIMARY KEY,
            embedding float[1024]
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS kb_chunks_fts USING fts5(
            normalized_content
        );

        CREATE TABLE IF NOT EXISTS kb_sync_folders (
            id            TEXT PRIMARY KEY,
            folder_path   TEXT NOT NULL UNIQUE,
            interval_secs INTEGER NOT NULL DEFAULT 60,
            last_scan_at  TEXT,
            created_at    TEXT NOT NULL
        );
        ",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_test_db() -> Connection {
        unsafe {
            #[allow(clippy::missing_transmute_annotations)]
            let ext = std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
            rusqlite::ffi::sqlite3_auto_extension(Some(ext));
        }
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn
    }

    #[test]
    fn migration_creates_tables() {
        let conn = open_test_db();
        run_migrations(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"kb_documents".to_string()));
        assert!(tables.contains(&"kb_document_metadata".to_string()));
        assert!(tables.contains(&"kb_chunks".to_string()));
        assert!(tables.contains(&"kb_sync_folders".to_string()));
        assert!(tables.contains(&"kb_meta".to_string()));
    }

    #[test]
    fn migration_idempotent() {
        let conn = open_test_db();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        let version: String = conn
            .query_row(
                "SELECT value FROM kb_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "1");
    }
}
