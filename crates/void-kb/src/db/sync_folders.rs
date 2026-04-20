use crate::models::SyncFolder;
use super::KbDatabase;

impl KbDatabase {
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

    /// Remove a sync folder registration and delete all documents that were
    /// indexed from it (source_type = 'synced' with source_path under the folder).
    /// Returns the number of documents removed.
    pub fn remove_sync_folder(&self, folder_path: &str) -> anyhow::Result<usize> {
        let doc_ids = {
            let conn = self.conn()?;
            let pattern = format!("{}%", folder_path);
            let mut stmt = conn.prepare(
                "SELECT id FROM kb_documents WHERE source_type = 'synced' AND source_path LIKE ?1",
            )?;
            let ids: Vec<String> = stmt
                .query_map([&pattern], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            ids
        };

        let count = doc_ids.len();
        for id in &doc_ids {
            self.delete_document(id)?;
        }

        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM kb_sync_folders WHERE folder_path = ?1",
            [folder_path],
        )?;

        Ok(count)
    }

    pub fn update_sync_folder_scan_time(
        &self,
        folder_path: &str,
        time: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE kb_sync_folders SET last_scan_at = ?1 WHERE folder_path = ?2",
            rusqlite::params![time, folder_path],
        )?;
        Ok(())
    }
}
