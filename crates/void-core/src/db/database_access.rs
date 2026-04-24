//! `Database` methods: hook logs and delegated CRUD entry points.

use crate::error::DbError;
use crate::models::{CalendarEvent, Contact, Conversation, Message};

use super::{conversations, directory, events, hook_logs, messages, mute_sync, Database};

impl Database {
    pub fn insert_hook_log(&self, log: &crate::hooks::HookLogInsert<'_>) -> Result<(), DbError> {
        hook_logs::insert(&*self.conn()?, log)
    }

    pub fn list_hook_logs(&self, limit: usize) -> Result<Vec<crate::hooks::HookLog>, DbError> {
        hook_logs::list(&*self.conn()?, limit)
    }

    // -- Conversations --

    pub fn upsert_conversation(&self, conv: &Conversation) -> Result<(), DbError> {
        conversations::upsert(&*self.conn()?, conv)
    }

    pub fn list_conversations(
        &self,
        connection_filter: Option<&str>,
        connector_filter: Option<&str>,
        limit: i64,
        include_muted: bool,
    ) -> Result<Vec<Conversation>, DbError> {
        conversations::list(
            &*self.conn()?,
            connection_filter,
            connector_filter,
            limit,
            0,
            include_muted,
        )
    }

    pub fn list_conversations_paginated(
        &self,
        connection_filter: Option<&str>,
        connector_filter: Option<&str>,
        limit: i64,
        offset: i64,
        include_muted: bool,
    ) -> Result<(Vec<Conversation>, i64), DbError> {
        let conn = self.conn()?;
        let rows = conversations::list(
            &conn,
            connection_filter,
            connector_filter,
            limit,
            offset,
            include_muted,
        )?;
        let total =
            conversations::count(&conn, connection_filter, connector_filter, include_muted)?;
        Ok((rows, total))
    }

    pub fn find_conversation_by_name(
        &self,
        name: &str,
        connector: &str,
    ) -> Result<Option<Conversation>, DbError> {
        conversations::find_by_name(&*self.conn()?, name, connector)
    }

    pub fn get_conversation(&self, id: &str) -> Result<Option<Conversation>, DbError> {
        conversations::get(&*self.conn()?, id)
    }

    // -- Messages --

    /// Returns `true` if a message with this (connection_id, external_id) already exists.
    pub fn message_exists(&self, connection_id: &str, external_id: &str) -> Result<bool, DbError> {
        messages::message_exists(&*self.conn()?, connection_id, external_id)
    }

    /// Insert or update a message. Returns `true` if the message was newly inserted.
    pub fn upsert_message(&self, msg: &Message) -> Result<bool, DbError> {
        let conn = self.conn()?;
        let is_new = messages::upsert_row(&conn, msg)?;
        drop(conn);
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
    ) -> Result<Vec<Message>, DbError> {
        messages::list_for_conversation(
            &*self.conn()?,
            conversation_id,
            limit,
            0,
            since,
            until,
            false,
        )
    }

    pub fn list_messages_paginated(
        &self,
        conversation_id: &str,
        limit: i64,
        offset: i64,
        since: Option<i64>,
        until: Option<i64>,
        dedup_context: bool,
    ) -> Result<(Vec<Message>, i64), DbError> {
        let conn = self.conn()?;
        let rows = messages::list_for_conversation(
            &conn,
            conversation_id,
            limit,
            offset,
            since,
            until,
            dedup_context,
        )?;
        let total =
            messages::count_for_conversation(&conn, conversation_id, since, until, dedup_context)?;
        Ok((rows, total))
    }

    pub fn get_message(&self, id: &str) -> Result<Option<Message>, DbError> {
        messages::get(&*self.conn()?, id)
    }

    pub fn latest_message_timestamp(
        &self,
        connection_id: &str,
        connector: &str,
    ) -> Result<Option<i64>, DbError> {
        messages::latest_timestamp(&*self.conn()?, connection_id, connector)
    }

    pub fn recent_messages(
        &self,
        connection_filter: Option<&str>,
        connector_filter: Option<&str>,
        limit: i64,
        include_archived: bool,
        include_muted: bool,
    ) -> Result<Vec<Message>, DbError> {
        messages::list_recent(
            &*self.conn()?,
            connection_filter,
            connector_filter,
            limit,
            0,
            include_archived,
            include_muted,
            false,
        )
    }

