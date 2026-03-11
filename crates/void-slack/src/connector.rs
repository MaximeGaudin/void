use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

use crate::api::{SlackApiClient, SlackConversation, SlackMessage, SlackReaction};

pub struct SlackConnector {
    account_id: String,
    api: SlackApiClient,
    /// Reserved for Socket Mode WebSocket connection (future).
    #[allow(dead_code)]
    app_token: String,
    exclude_channels: Vec<String>,
}

impl SlackConnector {
    pub fn new(
        account_id: &str,
        user_token: &str,
        app_token: &str,
        exclude_channels: Vec<String>,
    ) -> Self {
        Self {
            account_id: account_id.to_string(),
            api: SlackApiClient::new(user_token),
            app_token: app_token.to_string(),
            exclude_channels,
        }
    }

    async fn prefetch_users(&self) -> anyhow::Result<HashMap<String, String>> {
        info!(account_id = %self.account_id, "prefetching Slack users");
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

        info!(account_id = %self.account_id, users = cache.len(), "user prefetch complete");
        Ok(cache)
    }

    async fn list_all_conversations(&self) -> anyhow::Result<Vec<SlackConversation>> {
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

        if !self.exclude_channels.is_empty() {
            let before = all.len();
            all.retain(|conv| {
                let dominated_by_id = self.exclude_channels.iter().any(|exc| exc == &conv.id);
                let dominated_by_name = conv
                    .name
                    .as_ref()
                    .is_some_and(|n| self.exclude_channels.iter().any(|exc| exc == n));
                !(dominated_by_id || dominated_by_name)
            });
            let excluded = before - all.len();
            if excluded > 0 {
                info!(
                    account_id = %self.account_id,
                    excluded,
                    "excluded channels from sync"
                );
            }
        }

        Ok(all)
    }

    async fn backfill(&self, db: &Database) -> anyhow::Result<()> {
        info!(account_id = %self.account_id, "starting Slack backfill (last 7 days)");

        let user_cache = self.prefetch_users().await?;
        let conversations = self.list_all_conversations().await?;

        eprintln!(
            "[slack:{}] found {} conversations, fetching history…",
            self.account_id,
            conversations.len()
        );

        let oldest_ts = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(7))
            .unwrap_or_else(chrono::Utc::now)
            .timestamp()
            .to_string();

        let mut progress = void_core::progress::BackfillProgress::new(
            &format!("slack:{}", self.account_id),
            "conversations",
        )
        .with_secondary("messages");
        progress.set_items_total(conversations.len() as u64);

        for conv in &conversations {
            let conversation = map_conversation(conv, &self.account_id);
            db.upsert_conversation(&conversation)?;
            progress.inc(1);

            match self
                .api
                .conversations_history(&conv.id, 100, Some(&oldest_ts))
                .await
            {
                Ok(history) => {
                    for msg in &history.messages {
                        if let Some(message) = map_message_cached(
                            msg,
                            conv,
                            &conversation.id,
                            &self.account_id,
                            &user_cache,
                        ) {
                            db.upsert_message(&message)?;
                            progress.inc_secondary(1);
                        }
                    }
                    if let Some(last) = history.messages.first() {
                        let mut conv_update = conversation.clone();
                        conv_update.last_message_at = parse_ts(&last.ts);
                        db.upsert_conversation(&conv_update)?;
                    }
                }
                Err(e) => {
                    warn!(channel_id = %conv.id, "failed to fetch history: {e}");
                }
            }
        }

        progress.finish();
        info!(
            account_id = %self.account_id,
            conversations = progress.items,
            messages = progress.secondary,
            "backfill complete"
        );
        Ok(())
    }
}

