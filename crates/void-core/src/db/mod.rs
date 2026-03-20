//! Database access layer for conversations, messages, events, and sync state.

mod conversations;
mod database_access;
mod directory;
mod events;
mod hook_logs;
mod messages;
mod mute_sync;
mod row;
mod schema;
mod search;

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use tracing::{debug, info};

use crate::error::DbError;

pub use schema::SCHEMA_VERSION;
pub use search::fts5_escape;

pub struct Database {
    conn: Mutex<Connection>,
    hook_runner: std::sync::RwLock<Option<std::sync::Arc<crate::hooks::HookRunner>>>,
}

// SAFETY: All Connection access is protected by the Mutex.
unsafe impl Send for Database {}
unsafe impl Sync for Database {}

impl Database {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        info!(path = %path.display(), "opening database");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self {
            conn: Mutex::new(conn),
            hook_runner: std::sync::RwLock::new(None),
        };
        db.migrate()?;
        debug!("migration complete");
        Ok(db)
    }

    /// Attach a hook runner so that event hooks fire on new message inserts.
    pub fn set_hook_runner(&self, runner: std::sync::Arc<crate::hooks::HookRunner>) {
        if let Ok(mut guard) = self.hook_runner.write() {
            *guard = Some(runner);
        }
    }

    pub fn open_in_memory() -> Result<Self, DbError> {
        debug!("opening in-memory database");
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self {
            conn: Mutex::new(conn),
            hook_runner: std::sync::RwLock::new(None),
        };
        db.migrate()?;
        Ok(db)
    }

    pub(crate) fn conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, DbError> {
        self.conn.lock().map_err(|_| DbError::LockPoisoned)
    }

    fn migrate(&self) -> Result<(), DbError> {
        let conn = self.conn()?;
        schema::run_migrations(&conn)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