    /// Paginated inbox-style listing with optional filters (shared by CLI inbox/search).
    #[allow(clippy::too_many_arguments)]
    pub fn recent_messages_paginated(
        &self,
        connection_filter: Option<&str>,
        connector_filter: Option<&str>,
        limit: i64,
        offset: i64,
        include_archived: bool,
        include_muted: bool,
        dedup_context: bool,
    ) -> Result<(Vec<Message>, i64), DbError> {
        let conn = self.conn()?;
        let rows = messages::list_recent(
            &conn,
            connection_filter,
            connector_filter,
            limit,
            offset,
            include_archived,
            include_muted,
            dedup_context,
        )?;
        let total = messages::count_recent(
            &conn,
            connection_filter,
            connector_filter,
            include_archived,
            include_muted,
            dedup_context,
        )?;
        Ok((rows, total))
    }

    /// Archive all unarchived messages older than `before_ts`, optionally
    /// filtered by connector type. Returns the affected messages so callers
    /// can clean up cached files.
    pub fn bulk_archive_before(
        &self,
        before_ts: i64,
        connector_filter: Option<&str>,
    ) -> Result<Vec<Message>, DbError> {
        messages::bulk_archive_before(&*self.conn()?, before_ts, connector_filter)
    }

    pub fn mark_message_archived(&self, id: &str) -> Result<bool, DbError> {
        messages::mark_archived(&*self.conn()?, id)
    }

    pub fn update_message_metadata(
        &self,
        id: &str,
        metadata: &serde_json::Value,
    ) -> Result<bool, DbError> {
        messages::update_metadata(&*self.conn()?, id, metadata)
    }

    /// Reconcile `is_archived` for all messages of a connection to match the given inbox set.
    /// Returns (unarchived_count, archived_count).
    pub fn reconcile_inbox(
        &self,
        connection_id: &str,
        connector: &str,
        inbox_external_ids: &std::collections::HashSet<String>,
    ) -> Result<(usize, usize), DbError> {
        messages::reconcile_inbox(&*self.conn()?, connection_id, connector, inbox_external_ids)
    }

    pub fn find_message_by_external_id(
        &self,
        connection_id: &str,
        external_id: &str,
    ) -> Result<Option<Message>, DbError> {
        messages::find_by_external_id(&*self.conn()?, connection_id, external_id)
    }

    /// Resolve a Slack permalink to a stored message.
    ///
    /// Looks up by the Slack-native `(channel external_id, message ts)` pair,
    /// which is globally unique and independent of the void connection ID.
    /// This is necessary because the Slack workspace subdomain from the URL
    /// does not have to match the configured `connection_id`.
    pub fn find_slack_message_by_link(
        &self,
        channel_external_id: &str,
        message_ts: &str,
    ) -> Result<Option<Message>, DbError> {
        messages::find_by_slack_link(&*self.conn()?, channel_external_id, message_ts)
    }

    /// Resolve a Slack channel/DM from its native `external_id`, searching
    /// across all Slack connections.
    pub fn find_slack_conversation_by_link(
        &self,
        channel_external_id: &str,
    ) -> Result<Option<Conversation>, DbError> {
        messages::find_slack_conversation_by_external_id(&*self.conn()?, channel_external_id)
    }

    /// Populate the `context` field on each message by fetching all messages sharing the same `context_id`.
    pub fn enrich_with_context(&self, messages: &mut [Message]) -> Result<(), DbError> {
        messages::enrich_with_context(&*self.conn()?, messages)
    }

    /// Find messages that have files with `url_private` but no cached `local_path`.
    pub fn messages_pending_file_download(
        &self,
        connection_id: &str,
        connector: &str,
        limit: i64,
    ) -> Result<Vec<Message>, DbError> {
        messages::messages_pending_file_download(&*self.conn()?, connection_id, connector, limit)
    }

    /// Bulk-set `sender_avatar_url` for messages missing one.
    /// Returns the number of rows updated.
    pub fn backfill_avatar_urls(
        &self,
        connection_id: &str,
        connector: &str,
        avatars: &std::collections::HashMap<String, String>,
    ) -> Result<usize, DbError> {
        messages::backfill_avatar_urls(&*self.conn()?, connection_id, connector, avatars)
    }

    /// Return distinct sender IDs that have no `sender_avatar_url`.
    pub fn senders_missing_avatar(
        &self,
        connection_id: &str,
        connector: &str,
    ) -> Result<Vec<String>, DbError> {
        messages::senders_missing_avatar(&*self.conn()?, connection_id, connector)
    }

    /// Get the most recent message in a conversation (used for time-window context grouping).
    pub fn last_message_in_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<Message>, DbError> {
        messages::last_in_conversation(&*self.conn()?, conversation_id)
    }

    // -- Calendar events --

    pub fn upsert_event(&self, event: &CalendarEvent) -> Result<(), DbError> {
        events::upsert(&*self.conn()?, event)
    }

