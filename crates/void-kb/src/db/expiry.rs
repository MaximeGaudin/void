use super::KbDatabase;

impl KbDatabase {
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
}