#[async_trait]
impl Connector for SlackConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::Slack
    }

    fn account_id(&self) -> &str {
        &self.account_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let resp = self.api.auth_test().await?;
        info!(
            user = resp.user.as_deref().unwrap_or("?"),
            team = resp.team.as_deref().unwrap_or("?"),
            "Slack authenticated"
        );
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        let needs_backfill = db
            .get_sync_state(&self.account_id, "backfill_done")?
            .is_none();
        if needs_backfill {
            self.backfill(&db).await?;
            db.set_sync_state(&self.account_id, "backfill_done", "1")?;
        } else {
            info!(
                account_id = %self.account_id,
                "Slack backfill already complete, starting incremental sync"
            );
        }

        // After backfill, poll for new messages periodically
        // (Socket Mode can be added later for true real-time)
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!(account_id = %self.account_id, "Slack sync cancelled");
                    break;
                }
                _ = interval.tick() => {
                    debug!(account_id = %self.account_id, "Slack poll tick");
                    // Future: poll conversations.history for new messages
                    // since last known timestamp per conversation
                }
            }
        }
        Ok(())
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        match self.api.auth_test().await {
            Ok(resp) => Ok(HealthStatus {
                account_id: self.account_id.clone(),
                connector_type: ConnectorType::Slack,
                ok: true,
                message: format!(
                    "Authenticated as {} in {}",
                    resp.user.as_deref().unwrap_or("?"),
                    resp.team.as_deref().unwrap_or("?")
                ),
                last_sync: None,
                message_count: None,
            }),
            Err(e) => {
                warn!(account_id = %self.account_id, error = %e, "Slack health check failed");
                Ok(HealthStatus {
                    account_id: self.account_id.clone(),
                    connector_type: ConnectorType::Slack,
                    ok: false,
                    message: format!("Auth failed: {e}"),
                    last_sync: None,
                    message_count: None,
                })
            }
        }
    }

    async fn mark_read(
        &self,
        external_id: &str,
        conversation_external_id: &str,
    ) -> anyhow::Result<()> {
        info!(account_id = %self.account_id, ts = %external_id, channel = %conversation_external_id, "marking Slack message as read");
        self.api
            .conversations_mark(conversation_external_id, external_id)
            .await?;
        Ok(())
    }

    async fn send_message(&self, to: &str, content: MessageContent) -> anyhow::Result<String> {
        let text = match &content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::File { caption, .. } => {
                caption.clone().unwrap_or_else(|| "(file)".into())
            }
        };
        let resp = self.api.chat_post_message(to, &text, None).await?;
        Ok(resp.ts.unwrap_or_default())
    }

    async fn reply(
        &self,
        message_id: &str,
        content: MessageContent,
        in_thread: bool,
    ) -> anyhow::Result<String> {
        info!(account_id = %self.account_id, message_id = %message_id, in_thread, "sending Slack reply");

        let text = match &content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::File { caption, .. } => {
                caption.clone().unwrap_or_else(|| "(file)".into())
            }
        };

        let parts: Vec<&str> = message_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            anyhow::bail!("invalid message_id format, expected 'channel_id:ts'");
        }
        let (channel_id, ts) = (parts[0], parts[1]);

        let thread_ts = if in_thread { Some(ts) } else { None };
        let resp = self
            .api
            .chat_post_message(channel_id, &text, thread_ts)
            .await?;
        let reply_ts = resp.ts.clone().unwrap_or_default();
        debug!(account_id = %self.account_id, ts = %reply_ts, "Slack reply sent");
        Ok(reply_ts)
    }
}

fn map_conversation(conv: &SlackConversation, account_id: &str) -> Conversation {
    let kind = if conv.is_im.unwrap_or(false) {
        ConversationKind::Dm
    } else if conv.is_group.unwrap_or(false) || conv.is_mpim.unwrap_or(false) {
        ConversationKind::Group
    } else {
        ConversationKind::Channel
    };

    let name = conv
        .name
        .clone()
        .or_else(|| conv.user.clone())
        .unwrap_or_else(|| conv.id.clone());

    Conversation {
        id: format!("{}-{}", account_id, conv.id),
        account_id: account_id.to_string(),
        connector: "slack".into(),
        external_id: conv.id.clone(),
        name: Some(name),
        kind,
        last_message_at: None,
        unread_count: 0,
        is_muted: false,
        metadata: None,
    }
}

