use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use tracing::{debug, info};
use zerocopy::IntoBytes;

use crate::models::*;
use crate::normalize::normalize_for_search;
use crate::schema;

pub struct KbDatabase {
    conn: Mutex<Connection>,
}

unsafe impl Send for KbDatabase {}
unsafe impl Sync for KbDatabase {}

impl KbDatabase {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        info!(path = %path.display(), "opening KB database");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        register_sqlite_vec();
        let conn = Connection::open(path)?;
        Self::configure_and_migrate(conn)
    }

    pub fn open_in_memory() -> anyhow::Result<Self> {
        debug!("opening in-memory KB database");
        register_sqlite_vec();
        let conn = Connection::open_in_memory()?;
        Self::configure_and_migrate(conn)
    }

    fn configure_and_migrate(conn: Connection) -> anyhow::Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::run_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow::anyhow!("KB database lock poisoned"))
    }

    // ── Document CRUD ──────────────────────────────────────────────

    pub fn insert_document(
        &self,
        doc: &Document,
    ) -> anyhow::Result<()> {
        let conn = self.conn()?;
        let tx = conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO kb_documents (id, content, source_type, source_path, content_hash, expiration, source_mtime, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                doc.id,
                doc.content,
                doc.source_type.as_str(),
                doc.source_path,
                doc.content_hash,
                doc.expiration,
                doc.source_mtime,
                doc.created_at,
                doc.updated_at,
            ],
        )?;

        for m in &doc.metadata {
            tx.execute(
                "INSERT INTO kb_document_metadata (document_id, key, value) VALUES (?1, ?2, ?3)",
                rusqlite::params![doc.id, m.key, m.value],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn get_document(&self, id: &str) -> anyhow::Result<Option<Document>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, content, source_type, source_path, content_hash, expiration, source_mtime, created_at, updated_at
             FROM kb_documents WHERE id = ?1",
        )?;

        let doc = stmt.query_row([id], |row| {
            Ok(DocumentRow {
                id: row.get(0)?,
                content: row.get(1)?,
                source_type: row.get(2)?,
                source_path: row.get(3)?,
                content_hash: row.get(4)?,
                expiration: row.get(5)?,
                source_mtime: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        });

        match doc {
            Ok(row) => {
                let metadata = self.get_metadata_inner(&conn, &row.id)?;
                Ok(Some(row.into_document(metadata)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete_document(&self, id: &str) -> anyhow::Result<bool> {
        let conn = self.conn()?;
        let tx = conn.unchecked_transaction()?;

        let chunk_rowids: Vec<i64> = tx
            .prepare("SELECT id FROM kb_chunks WHERE document_id = ?1")?
            .query_map([id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        for rowid in &chunk_rowids {
            tx.execute("DELETE FROM kb_vec_chunks WHERE chunk_id = ?1", [rowid])?;
            tx.execute("DELETE FROM kb_chunks_fts WHERE rowid = ?1", [rowid])?;
        }

        let deleted = tx.execute("DELETE FROM kb_documents WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(deleted > 0)
    }

    pub fn list_documents(&self, limit: i64, offset: i64) -> anyhow::Result<(Vec<Document>, i64)> {
        let conn = self.conn()?;

        let total: i64 =
            conn.query_row("SELECT COUNT(*) FROM kb_documents", [], |row| row.get(0))?;

        let mut stmt = conn.prepare(
            "SELECT id, content, source_type, source_path, content_hash, expiration, source_mtime, created_at, updated_at
             FROM kb_documents ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        )?;

        let rows: Vec<DocumentRow> = stmt
            .query_map(rusqlite::params![limit, offset], |row| {
                Ok(DocumentRow {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    source_type: row.get(2)?,
                    source_path: row.get(3)?,
                    content_hash: row.get(4)?,
                    expiration: row.get(5)?,
                    source_mtime: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut docs = Vec::with_capacity(rows.len());
        for row in rows {
            let metadata = self.get_metadata_inner(&conn, &row.id)?;
            docs.push(row.into_document(metadata));
        }

        Ok((docs, total))
    }

    /// Find document by source path (for sync dedup).
    pub fn find_document_by_source_path(&self, path: &str) -> anyhow::Result<Option<Document>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, content, source_type, source_path, content_hash, expiration, source_mtime, created_at, updated_at
             FROM kb_documents WHERE source_path = ?1",
        )?;

        let doc = stmt.query_row([path], |row| {
            Ok(DocumentRow {
                id: row.get(0)?,
                content: row.get(1)?,
                source_type: row.get(2)?,
                source_path: row.get(3)?,
                content_hash: row.get(4)?,
                expiration: row.get(5)?,
                source_mtime: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        });

        match doc {
            Ok(row) => {
                let metadata = self.get_metadata_inner(&conn, &row.id)?;
                Ok(Some(row.into_document(metadata)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// List all synced document source paths with their hashes.
    pub fn list_synced_paths_for_folder(
        &self,
        folder: &str,
    ) -> anyhow::Result<Vec<(String, String, String)>> {
        let conn = self.conn()?;
        let pattern = format!("{}%", folder);
        let mut stmt = conn.prepare(
            "SELECT id, source_path, content_hash FROM kb_documents
             WHERE source_type = 'synced' AND source_path LIKE ?1",
        )?;
        let rows: Vec<(String, String, String)> = stmt
            .query_map([&pattern], |row| {
                Ok((row.get(0)?, row.get::<_, String>(1)?, row.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    // ── Chunks ─────────────────────────────────────────────────────

    pub fn insert_chunks_with_embeddings(
        &self,
        document_id: &str,
        chunks: &[(String, i64, i64, i64)], // (content, chunk_index, start_byte, end_byte)
        embeddings: &[Vec<f32>],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            chunks.len() == embeddings.len(),
            "chunk count ({}) != embedding count ({})",
            chunks.len(),
            embeddings.len()
        );

        let conn = self.conn()?;
        let tx = conn.unchecked_transaction()?;

        for (i, (content, chunk_index, start_byte, end_byte)) in chunks.iter().enumerate() {
            tx.execute(
                "INSERT INTO kb_chunks (document_id, content, chunk_index, start_byte, end_byte)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![document_id, content, chunk_index, start_byte, end_byte],
            )?;

            let chunk_rowid = tx.last_insert_rowid();

            let normalized = normalize_for_search(content);
            tx.execute(
                "INSERT INTO kb_chunks_fts (rowid, normalized_content) VALUES (?1, ?2)",
                rusqlite::params![chunk_rowid, normalized],
            )?;

            let embedding_bytes = embeddings[i].as_bytes();
            tx.execute(
                "INSERT INTO kb_vec_chunks (chunk_id, embedding) VALUES (?1, ?2)",
                rusqlite::params![chunk_rowid, embedding_bytes],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    // ── Search ─────────────────────────────────────────────────────

    /// Semantic KNN search. Returns (chunk_id, document_id, content, distance).
    pub fn semantic_search(
        &self,
        query_embedding: &[f32],
        limit: i64,
    ) -> anyhow::Result<Vec<(i64, String, String, f64)>> {
        let conn = self.conn()?;
        let embedding_bytes = query_embedding.as_bytes();

        let mut stmt = conn.prepare(
            "SELECT v.chunk_id, c.document_id, c.content, v.distance
             FROM kb_vec_chunks v
             JOIN kb_chunks c ON c.id = v.chunk_id
             WHERE v.embedding MATCH ?1
             AND k = ?2
             ORDER BY v.distance
             LIMIT ?2",
        )?;

        let rows: Vec<(i64, String, String, f64)> = stmt
            .query_map(rusqlite::params![embedding_bytes, limit], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    /// FTS5 grep search. Returns (chunk_id, document_id, content, bm25_score).
    pub fn grep_search(
        &self,
        query: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<(i64, String, String, f64)>> {
        let normalized = normalize_for_search(query);
        let fts_query = fts5_escape(&normalized);

        if fts_query.trim().is_empty() || fts_query == "\"\"" {
            return Ok(vec![]);
        }

        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT fts.rowid, c.document_id, c.content, bm25(kb_chunks_fts) AS score
             FROM kb_chunks_fts fts
             JOIN kb_chunks c ON c.id = fts.rowid
             WHERE kb_chunks_fts MATCH ?1
             ORDER BY score
             LIMIT ?2",
        )?;

        let rows: Vec<(i64, String, String, f64)> = stmt
            .query_map(rusqlite::params![fts_query, limit], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    // ── Expiration ─────────────────────────────────────────────────

    pub fn cleanup_expired(&self) -> anyhow::Result<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn()?;

        let expired_ids: Vec<String> = conn
            .prepare(
                "SELECT id FROM kb_documents WHERE expiration IS NOT NULL AND expiration < ?1",
            )?
            .query_map([&now], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        let count = expired_ids.len();
        drop(conn);

        for id in &expired_ids {
            self.delete_document(id)?;
        }

        Ok(count)
    }

    /// Filter out expired document IDs from a set.
    pub fn filter_expired_ids(&self, doc_ids: &[String]) -> anyhow::Result<Vec<String>> {
        if doc_ids.is_empty() {
            return Ok(vec![]);
        }
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn()?;
        let mut expired = Vec::new();
        for id in doc_ids {
            let is_expired: bool = conn
                .query_row(
                    "SELECT expiration IS NOT NULL AND expiration < ?1 FROM kb_documents WHERE id = ?2",
                    rusqlite::params![now, id],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            if is_expired {
                expired.push(id.clone());
            }
        }
        Ok(expired)
    }

    // ── Sync folders ───────────────────────────────────────────────

    pub fn upsert_sync_folder(&self, folder: &SyncFolder) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO kb_sync_folders (id, folder_path, interval_secs, last_scan_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(folder_path) DO UPDATE SET interval_secs = excluded.interval_secs, last_scan_at = excluded.last_scan_at",
            rusqlite::params![
                folder.id,
                folder.folder_path,
                folder.interval_secs,
                folder.last_scan_at,
                folder.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_sync_folders(&self) -> anyhow::Result<Vec<SyncFolder>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, folder_path, interval_secs, last_scan_at, created_at FROM kb_sync_folders",
        )?;
        let rows: Vec<SyncFolder> = stmt
            .query_map([], |row| {
                Ok(SyncFolder {
                    id: row.get(0)?,
                    folder_path: row.get(1)?,
                    interval_secs: row.get(2)?,
                    last_scan_at: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn update_sync_folder_scan_time(&self, folder_path: &str, time: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE kb_sync_folders SET last_scan_at = ?1 WHERE folder_path = ?2",
            rusqlite::params![time, folder_path],
        )?;
        Ok(())
    }

    // ── Status ─────────────────────────────────────────────────────

    pub fn status(&self) -> anyhow::Result<KbStatus> {
        let conn = self.conn()?;
        let document_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM kb_documents", [], |r| r.get(0))?;
        let chunk_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM kb_chunks", [], |r| r.get(0))?;
        let sync_folder_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM kb_sync_folders", [], |r| r.get(0))?;
        Ok(KbStatus {
            document_count,
            chunk_count,
            sync_folder_count,
            db_size_bytes: 0,
        })
    }

    /// Look up the best available timestamp for recency ranking.
    /// Returns `source_mtime` if present, otherwise `updated_at`.
    pub fn get_document_timestamp(&self, doc_id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        let ts: Option<String> = conn
            .query_row(
                "SELECT COALESCE(source_mtime, updated_at) FROM kb_documents WHERE id = ?1",
                [doc_id],
                |row| row.get(0),
            )
            .ok();
        Ok(ts)
    }

    // ── Helpers ────────────────────────────────────────────────────

    fn get_metadata_inner(
        &self,
        conn: &Connection,
        doc_id: &str,
    ) -> anyhow::Result<Vec<MetadataEntry>> {
        let mut stmt = conn.prepare(
            "SELECT key, value FROM kb_document_metadata WHERE document_id = ?1",
        )?;
        let rows: Vec<MetadataEntry> = stmt
            .query_map([doc_id], |row| {
                Ok(MetadataEntry {
                    key: row.get(0)?,
                    value: row.get(1)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }
}

fn register_sqlite_vec() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        #[allow(clippy::missing_transmute_annotations)]
        let ext = std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
        rusqlite::ffi::sqlite3_auto_extension(Some(ext));
    });
}

fn fts5_escape(query: &str) -> String {
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return "\"\"".to_string();
    }
    terms
        .iter()
        .map(|t| {
            let escaped = t.replace('"', "\"\"");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

struct DocumentRow {
    id: String,
    content: String,
    source_type: String,
    source_path: Option<String>,
    content_hash: String,
    expiration: Option<String>,
    source_mtime: Option<String>,
    created_at: String,
    updated_at: String,
}

impl DocumentRow {
    fn into_document(self, metadata: Vec<MetadataEntry>) -> Document {
        Document {
            id: self.id,
            content: self.content,
            source_type: self.source_type.parse().unwrap_or(SourceType::Text),
            source_path: self.source_path,
            content_hash: self.content_hash,
            expiration: self.expiration,
            source_mtime: self.source_mtime,
            created_at: self.created_at,
            updated_at: self.updated_at,
            metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> KbDatabase {
        KbDatabase::open_in_memory().unwrap()
    }

    fn make_doc(id: &str, content: &str) -> Document {
        let now = chrono::Utc::now().to_rfc3339();
        Document {
            id: id.to_string(),
            content: content.to_string(),
            source_type: SourceType::Text,
            source_path: None,
            content_hash: "abc123".to_string(),
            expiration: None,
            source_mtime: None,
            created_at: now.clone(),
            updated_at: now,
            metadata: vec![],
        }
    }

    #[test]
    fn insert_and_get_document() {
        let db = test_db();
        let doc = make_doc("d1", "hello world");
        db.insert_document(&doc).unwrap();

        let fetched = db.get_document("d1").unwrap().unwrap();
        assert_eq!(fetched.id, "d1");
        assert_eq!(fetched.content, "hello world");
    }

    #[test]
    fn get_missing_document_returns_none() {
        let db = test_db();
        assert!(db.get_document("nonexistent").unwrap().is_none());
    }

    #[test]
    fn insert_with_metadata() {
        let db = test_db();
        let mut doc = make_doc("d2", "with metadata");
        doc.metadata = vec![
            MetadataEntry { key: "author".into(), value: "Alice".into() },
            MetadataEntry { key: "tag".into(), value: "test".into() },
        ];
        db.insert_document(&doc).unwrap();

        let fetched = db.get_document("d2").unwrap().unwrap();
        assert_eq!(fetched.metadata.len(), 2);
        assert_eq!(fetched.metadata[0].key, "author");
    }

    #[test]
    fn delete_document_cascades() {
        let db = test_db();
        let doc = make_doc("d3", "to delete");
        db.insert_document(&doc).unwrap();

        let embedding = vec![0.0f32; 1024];
        db.insert_chunks_with_embeddings(
            "d3",
            &[("chunk one".to_string(), 0, 0, 9)],
            &[embedding],
        ).unwrap();

        assert!(db.delete_document("d3").unwrap());
        assert!(db.get_document("d3").unwrap().is_none());
    }

    #[test]
    fn delete_missing_returns_false() {
        let db = test_db();
        assert!(!db.delete_document("nope").unwrap());
    }

    #[test]
    fn list_documents_paginated() {
        let db = test_db();
        for i in 0..5 {
            db.insert_document(&make_doc(&format!("d{i}"), &format!("content {i}"))).unwrap();
        }

        let (docs, total) = db.list_documents(2, 0).unwrap();
        assert_eq!(total, 5);
        assert_eq!(docs.len(), 2);

        let (docs2, _) = db.list_documents(2, 2).unwrap();
        assert_eq!(docs2.len(), 2);
    }

    #[test]
    fn insert_chunks_and_semantic_search() {
        let db = test_db();
        let doc = make_doc("d4", "semantic test");
        db.insert_document(&doc).unwrap();

        let mut emb = vec![0.0f32; 1024];
        emb[0] = 1.0;
        db.insert_chunks_with_embeddings(
            "d4",
            &[("hello semantic".to_string(), 0, 0, 14)],
            &[emb.clone()],
        ).unwrap();

        let results = db.semantic_search(&emb, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "d4");
    }

    #[test]
    fn insert_chunks_and_grep_search() {
        let db = test_db();
        let doc = make_doc("d5", "grep test");
        db.insert_document(&doc).unwrap();

        let emb = vec![0.0f32; 1024];
        db.insert_chunks_with_embeddings(
            "d5",
            &[("The quick brown fox jumps".to_string(), 0, 0, 25)],
            &[emb],
        ).unwrap();

        let results = db.grep_search("quick brown", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "d5");
    }

    #[test]
    fn grep_search_accent_insensitive() {
        let db = test_db();
        let doc = make_doc("d6", "accent test");
        db.insert_document(&doc).unwrap();

        let emb = vec![0.0f32; 1024];
        db.insert_chunks_with_embeddings(
            "d6",
            &[("Le café est délicieux".to_string(), 0, 0, 24)],
            &[emb],
        ).unwrap();

        let results = db.grep_search("cafe", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "d6");
    }

    #[test]
    fn expiration_cleanup() {
        let db = test_db();
        let mut doc = make_doc("expired", "old content");
        doc.expiration = Some("2000-01-01T00:00:00Z".to_string());
        db.insert_document(&doc).unwrap();

        let mut doc2 = make_doc("fresh", "new content");
        doc2.expiration = Some("2099-01-01T00:00:00Z".to_string());
        db.insert_document(&doc2).unwrap();

        let cleaned = db.cleanup_expired().unwrap();
        assert_eq!(cleaned, 1);
        assert!(db.get_document("expired").unwrap().is_none());
        assert!(db.get_document("fresh").unwrap().is_some());
    }

    #[test]
    fn sync_folder_crud() {
        let db = test_db();
        let now = chrono::Utc::now().to_rfc3339();
        let folder = SyncFolder {
            id: "sf1".into(),
            folder_path: "/tmp/docs".into(),
            interval_secs: 60,
            last_scan_at: None,
            created_at: now,
        };
        db.upsert_sync_folder(&folder).unwrap();

        let folders = db.list_sync_folders().unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].folder_path, "/tmp/docs");

        db.update_sync_folder_scan_time("/tmp/docs", "2025-01-01T00:00:00Z").unwrap();
        let folders = db.list_sync_folders().unwrap();
        assert_eq!(folders[0].last_scan_at.as_deref(), Some("2025-01-01T00:00:00Z"));
    }

    #[test]
    fn status_counts() {
        let db = test_db();
        let status = db.status().unwrap();
        assert_eq!(status.document_count, 0);

        db.insert_document(&make_doc("s1", "a")).unwrap();
        db.insert_document(&make_doc("s2", "b")).unwrap();

        let emb = vec![0.0f32; 1024];
        db.insert_chunks_with_embeddings("s1", &[("chunk".into(), 0, 0, 5)], &[emb.clone()]).unwrap();

        let status = db.status().unwrap();
        assert_eq!(status.document_count, 2);
        assert_eq!(status.chunk_count, 1);
    }

    #[test]
    fn duplicate_document_id_rejected() {
        let db = test_db();
        let doc = make_doc("dup", "first");
        db.insert_document(&doc).unwrap();
        assert!(db.insert_document(&doc).is_err());
    }

    #[test]
    fn fts5_escape_basic() {
        assert_eq!(fts5_escape("hello world"), "\"hello\" \"world\"");
        assert_eq!(fts5_escape(""), "\"\"");
        assert_eq!(fts5_escape("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn find_by_source_path() {
        let db = test_db();
        let mut doc = make_doc("fp1", "file content");
        doc.source_type = SourceType::File;
        doc.source_path = Some("/tmp/test.txt".into());
        db.insert_document(&doc).unwrap();

        let found = db.find_document_by_source_path("/tmp/test.txt").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "fp1");

        assert!(db.find_document_by_source_path("/tmp/nope.txt").unwrap().is_none());
    }
}
