use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use tracing::{debug, info};

use crate::models::{CalendarEvent, Contact, Conversation, ConversationKind, Message};

pub const SCHEMA_VERSION: i32 = 4;

pub struct Database {
    conn: Mutex<Connection>,
}

// SAFETY: All Connection access is protected by the Mutex.
unsafe impl Send for Database {}
unsafe impl Sync for Database {}

impl Database {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
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
        };
        db.migrate()?;
        debug!("migration complete");
        Ok(db)
    }

    pub fn open_in_memory() -> anyhow::Result<Self> {
        debug!("opening in-memory database");
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("database mutex poisoned")
    }

    fn migrate(&self) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);",
        )?;

        let current: Option<i32> = conn
            .query_row(
                "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        let version = current.unwrap_or(0);
        info!(
            current_version = version,
            target_version = SCHEMA_VERSION,
            "running migrations"
        );
        if version < 1 {
            drop(conn);
            self.migrate_v1()?;
        } else {
            drop(conn);
        }
        if version < 2 {
            self.migrate_v2()?;
        }
        if version < 3 {
            self.migrate_v3()?;
        }
        if version < 4 {
            self.migrate_v4()?;
        }
        Ok(())
    }

    fn migrate_v1(&self) -> anyhow::Result<()> {
        debug!("running migration v1");
        self.conn().execute_batch(
            "
            CREATE TABLE IF NOT EXISTS conversations (
                id              TEXT PRIMARY KEY,
                account_id      TEXT NOT NULL,
                external_id     TEXT NOT NULL,
                name            TEXT,
                kind            TEXT NOT NULL,
                last_message_at INTEGER,
                unread_count    INTEGER NOT NULL DEFAULT 0,
                metadata        TEXT,
                UNIQUE(account_id, external_id)
            );

            CREATE TABLE IF NOT EXISTS messages (
                id              TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id),
                account_id      TEXT NOT NULL,
                external_id     TEXT NOT NULL,
                sender          TEXT NOT NULL,
                sender_name     TEXT,
                body            TEXT,
                timestamp       INTEGER NOT NULL,
                is_from_me      INTEGER NOT NULL DEFAULT 0,
                reply_to_id     TEXT,
                media_type      TEXT,
                metadata        TEXT,
                UNIQUE(account_id, external_id)
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                body,
                sender_name,
                content=messages,
                content_rowid=rowid
            );

            CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(rowid, body, sender_name)
                VALUES (new.rowid, new.body, new.sender_name);
            END;

            CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, body, sender_name)
                VALUES ('delete', old.rowid, old.body, old.sender_name);
            END;

            CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, body, sender_name)
                VALUES ('delete', old.rowid, old.body, old.sender_name);
                INSERT INTO messages_fts(rowid, body, sender_name)
                VALUES (new.rowid, new.body, new.sender_name);
            END;

            CREATE TABLE IF NOT EXISTS events (
                id              TEXT PRIMARY KEY,
                account_id      TEXT NOT NULL,
                external_id     TEXT NOT NULL,
                title           TEXT NOT NULL,
                description     TEXT,
                location        TEXT,
                start_at        INTEGER NOT NULL,
                end_at          INTEGER NOT NULL,
                all_day         INTEGER NOT NULL DEFAULT 0,
                attendees       TEXT,
                status          TEXT,
                calendar_name   TEXT,
                meet_link       TEXT,
                metadata        TEXT,
                UNIQUE(account_id, external_id)
            );

            CREATE TABLE IF NOT EXISTS sync_state (
                account_id      TEXT NOT NULL,
                key             TEXT NOT NULL,
                value           TEXT NOT NULL,
                PRIMARY KEY(account_id, key)
            );

            INSERT INTO schema_version (version) VALUES (1);
        ",
        )?;
        Ok(())
    }

    fn migrate_v2(&self) -> anyhow::Result<()> {
        debug!("running migration v2");
        let conn = self.conn();
        conn.execute_batch(
            "
            ALTER TABLE messages ADD COLUMN synced_at INTEGER;
            ALTER TABLE events ADD COLUMN synced_at INTEGER;

            INSERT OR REPLACE INTO schema_version (version) VALUES (2);
        ",
        )?;
        Ok(())
    }

    fn migrate_v3(&self) -> anyhow::Result<()> {
        debug!("running migration v3");
        let conn = self.conn();
        conn.execute_batch(
            "
            ALTER TABLE messages ADD COLUMN is_read INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE messages ADD COLUMN is_archived INTEGER NOT NULL DEFAULT 0;

            INSERT OR REPLACE INTO schema_version (version) VALUES (3);
        ",
        )?;
        Ok(())
    }

    fn migrate_v4(&self) -> anyhow::Result<()> {
        debug!("running migration v4");
        let conn = self.conn();
        conn.execute_batch(
            "
            ALTER TABLE conversations ADD COLUMN connector TEXT NOT NULL DEFAULT '';
            ALTER TABLE messages ADD COLUMN connector TEXT NOT NULL DEFAULT '';
            ALTER TABLE events ADD COLUMN connector TEXT NOT NULL DEFAULT '';

            INSERT OR REPLACE INTO schema_version (version) VALUES (4);
        ",
        )?;
        Ok(())
    }

    // -- Conversations --

    pub fn upsert_conversation(&self, conv: &Conversation) -> anyhow::Result<()> {
        debug!(conversation_id = %conv.id, "upserting conversation");
        self.conn().execute(
            "INSERT INTO conversations (id, account_id, connector, external_id, name, kind, last_message_at, unread_count, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(account_id, external_id) DO UPDATE SET
                name = excluded.name,
                connector = excluded.connector,
                kind = excluded.kind,
                last_message_at = COALESCE(excluded.last_message_at, last_message_at),
                unread_count = excluded.unread_count,
                metadata = excluded.metadata",
            params![
                conv.id,
                conv.account_id,
                conv.connector,
                conv.external_id,
                conv.name,
                conv.kind.to_string(),
                conv.last_message_at,
                conv.unread_count,
                conv.metadata.as_ref().map(|v| v.to_string()),
            ],
        )?;
        Ok(())
    }

    pub fn list_conversations(
        &self,
        account_filter: Option<&str>,
        limit: i64,
    ) -> anyhow::Result<Vec<Conversation>> {
        let conn = self.conn();
        if let Some(acct) = account_filter {
            let pattern = format!("%{acct}%");
            let mut stmt = conn.prepare(
                "SELECT id, account_id, connector, external_id, name, kind, last_message_at, unread_count, metadata
                 FROM conversations WHERE account_id LIKE ?1
                 ORDER BY last_message_at DESC NULLS LAST LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![pattern, limit], row_to_conversation)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, account_id, connector, external_id, name, kind, last_message_at, unread_count, metadata
                 FROM conversations ORDER BY last_message_at DESC NULLS LAST LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit], row_to_conversation)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    }

    pub fn get_conversation(&self, id: &str) -> anyhow::Result<Option<Conversation>> {
        self.conn()
            .query_row(
                "SELECT id, account_id, connector, external_id, name, kind, last_message_at, unread_count, metadata
                 FROM conversations WHERE id = ?1",
                params![id],
                row_to_conversation,
            )
            .optional()
            .map_err(Into::into)
    }

    // -- Messages --

    pub fn upsert_message(&self, msg: &Message) -> anyhow::Result<()> {
        debug!(message_id = %msg.id, "upserting message");
        let now = chrono::Utc::now().timestamp();
        self.conn().execute(
            "INSERT INTO messages (id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_from_me, is_read, is_archived, reply_to_id, media_type, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT(account_id, external_id) DO UPDATE SET
                body = excluded.body,
                connector = excluded.connector,
                sender_name = excluded.sender_name,
                is_read = MAX(is_read, excluded.is_read),
                is_archived = MAX(is_archived, excluded.is_archived),
                media_type = excluded.media_type,
                metadata = excluded.metadata",
            params![
                msg.id,
                msg.conversation_id,
                msg.account_id,
                msg.connector,
                msg.external_id,
                msg.sender,
                msg.sender_name,
                msg.body,
                msg.timestamp,
                msg.synced_at.unwrap_or(now),
                msg.is_from_me as i32,
                msg.is_read as i32,
                msg.is_archived as i32,
                msg.reply_to_id,
                msg.media_type,
                msg.metadata.as_ref().map(|v| v.to_string()),
            ],
        )?;
        Ok(())
    }

    pub fn list_messages(
        &self,
        conversation_id: &str,
        limit: i64,
        since: Option<i64>,
        until: Option<i64>,
    ) -> anyhow::Result<Vec<Message>> {
        let mut sql = String::from(
            "SELECT id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_from_me, is_read, is_archived, reply_to_id, media_type, metadata
             FROM messages WHERE conversation_id = ?1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(conversation_id.to_string())];

        if let Some(s) = since {
            sql.push_str(&format!(" AND timestamp >= ?{}", param_values.len() + 1));
            param_values.push(Box::new(s));
        }
        if let Some(u) = until {
            sql.push_str(&format!(" AND timestamp <= ?{}", param_values.len() + 1));
            param_values.push(Box::new(u));
        }

        sql.push_str(&format!(
            " ORDER BY timestamp ASC LIMIT ?{}",
            param_values.len() + 1
        ));
        param_values.push(Box::new(limit));

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), row_to_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_message(&self, id: &str) -> anyhow::Result<Option<Message>> {
        self.conn()
            .query_row(
                "SELECT id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_from_me, is_read, is_archived, reply_to_id, media_type, metadata
                 FROM messages WHERE id = ?1",
                params![id],
                row_to_message,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn search_messages(&self, query: &str, limit: i64) -> anyhow::Result<Vec<Message>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT m.id, m.conversation_id, m.account_id, m.connector, m.external_id, m.sender, m.sender_name, m.body, m.timestamp, m.synced_at, m.is_from_me, m.is_read, m.is_archived, m.reply_to_id, m.media_type, m.metadata
             FROM messages_fts fts
             JOIN messages m ON m.rowid = fts.rowid
             WHERE messages_fts MATCH ?1
             ORDER BY bm25(messages_fts)
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit], row_to_message)?;
        let results: Vec<Message> = rows.collect::<Result<_, _>>()?;
        debug!(query, result_count = results.len(), "search messages");
        Ok(results)
    }

    pub fn recent_messages(
        &self,
        account_filter: Option<&str>,
        limit: i64,
        include_archived: bool,
    ) -> anyhow::Result<Vec<Message>> {
        let archive_clause = if include_archived {
            ""
        } else {
            " AND is_archived = 0"
        };
        let conn = self.conn();
        if let Some(acct) = account_filter {
            let pattern = format!("%{acct}%");
            let sql = format!(
                "SELECT id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_from_me, is_read, is_archived, reply_to_id, media_type, metadata
                 FROM messages WHERE account_id LIKE ?1{archive_clause}
                 ORDER BY timestamp DESC LIMIT ?2"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![pattern, limit], row_to_message)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        } else {
            let sql = format!(
                "SELECT id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_from_me, is_read, is_archived, reply_to_id, media_type, metadata
                 FROM messages WHERE 1=1{archive_clause} ORDER BY timestamp DESC LIMIT ?1"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![limit], row_to_message)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    }

    pub fn mark_message_read(&self, id: &str) -> anyhow::Result<bool> {
        debug!(message_id = %id, "marking message as read");
        let updated = self
            .conn()
            .execute("UPDATE messages SET is_read = 1 WHERE id = ?1", params![id])?;
        Ok(updated > 0)
    }

    pub fn mark_message_archived(&self, id: &str) -> anyhow::Result<bool> {
        debug!(message_id = %id, "marking message as archived");
        let updated = self.conn().execute(
            "UPDATE messages SET is_archived = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(updated > 0)
    }

    pub fn update_message_metadata(
        &self,
        id: &str,
        metadata: &serde_json::Value,
    ) -> anyhow::Result<bool> {
        debug!(message_id = %id, "updating message metadata");
        let json = serde_json::to_string(metadata)?;
        let updated = self.conn().execute(
            "UPDATE messages SET metadata = ?2 WHERE id = ?1",
            params![id, json],
        )?;
        Ok(updated > 0)
    }

    pub fn find_message_by_external_id(
        &self,
        account_id: &str,
        external_id: &str,
    ) -> anyhow::Result<Option<Message>> {
        self.conn()
            .query_row(
                "SELECT id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_from_me, is_read, is_archived, reply_to_id, media_type, metadata
                 FROM messages WHERE account_id = ?1 AND external_id = ?2",
                params![account_id, external_id],
                row_to_message,
            )
            .optional()
            .map_err(Into::into)
    }

    // -- Calendar events --

    pub fn upsert_event(&self, event: &CalendarEvent) -> anyhow::Result<()> {
        debug!(event_id = %event.id, "upserting event");
        self.conn().execute(
            "INSERT INTO events (id, account_id, connector, external_id, title, description, location, start_at, end_at, all_day, attendees, status, calendar_name, meet_link, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(account_id, external_id) DO UPDATE SET
                title = excluded.title,
                connector = excluded.connector,
                description = excluded.description,
                location = excluded.location,
                start_at = excluded.start_at,
                end_at = excluded.end_at,
                all_day = excluded.all_day,
                attendees = excluded.attendees,
                status = excluded.status,
                calendar_name = excluded.calendar_name,
                meet_link = excluded.meet_link,
                metadata = excluded.metadata",
            params![
                event.id,
                event.account_id,
                event.connector,
                event.external_id,
                event.title,
                event.description,
                event.location,
                event.start_at,
                event.end_at,
                event.all_day as i32,
                event.attendees.as_ref().map(|v| v.to_string()),
                event.status,
                event.calendar_name,
                event.meet_link,
                event.metadata.as_ref().map(|v| v.to_string()),
            ],
        )?;
        Ok(())
    }

    pub fn list_events(
        &self,
        from: Option<i64>,
        to: Option<i64>,
        limit: i64,
    ) -> anyhow::Result<Vec<CalendarEvent>> {
        let mut sql = String::from(
            "SELECT id, account_id, connector, external_id, title, description, location, start_at, end_at, all_day, attendees, status, calendar_name, meet_link, metadata FROM events WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(f) = from {
            sql.push_str(&format!(" AND end_at >= ?{}", param_values.len() + 1));
            param_values.push(Box::new(f));
        }
        if let Some(t) = to {
            sql.push_str(&format!(" AND start_at <= ?{}", param_values.len() + 1));
            param_values.push(Box::new(t));
        }

        sql.push_str(&format!(
            " ORDER BY start_at ASC LIMIT ?{}",
            param_values.len() + 1
        ));
        param_values.push(Box::new(limit));

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), row_to_event)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // -- Contacts --

    pub fn list_contacts(
        &self,
        account_filter: Option<&str>,
        search: Option<&str>,
        limit: i64,
    ) -> anyhow::Result<Vec<Contact>> {
        let mut sql = String::from(
            "SELECT sender, sender_name, account_id, connector, COUNT(*) as msg_count, MAX(timestamp) as last_ts
             FROM messages WHERE is_from_me = 0",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(acct) = account_filter {
            let pattern = format!("%{acct}%");
            sql.push_str(&format!(" AND account_id LIKE ?{}", param_values.len() + 1));
            param_values.push(Box::new(pattern));
        }

        if let Some(q) = search {
            let pattern = format!("%{q}%");
            sql.push_str(&format!(
                " AND (sender LIKE ?{} OR sender_name LIKE ?{})",
                param_values.len() + 1,
                param_values.len() + 1
            ));
            param_values.push(Box::new(pattern));
        }

        sql.push_str(" GROUP BY sender, account_id");
        sql.push_str(&format!(
            " ORDER BY last_ts DESC LIMIT ?{}",
            param_values.len() + 1
        ));
        param_values.push(Box::new(limit));

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            Ok(Contact {
                sender: row.get(0)?,
                sender_name: row.get(1)?,
                account_id: row.get(2)?,
                connector: row.get(3)?,
                message_count: row.get(4)?,
                last_message_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // -- Channels (groups, channels — excluding DMs and threads) --

    pub fn list_channels(
        &self,
        account_filter: Option<&str>,
        search: Option<&str>,
        limit: i64,
    ) -> anyhow::Result<Vec<Conversation>> {
        let mut sql = String::from(
            "SELECT id, account_id, connector, external_id, name, kind, last_message_at, unread_count, metadata
             FROM conversations WHERE kind IN ('group', 'channel')",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(acct) = account_filter {
            let pattern = format!("%{acct}%");
            sql.push_str(&format!(" AND account_id LIKE ?{}", param_values.len() + 1));
            param_values.push(Box::new(pattern));
        }

        if let Some(q) = search {
            let pattern = format!("%{q}%");
            sql.push_str(&format!(" AND name LIKE ?{}", param_values.len() + 1));
            param_values.push(Box::new(pattern));
        }

        sql.push_str(&format!(
            " ORDER BY last_message_at DESC NULLS LAST LIMIT ?{}",
            param_values.len() + 1
        ));
        param_values.push(Box::new(limit));

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), row_to_conversation)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // -- Sync state --

    pub fn get_sync_state(&self, account_id: &str, key: &str) -> anyhow::Result<Option<String>> {
        self.conn()
            .query_row(
                "SELECT value FROM sync_state WHERE account_id = ?1 AND key = ?2",
                params![account_id, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn set_sync_state(&self, account_id: &str, key: &str, value: &str) -> anyhow::Result<()> {
        debug!(account_id, key, "setting sync state");
        self.conn().execute(
            "INSERT INTO sync_state (account_id, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(account_id, key) DO UPDATE SET value = excluded.value",
            params![account_id, key, value],
        )?;
        Ok(())
    }
}

fn parse_kind(s: &str) -> ConversationKind {
    match s {
        "dm" => ConversationKind::Dm,
        "group" => ConversationKind::Group,
        "channel" => ConversationKind::Channel,
        "thread" => ConversationKind::Thread,
        _ => ConversationKind::Dm,
    }
}

fn parse_json_opt(s: Option<String>) -> Option<serde_json::Value> {
    s.and_then(|v| serde_json::from_str(&v).ok())
}

fn row_to_conversation(row: &rusqlite::Row) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get(0)?,
        account_id: row.get(1)?,
        connector: row.get(2)?,
        external_id: row.get(3)?,
        name: row.get(4)?,
        kind: parse_kind(&row.get::<_, String>(5)?),
        last_message_at: row.get(6)?,
        unread_count: row.get(7)?,
        metadata: parse_json_opt(row.get(8)?),
    })
}

fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        account_id: row.get(2)?,
        connector: row.get(3)?,
        external_id: row.get(4)?,
        sender: row.get(5)?,
        sender_name: row.get(6)?,
        body: row.get(7)?,
        timestamp: row.get(8)?,
        synced_at: row.get(9)?,
        is_from_me: row.get::<_, i32>(10)? != 0,
        is_read: row.get::<_, i32>(11)? != 0,
        is_archived: row.get::<_, i32>(12)? != 0,
        reply_to_id: row.get(13)?,
        media_type: row.get(14)?,
        metadata: parse_json_opt(row.get(15)?),
    })
}

fn row_to_event(row: &rusqlite::Row) -> rusqlite::Result<CalendarEvent> {
    Ok(CalendarEvent {
        id: row.get(0)?,
        account_id: row.get(1)?,
        connector: row.get(2)?,
        external_id: row.get(3)?,
        title: row.get(4)?,
        description: row.get(5)?,
        location: row.get(6)?,
        start_at: row.get(7)?,
        end_at: row.get(8)?,
        all_day: row.get::<_, i32>(9)? != 0,
        attendees: parse_json_opt(row.get(10)?),
        status: row.get(11)?,
        calendar_name: row.get(12)?,
        meet_link: row.get(13)?,
        metadata: parse_json_opt(row.get(14)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn make_conversation(id: &str, account_id: &str, ext_id: &str) -> Conversation {
        Conversation {
            id: id.into(),
            account_id: account_id.into(),
            connector: "slack".into(),
            external_id: ext_id.into(),
            name: Some(format!("Conv {id}")),
            kind: ConversationKind::Dm,
            last_message_at: Some(1_700_000_000),
            unread_count: 0,
            metadata: None,
        }
    }

    fn make_message(id: &str, conv_id: &str, account_id: &str, body: &str, ts: i64) -> Message {
        Message {
            id: id.into(),
            conversation_id: conv_id.into(),
            account_id: account_id.into(),
            connector: "slack".into(),
            external_id: format!("ext-{id}"),
            sender: "sender@test".into(),
            sender_name: Some("Test Sender".into()),
            body: Some(body.into()),
            timestamp: ts,
            synced_at: None,
            is_from_me: false,
            is_read: false,
            is_archived: false,
            reply_to_id: None,
            media_type: None,
            metadata: None,
        }
    }

    #[test]
    fn migration_runs() {
        let db = test_db();
        let conn = db.conn();
        let version: i32 = conn
            .query_row(
                "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn conversation_crud() {
        let db = test_db();
        let conv = make_conversation("c1", "work-slack", "C123");

        db.upsert_conversation(&conv).unwrap();
        let loaded = db.get_conversation("c1").unwrap().unwrap();
        assert_eq!(loaded.name.as_deref(), Some("Conv c1"));

        let list = db.list_conversations(None, 100).unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn conversation_upsert_updates() {
        let db = test_db();
        let mut conv = make_conversation("c1", "work-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        conv.name = Some("Updated".into());
        db.upsert_conversation(&conv).unwrap();

        let loaded = db.get_conversation("c1").unwrap().unwrap();
        assert_eq!(loaded.name.as_deref(), Some("Updated"));
    }

    #[test]
    fn message_crud() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let msg = make_message("m1", "c1", "test-slack", "Hello world", 1_700_000_000);
        db.upsert_message(&msg).unwrap();

        let loaded = db.get_message("m1").unwrap().unwrap();
        assert_eq!(loaded.body.as_deref(), Some("Hello world"));

        let list = db.list_messages("c1", 100, None, None).unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn message_synced_at_auto_populated() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let msg = make_message("m1", "c1", "test-slack", "hello", 1_700_000_000);
        assert!(msg.synced_at.is_none());

        db.upsert_message(&msg).unwrap();

        let loaded = db.get_message("m1").unwrap().unwrap();
        assert!(
            loaded.synced_at.is_some(),
            "synced_at should be auto-populated on insert"
        );
        let synced = loaded.synced_at.unwrap();
        assert!(
            synced >= loaded.timestamp,
            "synced_at ({synced}) should be >= message timestamp ({})",
            loaded.timestamp
        );
    }

    #[test]
    fn message_synced_at_preserved_on_upsert() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let msg = make_message("m1", "c1", "test-slack", "original", 1_700_000_000);
        db.upsert_message(&msg).unwrap();

        let first_load = db.get_message("m1").unwrap().unwrap();
        let original_synced_at = first_load.synced_at.unwrap();

        let mut updated = make_message("m1", "c1", "test-slack", "edited body", 1_700_000_000);
        updated.body = Some("edited body".into());
        db.upsert_message(&updated).unwrap();

        let reloaded = db.get_message("m1").unwrap().unwrap();
        assert_eq!(reloaded.body.as_deref(), Some("edited body"));
        assert_eq!(
            reloaded.synced_at.unwrap(),
            original_synced_at,
            "synced_at should not change on upsert/update"
        );
    }

    #[test]
    fn fts5_search() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        db.upsert_message(&make_message(
            "m1",
            "c1",
            "test-slack",
            "meeting tomorrow at 10am",
            1_700_000_000,
        ))
        .unwrap();
        db.upsert_message(&make_message(
            "m2",
            "c1",
            "test-slack",
            "lunch plans for Friday",
            1_700_000_001,
        ))
        .unwrap();
        db.upsert_message(&make_message(
            "m3",
            "c1",
            "test-slack",
            "quarterly budget review meeting",
            1_700_000_002,
        ))
        .unwrap();

        let results = db.search_messages("meeting", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn message_date_range_filter() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        db.upsert_message(&make_message(
            "m1",
            "c1",
            "test-slack",
            "old msg",
            1_000_000,
        ))
        .unwrap();
        db.upsert_message(&make_message(
            "m2",
            "c1",
            "test-slack",
            "mid msg",
            2_000_000,
        ))
        .unwrap();
        db.upsert_message(&make_message(
            "m3",
            "c1",
            "test-slack",
            "new msg",
            3_000_000,
        ))
        .unwrap();

        let results = db
            .list_messages("c1", 100, Some(1_500_000), Some(2_500_000))
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "m2");
    }

    #[test]
    fn event_crud() {
        let db = test_db();
        let event = CalendarEvent {
            id: "e1".into(),
            account_id: "my-calendar".into(),
            connector: "calendar".into(),
            external_id: "goog123".into(),
            title: "Standup".into(),
            description: None,
            location: None,
            start_at: 1_700_000_000,
            end_at: 1_700_001_800,
            all_day: false,
            attendees: None,
            status: Some("confirmed".into()),
            calendar_name: Some("primary".into()),
            meet_link: Some("https://meet.google.com/abc".into()),
            metadata: None,
        };

        db.upsert_event(&event).unwrap();
        let list = db
            .list_events(Some(1_700_000_000), Some(1_700_002_000), 100)
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].meet_link.as_deref(),
            Some("https://meet.google.com/abc")
        );
    }

    #[test]
    fn sync_state_crud() {
        let db = test_db();
        db.set_sync_state("gmail-1", "history_id", "12345").unwrap();

        let val = db.get_sync_state("gmail-1", "history_id").unwrap();
        assert_eq!(val.as_deref(), Some("12345"));

        db.set_sync_state("gmail-1", "history_id", "67890").unwrap();
        let val = db.get_sync_state("gmail-1", "history_id").unwrap();
        assert_eq!(val.as_deref(), Some("67890"));

        let missing = db.get_sync_state("gmail-1", "nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn recent_messages_ordered() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        db.upsert_message(&make_message("m1", "c1", "test-slack", "first", 1_000))
            .unwrap();
        db.upsert_message(&make_message("m2", "c1", "test-slack", "second", 2_000))
            .unwrap();
        db.upsert_message(&make_message("m3", "c1", "test-slack", "third", 3_000))
            .unwrap();

        let results = db.recent_messages(None, 2, true).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "m3");
        assert_eq!(results[1].id, "m2");
    }

    #[test]
    fn list_contacts_basic() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let mut m1 = make_message("m1", "c1", "test-slack", "hello", 1_000);
        m1.sender = "alice@test.com".into();
        m1.sender_name = Some("Alice".into());
        m1.is_from_me = false;
        db.upsert_message(&m1).unwrap();

        let mut m2 = make_message("m2", "c1", "test-slack", "world", 2_000);
        m2.sender = "alice@test.com".into();
        m2.sender_name = Some("Alice".into());
        m2.is_from_me = false;
        db.upsert_message(&m2).unwrap();

        let mut m3 = make_message("m3", "c1", "test-slack", "bye", 3_000);
        m3.sender = "bob@test.com".into();
        m3.sender_name = Some("Bob".into());
        m3.is_from_me = false;
        db.upsert_message(&m3).unwrap();

        let contacts = db.list_contacts(None, None, 100).unwrap();
        assert_eq!(contacts.len(), 2);
        assert_eq!(contacts[0].sender, "bob@test.com");
        assert_eq!(contacts[0].message_count, 1);
        assert_eq!(contacts[1].sender, "alice@test.com");
        assert_eq!(contacts[1].message_count, 2);
    }

    #[test]
    fn list_contacts_search() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let mut m1 = make_message("m1", "c1", "test-slack", "hi", 1_000);
        m1.sender = "alice@test.com".into();
        m1.sender_name = Some("Alice".into());
        m1.is_from_me = false;
        db.upsert_message(&m1).unwrap();

        let mut m2 = make_message("m2", "c1", "test-slack", "hey", 2_000);
        m2.sender = "bob@test.com".into();
        m2.sender_name = Some("Bob".into());
        m2.is_from_me = false;
        db.upsert_message(&m2).unwrap();

        let results = db.list_contacts(None, Some("alice"), 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].sender, "alice@test.com");
    }

    #[test]
    fn list_contacts_account_filter() {
        let db = test_db();
        let c1 = make_conversation("c1", "gladiaio", "C123");
        db.upsert_conversation(&c1).unwrap();
        let mut c2 = make_conversation("c2", "33651090627", "W123");
        c2.connector = "whatsapp".into();
        db.upsert_conversation(&c2).unwrap();

        let mut m1 = make_message("m1", "c1", "gladiaio", "hi", 1_000);
        m1.sender = "alice@slack".into();
        m1.is_from_me = false;
        db.upsert_message(&m1).unwrap();

        let mut m2 = make_message("m2", "c2", "33651090627", "hey", 2_000);
        m2.sender = "bob@wa".into();
        m2.connector = "whatsapp".into();
        m2.is_from_me = false;
        db.upsert_message(&m2).unwrap();

        let results = db.list_contacts(Some("gladiaio"), None, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].sender, "alice@slack");
    }

    #[test]
    fn list_contacts_excludes_own_messages() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let mut m1 = make_message("m1", "c1", "test-slack", "hi", 1_000);
        m1.sender = "me@test.com".into();
        m1.is_from_me = true;
        db.upsert_message(&m1).unwrap();

        let contacts = db.list_contacts(None, None, 100).unwrap();
        assert!(contacts.is_empty());
    }

    #[test]
    fn list_channels_basic() {
        let db = test_db();
        let mut group = make_conversation("c1", "test-slack", "G123");
        group.kind = ConversationKind::Group;
        group.name = Some("Engineering".into());
        db.upsert_conversation(&group).unwrap();

        let mut channel = make_conversation("c2", "test-slack", "C456");
        channel.kind = ConversationKind::Channel;
        channel.name = Some("General".into());
        db.upsert_conversation(&channel).unwrap();

        let dm = make_conversation("c3", "test-slack", "D789");
        db.upsert_conversation(&dm).unwrap();

        let channels = db.list_channels(None, None, 100).unwrap();
        assert_eq!(channels.len(), 2);
    }

    #[test]
    fn list_channels_search() {
        let db = test_db();
        let mut c1 = make_conversation("c1", "test-slack", "G123");
        c1.kind = ConversationKind::Group;
        c1.name = Some("Engineering".into());
        db.upsert_conversation(&c1).unwrap();

        let mut c2 = make_conversation("c2", "test-slack", "C456");
        c2.kind = ConversationKind::Channel;
        c2.name = Some("General".into());
        db.upsert_conversation(&c2).unwrap();

        let results = db.list_channels(None, Some("engi"), 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name.as_deref(), Some("Engineering"));
    }
}