    pub fn delete_event(&self, connection_id: &str, external_id: &str) -> Result<bool, DbError> {
        events::delete(&*self.conn()?, connection_id, external_id)
    }

    /// Delete all data (messages, conversations, events, sync_state) for a given connector type.
    /// Returns a summary of how many rows were deleted from each table.
    pub fn clear_connector_data(
        &self,
        connector_type: &str,
    ) -> Result<(usize, usize, usize, usize), DbError> {
        events::clear_connector_data(&*self.conn()?, connector_type)
    }

    pub fn list_events(
        &self,
        from: Option<i64>,
        to: Option<i64>,
        connection_filter: Option<&str>,
        connector_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<CalendarEvent>, DbError> {
        events::list(
            &*self.conn()?,
            from,
            to,
            connection_filter,
            connector_filter,
            limit,
        )
    }

    // -- Contacts --

    pub fn list_contacts(
        &self,
        connection_filter: Option<&str>,
        connector_filter: Option<&str>,
        search: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Contact>, DbError> {
        directory::list_contacts(
            &*self.conn()?,
            connection_filter,
            connector_filter,
            search,
            limit,
            0,
        )
    }

    pub fn list_contacts_paginated(
        &self,
        connection_filter: Option<&str>,
        connector_filter: Option<&str>,
        search: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Contact>, i64), DbError> {
        let conn = self.conn()?;
        let rows = directory::list_contacts(
            &conn,
            connection_filter,
            connector_filter,
            search,
            limit,
            offset,
        )?;
        let total = directory::count_contacts(&conn, connection_filter, connector_filter, search)?;
        Ok((rows, total))
    }

    // -- Channels --

    pub fn list_channels(
        &self,
        connection_filter: Option<&str>,
        connector_filter: Option<&str>,
        search: Option<&str>,
        limit: i64,
        include_muted: bool,
    ) -> Result<Vec<Conversation>, DbError> {
        directory::list_channels(
            &*self.conn()?,
            connection_filter,
            connector_filter,
            search,
            limit,
            0,
            include_muted,
        )
    }

    pub fn list_channels_paginated(
        &self,
        connection_filter: Option<&str>,
        connector_filter: Option<&str>,
        search: Option<&str>,
        limit: i64,
        offset: i64,
        include_muted: bool,
    ) -> Result<(Vec<Conversation>, i64), DbError> {
        let conn = self.conn()?;
        let rows = directory::list_channels(
            &conn,
            connection_filter,
            connector_filter,
            search,
            limit,
            offset,
            include_muted,
        )?;
        let total = directory::count_channels(
            &conn,
            connection_filter,
            connector_filter,
            search,
            include_muted,
        )?;
        Ok((rows, total))
    }

    // -- Mute state --

    pub fn update_conversation_mute(
        &self,
        conversation_id: &str,
        is_muted: bool,
    ) -> Result<bool, DbError> {
        mute_sync::update_conversation_mute(&*self.conn()?, conversation_id, is_muted)
    }

    /// Set mute state for a conversation identified by its external_id and connection_id.
    /// Returns true if a row was updated.
    pub fn set_mute_by_external_id(
        &self,
        connection_id: &str,
        external_id: &str,
        is_muted: bool,
    ) -> Result<bool, DbError> {
        mute_sync::set_mute_by_external_id(&*self.conn()?, connection_id, external_id, is_muted)
    }

    /// Auto-mute conversations matching the given patterns (case-insensitive
    /// substring on name or external_id). Returns number of newly muted conversations.
    pub fn auto_mute_matching_conversations(
        &self,
        connection_id: &str,
        patterns: &[String],
    ) -> Result<usize, DbError> {
        mute_sync::auto_mute_matching_conversations(&*self.conn()?, connection_id, patterns)
    }

    // -- Sync state --

    pub fn list_sync_states(&self) -> Result<Vec<(String, String, String)>, DbError> {
        mute_sync::list_sync_states(&*self.conn()?)
    }

    pub fn get_sync_state(
        &self,
        connection_id: &str,
        key: &str,
    ) -> Result<Option<String>, DbError> {
        mute_sync::get_sync_state(&*self.conn()?, connection_id, key)
    }

    pub fn set_sync_state(
        &self,
        connection_id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), DbError> {
        mute_sync::set_sync_state(&*self.conn()?, connection_id, key, value)
    }

    pub fn rename_connection(&self, old_id: &str, new_id: &str) -> Result<(), DbError> {
        mute_sync::rename_connection(&*self.conn()?, old_id, new_id)
    }
}
