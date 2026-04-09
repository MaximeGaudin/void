//! Sync operations: prefetch users, list conversations, backfill, catch-up, fetch history.

use std::collections::HashMap;

use tracing::{info, warn};

use void_core::db::Database;
use void_core::models::Message;

use crate::api::SlackConversation;
use crate::connector::mapping::{
    assign_time_window_context, map_conversation, map_message_cached, parse_ts, CachedUser,
};
use crate::connector::SlackConnector;

impl SlackConnector {
    pub(crate) async fn prefetch_users(&self) -> anyhow::Result<HashMap<String, CachedUser>> {
        info!(connection_id = %self.connection_id, "prefetching Slack users");
        let mut cache = HashMap::new();
        let mut cursor: Option<String> = None;

        loop {
            let resp = self.api.users_list(cursor.as_deref(), 200).await?;
            for user in &resp.members {
                let profile = user.profile.as_ref();
                let name = profile
                    .and_then(|p| p.display_name.clone().filter(|n| !n.is_empty()))
                    .or_else(|| user.real_name.clone())
                    .unwrap_or_else(|| user.name.clone());
                let avatar_url = profile.and_then(|p| p.image_72.clone());
                cache.insert(user.id.clone(), CachedUser { name, avatar_url });
            }

            cursor = resp
                .response_metadata
                .and_then(|m| m.next_cursor)
                .filter(|c| !c.is_empty());
            if cursor.is_none() {
                break;
            }
        }

        info!(connection_id = %self.connection_id, users = cache.len(), "user prefetch complete");
        Ok(cache)
    }

    pub(crate) async fn list_all_conversations(&self) -> anyhow::Result<Vec<SlackConversation>> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let resp = self.api.conversations_list(cursor.as_deref(), 200).await?;
            all.extend(resp.channels);

