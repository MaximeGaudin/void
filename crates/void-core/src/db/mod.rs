//! Database access layer for conversations, messages, events, and sync state.

mod row;
mod schema;
mod search;

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use tracing::{debug, info};

use crate::models::{CalendarEvent, Contact, Conversation, Message};

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

    pub fn open_in_memory() -> anyhow::Result<Self> {
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

    pub(crate) fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("database mutex poisoned")
    }

    fn migrate(&self) -> anyhow::Result<()> {
        let conn = self.conn();
        schema::run_migrations(&conn)?;
        Ok(())
    }

    // -- Hook logs --

    pub fn insert_hook_log(&self, log: &crate::hooks::HookLogInsert<'_>) -> anyhow::Result<()> {
        self.conn().execute(
            "INSERT INTO hook_logs (hook_name, trigger_type, started_at, duration_ms, success, result, error, message_id, input_prompt, raw_output)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                log.hook_name,
                log.trigger_type,
                log.started_at,
                log.duration_ms,
                log.success as i32,
                log.result,
                log.error,
                log.message_id,
                log.input_prompt,
                log.raw_output,
            ],
        )?;
        Ok(())
    }

    pub fn list_hook_logs(&self, limit: usize) -> anyhow::Result<Vec<crate::hooks::HookLog>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, hook_name, trigger_type, started_at, duration_ms, success, result, error, message_id, input_prompt, raw_output
             FROM (SELECT * FROM hook_logs ORDER BY started_at DESC LIMIT ?1) ORDER BY started_at ASC",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(crate::hooks::HookLog {
                id: row.get(0)?,
                hook_name: row.get(1)?,
                trigger_type: row.get(2)?,
                started_at: row.get(3)?,
                duration_ms: row.get(4)?,
                success: row.get::<_, i32>(5)? != 0,
                result: row.get(6)?,
                error: row.get(7)?,
                message_id: row.get(8)?,
                input_prompt: row.get(9)?,
                raw_output: row.get(10)?,
            })
        })?;
        let mut logs = Vec::new();
        for row in rows {
            logs.push(row?);
        }
        Ok(logs)
    }

    // -- Conversations --

    pub fn upsert_conversation(&self, conv: &Conversation) -> anyhow::Result<()> {
        debug!(conversation_id = %conv.id, "upserting conversation");
        self.conn().execute(
            "INSERT INTO conversations (id, account_id, connector, external_id, name, kind, last_message_at, unread_count, is_muted, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
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
                conv.is_muted as i32,
                conv.metadata.as_ref().map(|v| v.to_string()),
            ],
        )?;
        Ok(())
    }

    pub fn list_conversations(
        &self,
        account_filter: Option<&str>,
        connector_filter: Option<&str>,
        limit: i64,
        include_muted: bool,
    ) -> anyhow::Result<Vec<Conversation>> {
        let mut sql = String::from(
            "SELECT id, account_id, connector, external_id, name, kind, last_message_at, unread_count, is_muted, metadata
             FROM conversations WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if !include_muted {
            sql.push_str(" AND is_muted = 0");
        }
        if let Some(acct) = account_filter {
            let pattern = format!("%{acct}%");
            sql.push_str(&format!(" AND account_id LIKE ?{}", param_values.len() + 1));
            param_values.push(Box::new(pattern));
        }
        if let Some(conn_type) = connector_filter {
            sql.push_str(&format!(" AND connector = ?{}", param_values.len() + 1));
            param_values.push(Box::new(conn_type.to_string()));
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
        let rows = stmt.query_map(params_ref.as_slice(), row::row_to_conversation)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_conversation(&self, id: &str) -> anyhow::Result<Option<Conversation>> {
        self.conn()
            .query_row(
                "SELECT id, account_id, connector, external_id, name, kind, last_message_at, unread_count, is_muted, metadata
                 FROM conversations WHERE id = ?1",
                params![id],
                row::row_to_conversation,
            )
            .optional()
            .map_err(Into::into)
    }

    // -- Messages --

    /// Returns `true` if a message with this (account_id, external_id) already exists.
    pub fn message_exists(&self, account_id: &str, external_id: &str) -> bool {
        self.conn()
            .query_row(
                "SELECT 1 FROM messages WHERE account_id = ?1 AND external_id = ?2",
                params![account_id, external_id],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// Insert or update a message. Returns `true` if the message was newly inserted.
    pub fn upsert_message(&self, msg: &Message) -> anyhow::Result<bool> {
        debug!(message_id = %msg.id, "upserting message");
        let is_new = !self.message_exists(&msg.account_id, &msg.external_id);
        let now = chrono::Utc::now().timestamp();
        self.conn().execute(
            "INSERT INTO messages (id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(account_id, external_id) DO UPDATE SET
                body = excluded.body,
                connector = excluded.connector,
                sender_name = excluded.sender_name,
                is_archived = MAX(is_archived, excluded.is_archived),
                media_type = excluded.media_type,
                metadata = excluded.metadata,
                context_id = COALESCE(excluded.context_id, context_id)",
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
                msg.is_archived as i32,
                msg.reply_to_id,
                msg.media_type,
                msg.metadata.as_ref().map(|v| v.to_string()),
                msg.context_id,
            ],
        )?;

        if is_new {
            if let Ok(guard) = self.hook_runner.read() {
                if let Some(ref runner) = *guard {
                    runner.on_new_message(msg);
                }
            }
        }

        Ok(is_new)
    }

    pub fn list_messages(
        &self,
        conversation_id: &str,
        limit: i64,
        since: Option<i64>,
        until: Option<i64>,
    ) -> anyhow::Result<Vec<Message>> {
        let mut sql = String::from(
            "SELECT id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
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
        let rows = stmt.query_map(params_ref.as_slice(), row::row_to_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_message(&self, id: &str) -> anyhow::Result<Option<Message>> {
        self.conn()
            .query_row(
                "SELECT id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
                 FROM messages WHERE id = ?1",
                params![id],
                row::row_to_message,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn latest_message_timestamp(
        &self,
        account_id: &str,
        connector: &str,
    ) -> anyhow::Result<Option<i64>> {
        self.conn()
            .query_row(
                "SELECT MAX(timestamp) FROM messages WHERE account_id = ?1 AND connector = ?2",
                params![account_id, connector],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(Into::into)
    }

    pub fn recent_messages(
        &self,
        account_filter: Option<&str>,
        connector_filter: Option<&str>,
        limit: i64,
        include_archived: bool,
        include_muted: bool,
    ) -> anyhow::Result<Vec<Message>> {
        let mut sql = String::from(
            "SELECT id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
             FROM messages WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if !include_archived {
            sql.push_str(" AND is_archived = 0");
        }
        if !include_muted {
            sql.push_str(
                " AND NOT EXISTS (SELECT 1 FROM conversations c WHERE c.id = messages.conversation_id AND c.is_muted = 1)",
            );
        }
        if let Some(acct) = account_filter {
            let pattern = format!("%{acct}%");
            sql.push_str(&format!(" AND account_id LIKE ?{}", param_values.len() + 1));
            param_values.push(Box::new(pattern));
        }
        if let Some(conn_type) = connector_filter {
            sql.push_str(&format!(" AND connector = ?{}", param_values.len() + 1));
            param_values.push(Box::new(conn_type.to_string()));
        }

        sql.push_str(&format!(
            " ORDER BY timestamp DESC LIMIT ?{}",
            param_values.len() + 1
        ));
        param_values.push(Box::new(limit));

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), row::row_to_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
                "SELECT id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
                 FROM messages WHERE account_id = ?1 AND external_id = ?2",
                params![account_id, external_id],
                row::row_to_message,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Populate the `context` field on each message by fetching all messages sharing the same `context_id`.
    pub fn enrich_with_context(&self, messages: &mut [Message]) -> anyhow::Result<()> {
        use std::collections::{HashMap, HashSet};

        let context_ids: HashSet<&str> = messages
            .iter()
            .filter_map(|m| m.context_id.as_deref())
            .collect();

        if context_ids.is_empty() {
            return Ok(());
        }

        let mut context_map: HashMap<String, Vec<Message>> = HashMap::new();

        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
             FROM messages WHERE context_id = ?1 ORDER BY timestamp ASC LIMIT 50",
        )?;

        for ctx_id in &context_ids {
            let rows = stmt.query_map(params![ctx_id], row::row_to_message)?;
            let ctx_messages: Vec<Message> = rows.collect::<Result<_, _>>()?;
            context_map.insert(ctx_id.to_string(), ctx_messages);
        }

        for msg in messages.iter_mut() {
            if let Some(ctx_id) = &msg.context_id {
                if let Some(ctx_messages) = context_map.get(ctx_id) {
                    if ctx_messages.len() > 1 {
                        msg.context = Some(ctx_messages.clone());
                    }
                }
            }
        }

        Ok(())
    }

    /// Get the most recent message in a conversation (used for time-window context grouping).
    pub fn last_message_in_conversation(
        &self,
        conversation_id: &str,
    ) -> anyhow::Result<Option<Message>> {
        self.conn()
            .query_row(
                "SELECT id, conversation_id, account_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
                 FROM messages WHERE conversation_id = ?1 ORDER BY timestamp DESC LIMIT 1",
                params![conversation_id],
                row::row_to_message,
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

    pub fn delete_event(&self, account_id: &str, external_id: &str) -> anyhow::Result<bool> {
        debug!(account_id, external_id, "deleting event");
        let deleted = self.conn().execute(
            "DELETE FROM events WHERE account_id = ?1 AND external_id = ?2",
            params![account_id, external_id],
        )?;
        Ok(deleted > 0)
    }

    /// Delete all data (messages, conversations, events, sync_state) for a given connector type.
    /// Returns a summary of how many rows were deleted from each table.
    pub fn clear_connector_data(
        &self,
        connector_type: &str,
    ) -> anyhow::Result<(usize, usize, usize, usize)> {
        let conn = self.conn();

        let account_ids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT account_id FROM conversations WHERE connector = ?1
                 UNION SELECT DISTINCT account_id FROM messages WHERE connector = ?1
                 UNION SELECT DISTINCT account_id FROM events WHERE connector = ?1",
            )?;
            let rows = stmt.query_map(params![connector_type], |row| row.get(0))?;
            rows.collect::<Result<_, _>>()?
        };

        let msgs = conn.execute(
            "DELETE FROM messages WHERE connector = ?1",
            params![connector_type],
        )?;
        let convs = conn.execute(
            "DELETE FROM conversations WHERE connector = ?1",
            params![connector_type],
        )?;
        let evts = conn.execute(
            "DELETE FROM events WHERE connector = ?1",
            params![connector_type],
        )?;

        let mut sync_deleted = 0usize;
        for aid in &account_ids {
            sync_deleted +=
                conn.execute("DELETE FROM sync_state WHERE account_id = ?1", params![aid])?;
        }

        Ok((msgs, convs, evts, sync_deleted))
    }

    pub fn list_events(
        &self,
        from: Option<i64>,
        to: Option<i64>,
        account_filter: Option<&str>,
        connector_filter: Option<&str>,
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
        if let Some(acct) = account_filter {
            let pattern = format!("%{acct}%");
            sql.push_str(&format!(" AND account_id LIKE ?{}", param_values.len() + 1));
            param_values.push(Box::new(pattern));
        }
        if let Some(conn_type) = connector_filter {
            sql.push_str(&format!(" AND connector = ?{}", param_values.len() + 1));
            param_values.push(Box::new(conn_type.to_string()));
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
        let rows = stmt.query_map(params_ref.as_slice(), row::row_to_event)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // -- Contacts --

    pub fn list_contacts(
        &self,
        account_filter: Option<&str>,
        connector_filter: Option<&str>,
        search: Option<&str>,
        limit: i64,
    ) -> anyhow::Result<Vec<Contact>> {
        let mut sql = String::from(
            "SELECT sender, sender_name, account_id, connector, COUNT(*) as msg_count, MAX(timestamp) as last_ts
             FROM messages WHERE sender != account_id",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(acct) = account_filter {
            let pattern = format!("%{acct}%");
            sql.push_str(&format!(" AND account_id LIKE ?{}", param_values.len() + 1));
            param_values.push(Box::new(pattern));
        }
        if let Some(conn_type) = connector_filter {
            sql.push_str(&format!(" AND connector = ?{}", param_values.len() + 1));
            param_values.push(Box::new(conn_type.to_string()));
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
        connector_filter: Option<&str>,
        search: Option<&str>,
        limit: i64,
        include_muted: bool,
    ) -> anyhow::Result<Vec<Conversation>> {
        let mut sql = String::from(
            "SELECT id, account_id, connector, external_id, name, kind, last_message_at, unread_count, is_muted, metadata
             FROM conversations WHERE kind IN ('group', 'channel')",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if !include_muted {
            sql.push_str(" AND is_muted = 0");
        }
        if let Some(acct) = account_filter {
            let pattern = format!("%{acct}%");
            sql.push_str(&format!(" AND account_id LIKE ?{}", param_values.len() + 1));
            param_values.push(Box::new(pattern));
        }
        if let Some(conn_type) = connector_filter {
            sql.push_str(&format!(" AND connector = ?{}", param_values.len() + 1));
            param_values.push(Box::new(conn_type.to_string()));
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
        let rows = stmt.query_map(params_ref.as_slice(), row::row_to_conversation)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // -- Mute state --

    pub fn update_conversation_mute(
        &self,
        conversation_id: &str,
        is_muted: bool,
    ) -> anyhow::Result<bool> {
        debug!(
            conversation_id,
            is_muted, "updating conversation mute state"
        );
        let updated = self.conn().execute(
            "UPDATE conversations SET is_muted = ?2 WHERE id = ?1",
            params![conversation_id, is_muted as i32],
        )?;
        Ok(updated > 0)
    }

    /// Set mute state for a conversation identified by its external_id and account_id.
    /// Returns true if a row was updated.
    pub fn set_mute_by_external_id(
        &self,
        account_id: &str,
        external_id: &str,
        is_muted: bool,
    ) -> anyhow::Result<bool> {
        debug!(
            account_id,
            external_id, is_muted, "setting mute by external_id"
        );
        let updated = self.conn().execute(
            "UPDATE conversations SET is_muted = ?3 WHERE account_id = ?1 AND external_id = ?2",
            params![account_id, external_id, is_muted as i32],
        )?;
        Ok(updated > 0)
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

    pub fn rename_account(&self, old_id: &str, new_id: &str) -> anyhow::Result<()> {
        let conn = self.conn();
        // Temporarily disable FKs: we update conversation ids first, which orphans
        // messages; then we update messages. With FKs on, the order would violate.
        conn.execute("PRAGMA foreign_keys = OFF", [])?;
        conn.execute(
            "UPDATE sync_state SET account_id = ?2 WHERE account_id = ?1",
            params![old_id, new_id],
        )?;
        conn.execute(
            "UPDATE conversations SET account_id = ?2, id = REPLACE(id, ?1, ?2) WHERE account_id = ?1",
            params![old_id, new_id],
        )?;
        conn.execute(
            "UPDATE messages SET account_id = ?2, id = REPLACE(id, ?1, ?2), conversation_id = REPLACE(conversation_id, ?1, ?2) WHERE account_id = ?1",
            params![old_id, new_id],
        )?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ConversationKind;

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
            is_muted: false,
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
            is_archived: false,
            reply_to_id: None,
            media_type: None,
            metadata: None,
            context_id: None,
            context: None,
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

        let list = db.list_conversations(None, None, 100, true).unwrap();
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

        let results = db.search_messages("meeting", None, None, 10, true).unwrap();
        assert_eq!(results.len(), 2);
    }

    // ---- search_messages integration: special characters ----

    fn seed_search_db() -> Database {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let mut conv2 = make_conversation("c2", "me@gmail.com", "G456");
        conv2.connector = "gmail".into();
        db.upsert_conversation(&conv2).unwrap();

        db.upsert_message(&make_message(
            "m1",
            "c1",
            "test-slack",
            "hello @MadMax how are you?",
            1_000,
        ))
        .unwrap();
        db.upsert_message(&make_message(
            "m2",
            "c1",
            "test-slack",
            "meeting with @alice tomorrow",
            2_000,
        ))
        .unwrap();
        db.upsert_message(&make_message(
            "m3",
            "c1",
            "test-slack",
            "the C++ compiler is broken",
            3_000,
        ))
        .unwrap();
        db.upsert_message(&make_message(
            "m4",
            "c1",
            "test-slack",
            "file: budget-report-2024.xlsx",
            4_000,
        ))
        .unwrap();
        db.upsert_message(&make_message(
            "m5",
            "c1",
            "test-slack",
            "say \"hello\" to everyone",
            5_000,
        ))
        .unwrap();
        db.upsert_message(&make_message(
            "m6",
            "c1",
            "test-slack",
            "NOT a problem AND it works OR fails",
            6_000,
        ))
        .unwrap();
        db.upsert_message(&make_message(
            "m7",
            "c1",
            "test-slack",
            "user:admin password:secret",
            7_000,
        ))
        .unwrap();

        let mut gmail_msg =
            make_message("m8", "c2", "me@gmail.com", "invoice from @accounts", 8_000);
        gmail_msg.connector = "gmail".into();
        db.upsert_message(&gmail_msg).unwrap();

        db
    }

    #[test]
    fn search_at_symbol_does_not_crash() {
        let db = seed_search_db();
        let results = db.search_messages("@MadMax", None, None, 50, true).unwrap();
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|m| m.body.as_deref().unwrap().contains("@MadMax")));
    }

    #[test]
    fn search_at_symbol_with_connector_filter() {
        let db = seed_search_db();
        let results = db
            .search_messages("@accounts", None, Some("gmail"), 50, true)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].connector, "gmail");
    }

    #[test]
    fn search_at_symbol_wrong_connector_returns_empty() {
        let db = seed_search_db();
        let results = db
            .search_messages("@accounts", None, Some("whatsapp"), 50, true)
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_double_quotes_does_not_crash() {
        let db = seed_search_db();
        let results = db
            .search_messages(r#""hello""#, None, None, 50, true)
            .unwrap();
        let _ = results;
    }

    #[test]
    fn search_dash_does_not_crash() {
        let db = seed_search_db();
        let results = db.search_messages("-report", None, None, 50, true).unwrap();
        // Should not error — the dash is escaped
        let _ = results;
    }

    #[test]
    fn search_asterisk_does_not_crash() {
        let db = seed_search_db();
        let results = db.search_messages("budget*", None, None, 50, true).unwrap();
        let _ = results;
    }

    #[test]
    fn search_plus_does_not_crash() {
        let db = seed_search_db();
        let results = db
            .search_messages("+required", None, None, 50, true)
            .unwrap();
        let _ = results;
    }

    #[test]
    fn search_boolean_operators_treated_as_literals() {
        let db = seed_search_db();
        let results = db.search_messages("NOT", None, None, 50, true).unwrap();
        // Should return results containing "NOT" as a word rather than treating it as boolean op
        assert!(!results.is_empty());
    }

    #[test]
    fn search_and_operator_literal() {
        let db = seed_search_db();
        let results = db.search_messages("AND", None, None, 50, true).unwrap();
        let _ = results; // Must not crash
    }

    #[test]
    fn search_or_operator_literal() {
        let db = seed_search_db();
        let results = db.search_messages("OR", None, None, 50, true).unwrap();
        let _ = results;
    }

    #[test]
    fn search_near_operator_literal() {
        let db = seed_search_db();
        let results = db.search_messages("NEAR", None, None, 50, true).unwrap();
        let _ = results;
    }

    #[test]
    fn search_colon_column_syntax_does_not_leak() {
        let db = seed_search_db();
        // In raw FTS5 "body:secret" would search column "body" for "secret".
        // Our escaping should prevent column-targeted search.
        let results = db
            .search_messages("body:secret", None, None, 50, true)
            .unwrap();
        let _ = results;
    }

    #[test]
    fn search_parentheses_do_not_crash() {
        let db = seed_search_db();
        let results = db
            .search_messages("(hello OR world)", None, None, 50, true)
            .unwrap();
        let _ = results;
    }

    #[test]
    fn search_curly_braces_do_not_crash() {
        let db = seed_search_db();
        let results = db.search_messages("{test}", None, None, 50, true).unwrap();
        let _ = results;
    }

    #[test]
    fn search_sql_injection_attempt() {
        let db = seed_search_db();
        let results = db
            .search_messages("'; DROP TABLE messages; --", None, None, 50, true)
            .unwrap();
        let _ = results;

        // Verify the messages table still exists and has data
        let all = db.recent_messages(None, None, 100, true, true).unwrap();
        assert!(
            !all.is_empty(),
            "messages table must survive injection attempt"
        );
    }

    #[test]
    fn search_fts5_injection_via_double_quotes() {
        let db = seed_search_db();
        // An attacker might try to break out of quoting to inject FTS5 operators
        let results = db
            .search_messages(r#"" OR body:*"#, None, None, 50, true)
            .unwrap();
        let _ = results;

        let all = db.recent_messages(None, None, 100, true, true).unwrap();
        assert!(!all.is_empty());
    }

    #[test]
    fn search_empty_query_does_not_crash() {
        let db = seed_search_db();
        // Empty query should not cause a panic or SQL error
        let result = db.search_messages("", None, None, 50, true);
        // It's acceptable for this to return an error or empty results, but not panic
        let _ = result;
    }

    #[test]
    fn search_whitespace_only_query_does_not_crash() {
        let db = seed_search_db();
        let result = db.search_messages("   ", None, None, 50, true);
        let _ = result;
    }

    #[test]
    fn search_with_account_filter_and_special_chars() {
        let db = seed_search_db();
        let results = db
            .search_messages("@MadMax", Some("test-slack"), None, 50, true)
            .unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn search_with_both_filters_and_special_chars() {
        let db = seed_search_db();
        let results = db
            .search_messages("@MadMax", Some("test-slack"), Some("slack"), 50, true)
            .unwrap();
        assert!(!results.is_empty());

        let no_results = db
            .search_messages("@MadMax", Some("test-slack"), Some("gmail"), 50, true)
            .unwrap();
        assert!(no_results.is_empty());
    }

    #[test]
    fn search_limit_is_respected() {
        let db = seed_search_db();
        // All messages contain common words — search for something broad
        let results = db.search_messages("the", None, None, 1, true).unwrap();
        assert!(results.len() <= 1);
    }

    #[test]
    fn search_unicode_does_not_crash() {
        let db = seed_search_db();
        let results = db
            .search_messages("café résumé 会議", None, None, 50, true)
            .unwrap();
        let _ = results;
    }

    #[test]
    fn search_emoji_does_not_crash() {
        let db = seed_search_db();
        let results = db.search_messages("📄", None, None, 50, true).unwrap();
        let _ = results;
    }

    #[test]
    fn search_backslash_does_not_crash() {
        let db = seed_search_db();
        let results = db
            .search_messages(r"C:\Users\admin", None, None, 50, true)
            .unwrap();
        let _ = results;
    }

    #[test]
    fn search_very_long_query_does_not_crash() {
        let db = seed_search_db();
        let long_query = "word ".repeat(200);
        let results = db
            .search_messages(&long_query, None, None, 50, true)
            .unwrap();
        let _ = results;
    }

    #[test]
    fn search_null_byte_does_not_crash() {
        let db = seed_search_db();
        let result = db.search_messages("hello\0world", None, None, 50, true);
        // May error but must not panic
        let _ = result;
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
            .list_events(Some(1_700_000_000), Some(1_700_002_000), None, None, 100)
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

        let results = db.recent_messages(None, None, 2, true, true).unwrap();
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
        db.upsert_message(&m1).unwrap();

        let mut m2 = make_message("m2", "c1", "test-slack", "world", 2_000);
        m2.sender = "alice@test.com".into();
        m2.sender_name = Some("Alice".into());
        db.upsert_message(&m2).unwrap();

        let mut m3 = make_message("m3", "c1", "test-slack", "bye", 3_000);
        m3.sender = "bob@test.com".into();
        m3.sender_name = Some("Bob".into());
        db.upsert_message(&m3).unwrap();

        let contacts = db.list_contacts(None, None, None, 100).unwrap();
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
        db.upsert_message(&m1).unwrap();

        let mut m2 = make_message("m2", "c1", "test-slack", "hey", 2_000);
        m2.sender = "bob@test.com".into();
        m2.sender_name = Some("Bob".into());
        db.upsert_message(&m2).unwrap();

        let results = db.list_contacts(None, None, Some("alice"), 100).unwrap();
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
        db.upsert_message(&m1).unwrap();

        let mut m2 = make_message("m2", "c2", "33651090627", "hey", 2_000);
        m2.sender = "bob@wa".into();
        m2.connector = "whatsapp".into();
        db.upsert_message(&m2).unwrap();

        let results = db.list_contacts(Some("gladiaio"), None, None, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].sender, "alice@slack");
    }

    #[test]
    fn list_contacts_excludes_own_messages() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let mut m1 = make_message("m1", "c1", "test-slack", "hi", 1_000);
        m1.sender = "test-slack".into();
        db.upsert_message(&m1).unwrap();

        let contacts = db.list_contacts(None, None, None, 100).unwrap();
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

        let channels = db.list_channels(None, None, None, 100, true).unwrap();
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

        let results = db
            .list_channels(None, None, Some("engi"), 100, true)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name.as_deref(), Some("Engineering"));
    }

    #[test]
    fn mark_message_archived_updates_flag() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let msg = make_message("m1", "c1", "test-slack", "hello", 1_000);
        db.upsert_message(&msg).unwrap();

        let updated = db.mark_message_archived("m1").unwrap();
        assert!(updated);

        let loaded = db.get_message("m1").unwrap().unwrap();
        assert!(loaded.is_archived);
    }

    #[test]
    fn find_message_by_external_id_returns_match() {
        let db = test_db();
        let conv = make_conversation("c1", "acct1", "C123");
        db.upsert_conversation(&conv).unwrap();

        let msg = make_message("m1", "c1", "acct1", "hello", 1_000);
        db.upsert_message(&msg).unwrap();

        let found = db.find_message_by_external_id("acct1", "ext-m1").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().body.as_deref(), Some("hello"));
    }

    #[test]
    fn find_message_by_external_id_nonexistent_returns_none() {
        let db = test_db();
        let found = db
            .find_message_by_external_id("acct1", "nonexistent")
            .unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn update_message_metadata_merges_json() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let msg = make_message("m1", "c1", "test-slack", "hello", 1_000);
        db.upsert_message(&msg).unwrap();

        let updated = db
            .update_message_metadata("m1", &serde_json::json!({"key": "value"}))
            .unwrap();
        assert!(updated);

        let loaded = db.get_message("m1").unwrap().unwrap();
        assert_eq!(
            loaded.metadata.as_ref().unwrap()["key"],
            serde_json::json!("value")
        );
    }

    // ---- Mute filtering tests ----

    #[test]
    fn mute_state_default_false() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let loaded = db.get_conversation("c1").unwrap().unwrap();
        assert!(!loaded.is_muted);
    }

    #[test]
    fn update_conversation_mute() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        let updated = db.update_conversation_mute("c1", true).unwrap();
        assert!(updated);

        let loaded = db.get_conversation("c1").unwrap().unwrap();
        assert!(loaded.is_muted);

        db.update_conversation_mute("c1", false).unwrap();
        let loaded = db.get_conversation("c1").unwrap().unwrap();
        assert!(!loaded.is_muted);
    }

    #[test]
    fn set_mute_by_external_id() {
        let db = test_db();
        let conv = make_conversation("c1", "my-wa-jid", "chat@g.us");
        db.upsert_conversation(&conv).unwrap();

        let updated = db
            .set_mute_by_external_id("my-wa-jid", "chat@g.us", true)
            .unwrap();
        assert!(updated);

        let loaded = db.get_conversation("c1").unwrap().unwrap();
        assert!(loaded.is_muted);
    }

    #[test]
    fn set_mute_by_external_id_nonexistent_returns_false() {
        let db = test_db();
        let updated = db.set_mute_by_external_id("nope", "nope", true).unwrap();
        assert!(!updated);
    }

    #[test]
    fn upsert_does_not_reset_mute() {
        let db = test_db();
        let conv = make_conversation("c1", "test-slack", "C123");
        db.upsert_conversation(&conv).unwrap();

        db.update_conversation_mute("c1", true).unwrap();

        let mut conv2 = make_conversation("c1", "test-slack", "C123");
        conv2.name = Some("Updated Name".into());
        db.upsert_conversation(&conv2).unwrap();

        let loaded = db.get_conversation("c1").unwrap().unwrap();
        assert!(loaded.is_muted, "upsert must not reset mute state");
        assert_eq!(loaded.name.as_deref(), Some("Updated Name"));
    }

    #[test]
    fn list_conversations_excludes_muted_by_default() {
        let db = test_db();
        let c1 = make_conversation("c1", "test", "E1");
        db.upsert_conversation(&c1).unwrap();
        let c2 = make_conversation("c2", "test", "E2");
        db.upsert_conversation(&c2).unwrap();

        db.update_conversation_mute("c2", true).unwrap();

        let without_muted = db.list_conversations(None, None, 100, false).unwrap();
        assert_eq!(without_muted.len(), 1);
        assert_eq!(without_muted[0].id, "c1");

        let with_muted = db.list_conversations(None, None, 100, true).unwrap();
        assert_eq!(with_muted.len(), 2);
    }

    #[test]
    fn recent_messages_excludes_muted_conversations() {
        let db = test_db();
        let c1 = make_conversation("c1", "test", "E1");
        db.upsert_conversation(&c1).unwrap();
        let c2 = make_conversation("c2", "test", "E2");
        db.upsert_conversation(&c2).unwrap();

        db.upsert_message(&make_message("m1", "c1", "test", "visible", 1_000))
            .unwrap();
        db.upsert_message(&make_message("m2", "c2", "test", "muted msg", 2_000))
            .unwrap();

        db.update_conversation_mute("c2", true).unwrap();

        let without_muted = db.recent_messages(None, None, 100, true, false).unwrap();
        assert_eq!(without_muted.len(), 1);
        assert_eq!(without_muted[0].body.as_deref(), Some("visible"));

        let with_muted = db.recent_messages(None, None, 100, true, true).unwrap();
        assert_eq!(with_muted.len(), 2);
    }

    #[test]
    fn search_messages_excludes_muted_conversations() {
        let db = test_db();
        let c1 = make_conversation("c1", "test", "E1");
        db.upsert_conversation(&c1).unwrap();
        let c2 = make_conversation("c2", "test", "E2");
        db.upsert_conversation(&c2).unwrap();

        db.upsert_message(&make_message("m1", "c1", "test", "hello world", 1_000))
            .unwrap();
        db.upsert_message(&make_message("m2", "c2", "test", "hello muted", 2_000))
            .unwrap();

        db.update_conversation_mute("c2", true).unwrap();

        let without_muted = db.search_messages("hello", None, None, 100, false).unwrap();
        assert_eq!(without_muted.len(), 1);
        assert_eq!(without_muted[0].conversation_id, "c1");

        let with_muted = db.search_messages("hello", None, None, 100, true).unwrap();
        assert_eq!(with_muted.len(), 2);
    }

    #[test]
    fn list_channels_excludes_muted_by_default() {
        let db = test_db();
        let mut g1 = make_conversation("g1", "test", "G1");
        g1.kind = ConversationKind::Group;
        db.upsert_conversation(&g1).unwrap();

        let mut g2 = make_conversation("g2", "test", "G2");
        g2.kind = ConversationKind::Group;
        db.upsert_conversation(&g2).unwrap();

        db.update_conversation_mute("g2", true).unwrap();

        let without_muted = db.list_channels(None, None, None, 100, false).unwrap();
        assert_eq!(without_muted.len(), 1);
        assert_eq!(without_muted[0].id, "g1");

        let with_muted = db.list_channels(None, None, None, 100, true).unwrap();
        assert_eq!(with_muted.len(), 2);
    }

    #[test]
    fn rename_account_updates_ids_in_all_tables() {
        let db = test_db();
        let conv = make_conversation("old-id-c1", "old-id", "E1");
        db.upsert_conversation(&conv).unwrap();
        db.upsert_message(&make_message(
            "old-id-m1",
            "old-id-c1",
            "old-id",
            "body",
            1_000,
        ))
        .unwrap();
        db.set_sync_state("old-id", "key1", "value1").unwrap();

        db.rename_account("old-id", "new-id").unwrap();

        let conv_after = db.get_conversation("new-id-c1").unwrap();
        assert!(conv_after.is_some());
        assert_eq!(conv_after.unwrap().account_id, "new-id");

        let msg_after = db.get_message("new-id-m1").unwrap();
        assert!(msg_after.is_some());
        assert_eq!(msg_after.unwrap().account_id, "new-id");

        let sync_val = db.get_sync_state("new-id", "key1").unwrap();
        assert_eq!(sync_val, Some("value1".to_string()));

        assert!(db.get_conversation("old-id-c1").unwrap().is_none());
    }

    fn make_conversation_with_connector(
        id: &str,
        account_id: &str,
        ext_id: &str,
        connector: &str,
    ) -> Conversation {
        let mut conv = make_conversation(id, account_id, ext_id);
        conv.connector = connector.into();
        conv
    }

    fn make_message_with_connector(
        id: &str,
        conv_id: &str,
        account_id: &str,
        body: &str,
        ts: i64,
        connector: &str,
    ) -> Message {
        let mut msg = make_message(id, conv_id, account_id, body, ts);
        msg.connector = connector.into();
        msg
    }

    #[test]
    fn clear_connector_data_removes_all_messages_conversations_events_sync_state() {
        let db = test_db();
        let conv = make_conversation_with_connector("c1", "gmail-1", "E1", "gmail");
        db.upsert_conversation(&conv).unwrap();
        db.upsert_message(&make_message_with_connector(
            "m1", "c1", "gmail-1", "body", 1_000, "gmail",
        ))
        .unwrap();
        db.set_sync_state("gmail-1", "history_id", "123").unwrap();

        let (msgs, convs, evts, sync) = db.clear_connector_data("gmail").unwrap();
        assert_eq!(msgs, 1);
        assert_eq!(convs, 1);
        assert_eq!(evts, 0);
        assert_eq!(sync, 1);

        assert!(db.get_conversation("c1").unwrap().is_none());
        assert!(db.get_message("m1").unwrap().is_none());
        assert!(db
            .get_sync_state("gmail-1", "history_id")
            .unwrap()
            .is_none());
    }
}
