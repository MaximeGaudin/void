use zerocopy::IntoBytes;

use super::KbDatabase;
use crate::normalize::normalize_for_search;

impl KbDatabase {
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
}

/// Used by unit tests in `tests.rs`.
pub(crate) fn fts5_escape(query: &str) -> String {
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
