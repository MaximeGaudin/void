//! Sync operations: prefetch users, list conversations, backfill, catch-up, fetch history.

use std::collections::HashMap;

use tracing::{info, warn};

use void_core::db::Database;
use void_core::models::Message;

use crate::api::SlackConversation;
use crate::connector::mapping::{
    assign_time_window_context, map_conversation, map_message_cached, parse_ts,
};
use crate::connector::SlackConnector;

impl SlackConnector {
    pub(crate) async fn prefetch_users(&self) -> anyhow::Result<HashMap<String, String>> {
        info!(connection_id = %self.connection_id, "prefetching Slack users");
        let mut cache = HashMap::new();
        let mut cursor: Option<String> = None;

        loop {
            let resp = self.api.users_list(cursor.as_deref(), 200).await?;
            for user in &resp.members {
                let name = user
                    .profile
                    .as_ref()
                    .and_then(|p| p.display_name.clone().filter(|n| !n.is_empty()))
                    .or_else(|| user.real_name.clone())
                    .unwrap_or_else(|| user.name.clone());
                cache.insert(user.id.clone(), name);
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
        Ok(())
    }
}
