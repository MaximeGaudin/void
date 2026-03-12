use anyhow::Context;
use reqwest::Response;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::{debug, error, warn};

const MAX_RETRIES: u32 = 5;
const DEFAULT_RETRY_SECS: u64 = 5;

const DEFAULT_BASE_URL: &str = "https://slack.com/api";

/// Low-level Slack Web API client using user token.
pub struct SlackApiClient {
    http: reqwest::Client,
    user_token: String,
    base_url: String,
}

impl SlackApiClient {
    pub fn new(user_token: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            user_token: user_token.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(user_token: &str, base_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            user_token: user_token.to_string(),
            base_url: base_url.to_string(),
        }
    }

    /// Extract `Retry-After` header (seconds) from a response, default to `DEFAULT_RETRY_SECS`.
    fn retry_after(resp: &Response) -> u64 {
        resp.headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_RETRY_SECS)
    }

    /// GET with automatic retry on 429 / `ratelimited`.
    async fn get_with_retry<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, String)],
        label: &str,
    ) -> anyhow::Result<T> {
        for attempt in 0..=MAX_RETRIES {
            let resp = self
                .http
                .get(url)
                .bearer_auth(&self.user_token)
                .query(params)
                .send()
                .await?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let wait = Self::retry_after(&resp);
                if attempt < MAX_RETRIES {
                    warn!(
                        wait_secs = wait,
                        attempt, label, "rate limited, backing off"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                anyhow::bail!("rate limited after {MAX_RETRIES} retries: {label}");
            }

            let slack_resp: SlackResponse<T> = resp.json().await?;
            if let Some(ref err) = slack_resp.error {
                if err == "ratelimited" && attempt < MAX_RETRIES {
                    warn!(attempt, label, "rate limited (json), backing off");
                    tokio::time::sleep(std::time::Duration::from_secs(DEFAULT_RETRY_SECS)).await;
                    continue;
                }
            }
            return slack_resp.into_result().context(format!("{label} failed"));
        }
        unreachable!()
    }

    /// POST (JSON body) with automatic retry on 429 / `ratelimited`.
    async fn post_with_retry<T: DeserializeOwned>(
        &self,
        url: &str,
        body: &serde_json::Value,
        label: &str,
    ) -> anyhow::Result<T> {
        for attempt in 0..=MAX_RETRIES {
            let resp = self
                .http
                .post(url)
                .bearer_auth(&self.user_token)
                .json(body)
                .send()
                .await?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let wait = Self::retry_after(&resp);
                if attempt < MAX_RETRIES {
                    warn!(
                        wait_secs = wait,
                        attempt, label, "rate limited, backing off"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                anyhow::bail!("rate limited after {MAX_RETRIES} retries: {label}");
            }

            let slack_resp: SlackResponse<T> = resp.json().await?;
            if let Some(ref err) = slack_resp.error {
                if err == "ratelimited" && attempt < MAX_RETRIES {
                    warn!(attempt, label, "rate limited (json), backing off");
                    tokio::time::sleep(std::time::Duration::from_secs(DEFAULT_RETRY_SECS)).await;
                    continue;
                }
            }
            return slack_resp.into_result().context(format!("{label} failed"));
        }
        unreachable!()
    }

    pub async fn auth_test(&self) -> anyhow::Result<AuthTestResponse> {
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
    ) -> anyhow::Result<ConversationsListResponse> {
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
    ) -> anyhow::Result<ConversationsHistoryResponse> {
        let mut params: Vec<(&str, String)> = vec![
            ("channel", channel_id.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(o) = oldest {
            params.push(("oldest", o.to_string()));
        }
        self.get_with_retry(
            &format!("{}/conversations.history", self.base_url),
            &params,
            "conversations.history",
        )
        .await
    }

    pub async fn users_info(&self, user_id: &str) -> anyhow::Result<UserInfoResponse> {
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
    ) -> anyhow::Result<UsersListResponse> {
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

    pub async fn conversations_info(&self, channel: &str) -> anyhow::Result<SlackConversation> {
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

    pub async fn conversations_mark(&self, channel: &str, ts: &str) -> anyhow::Result<()> {
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

    pub async fn chat_post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> anyhow::Result<ChatPostMessageResponse> {
        debug!(channel, thread_ts, "slack: chat.postMessage");
        let mut body = serde_json::json!({
            "channel": channel,
            "text": text,
        });
        if let Some(ts) = thread_ts {
            body["thread_ts"] = serde_json::Value::String(ts.to_string());
        }
        let result: ChatPostMessageResponse = self
            .post_with_retry(
                &format!("{}/chat.postMessage", self.base_url),
                &body,
                "chat.postMessage",
            )
            .await?;
        debug!(ts = ?result.ts, "slack: chat.postMessage success");
        Ok(result)
    }

    pub async fn chat_update(
        &self,
        channel: &str,
        ts: &str,
        text: &str,
    ) -> anyhow::Result<ChatUpdateResponse> {
        debug!(channel, ts, "slack: chat.update");
        let body = serde_json::json!({
            "channel": channel,
            "ts": ts,
            "text": text,
        });
        let result: ChatUpdateResponse = self
            .post_with_retry(
                &format!("{}/chat.update", self.base_url),
                &body,
                "chat.update",
            )
            .await?;
        debug!(ts = ?result.ts, "slack: chat.update success");
        Ok(result)
    }

    /// Call apps.connections.open with an app-level token to get a WebSocket URL for Socket Mode.
    pub async fn connections_open(
        &self,
        app_token: &str,
    ) -> anyhow::Result<ConnectionsOpenResponse> {
        let resp = self
            .http
            .post(format!("{}/apps.connections.open", self.base_url))
            .bearer_auth(app_token)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .await?;

        let slack_resp: SlackResponse<ConnectionsOpenResponse> = resp.json().await?;
        slack_resp
            .into_result()
            .context("apps.connections.open failed")
    }

    pub async fn chat_schedule_message(
        &self,
        channel: &str,
        text: &str,
        post_at: i64,
        thread_ts: Option<&str>,
    ) -> anyhow::Result<ChatScheduleMessageResponse> {
        debug!(channel, post_at, "slack: chat.scheduleMessage");
        let mut body = serde_json::json!({
            "channel": channel,
            "text": text,
            "post_at": post_at,
        });
        if let Some(ts) = thread_ts {
            body["thread_ts"] = serde_json::Value::String(ts.to_string());
        }
        let result: ChatScheduleMessageResponse = self
            .post_with_retry(
                &format!("{}/chat.scheduleMessage", self.base_url),
                &body,
                "chat.scheduleMessage",
            )
            .await?;
        debug!(scheduled_message_id = ?result.scheduled_message_id, "slack: chat.scheduleMessage success");
        Ok(result)
    }

    pub async fn reactions_add(&self, channel: &str, ts: &str, emoji: &str) -> anyhow::Result<()> {
        debug!(channel, ts, emoji, "slack: reactions.add");
        let body = serde_json::json!({
            "channel": channel,
            "timestamp": ts,
            "name": emoji,
        });
        let _: serde_json::Value = self
            .post_with_retry(
                &format!("{}/reactions.add", self.base_url),
                &body,
                "reactions.add",
            )
            .await?;
        debug!(emoji, "slack: reactions.add success");
        Ok(())
    }
}

// -- Slack API response types --

#[derive(Debug, Deserialize)]
struct SlackResponse<T> {
    ok: bool,
    error: Option<String>,
    #[serde(flatten)]
    data: Option<T>,
}

impl<T> SlackResponse<T> {
    fn into_result(self) -> anyhow::Result<T> {
        if self.ok {
            self.data
                .ok_or_else(|| anyhow::anyhow!("Slack returned ok=true but no data"))
        } else {
            let err = self.error.unwrap_or_else(|| "unknown".into());
            error!(error = %err, "slack: API error");
            Err(anyhow::anyhow!("Slack API error: {}", err))
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AuthTestResponse {
    pub url: Option<String>,
    pub team: Option<String>,
    pub user: Option<String>,
    pub team_id: Option<String>,
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConversationsListResponse {
    pub channels: Vec<SlackConversation>,
    pub response_metadata: Option<ResponseMetadata>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseMetadata {
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackConversation {
    pub id: String,
    pub name: Option<String>,
    pub is_channel: Option<bool>,
    pub is_group: Option<bool>,
    pub is_im: Option<bool>,
    pub is_mpim: Option<bool>,
    pub is_private: Option<bool>,
    pub user: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConversationInfoResponse {
    pub channel: SlackConversation,
}

#[derive(Debug, Deserialize)]
pub struct ConversationsHistoryResponse {
    pub messages: Vec<SlackMessage>,
    pub has_more: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackMessage {
    pub ts: String,
    pub user: Option<String>,
    pub text: Option<String>,
    pub thread_ts: Option<String>,
    #[serde(rename = "type")]
    pub msg_type: Option<String>,
    pub subtype: Option<String>,
    #[serde(default)]
    pub reactions: Vec<SlackReaction>,
    #[serde(default)]
    pub files: Vec<SlackFile>,
    #[serde(default)]
    pub attachments: Vec<SlackAttachment>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackFile {
    pub id: String,
    pub name: Option<String>,
    pub title: Option<String>,
    pub mimetype: Option<String>,
    pub filetype: Option<String>,
    pub size: Option<u64>,
    pub url_private: Option<String>,
    pub permalink: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackAttachment {
    pub fallback: Option<String>,
    pub title: Option<String>,
    pub text: Option<String>,
    pub image_url: Option<String>,
    pub from_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackReaction {
    pub name: String,
    pub count: u32,
    #[serde(default)]
    pub users: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UserInfoResponse {
    pub user: Option<SlackUser>,
}

#[derive(Debug, Deserialize)]
pub struct UsersListResponse {
    pub members: Vec<SlackUser>,
    pub response_metadata: Option<ResponseMetadata>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackUser {
    pub id: String,
    pub name: String,
    pub real_name: Option<String>,
    pub profile: Option<SlackUserProfile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackUserProfile {
    pub display_name: Option<String>,
    pub real_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatPostMessageResponse {
    pub channel: Option<String>,
    pub ts: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatUpdateResponse {
    pub channel: Option<String>,
    pub ts: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatScheduleMessageResponse {
    pub channel: Option<String>,
    pub scheduled_message_id: Option<String>,
    pub post_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ConnectionsOpenResponse {
    pub url: String,
}