fn map_message_cached(
    msg: &SlackMessage,
    conv: &SlackConversation,
    conversation_id: &str,
    account_id: &str,
    user_cache: &HashMap<String, String>,
) -> Option<Message> {
    if msg.subtype.is_some() {
        return None;
    }

    let sender = msg.user.clone().unwrap_or_else(|| "unknown".into());
    let sender_name = user_cache
        .get(&sender)
        .cloned()
        .unwrap_or_else(|| sender.clone());

    let mut metadata = build_metadata(conv, &msg.reactions);
    let text = msg.text.clone().unwrap_or_default();

    let (body, media_type) = if !msg.files.is_empty() {
        let file_descriptions: Vec<String> = msg
            .files
            .iter()
            .map(|f| {
                let name = f.name.as_deref().or(f.title.as_deref()).unwrap_or("file");
                let icon = match f.mimetype.as_deref() {
                    Some(m) if m.starts_with("image/") => "🖼️",
                    Some(m) if m.starts_with("video/") => "🎬",
                    Some(m) if m.starts_with("audio/") => "🎵",
                    _ => "📎",
                };
                format!("{icon} {name}")
            })
            .collect();

        if let Some(meta) = metadata.as_mut() {
            let files_json: Vec<serde_json::Value> = msg
                .files
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "id": f.id,
                        "name": f.name,
                        "title": f.title,
                        "mimetype": f.mimetype,
                        "filetype": f.filetype,
                        "size": f.size,
                        "url_private": f.url_private,
                        "permalink": f.permalink,
                    })
                })
                .collect();
            meta["files"] = serde_json::Value::Array(files_json);
        }

        let first_mime = msg.files[0].mimetype.as_deref();
        let mtype = first_mime.map(|m| {
            if m.starts_with("image/") {
                "image".to_string()
            } else if m.starts_with("video/") {
                "video".to_string()
            } else if m.starts_with("audio/") {
                "audio".to_string()
            } else {
                "file".to_string()
            }
        });

        let body = if text.is_empty() {
            file_descriptions.join(", ")
        } else {
            format!("{text}\n{}", file_descriptions.join(", "))
        };
        (Some(body), mtype)
    } else if !msg.attachments.is_empty() && text.is_empty() {
        let fallback: Vec<String> = msg
            .attachments
            .iter()
            .filter_map(|a| {
                a.title
                    .clone()
                    .or_else(|| a.fallback.clone())
                    .or_else(|| a.text.clone())
            })
            .collect();
        if fallback.is_empty() {
            (Some(text), None)
        } else {
            (Some(fallback.join("\n")), None)
        }
    } else {
        (if text.is_empty() { None } else { Some(text) }, None)
    };

    Some(Message {
        id: format!("{account_id}-{}", msg.ts),
        conversation_id: conversation_id.to_string(),
        account_id: account_id.to_string(),
        connector: "slack".into(),
        external_id: msg.ts.clone(),
        sender: sender.clone(),
        sender_name: Some(sender_name),
        body,
        timestamp: parse_ts(&msg.ts).unwrap_or(0),
        synced_at: None,
        is_from_me: false,
        is_read: false,
        is_archived: false,
        reply_to_id: msg
            .thread_ts
            .as_ref()
            .map(|ts| format!("{account_id}-{ts}")),
        media_type,
        metadata,
    })
}

fn build_metadata(
    conv: &SlackConversation,
    reactions: &[SlackReaction],
) -> Option<serde_json::Value> {
    let kind = if conv.is_im.unwrap_or(false) {
        "dm"
    } else if conv.is_mpim.unwrap_or(false) {
        "group_dm"
    } else if conv.is_group.unwrap_or(false) || conv.is_private.unwrap_or(false) {
        "private_channel"
    } else {
        "channel"
    };

    let mut meta = serde_json::json!({
        "channel_id": conv.id,
        "channel_name": conv.name.as_deref().or(conv.user.as_deref()).unwrap_or(&conv.id),
        "channel_kind": kind,
        "is_private": conv.is_private.unwrap_or(false) || conv.is_im.unwrap_or(false) || conv.is_mpim.unwrap_or(false),
    });

    if !reactions.is_empty() {
        let r: Vec<serde_json::Value> = reactions
            .iter()
            .map(|r| serde_json::json!({"name": r.name, "count": r.count}))
            .collect();
        meta["reactions"] = serde_json::Value::Array(r);
    }

    Some(meta)
}