            cursor = resp
                .response_metadata
                .and_then(|m| m.next_cursor)
                .filter(|c| !c.is_empty());
            if cursor.is_none() {
                break;
            }
        }

        Ok(all)
    }

    pub(crate) async fn backfill(&self, db: &Database) -> anyhow::Result<()> {
        let oldest_ts = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(7))
            .unwrap_or_else(chrono::Utc::now)
            .timestamp()
            .to_string();

        info!(connection_id = %self.connection_id, since = %oldest_ts, "starting Slack backfill (last 7 days)");
        self.fetch_history(db, &oldest_ts, "backfill").await
    }

    pub(crate) async fn catch_up(&self, db: &Database) -> anyhow::Result<()> {
        let latest = db.latest_message_timestamp(&self.connection_id, "slack")?;
        let oldest_ts = match latest {
            Some(ts) => ts.to_string(),
            None => {
                info!(connection_id = %self.connection_id, "no previous messages found, skipping catch-up");
                return Ok(());
            }
        };

        info!(connection_id = %self.connection_id, since = %oldest_ts, "catching up missed Slack messages");
        self.fetch_history(db, &oldest_ts, "catch-up").await
    }

    async fn fetch_history(
        &self,
        db: &Database,
        oldest_ts: &str,
        label: &str,
    ) -> anyhow::Result<()> {
        let user_cache = self.prefetch_users().await?;

        self.backfill_avatars(db, &user_cache).await;

        let conversations = self.list_all_conversations().await?;

        let oldest_secs: u64 = oldest_ts.parse().unwrap_or(0);

        let active: Vec<_> = conversations
            .iter()
            .filter(|c| c.updated.map_or(true, |u| u >= oldest_secs))
            .collect();

        eprintln!(
            "[slack:{}] {} — {}/{} conversations active since {}, fetching…",
            self.connection_id,
            label,
            active.len(),
            conversations.len(),
            oldest_ts
        );

        let mut progress = void_core::progress::BackfillProgress::new(
            &format!("slack:{}", self.connection_id),
            "conversations",
        )
        .with_secondary("messages");
        progress.set_items_total(active.len() as u64);

        for conv in &active {
            let conversation = map_conversation(conv, &self.connection_id, &user_cache);
            db.upsert_conversation(&conversation)?;
            progress.inc(1);

            let mut all_messages = Vec::new();
            let mut cursor: Option<String> = None;
            let max_pages = 10;
            let mut page = 0;

            loop {
                match self
                    .api
                    .conversations_history(&conv.id, 200, Some(oldest_ts), cursor.as_deref())
                    .await
                {
                    Ok(history) => {
                        all_messages.extend(history.messages);
                        page += 1;

                        cursor = history
                            .response_metadata
                            .and_then(|m| m.next_cursor)
                            .filter(|c| !c.is_empty());

                        if cursor.is_none()
                            || !history.has_more.unwrap_or(false)
                            || page >= max_pages
                        {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!(channel_id = %conv.id, "{label}: failed to fetch history: {e}");
                        break;
                    }
                }
            }

            if !all_messages.is_empty() {
                let mut mapped: Vec<Message> = all_messages
                    .iter()
                    .filter_map(|msg| {
                        map_message_cached(
                            msg,
                            conv,
                            &conversation.id,
                            &self.connection_id,
                            &user_cache,
                        )
                    })
                    .collect();
                mapped.sort_by_key(|m| m.timestamp);
                assign_time_window_context(&mut mapped, &self.connection_id, &conv.id);
                self.download_message_files(&mut mapped).await;
                for message in &mapped {
                    db.upsert_message(message)?;
                    progress.inc_secondary(1);
                }
                if let Some(last) = all_messages.first() {
                    let mut conv_update = conversation.clone();
                    conv_update.last_message_at = parse_ts(&last.ts);
                    db.upsert_conversation(&conv_update)?;
                }
            }
        }

        progress.finish();
        info!(
            connection_id = %self.connection_id,
            conversations = progress.items,
            messages = progress.secondary,
            "{label} complete"
        );

        self.download_pending_files(db).await;

        Ok(())
    }

    /// Backfill avatar URLs: first from the prefetched user cache, then resolve
    /// remaining unknown senders individually via `users.info`.
    async fn backfill_avatars(&self, db: &Database, user_cache: &HashMap<String, CachedUser>) {
        let avatar_map: HashMap<String, String> = user_cache
            .iter()
            .filter_map(|(id, u)| u.avatar_url.as_ref().map(|url| (id.clone(), url.clone())))
            .collect();
        if !avatar_map.is_empty() {
            match db.backfill_avatar_urls(&self.connection_id, "slack", &avatar_map) {
                Ok(n) if n > 0 => {
                    info!(connection_id = %self.connection_id, updated = n, "backfilled avatar URLs from cache")
                }
                Err(e) => {
                    warn!(connection_id = %self.connection_id, error = %e, "avatar backfill failed")
                }
                _ => {}
            }
        }

        let missing = match db.senders_missing_avatar(&self.connection_id, "slack") {
            Ok(ids) => ids,
            Err(e) => {
                warn!(connection_id = %self.connection_id, error = %e, "failed to query missing avatars");
                return;
            }
        };
        if missing.is_empty() {
            return;
        }

        info!(connection_id = %self.connection_id, count = missing.len(), "resolving unknown senders via users.info");
        let mut resolved = HashMap::new();
        for user_id in &missing {
            match self.api.users_info(user_id).await {
                Ok(resp) => {
                    if let Some(user) = resp.user {
                        if let Some(url) = user.profile.as_ref().and_then(|p| p.image_72.clone()) {
                            resolved.insert(user_id.clone(), url);
                        }
                    }
                }
                Err(e) => {
                    warn!(user_id, error = %e, "users.info lookup failed, skipping");
                }
            }
        }
        if !resolved.is_empty() {
            match db.backfill_avatar_urls(&self.connection_id, "slack", &resolved) {
                Ok(n) => info!(connection_id = %self.connection_id, resolved = resolved.len(), updated = n, "resolved unknown sender avatars"),
                Err(e) => warn!(connection_id = %self.connection_id, error = %e, "failed to store resolved avatars"),
            }
        }
    }

    /// Download files for previously synced messages that are missing a local copy.
    async fn download_pending_files(&self, db: &Database) {
        let mut pending = match db.messages_pending_file_download(&self.connection_id, "slack", 500)
        {
            Ok(msgs) => msgs,
            Err(e) => {
                warn!(error = %e, "failed to query messages pending file download");
                return;
            }
        };
        if pending.is_empty() {
            return;
        }

        info!(
            connection_id = %self.connection_id,
            count = pending.len(),
            "downloading files for previously synced messages"
        );

        self.download_message_files(&mut pending).await;

        for msg in &pending {
            if let Some(ref meta) = msg.metadata {
                if let Err(e) = db.update_message_metadata(&msg.id, meta) {
                    warn!(message_id = %msg.id, error = %e, "failed to update metadata after file download");
                }
            }
        }
    }
}
