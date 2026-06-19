//! `conversations.*`, `users.*`, and `auth.test` endpoint wrappers.

use tracing::debug;

use super::types::*;
use super::SlackApiClient;
use crate::error::SlackError;

impl SlackApiClient {
    pub async fn auth_test(&self) -> Result<AuthTestResponse, SlackError> {
        self.post_with_retry(
            &format!("{}/auth.test", self.base_url),
            &serde_json::json!({}),
            "auth.test",
        )
        .await
    }

    pub async fn conversations_list(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<ConversationsListResponse, SlackError> {
        let mut params: Vec<(&str, String)> = vec![
            ("types", "public_channel,private_channel,mpim,im".into()),
            ("limit", limit.to_string()),
            ("exclude_archived", "true".into()),
        ];
        if let Some(c) = cursor {
            params.push(("cursor", c.to_string()));
        }
        self.get_with_retry(
            &format!("{}/conversations.list", self.base_url),
            &params,
            "conversations.list",
        )
        .await
    }

    pub async fn conversations_history(
        &self,
        channel_id: &str,
        limit: u32,
        oldest: Option<&str>,
        cursor: Option<&str>,
    ) -> Result<ConversationsHistoryResponse, SlackError> {
        let mut params: Vec<(&str, String)> = vec![
            ("channel", channel_id.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(o) = oldest {
            params.push(("oldest", o.to_string()));
        }
        if let Some(c) = cursor {
            params.push(("cursor", c.to_string()));
        }
        self.get_with_retry(
            &format!("{}/conversations.history", self.base_url),
            &params,
            "conversations.history",
        )
        .await
    }

    /// Fetch replies for a single thread.  Slack returns the parent message
    /// first, followed by all replies. Pagination via `cursor` is supported.
    pub async fn conversations_replies(
        &self,
        channel_id: &str,
        thread_ts: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<ConversationsHistoryResponse, SlackError> {
        let mut params: Vec<(&str, String)> = vec![
            ("channel", channel_id.to_string()),
            ("ts", thread_ts.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(c) = cursor {
            params.push(("cursor", c.to_string()));
        }
        self.get_with_retry(
            &format!("{}/conversations.replies", self.base_url),
            &params,
            "conversations.replies",
        )
        .await
    }

    pub async fn get_single_message(
        &self,
        channel_id: &str,
        ts: &str,
    ) -> Result<Option<SlackMessage>, SlackError> {
        debug!(channel_id, ts, "slack: get single message");
        let params: Vec<(&str, String)> = vec![
            ("channel", channel_id.to_string()),
            ("latest", ts.to_string()),
            ("oldest", ts.to_string()),
            ("inclusive", "true".to_string()),
            ("limit", "1".to_string()),
        ];
        let resp: ConversationsHistoryResponse = self
            .get_with_retry(
                &format!("{}/conversations.history", self.base_url),
                &params,
                "conversations.history (single)",
            )
            .await?;
        Ok(resp.messages.into_iter().next())
    }

    pub async fn users_info(&self, user_id: &str) -> Result<UserInfoResponse, SlackError> {
        debug!(user_id, "slack: users.info");
        let params: Vec<(&str, String)> = vec![("user", user_id.to_string())];
        let result: UserInfoResponse = self
            .get_with_retry(
                &format!("{}/users.info", self.base_url),
                &params,
                "users.info",
            )
            .await?;
        debug!(user_id = ?result.user.as_ref().map(|u| &u.id), "slack: users.info success");
        Ok(result)
    }

    pub async fn users_list(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<UsersListResponse, SlackError> {
        let mut params: Vec<(&str, String)> = vec![("limit", limit.to_string())];
        if let Some(c) = cursor {
            params.push(("cursor", c.to_string()));
        }
        self.get_with_retry(
            &format!("{}/users.list", self.base_url),
            &params,
            "users.list",
        )
        .await
    }

    /// Resolve a channel name (without `#`) to its ID via `conversations.list` pagination.
    pub async fn resolve_channel_id_by_name(&self, name: &str) -> anyhow::Result<String> {
        let mut cursor: Option<String> = None;
        loop {
            let resp = self
                .conversations_list(cursor.as_deref(), 1000)
                .await
                .map_err(|e| anyhow::anyhow!("conversations.list failed: {e}"))?;
            if let Some(ch) = resp
                .channels
                .iter()
                .find(|c| c.name.as_deref() == Some(name))
            {
                return Ok(ch.id.clone());
            }
            match resp
                .response_metadata
                .and_then(|m| m.next_cursor)
                .filter(|c| !c.is_empty())
            {
                Some(next) => cursor = Some(next),
                None => anyhow::bail!("channel #{name} not found"),
            }
        }
    }

    pub async fn conversations_info(&self, channel: &str) -> Result<SlackConversation, SlackError> {
        debug!(channel, "slack: conversations.info");
        let resp: ConversationInfoResponse = self
            .get_with_retry(
                &format!("{}/conversations.info", self.base_url),
                &[("channel", channel.to_string())],
                "conversations.info",
            )
            .await?;
        Ok(resp.channel)
    }

    pub async fn conversations_mark(&self, channel: &str, ts: &str) -> Result<(), SlackError> {
        debug!(channel, ts, "slack: conversations.mark");
        let body = serde_json::json!({
            "channel": channel,
            "ts": ts,
        });
        let _: serde_json::Value = self
            .post_with_retry(
                &format!("{}/conversations.mark", self.base_url),
                &body,
                "conversations.mark",
            )
            .await?;
        debug!(channel, ts, "slack: conversations.mark success");
        Ok(())
    }

    pub async fn conversations_open(
        &self,
        users: &[&str],
    ) -> Result<ConversationsOpenResponse, SlackError> {
        debug!(users = ?users, "slack: conversations.open");
        let body = serde_json::json!({
            "users": users.join(","),
        });
        let result: ConversationsOpenResponse = self
            .post_with_retry(
                &format!("{}/conversations.open", self.base_url),
                &body,
                "conversations.open",
            )
            .await?;
        debug!(channel_id = ?result.channel.id, "slack: conversations.open success");
        Ok(result)
    }

    /// Search for messages saved for later (`is:saved` modifier).
    pub async fn search_messages_saved(
        &self,
        cursor: Option<&str>,
        count: u32,
    ) -> Result<SearchMessagesResponse, SlackError> {
        let mut params: Vec<(&str, String)> = vec![
            ("query", "is:saved".into()),
            ("sort", "timestamp".into()),
            ("sort_dir", "desc".into()),
            ("count", count.to_string()),
        ];
        if let Some(c) = cursor {
            params.push(("cursor", c.to_string()));
        }
        self.get_with_retry(
            &format!("{}/search.messages", self.base_url),
            &params,
            "search.messages (saved)",
        )
        .await
    }
}
