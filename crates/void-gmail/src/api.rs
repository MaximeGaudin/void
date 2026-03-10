use anyhow::Context;
use serde::Deserialize;

/// Low-level Gmail API client.
pub struct GmailApiClient {
    http: reqwest::Client,
    access_token: String,
}

impl GmailApiClient {
    pub fn new(access_token: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            access_token: access_token.to_string(),
        }
    }

    pub fn set_token(&mut self, token: &str) {
        self.access_token = token.to_string();
    }

    pub async fn get_profile(&self) -> anyhow::Result<GmailProfile> {
        let resp: GmailProfile = self
            .http
            .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .json()
            .await
            .context("gmail: failed to get profile")?;
        Ok(resp)
    }

    pub async fn list_messages(
        &self,
        max_results: u32,
        page_token: Option<&str>,
    ) -> anyhow::Result<MessageListResponse> {
        let mut params = vec![("maxResults", max_results.to_string())];
        if let Some(pt) = page_token {
            params.push(("pageToken", pt.to_string()));
        }
        let resp: MessageListResponse = self
            .http
            .get("https://gmail.googleapis.com/gmail/v1/users/me/messages")
            .bearer_auth(&self.access_token)
            .query(&params)
            .send()
            .await?
            .json()
            .await
            .context("gmail: failed to list messages")?;
        Ok(resp)
    }

    pub async fn get_message(&self, message_id: &str) -> anyhow::Result<GmailMessage> {
        let resp: GmailMessage = self
            .http
            .get(format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/messages/{message_id}"
            ))
            .bearer_auth(&self.access_token)
            .query(&[
                ("format", "metadata"),
                ("metadataHeaders", "From"),
                ("metadataHeaders", "To"),
                ("metadataHeaders", "Subject"),
                ("metadataHeaders", "Date"),
                ("metadataHeaders", "In-Reply-To"),
                ("metadataHeaders", "Message-ID"),
            ])
            .send()
            .await?
            .json()
            .await
            .context("gmail: failed to get message")?;
        Ok(resp)
    }

    pub async fn list_history(
        &self,
        start_history_id: &str,
    ) -> anyhow::Result<HistoryListResponse> {
        let resp: HistoryListResponse = self
            .http
            .get("https://gmail.googleapis.com/gmail/v1/users/me/history")
            .bearer_auth(&self.access_token)
            .query(&[("startHistoryId", start_history_id)])
            .send()
            .await?
            .json()
            .await
            .context("gmail: failed to list history")?;
        Ok(resp)
    }

    pub async fn send_message(&self, raw: &str) -> anyhow::Result<GmailMessage> {
        let body = serde_json::json!({ "raw": raw });
        let resp: GmailMessage = self
            .http
            .post("https://gmail.googleapis.com/gmail/v1/users/me/messages/send")
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?
            .json()
            .await
            .context("gmail: failed to send message")?;
        Ok(resp)
    }
}

// -- Gmail API types --

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailProfile {
    pub email_address: Option<String>,
    pub history_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageListResponse {
    pub messages: Option<Vec<MessageRef>>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MessageRef {
    pub id: String,
    #[serde(rename = "threadId")]
    pub thread_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailMessage {
    pub id: Option<String>,
    pub thread_id: Option<String>,
    pub snippet: Option<String>,
    pub internal_date: Option<String>,
    pub payload: Option<MessagePayload>,
}

#[derive(Debug, Deserialize)]
pub struct MessagePayload {
    pub headers: Option<Vec<MessageHeader>>,
}

#[derive(Debug, Deserialize)]
pub struct MessageHeader {
    pub name: String,
    pub value: String,
}

impl GmailMessage {
    pub fn get_header(&self, name: &str) -> Option<String> {
        self.payload
            .as_ref()?
            .headers
            .as_ref()?
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.clone())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryListResponse {
    pub history: Option<Vec<HistoryRecord>>,
    pub history_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    pub messages_added: Option<Vec<HistoryMessageAdded>>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryMessageAdded {
    pub message: MessageRef,
}
