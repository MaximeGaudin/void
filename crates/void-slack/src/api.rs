use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Low-level Slack Web API client using user token.
pub struct SlackApiClient {
    http: reqwest::Client,
    user_token: String,
}

impl SlackApiClient {
    pub fn new(user_token: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            user_token: user_token.to_string(),
        }
    }

    pub async fn auth_test(&self) -> anyhow::Result<AuthTestResponse> {
        let resp: SlackResponse<AuthTestResponse> = self
            .http
            .post("https://slack.com/api/auth.test")
            .bearer_auth(&self.user_token)
            .send()
            .await?
            .json()
            .await?;
        resp.into_result().context("auth.test failed")
    }

    pub async fn conversations_list(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> anyhow::Result<ConversationsListResponse> {
        let mut params = vec![
            (
                "types",
                "public_channel,private_channel,mpim,im".to_string(),
            ),
            ("limit", limit.to_string()),
            ("exclude_archived", "true".to_string()),
        ];
        if let Some(c) = cursor {
            params.push(("cursor", c.to_string()));
        }
        let resp: SlackResponse<ConversationsListResponse> = self
            .http
            .get("https://slack.com/api/conversations.list")
            .bearer_auth(&self.user_token)
            .query(&params)
            .send()
            .await?
            .json()
            .await?;
        resp.into_result().context("conversations.list failed")
    }

    pub async fn conversations_history(
        &self,
        channel_id: &str,
        limit: u32,
        oldest: Option<&str>,
    ) -> anyhow::Result<ConversationsHistoryResponse> {
        let mut params = vec![
            ("channel", channel_id.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(o) = oldest {
            params.push(("oldest", o.to_string()));
        }
        let resp: SlackResponse<ConversationsHistoryResponse> = self
            .http
            .get("https://slack.com/api/conversations.history")
            .bearer_auth(&self.user_token)
            .query(&params)
            .send()
            .await?
            .json()
            .await?;
        resp.into_result().context("conversations.history failed")
    }

    pub async fn users_info(&self, user_id: &str) -> anyhow::Result<UserInfoResponse> {
        let resp: SlackResponse<UserInfoResponse> = self
            .http
            .get("https://slack.com/api/users.info")
            .bearer_auth(&self.user_token)
            .query(&[("user", user_id)])
            .send()
            .await?
            .json()
            .await?;
        resp.into_result().context("users.info failed")
    }

    pub async fn chat_post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> anyhow::Result<ChatPostMessageResponse> {
        let mut body = serde_json::json!({
            "channel": channel,
            "text": text,
        });
        if let Some(ts) = thread_ts {
            body["thread_ts"] = serde_json::Value::String(ts.to_string());
        }
        let resp: SlackResponse<ChatPostMessageResponse> = self
            .http
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.user_token)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        resp.into_result().context("chat.postMessage failed")
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
            Err(anyhow::anyhow!(
                "Slack API error: {}",
                self.error.unwrap_or_else(|| "unknown".into())
            ))
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
}

#[derive(Debug, Deserialize)]
pub struct UserInfoResponse {
    pub user: Option<SlackUser>,
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