fn parse_ts(ts: &str) -> Option<i64> {
    ts.split('.').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_conversation_dm() {
        let conv = SlackConversation {
            id: "D123".into(),
            name: None,
            is_channel: Some(false),
            is_group: Some(false),
            is_im: Some(true),
            is_mpim: Some(false),
            is_private: Some(true),
            user: Some("U456".into()),
        };
        let result = map_conversation(&conv, "work-slack");
        assert_eq!(result.kind, ConversationKind::Dm);
        assert_eq!(result.connector, "slack");
        assert_eq!(result.name.as_deref(), Some("U456"));
        assert_eq!(result.external_id, "D123");
    }

    #[test]
    fn map_conversation_channel() {
        let conv = SlackConversation {
            id: "C789".into(),
            name: Some("general".into()),
            is_channel: Some(true),
            is_group: Some(false),
            is_im: Some(false),
            is_mpim: Some(false),
            is_private: Some(false),
            user: None,
        };
        let result = map_conversation(&conv, "work-slack");
        assert_eq!(result.kind, ConversationKind::Channel);
        assert_eq!(result.connector, "slack");
        assert_eq!(result.name.as_deref(), Some("general"));
    }

    #[test]
    fn parse_slack_ts() {
        assert_eq!(parse_ts("1700000000.123456"), Some(1_700_000_000));
        assert_eq!(parse_ts("invalid"), None);
    }

    #[test]
    fn build_metadata_channel_no_reactions() {
        let conv = SlackConversation {
            id: "C789".into(),
            name: Some("general".into()),
            is_channel: Some(true),
            is_group: Some(false),
            is_im: Some(false),
            is_mpim: Some(false),
            is_private: Some(false),
            user: None,
        };
        let meta = build_metadata(&conv, &[]).unwrap();
        assert_eq!(meta["channel_id"], "C789");
        assert_eq!(meta["channel_name"], "general");
        assert_eq!(meta["channel_kind"], "channel");
        assert_eq!(meta["is_private"], false);
        assert!(meta.get("reactions").is_none());
    }

    #[test]
    fn build_metadata_dm_with_reactions() {
        let conv = SlackConversation {
            id: "D123".into(),
            name: None,
            is_channel: Some(false),
            is_group: Some(false),
            is_im: Some(true),
            is_mpim: Some(false),
            is_private: Some(true),
            user: Some("U456".into()),
        };
        let reactions = vec![
            SlackReaction {
                name: "thumbsup".into(),
                count: 3,
                users: vec![],
            },
            SlackReaction {
                name: "heart".into(),
                count: 1,
                users: vec![],
            },
        ];
        let meta = build_metadata(&conv, &reactions).unwrap();
        assert_eq!(meta["channel_id"], "D123");
        assert_eq!(meta["channel_name"], "U456");
        assert_eq!(meta["channel_kind"], "dm");
        assert_eq!(meta["is_private"], true);
        let r = meta["reactions"].as_array().unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0]["name"], "thumbsup");
        assert_eq!(r[0]["count"], 3);
        assert_eq!(r[1]["name"], "heart");
    }

    #[test]
    fn build_metadata_private_channel() {
        let conv = SlackConversation {
            id: "G111".into(),
            name: Some("secret-project".into()),
            is_channel: Some(false),
            is_group: Some(true),
            is_im: Some(false),
            is_mpim: Some(false),
            is_private: Some(true),
            user: None,
        };
        let meta = build_metadata(&conv, &[]).unwrap();
        assert_eq!(meta["channel_kind"], "private_channel");
        assert_eq!(meta["is_private"], true);
        assert_eq!(meta["channel_name"], "secret-project");
    }
}
