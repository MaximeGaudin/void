use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{CalendarEvent, ChannelType, Conversation, ConversationKind, Message};

pub const SCHEMA_VERSION: i32 = 1;

pub struct Database {
    conn: Mutex<Connection>,
}

// SAFETY: All Connection access is protected by the Mutex.
unsafe impl Send for Database {}
unsafe impl Sync for Database {}

impl Database {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
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
        Ok(db)
    }

    pub fn open_in_memory() -> anyhow::Result<Self> {
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
        if version < 1 {
            drop(conn);
            self.migrate_v1()?;
        }
        Ok(())
    }

    fn migrate_v1(&self) -> anyhow::Result<()> {
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

    // -- Conversations --

    pub fn upsert_conversation(&self, conv: &Conversation) -> anyhow::Result<()> {
        self.conn().execute(
            "INSERT INTO conversations (id, account_id, external_id, name, kind, last_message_at, unread_count, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(account_id, external_id) DO UPDATE SET
                name = excluded.name,
                kind = excluded.kind,
                last_message_at = COALESCE(excluded.last_message_at, last_message_at),
                unread_count = excluded.unread_count,
                metadata = excluded.metadata",
            params![
                conv.id,
                conv.account_id,
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
        channel_type_filter: Option<&str>,
        limit: i64,
    ) -> anyhow::Result<Vec<Conversation>> {
        let conn = self.conn();
        if let Some(ct) = channel_type_filter {
            let pattern = format!("%-{ct}");
            let mut stmt = conn.prepare(
                "SELECT id, account_id, external_id, name, kind, last_message_at, unread_count, metadata
                 FROM conversations WHERE account_id LIKE ?1
                 ORDER BY last_message_at DESC NULLS LAST LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![pattern, limit], row_to_conversation)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, account_id, external_id, name, kind, last_message_at, unread_count, metadata
                 FROM conversations ORDER BY last_message_at DESC NULLS LAST LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit], row_to_conversation)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    }

    pub fn get_conversation(&self, id: &str) -> anyhow::Result<Option<Conversation>> {
        self.conn()
            .query_row(
                "SELECT id, account_id, external_id, name, kind, last_message_at, unread_count, metadata
                 FROM conversations WHERE id = ?1",
                params![id],
                row_to_conversation,
            )
            .optional()
            .map_err(Into::into)
    }

    // -- Messages --

    pub fn upsert_message(&self, msg: &Message) -> anyhow::Result<()> {
        self.conn().execute(
            "INSERT INTO messages (id, conversation_id, account_id, external_id, sender, sender_name, body, timestamp, is_from_me, reply_to_id, media_type, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(account_id, external_id) DO UPDATE SET
                body = excluded.body,
                sender_name = excluded.sender_name,
                media_type = excluded.media_type,
                metadata = excluded.metadata",
            params![
                msg.id,
                msg.conversation_id,
                msg.account_id,
                msg.external_id,
                msg.sender,
                msg.sender_name,
                msg.body,
                msg.timestamp,
                msg.is_from_me as i32,
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
            "SELECT id, conversation_id, account_id, external_id, sender, sender_name, body, timestamp, is_from_me, reply_to_id, media_type, metadata
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
                "SELECT id, conversation_id, account_id, external_id, sender, sender_name, body, timestamp, is_from_me, reply_to_id, media_type, metadata
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
            "SELECT m.id, m.conversation_id, m.account_id, m.external_id, m.sender, m.sender_name, m.body, m.timestamp, m.is_from_me, m.reply_to_id, m.media_type, m.metadata
             FROM messages_fts fts
             JOIN messages m ON m.rowid = fts.rowid
             WHERE messages_fts MATCH ?1
             ORDER BY bm25(messages_fts)
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit], row_to_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn recent_messages(
        &self,
        channel_filter: Option<&str>,
        limit: i64,
    ) -> anyhow::Result<Vec<Message>> {
        let conn = self.conn();
        if let Some(ct) = channel_filter {
            let pattern = format!("%-{ct}");
            let mut stmt = conn.prepare(
                "SELECT id, conversation_id, account_id, external_id, sender, sender_name, body, timestamp, is_from_me, reply_to_id, media_type, metadata
                 FROM messages WHERE account_id LIKE ?1
                 ORDER BY timestamp DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![pattern, limit], row_to_message)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, conversation_id, account_id, external_id, sender, sender_name, body, timestamp, is_from_me, reply_to_id, media_type, metadata
                 FROM messages ORDER BY timestamp DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit], row_to_message)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    }

    // -- Calendar events --

    pub fn upsert_event(&self, event: &CalendarEvent) -> anyhow::Result<()> {
        self.conn().execute(
            "INSERT INTO events (id, account_id, external_id, title, description, location, start_at, end_at, all_day, attendees, status, calendar_name, meet_link, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(account_id, external_id) DO UPDATE SET
                title = excluded.title,
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
            "SELECT id, account_id, external_id, title, description, location, start_at, end_at, all_day, attendees, status, calendar_name, meet_link, metadata FROM events WHERE 1=1",
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
        external_id: row.get(2)?,
        name: row.get(3)?,
        kind: parse_kind(&row.get::<_, String>(4)?),
        last_message_at: row.get(5)?,
        unread_count: row.get(6)?,
        metadata: parse_json_opt(row.get(7)?),
    })
}

fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        account_id: row.get(2)?,
        external_id: row.get(3)?,
        sender: row.get(4)?,
        sender_name: row.get(5)?,
        body: row.get(6)?,
        timestamp: row.get(7)?,
        is_from_me: row.get::<_, i32>(8)? != 0,
        reply_to_id: row.get(9)?,
        media_type: row.get(10)?,
        metadata: parse_json_opt(row.get(11)?),
    })
}

fn row_to_event(row: &rusqlite::Row) -> rusqlite::Result<CalendarEvent> {
    Ok(CalendarEvent {
        id: row.get(0)?,
        account_id: row.get(1)?,
        external_id: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        location: row.get(5)?,
        start_at: row.get(6)?,
        end_at: row.get(7)?,
        all_day: row.get::<_, i32>(8)? != 0,
        attendees: parse_json_opt(row.get(9)?),
        status: row.get(10)?,
        calendar_name: row.get(11)?,
        meet_link: row.get(12)?,
        metadata: parse_json_opt(row.get(13)?),
    })
}

impl ChannelType {
    pub fn account_id_suffix(&self) -> &'static str {
        match self {
            Self::WhatsApp => "whatsapp",
            Self::Slack => "slack",
            Self::Gmail => "gmail",
            Self::Calendar => "calendar",
        }
    }
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
            external_id: format!("ext-{id}"),
            sender: "sender@test".into(),
            sender_name: Some("Test Sender".into()),
            body: Some(body.into()),
            timestamp: ts,
            is_from_me: false,
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

        let results = db.recent_messages(None, 2).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "m3");
        assert_eq!(results[1].id, "m2");
    }
}
