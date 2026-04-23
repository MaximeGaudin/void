use rusqlite::Connection;

use super::KbDatabase;
use crate::models::*;

impl KbDatabase {
    pub fn insert_document(&self, doc: &Document) -> anyhow::Result<()> {
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

    pub(super) fn get_metadata_inner(
        &self,
        conn: &Connection,
        doc_id: &str,
    ) -> anyhow::Result<Vec<MetadataEntry>> {
        let mut stmt =
            conn.prepare("SELECT key, value FROM kb_document_metadata WHERE document_id = ?1")?;
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
