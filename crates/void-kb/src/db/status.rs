use crate::models::KbStatus;
use super::KbDatabase;

impl KbDatabase {
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
}
