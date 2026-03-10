use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use void_core::channel::Channel;
use void_core::db::Database;
use void_core::models::*;

use crate::api::{SlackApiClient, SlackConversation, SlackMessage};

pub struct SlackChannel {
    account_id: String,
    api: SlackApiClient,
    /// Reserved for Socket Mode WebSocket connection (future).
    #[allow(dead_code)]
    app_token: String,
}

impl SlackChannel {
    pub fn new(account_id: &str, user_token: &str, app_token: &str) -> Self {
        Self {
            account_id: account_id.to_string(),
            api: SlackApiClient::new(user_token),
            app_token: app_token.to_string(),
        }
    }

    async fn backfill(&self, db: &Database) -> anyhow::Result<()> {
        info!(account_id = %self.account_id, "starting Slack backfill");

        let mut user_cache: HashMap<String, String> = HashMap::new();
        let mut cursor: Option<String> = None;
        let mut total_convs = 0u32;
        let mut total_msgs = 0u32;

        loop {
            let resp = self.api.conversations_list(cursor.as_deref(), 200).await?;
            for conv in &resp.channels {
                let conversation = map_conversation(conv, &self.account_id);
                db.upsert_conversation(&conversation)?;
                total_convs += 1;

                match self.api.conversations_history(&conv.id, 100, None).await {
                    Ok(history) => {
                        for msg in &history.messages {
                            if let Some(message) = map_message(
                                msg,
                                &conversation.id,
                                &self.account_id,
                                &mut user_cache,
                                &self.api,
                            )
                            .await
                            {
                                db.upsert_message(&message)?;
                                total_msgs += 1;
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

            cursor = resp
                .response_metadata
                .and_then(|m| m.next_cursor)
                .filter(|c| !c.is_empty());
            if cursor.is_none() {
                break;
            }
        }

        info!(account_id = %self.account_id, conversations = total_convs, messages = total_msgs, "backfill complete");
        Ok(())
    }
}

#[async_trait]
impl Channel for SlackChannel {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Slack
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
        self.backfill(&db).await?;

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
                channel_type: ChannelType::Slack,
                ok: true,
                message: format!(
                    "Authenticated as {} in {}",
                    resp.user.as_deref().unwrap_or("?"),
                    resp.team.as_deref().unwrap_or("?")
                ),
                last_sync: None,
                message_count: None,
            }),
            Err(e) => Ok(HealthStatus {
                account_id: self.account_id.clone(),
                channel_type: ChannelType::Slack,
                ok: false,
                message: format!("Auth failed: {e}"),
                last_sync: None,
                message_count: None,
            }),
        }
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
        let text = match &content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::File { caption, .. } => {
                caption.clone().unwrap_or_else(|| "(file)".into())
            }
        };

        // message_id format: "channel_id:ts"
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
        Ok(resp.ts.unwrap_or_default())
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
        external_id: conv.id.clone(),
        name: Some(name),
        kind,
        last_message_at: None,
        unread_count: 0,
        metadata: None,
    }
}

async fn map_message(
    msg: &SlackMessage,
    conversation_id: &str,
    account_id: &str,
    user_cache: &mut HashMap<String, String>,
    api: &SlackApiClient,
) -> Option<Message> {
    if msg.subtype.is_some() {
        return None;
    }

    let sender = msg.user.clone().unwrap_or_else(|| "unknown".into());
    let sender_name = resolve_user_name(&sender, user_cache, api).await;

    Some(Message {
        id: format!("{account_id}-{}", msg.ts),
        conversation_id: conversation_id.to_string(),
        account_id: account_id.to_string(),
        external_id: msg.ts.clone(),
        sender: sender.clone(),
        sender_name: Some(sender_name),
        body: msg.text.clone(),
        timestamp: parse_ts(&msg.ts).unwrap_or(0),
        is_from_me: false,
        reply_to_id: msg
            .thread_ts
            .as_ref()
            .map(|ts| format!("{account_id}-{ts}")),
        media_type: None,
        metadata: None,
    })
}

async fn resolve_user_name(
    user_id: &str,
    cache: &mut HashMap<String, String>,
    api: &SlackApiClient,
) -> String {
    if let Some(name) = cache.get(user_id) {
        return name.clone();
    }
    match api.users_info(user_id).await {
        Ok(resp) => {
            let name = resp
                .user
                .as_ref()
                .and_then(|u| {
                    u.profile
                        .as_ref()
                        .and_then(|p| p.display_name.clone().filter(|n| !n.is_empty()))
                        .or_else(|| u.real_name.clone())
                        .or(Some(u.name.clone()))
                })
                .unwrap_or_else(|| user_id.to_string());
            cache.insert(user_id.to_string(), name.clone());
            name
        }
        Err(_) => {
            cache.insert(user_id.to_string(), user_id.to_string());
            user_id.to_string()
        }
    }
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
        assert_eq!(result.name.as_deref(), Some("general"));
    }

    #[test]
    fn parse_slack_ts() {
        assert_eq!(parse_ts("1700000000.123456"), Some(1_700_000_000));
        assert_eq!(parse_ts("invalid"), None);
    }
}
