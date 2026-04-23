use zerocopy::IntoBytes;

use super::KbDatabase;
use crate::normalize::normalize_for_search;

impl KbDatabase {
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
}
