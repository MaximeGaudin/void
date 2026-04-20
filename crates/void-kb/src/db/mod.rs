use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use tracing::{debug, info};

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

    pub(super) fn conn(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow::anyhow!("KB database lock poisoned"))
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

mod chunks;
mod documents;
mod expiry;
mod search;
mod status;
mod sync_folders;

/// Inherent methods on [`KbDatabase`] live in the submodules above; this re-export is for tests.
#[cfg(test)]
pub(crate) use search::fts5_escape;

#[cfg(test)]
mod tests;
