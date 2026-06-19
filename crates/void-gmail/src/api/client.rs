use std::time::Duration;

use crate::error::GmailError;
use tracing::{debug, info};

use super::types::{
    AttachmentResponse, DraftListResponse, GmailDraft, GmailMessage, GmailProfile, GmailThread,
    HistoryListResponse, HistoryRecord, LabelListResponse, MessageListResponse,
};

const DEFAULT_BASE_URL: &str = "https://gmail.googleapis.com";
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .expect("failed to build HTTP client")
}

/// Low-level Gmail API client.
pub struct GmailApiClient {
    http: reqwest::Client,
    access_token: String,
    base_url: String,
}

impl GmailApiClient {
    pub fn new(access_token: &str) -> Self {
        Self {
            http: build_http_client(),
            access_token: access_token.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(access_token: &str, base_url: &str) -> Self {
        Self {
            http: build_http_client(),
            access_token: access_token.to_string(),
            base_url: base_url.to_string(),
        }
    }

    pub fn set_token(&mut self, token: &str) {
        self.access_token = token.to_string();
    }

    pub async fn get_profile(&self) -> Result<GmailProfile, GmailError> {
        debug!("gmail: get_profile");
        let resp: GmailProfile = self
            .http
            .get(format!("{}/gmail/v1/users/me/profile", self.base_url))
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .json()
            .await?;
        debug!(email = ?resp.email_address, "gmail: got profile");
        Ok(resp)
    }

    pub async fn list_messages(
        &self,
        max_results: u32,
        page_token: Option<&str>,
        label_ids: Option<&[&str]>,
        query: Option<&str>,
    ) -> Result<MessageListResponse, GmailError> {
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
            .get(format!("{}/gmail/v1/users/me/messages", self.base_url))
            .bearer_auth(&self.access_token)
            .query(&params)
            .send()
            .await?
            .json()
            .await?;
        let count = resp.messages.as_ref().map(|m| m.len()).unwrap_or(0);
        debug!(
            message_count = count,
            has_more = resp.next_page_token.is_some(),
            "gmail: listed messages"
        );
        Ok(resp)
    }

    pub async fn get_message(&self, message_id: &str) -> Result<GmailMessage, GmailError> {
        debug!(message_id, "gmail: get_message");
        let resp: GmailMessage = self
            .http
            .get(format!(
                "{}/gmail/v1/users/me/messages/{message_id}",
                self.base_url
            ))
            .bearer_auth(&self.access_token)
            .query(&[("format", "full")])
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn list_history(
        &self,
        start_history_id: &str,
        label_id: Option<&str>,
    ) -> Result<HistoryListResponse, GmailError> {
        debug!(start_history_id, ?label_id, "gmail: list_history");
        let mut all_records: Vec<HistoryRecord> = Vec::new();
        let mut page_token: Option<String> = None;
        let mut latest_history_id: Option<String> = None;
        let max_pages = 10u32;

        for page in 0..max_pages {
            let mut params = vec![("startHistoryId", start_history_id.to_string())];
            if let Some(label) = label_id {
                params.push(("labelId", label.to_string()));
            }
            if let Some(pt) = &page_token {
                params.push(("pageToken", pt.clone()));
            }
            let resp: HistoryListResponse = self
                .http
                .get(format!("{}/gmail/v1/users/me/history", self.base_url))
                .bearer_auth(&self.access_token)
                .query(&params)
                .send()
                .await?
                .json()
                .await?;

            if let Some(records) = resp.history {
                let count = records.len();
                all_records.extend(records);
                debug!(page, record_count = count, "gmail: listed history page");
            }
            latest_history_id = resp.history_id.or(latest_history_id);
            page_token = resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        debug!(
            total_records = all_records.len(),
            "gmail: listed history (all pages)"
        );
        Ok(HistoryListResponse {
            history: if all_records.is_empty() {
                None
            } else {
                Some(all_records)
            },
            history_id: latest_history_id,
            next_page_token: None,
        })
    }

    pub async fn modify_message(
        &self,
        message_id: &str,
        add_labels: &[&str],
        remove_labels: &[&str],
    ) -> Result<GmailMessage, GmailError> {
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
                "{}/gmail/v1/users/me/messages/{message_id}/modify",
                self.base_url
            ))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        debug!(message_id, "gmail: message modified");
        Ok(resp)
    }

    pub async fn send_message(&self, raw: &str) -> Result<GmailMessage, GmailError> {
        info!("gmail: send_message");
        let body = serde_json::json!({ "raw": raw });
        let resp: GmailMessage = self
            .http
            .post(format!("{}/gmail/v1/users/me/messages/send", self.base_url))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        debug!(message_id = ?resp.id, "gmail: sent message");
        Ok(resp)
    }

    pub async fn get_thread(&self, thread_id: &str) -> Result<GmailThread, GmailError> {
        debug!(thread_id, "gmail: get_thread");
        let resp: GmailThread = self
            .http
            .get(format!(
                "{}/gmail/v1/users/me/threads/{thread_id}",
                self.base_url
            ))
            .bearer_auth(&self.access_token)
            .query(&[("format", "full")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let msg_count = resp.messages.as_ref().map(|m| m.len()).unwrap_or(0);
        debug!(thread_id, msg_count, "gmail: get_thread ok");
        Ok(resp)
    }

    pub async fn get_attachment(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<AttachmentResponse, GmailError> {
        debug!(message_id, attachment_id, "gmail: get_attachment");
        let resp: AttachmentResponse = self
            .http
            .get(format!(
                "{}/gmail/v1/users/me/messages/{message_id}/attachments/{attachment_id}",
                self.base_url
            ))
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        debug!(message_id, attachment_id, "gmail: get_attachment ok");
        Ok(resp)
    }

    pub async fn list_labels(&self) -> Result<LabelListResponse, GmailError> {
        debug!("gmail: list_labels");
        let resp: LabelListResponse = self
            .http
            .get(format!("{}/gmail/v1/users/me/labels", self.base_url))
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let count = resp.labels.as_ref().map(|l| l.len()).unwrap_or(0);
        debug!(count, "gmail: list_labels ok");
        Ok(resp)
    }

    pub async fn modify_thread(
        &self,
        thread_id: &str,
        add_labels: &[&str],
        remove_labels: &[&str],
    ) -> Result<GmailThread, GmailError> {
        debug!(
            thread_id,
            ?add_labels,
            ?remove_labels,
            "gmail: modify_thread"
        );
        let body = serde_json::json!({
            "addLabelIds": add_labels,
            "removeLabelIds": remove_labels,
        });
        let resp: GmailThread = self
            .http
            .post(format!(
                "{}/gmail/v1/users/me/threads/{thread_id}/modify",
                self.base_url
            ))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        debug!(thread_id, "gmail: modify_thread ok");
        Ok(resp)
    }

    pub async fn batch_modify_messages(
        &self,
        message_ids: &[&str],
        add_labels: &[&str],
        remove_labels: &[&str],
    ) -> Result<(), GmailError> {
        debug!(
            ?message_ids,
            ?add_labels,
            ?remove_labels,
            "gmail: batch_modify"
        );
        let body = serde_json::json!({
            "ids": message_ids,
            "addLabelIds": add_labels,
            "removeLabelIds": remove_labels,
        });
        self.http
            .post(format!(
                "{}/gmail/v1/users/me/messages/batchModify",
                self.base_url
            ))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        debug!("gmail: batch_modify ok");
        Ok(())
    }

    pub async fn list_drafts(&self, max_results: u32) -> Result<DraftListResponse, GmailError> {
        debug!(max_results, "gmail: list_drafts");
        let resp: DraftListResponse = self
            .http
            .get(format!("{}/gmail/v1/users/me/drafts", self.base_url))
            .bearer_auth(&self.access_token)
            .query(&[("maxResults", max_results.to_string())])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let count = resp.drafts.as_ref().map(|d| d.len()).unwrap_or(0);
        debug!(count, "gmail: list_drafts ok");
        Ok(resp)
    }

    pub async fn get_draft(&self, draft_id: &str) -> Result<GmailDraft, GmailError> {
        debug!(draft_id, "gmail: get_draft");
        let resp: GmailDraft = self
            .http
            .get(format!(
                "{}/gmail/v1/users/me/drafts/{draft_id}",
                self.base_url
            ))
            .bearer_auth(&self.access_token)
            .query(&[("format", "full")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        debug!(draft_id, "gmail: get_draft ok");
        Ok(resp)
    }

    pub async fn create_draft(
        &self,
        raw: &str,
        thread_id: Option<&str>,
    ) -> Result<GmailDraft, GmailError> {
        info!("gmail: create_draft");
        let mut message = serde_json::json!({ "raw": raw });
        if let Some(tid) = thread_id {
            message["threadId"] = serde_json::Value::String(tid.to_string());
        }
        let body = serde_json::json!({ "message": message });
        let resp: GmailDraft = self
            .http
            .post(format!("{}/gmail/v1/users/me/drafts", self.base_url))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        debug!(draft_id = ?resp.id, "gmail: create_draft ok");
        Ok(resp)
    }

    pub async fn update_draft(&self, draft_id: &str, raw: &str) -> Result<GmailDraft, GmailError> {
        debug!(draft_id, "gmail: update_draft");
        let body = serde_json::json!({
            "message": { "raw": raw }
        });
        let resp: GmailDraft = self
            .http
            .put(format!(
                "{}/gmail/v1/users/me/drafts/{draft_id}",
                self.base_url
            ))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        debug!(draft_id, "gmail: update_draft ok");
        Ok(resp)
    }

    pub async fn delete_draft(&self, draft_id: &str) -> Result<(), GmailError> {
        debug!(draft_id, "gmail: delete_draft");
        self.http
            .delete(format!(
                "{}/gmail/v1/users/me/drafts/{draft_id}",
                self.base_url
            ))
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()?;
        debug!(draft_id, "gmail: delete_draft ok");
        Ok(())
    }
}
