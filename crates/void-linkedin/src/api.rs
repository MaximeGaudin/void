use std::path::Path;

use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::error::LinkedInError;

/// Unipile LinkedIn payloads often use `0`/`1` integers where docs describe booleans.
/// See https://developer.unipile.com/docs/message-payload
mod flexible {
    use serde::de::Deserializer;
    use serde::Deserialize;

    pub fn option_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum BoolOrInt {
            Bool(bool),
            Int(i64),
        }

        match Option::<BoolOrInt>::deserialize(deserializer)? {
            None => Ok(None),
            Some(BoolOrInt::Bool(b)) => Ok(Some(b)),
            Some(BoolOrInt::Int(0)) => Ok(Some(false)),
            Some(BoolOrInt::Int(n)) => Ok(Some(n != 0)),
        }
    }
}

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

#[derive(Debug, Clone, Deserialize)]
pub struct ListResponse<T> {
    #[serde(default)]
    pub items: Vec<T>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// https://developer.unipile.com/docs/message-payload (chat parent object)
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnipileChat {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub account_type: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub attendee_provider_id: Option<String>,
    #[serde(default)]
    pub r#type: Option<i32>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub unread_count: Option<i32>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub pinned: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub archived: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub read_only: Option<bool>,
}

/// https://developer.unipile.com/docs/message-payload
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnipileMessage {
    /// Canonical Unipile message id (dedup key).
    #[serde(alias = "message_id")]
    pub id: String,
    #[serde(default)]
    pub chat_id: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub chat_provider_id: Option<String>,
    #[serde(default)]
    pub sender_id: Option<String>,
    #[serde(default)]
    pub sender_attendee_id: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub attachments: Option<Vec<UnipileAttachment>>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub is_sender: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub seen: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub delivered: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub hidden: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub deleted: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub edited: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub is_event: Option<bool>,
    #[serde(default)]
    pub event_type: Option<i32>,
    #[serde(default)]
    pub message_type: Option<String>,
}

impl UnipileMessage {
    /// Whether this payload should be stored in the Void inbox.
    pub fn is_syncable(&self) -> bool {
        if self.id.is_empty() {
            return false;
        }
        if self.deleted.unwrap_or(false) {
            return false;
        }
        if self.hidden.unwrap_or(false) {
            return false;
        }
        if self.is_event.unwrap_or(false) {
            return false;
        }
        true
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnipileAttachment {
    pub id: String,
    #[serde(default)]
    pub mimetype: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub unavailable: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub sticker: Option<bool>,
}

/// https://developer.unipile.com/docs/retrieving-users
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnipileUserProfile {
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub public_identifier: Option<String>,
    #[serde(default)]
    pub profile_picture_url: Option<String>,
    #[serde(default)]
    pub public_profile_url: Option<String>,
}

impl UnipileUserProfile {
    pub fn display_name(&self) -> Option<String> {
        match (&self.first_name, &self.last_name) {
            (Some(f), Some(l)) if !f.is_empty() || !l.is_empty() => {
                Some(format!("{} {}", f.trim(), l.trim()).trim().to_string())
            }
            (Some(f), None) if !f.is_empty() => Some(f.trim().to_string()),
            (None, Some(l)) if !l.is_empty() => Some(l.trim().to_string()),
            _ => None,
        }
    }

    pub fn profile_url(&self) -> Option<String> {
        if let Some(url) = &self.public_profile_url {
            if !url.is_empty() {
                return Some(url.clone());
            }
        }
        self.public_identifier.as_ref().map(|id| {
            if id.starts_with("http") {
                id.clone()
            } else {
                format!("https://www.linkedin.com/in/{id}")
            }
        })
    }
}

/// https://developer.unipile.com/docs/retrieving-users
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnipileChatAttendee {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub profile_url: Option<String>,
    #[serde(default)]
    pub picture_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SendMessageResponse {
    #[serde(default, alias = "message_id")]
    id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UnipileClient {
    base_url: String,
    api_key: String,
    http: Client,
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

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn get_json(&self, path: &str, query: &[(&str, String)]) -> Result<Value, LinkedInError> {
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

    async fn parse_send_response(
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

#[cfg(test)]
mod integration;

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct BoolFixture {
        #[serde(default, deserialize_with = "flexible::option_bool")]
        is_sender: Option<bool>,
        #[serde(default, deserialize_with = "flexible::option_bool")]
        hidden: Option<bool>,
    }

    /// Captured from GET /chats/{id}/messages (LinkedIn, May 2026).
    const LIVE_MESSAGE_JSON: &str = r#"{
        "object": "MessageList",
        "items": [{
            "object": "Message",
            "seen": 0,
            "text": "Gladia's latency benchmarks for real-time audio transcription are genuinely impressive - sub-300ms in production is rare to pull off at scale.\n\nCurious what the hardest infrastructure tradeoff was to get there.",
            "edited": 0,
            "hidden": 0,
            "chat_id": "Efc-rFoUVMy4MRBsN6BWSw",
            "deleted": 0,
            "seen_by": {},
            "subject": null,
            "behavior": null,
            "is_event": 0,
            "original": "",
            "delivered": 1,
            "is_sender": 0,
            "reactions": [],
            "sender_id": "ACoAAA8Br58BDseKnXYW51e4TU617k-ohrisrcs",
            "timestamp": "2026-05-19T11:41:45.871Z",
            "account_id": "nKz6AVaoTcSef6grRHqsYA",
            "attachments": [],
            "provider_id": "2-MTc3OTE5MDkwNTg3MWI5NTU0My0xMDAmOWYyMjMyNTItYzkzZC00ZjdjLWJjMjYtMTBlNTJmNDJlNTkyXzEwMA==",
            "message_type": "MESSAGE",
            "attendee_type": "MEMBER",
            "chat_provider_id": "2-OWYyMjMyNTItYzkzZC00ZjdjLWJjMjYtMTBlNTJmNDJlNTkyXzEwMA==",
            "attendee_distance": 1,
            "sender_attendee_id": "kZ86fPIEVVmgQbhVgb7auw",
            "id": "lD0rb4Q5W4KdoUICf_MgDQ"
        }]
    }"#;

    /// Captured from GET /chats?account_type=LINKEDIN (May 2026).
    const LIVE_CHAT_JSON: &str = r#"{
        "object": "ChatList",
        "items": [{
            "object": "Chat",
            "name": null,
            "type": 0,
            "folder": ["INBOX", "INBOX_LINKEDIN_CLASSIC"],
            "pinned": 0,
            "unread": 1,
            "archived": 0,
            "read_only": 0,
            "timestamp": "2026-05-19T11:41:46.000Z",
            "account_id": "nKz6AVaoTcSef6grRHqsYA",
            "provider_id": "2-OWYyMjMyNTItYzkzZC00ZjdjLWJjMjYtMTBlNTJmNDJlNTkyXzEwMA==",
            "account_type": "LINKEDIN",
            "unread_count": 1,
            "disabledFeatures": [],
            "attendee_provider_id": "ACoAAA8Br58BDseKnXYW51e4TU617k-ohrisrcs",
            "id": "Efc-rFoUVMy4MRBsN6BWSw",
            "muted_until": null
        }]
    }"#;

    #[test]
    fn normalize_api_base_adds_https_when_scheme_missing() {
        assert_eq!(
            normalize_api_base("api45.unipile.com:17560"),
            "https://api45.unipile.com:17560/api/v1"
        );
    }

    #[test]
    fn deserialize_live_linkedin_message() {
        let list: ListResponse<UnipileMessage> = serde_json::from_str(LIVE_MESSAGE_JSON).unwrap();
        let msg = &list.items[0];
        assert_eq!(msg.id, "lD0rb4Q5W4KdoUICf_MgDQ");
        assert_eq!(msg.is_sender, Some(false));
        assert_eq!(msg.delivered, Some(true));
        assert_eq!(msg.is_event, Some(false));
        assert!(msg.is_syncable());
        assert!(msg.text.as_ref().unwrap().contains("Gladia"));
    }

    #[test]
    fn deserialize_live_linkedin_chat() {
        let list: ListResponse<UnipileChat> = serde_json::from_str(LIVE_CHAT_JSON).unwrap();
        let chat = &list.items[0];
        assert_eq!(chat.id, "Efc-rFoUVMy4MRBsN6BWSw");
        assert_eq!(chat.pinned, Some(false));
        assert_eq!(chat.archived, Some(false));
        assert_eq!(chat.unread_count, Some(1));
    }

    #[test]
    fn skips_hidden_event_and_deleted_messages() {
        let json = r#"{
            "object": "MessageList",
            "items": [
                {"object": "Message", "id": "a", "hidden": 1, "is_event": 0},
                {"object": "Message", "id": "b", "hidden": 0, "is_event": 1, "event_type": 1},
                {"object": "Message", "id": "c", "hidden": 0, "is_event": 0, "deleted": 1},
                {"object": "Message", "id": "d", "hidden": 0, "is_event": 0, "text": "ok"}
            ]
        }"#;
        let list: ListResponse<UnipileMessage> = serde_json::from_str(json).unwrap();
        assert!(!list.items[0].is_syncable());
        assert!(!list.items[1].is_syncable());
        assert!(!list.items[2].is_syncable());
        assert!(list.items[3].is_syncable());
    }

    #[test]
    fn deserialize_legacy_message_id_alias() {
        let json = r#"{"object":"MessageList","items":[{"object":"Message","message_id":"legacy1","is_sender":0}]}"#;
        let list: ListResponse<UnipileMessage> = serde_json::from_str(json).unwrap();
        assert_eq!(list.items[0].id, "legacy1");
    }

    #[test]
    fn flexible_option_bool_deserializes_integers() {
        let v: BoolFixture = serde_json::from_str(r#"{"is_sender":0,"hidden":1}"#).unwrap();
        assert_eq!(v.is_sender, Some(false));
        assert_eq!(v.hidden, Some(true));
    }

    #[test]
    fn flexible_option_bool_deserializes_json_bools() {
        let v: BoolFixture = serde_json::from_str(r#"{"is_sender":true,"hidden":false}"#).unwrap();
        assert_eq!(v.is_sender, Some(true));
        assert_eq!(v.hidden, Some(false));
    }

    #[test]
    fn deserialize_chat_attendee_profile() {
        let json = r#"{
            "object": "ChatAttendee",
            "id": "kZ86fPIEVVmgQbhVgb7auw",
            "name": "Zhirayr Gumruyan",
            "provider_id": "ACoAAA8Br58BDseKnXYW51e4TU617k-ohrisrcs",
            "profile_url": "https://www.linkedin.com/in/gumruyan",
            "picture_url": "https://media.licdn.com/dms/image/example.jpg"
        }"#;
        let attendee: UnipileChatAttendee = serde_json::from_str(json).unwrap();
        assert_eq!(attendee.id, "kZ86fPIEVVmgQbhVgb7auw");
        assert_eq!(attendee.name.as_deref(), Some("Zhirayr Gumruyan"));
        assert_eq!(
            attendee.provider_id,
            "ACoAAA8Br58BDseKnXYW51e4TU617k-ohrisrcs"
        );
    }
}
