use anyhow::Context;
use serde::Deserialize;
use tracing::{debug, info};

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
        debug!("gmail: get_profile");
        let resp: GmailProfile = self
            .http
            .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .json()
            .await
            .context("gmail: failed to get profile")?;
        debug!(email = ?resp.email_address, "gmail: got profile");
        Ok(resp)
    }

    pub async fn list_messages(
        &self,
        max_results: u32,
        page_token: Option<&str>,
        label_ids: Option<&[&str]>,
        query: Option<&str>,
    ) -> anyhow::Result<MessageListResponse> {
        debug!(
            max_results,
            has_page_token = page_token.is_some(),
            query,
            "gmail: list_messages"
        );
        let mut params = vec![("maxResults", max_results.to_string())];
        if let Some(pt) = page_token {
            params.push(("pageToken", pt.to_string()));
        }
        if let Some(labels) = label_ids {
            for label in labels {
                params.push(("labelIds", label.to_string()));
            }
        }
        if let Some(q) = query {
            params.push(("q", q.to_string()));
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
        let count = resp.messages.as_ref().map(|m| m.len()).unwrap_or(0);
        debug!(
            message_count = count,
            has_more = resp.next_page_token.is_some(),
            "gmail: listed messages"
        );
        Ok(resp)
    }

    pub async fn get_message(&self, message_id: &str) -> anyhow::Result<GmailMessage> {
        debug!(message_id, "gmail: get_message");
        let resp: GmailMessage = self
            .http
            .get(format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/messages/{message_id}"
            ))
            .bearer_auth(&self.access_token)
            .query(&[("format", "full")])
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
        debug!(start_history_id, "gmail: list_history");
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
        let count = resp.history.as_ref().map(|h| h.len()).unwrap_or(0);
        debug!(record_count = count, "gmail: listed history");
        Ok(resp)
    }

    pub async fn modify_message(
        &self,
        message_id: &str,
        add_labels: &[&str],
        remove_labels: &[&str],
    ) -> anyhow::Result<GmailMessage> {
        debug!(
            message_id,
            ?add_labels,
            ?remove_labels,
            "gmail: modify_message"
        );
        let body = serde_json::json!({
            "addLabelIds": add_labels,
            "removeLabelIds": remove_labels,
        });
        let resp: GmailMessage = self
            .http
            .post(format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/messages/{message_id}/modify"
            ))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?
            .json()
            .await
            .context("gmail: failed to modify message")?;
        debug!(message_id, "gmail: message modified");
        Ok(resp)
    }

    pub async fn send_message(&self, raw: &str) -> anyhow::Result<GmailMessage> {
        info!("gmail: send_message");
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
        debug!(message_id = ?resp.id, "gmail: sent message");
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
    pub label_ids: Option<Vec<String>>,
    pub payload: Option<MessagePayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePayload {
    pub mime_type: Option<String>,
    pub headers: Option<Vec<MessageHeader>>,
    pub body: Option<MessagePartBody>,
    pub parts: Option<Vec<MessagePart>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePart {
    pub mime_type: Option<String>,
    pub headers: Option<Vec<MessageHeader>>,
    pub body: Option<MessagePartBody>,
    pub parts: Option<Vec<MessagePart>>,
}

#[derive(Debug, Deserialize)]
pub struct MessagePartBody {
    pub data: Option<String>,
    pub size: Option<u64>,
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

    /// Extract the plain text body by walking the MIME tree.
    pub fn text_body(&self) -> Option<String> {
        self.payload
            .as_ref()
            .and_then(|p| extract_body_by_mime(p, "text/plain"))
    }

    /// Extract the HTML body by walking the MIME tree.
    pub fn html_body(&self) -> Option<String> {
        self.payload
            .as_ref()
            .and_then(|p| extract_body_by_mime(p, "text/html"))
    }
}

fn extract_body_by_mime(payload: &MessagePayload, target_mime: &str) -> Option<String> {
    if let Some(mime) = &payload.mime_type {
        if mime == target_mime {
            return decode_body_data(&payload.body);
        }
    }

    if let Some(parts) = &payload.parts {
        for part in parts {
            if let Some(result) = extract_body_from_part(part, target_mime) {
                return Some(result);
            }
        }
    }

    None
}

fn extract_body_from_part(part: &MessagePart, target_mime: &str) -> Option<String> {
    if let Some(mime) = &part.mime_type {
        if mime == target_mime {
            return decode_body_data(&part.body);
        }
    }

    if let Some(sub_parts) = &part.parts {
        for sub in sub_parts {
            if let Some(result) = extract_body_from_part(sub, target_mime) {
                return Some(result);
            }
        }
    }

    None
}

fn decode_body_data(body: &Option<MessagePartBody>) -> Option<String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let data = body.as_ref()?.data.as_deref()?;
    let bytes = URL_SAFE_NO_PAD.decode(data).ok()?;
    String::from_utf8(bytes).ok()
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
