use std::path::Path;

use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde_json::Value;

use super::types::{
    AccountResponse, ListResponse, SendMessageResponse, UnipileChat, UnipileChatAttendee,
    UnipileMessage, UnipileUserProfile,
};
use crate::error::LinkedInError;

/// Normalize a Unipile DSN (base URL) to `https://{host}/api/v1`.
pub fn normalize_api_base(dsn: &str) -> String {
    let mut trimmed = dsn.trim().trim_end_matches('/').to_string();
    if !trimmed.contains("://") {
        trimmed = format!("https://{trimmed}");
    }
    if trimmed.ends_with("/api/v1") {
        trimmed
    } else {
        format!("{trimmed}/api/v1")
    }
}

#[derive(Debug, Clone)]
pub struct UnipileClient {
    pub(in crate::api) base_url: String,
    pub(in crate::api) api_key: String,
    pub(in crate::api) http: Client,
}

impl UnipileClient {
    pub fn new(dsn: &str, api_key: &str) -> Self {
        Self {
            base_url: normalize_api_base(dsn),
            api_key: api_key.to_string(),
            http: Client::new(),
        }
    }

    /// Build a client against an explicit API base (for tests and wiremock).
    /// `base_url` must include `/api/v1`, e.g. `http://127.0.0.1:PORT/api/v1`.
    pub fn with_api_base(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            http: Client::new(),
        }
    }

    pub(in crate::api) fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    pub(in crate::api) async fn get_json(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value, LinkedInError> {
        let mut req = self
            .http
            .get(self.url(path))
            .header("X-API-KEY", &self.api_key)
            .header("accept", "application/json");

        for (key, value) in query {
            req = req.query(&[(key, value)]);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| LinkedInError::Connection(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(LinkedInError::Auth(
                "Invalid Unipile API key (401 Unauthorized)".into(),
            ));
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(LinkedInError::Connection(format!(
                "GET {path} failed ({status}): {body}"
            )));
        }

        resp.json()
            .await
            .map_err(|e| LinkedInError::Decode(e.to_string()))
    }

    pub async fn get_user_profile(
        &self,
        account_id: &str,
        provider_id: &str,
    ) -> Result<UnipileUserProfile, LinkedInError> {
        let value = self
            .get_json(
                &format!("users/{}", urlencoding::encode(provider_id)),
                &[("account_id", account_id.to_string())],
            )
            .await?;
        serde_json::from_value(value).map_err(|e| LinkedInError::Decode(e.to_string()))
    }

    pub async fn get_chat_attendee(
        &self,
        attendee_id: &str,
    ) -> Result<UnipileChatAttendee, LinkedInError> {
        let value = self
            .get_json(&format!("chat_attendees/{attendee_id}"), &[])
            .await?;
        serde_json::from_value(value).map_err(|e| LinkedInError::Decode(e.to_string()))
    }

    pub async fn get_account(&self, account_id: &str) -> Result<AccountResponse, LinkedInError> {
        let value = self
            .get_json(&format!("accounts/{account_id}"), &[])
            .await?;
        serde_json::from_value(value).map_err(|e| LinkedInError::Decode(e.to_string()))
    }

    pub async fn list_chats(
        &self,
        account_id: &str,
        cursor: Option<&str>,
        after: Option<&str>,
        limit: u32,
    ) -> Result<ListResponse<UnipileChat>, LinkedInError> {
        let mut query = vec![
            ("account_id", account_id.to_string()),
            ("account_type", "LINKEDIN".to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(c) = cursor {
            query.push(("cursor", c.to_string()));
        }
        if let Some(a) = after {
            query.push(("after", a.to_string()));
        }

        let value = self.get_json("chats", &query).await?;
        serde_json::from_value(value).map_err(|e| LinkedInError::Decode(e.to_string()))
    }

    pub async fn list_chat_messages(
        &self,
        chat_id: &str,
        cursor: Option<&str>,
        after: Option<&str>,
        limit: u32,
    ) -> Result<ListResponse<UnipileMessage>, LinkedInError> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(c) = cursor {
            query.push(("cursor", c.to_string()));
        }
        if let Some(a) = after {
            query.push(("after", a.to_string()));
        }

        let value = self
            .get_json(&format!("chats/{chat_id}/messages"), &query)
            .await?;
        serde_json::from_value(value).map_err(|e| LinkedInError::Decode(e.to_string()))
    }

    pub async fn send_message_in_chat(
        &self,
        chat_id: &str,
        text: &str,
        file_path: Option<&Path>,
    ) -> Result<String, LinkedInError> {
        let mut form = Form::new().text("text", text.to_string());
        if let Some(path) = file_path {
            let bytes = std::fs::read(path)
                .map_err(|e| LinkedInError::Media(format!("read file {}: {e}", path.display())))?;
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("attachment");
            let part = Part::bytes(bytes)
                .file_name(file_name.to_string())
                .mime_str("application/octet-stream")
                .map_err(|e| LinkedInError::Media(e.to_string()))?;
            form = form.part("attachments", part);
        }

        let resp = self
            .http
            .post(self.url(&format!("chats/{chat_id}/messages")))
            .header("X-API-KEY", &self.api_key)
            .header("accept", "application/json")
            .multipart(form)
            .send()
            .await
            .map_err(|e| LinkedInError::Connection(e.to_string()))?;

        Self::parse_send_response(resp, "send message in chat").await
    }

    pub async fn start_new_chat(
        &self,
        account_id: &str,
        attendee_id: &str,
        text: &str,
        file_path: Option<&Path>,
    ) -> Result<String, LinkedInError> {
        let mut form = Form::new()
            .text("account_id", account_id.to_string())
            .text("text", text.to_string())
            .text("attendees_ids", attendee_id.to_string());

        if let Some(path) = file_path {
            let bytes = std::fs::read(path)
                .map_err(|e| LinkedInError::Media(format!("read file {}: {e}", path.display())))?;
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("attachment");
            let part = Part::bytes(bytes)
                .file_name(file_name.to_string())
                .mime_str("application/octet-stream")
                .map_err(|e| LinkedInError::Media(e.to_string()))?;
            form = form.part("attachments", part);
        }

        let resp = self
            .http
            .post(self.url("chats"))
            .header("X-API-KEY", &self.api_key)
            .header("accept", "application/json")
            .multipart(form)
            .send()
            .await
            .map_err(|e| LinkedInError::Connection(e.to_string()))?;

        Self::parse_send_response(resp, "start new chat").await
    }

    pub async fn download_attachment(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, LinkedInError> {
        let resp = self
            .http
            .get(self.url(&format!(
                "messages/{message_id}/attachments/{attachment_id}"
            )))
            .header("X-API-KEY", &self.api_key)
            .send()
            .await
            .map_err(|e| LinkedInError::Connection(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(LinkedInError::Media(format!(
                "download attachment failed ({status}): {body}"
            )));
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| LinkedInError::Media(e.to_string()))?;

        if content_type.contains("application/json") {
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|e| LinkedInError::Decode(e.to_string()))?;
            if let Some(b64) = value.get("data").and_then(|v| v.as_str()) {
                use base64::Engine;
                return base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| LinkedInError::Decode(e.to_string()));
            }
        }

        Ok(bytes.to_vec())
    }

    pub(super) async fn parse_send_response(
        resp: reqwest::Response,
        action: &str,
    ) -> Result<String, LinkedInError> {
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(LinkedInError::Auth(
                "Invalid Unipile API key (401 Unauthorized)".into(),
            ));
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(LinkedInError::Connection(format!(
                "{action} failed ({status}): {body}"
            )));
        }

        let value: Value = resp
            .json()
            .await
            .map_err(|e| LinkedInError::Decode(e.to_string()))?;

        if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
            return Ok(id.to_string());
        }
        if let Some(id) = value.get("message_id").and_then(|v| v.as_str()) {
            return Ok(id.to_string());
        }

        let parsed: SendMessageResponse = serde_json::from_value(value.clone())
            .map_err(|e| LinkedInError::Decode(e.to_string()))?;
        parsed
            .id
            .ok_or_else(|| LinkedInError::Decode(format!("no message id in response: {value}")))
    }
}
