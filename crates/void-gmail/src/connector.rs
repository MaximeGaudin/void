use std::sync::Arc;

use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

use crate::api::{GmailApiClient, GmailMessage};
use crate::auth;

pub struct GmailConnector {
    config_id: String,
    credentials_file: Option<String>,
    store_path: std::path::PathBuf,
    my_email: std::sync::Mutex<Option<String>>,
}

impl GmailConnector {
    pub fn new(
        account_id: &str,
        credentials_file: Option<&str>,
        store_path: &std::path::Path,
    ) -> Self {
        Self {
            config_id: account_id.to_string(),
            credentials_file: credentials_file.map(|s| s.to_string()),
            store_path: store_path.to_path_buf(),
            my_email: std::sync::Mutex::new(None),
        }
    }

    fn token_path(&self) -> std::path::PathBuf {
        auth::token_cache_path(&self.store_path, &self.config_id)
    }

    fn display_account_id(&self) -> String {
        self.my_email
            .lock()
            .expect("mutex")
            .clone()
            .unwrap_or_else(|| self.config_id.clone())
    }

    async fn get_client(&self) -> anyhow::Result<GmailApiClient> {
        let token_path = self.token_path();
        let mut cache = auth::TokenCache::load(&token_path)?;

        let is_expired = cache
            .expires_at
            .map(|exp| chrono::Utc::now().timestamp() >= exp - 60)
            .unwrap_or(true);

        if is_expired {
            debug!(config_id = %self.config_id, "refreshing access token");
            if let Some(ref refresh_token) = cache.refresh_token {
                let creds = auth::load_client_credentials(self.credentials_file.as_deref())?;
                let http = reqwest::Client::new();
                cache = auth::refresh_access_token(&http, &creds, refresh_token).await?;
                cache.save(&token_path)?;
            } else {
                anyhow::bail!("token expired and no refresh token available. Run `void setup`");
            }
        } else {
            debug!(config_id = %self.config_id, "token fresh, reusing");
        }

        Ok(GmailApiClient::new(&cache.access_token))
    }

    async fn initial_sync(&self, db: &Database) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        info!(config_id = %self.config_id, "starting Gmail initial sync");

        let profile = api.get_profile().await?;
        if let Some(email) = &profile.email_address {
            *self.my_email.lock().expect("mutex") = Some(email.clone());
        }

        if let Some(history_id) = &profile.history_id {
            db.set_sync_state(&self.config_id, "history_id", history_id)?;
        }

        let mut page_token: Option<String> = None;
        let max_pages: u64 = 5;

        let mut progress = void_core::progress::BackfillProgress::new(
            &format!("gmail:{}", self.config_id),
            "messages",
        );
        progress.set_pages(max_pages);

        loop {
            let resp = api
                .list_messages(
                    100,
                    page_token.as_deref(),
                    Some(&["INBOX"]),
                    Some("newer_than:7d"),
                )
                .await?;
            progress.inc_page();

            if let Some(msgs) = resp.messages {
                for msg_ref in &msgs {
                    match api.get_message(&msg_ref.id).await {
                        Ok(msg) => {
                            self.store_message(db, &msg)?;
                            progress.inc(1);
                        }
                        Err(e) => {
                            warn!(message_id = %msg_ref.id, "failed to fetch message: {e}");
                        }
                    }
                }
            }

            page_token = resp.next_page_token;
            if page_token.is_none() || progress.pages_done >= max_pages {
                break;
            }
        }

        progress.finish();
        info!(config_id = %self.config_id, messages = progress.items, "Gmail initial sync complete");
        Ok(())
    }

    async fn incremental_sync(&self, db: &Database) -> anyhow::Result<()> {
        let Some(history_id) = db.get_sync_state(&self.config_id, "history_id")? else {
            debug!("no history_id, skipping incremental sync");
            return Ok(());
        };

        let api = self.get_client().await?;
        let resp = api.list_history(&history_id).await?;

        if let Some(records) = resp.history {
            for record in &records {
                if let Some(added) = &record.messages_added {
                    for item in added {
                        match api.get_message(&item.message.id).await {
                            Ok(msg) => {
                                let from = msg.get_header("From").unwrap_or_default();
                                let subject = msg
                                    .get_header("Subject")
                                    .unwrap_or_else(|| "(no subject)".into());
                                eprintln!(
                                    "[gmail:{}] new: {} — {}",
                                    self.display_account_id(),
                                    from,
                                    subject
                                );
                                self.store_message(db, &msg)?;
                            }
                            Err(e) => {
                                warn!(message_id = %item.message.id, "failed to fetch: {e}");
                            }
                        }
                    }
                }
            }
        }

        if let Some(new_id) = resp.history_id {
            db.set_sync_state(&self.config_id, "history_id", &new_id)?;
        }

        Ok(())
    }

    fn store_message(&self, db: &Database, msg: &GmailMessage) -> anyhow::Result<()> {
        let msg_id = msg.id.as_deref().unwrap_or("");
        let thread_id = msg.thread_id.as_deref().unwrap_or(msg_id);
        let from = msg.get_header("From").unwrap_or_default();
        let account_id = self.display_account_id();
        debug!(message_id = %msg_id, thread_id = %thread_id, from = %from, "storing message");

        let conv_id = format!("{}-{}", account_id, thread_id);
        let subject = msg
            .get_header("Subject")
            .unwrap_or_else(|| "(no subject)".into());

        let conversation = Conversation {
            id: conv_id.clone(),
            account_id: account_id.clone(),
            connector: "gmail".into(),
            external_id: thread_id.to_string(),
            name: Some(subject),
            kind: ConversationKind::Thread,
            last_message_at: msg
                .internal_date
                .as_deref()
                .and_then(|d| d.parse().ok())
                .map(|ms: i64| ms / 1000),
            unread_count: 0,
            is_muted: false,
            metadata: None,
        };
        db.upsert_conversation(&conversation)?;

        let sender_email = parse_email_address(&from);
        let sender_name = parse_email_name(&from);

        let text_body = msg.text_body();
        let html_body = msg.html_body();

        let body = match (text_body, &html_body) {
            (Some(text), _) if !looks_like_html(&text) => Some(text),
            (Some(text), _) => Some(html_to_markdown(&text)),
            (None, Some(html)) => Some(html_to_markdown(html)),
            (None, None) => msg.snippet.clone(),
        };

        let metadata = if html_body.is_some() {
            Some(serde_json::json!({ "has_html": true, "snippet": msg.snippet }))
        } else {
            None
        };

        let message = Message {
            id: format!("{}-{}", account_id, msg_id),
            conversation_id: conv_id,
            account_id: account_id.clone(),
            connector: "gmail".into(),
            external_id: msg_id.to_string(),
            sender: sender_email,
            sender_name: Some(sender_name),
            body,
            timestamp: msg
                .internal_date
                .as_deref()
                .and_then(|d| d.parse().ok())
                .map(|ms: i64| ms / 1000)
                .unwrap_or(0),
            synced_at: None,
            is_archived: !msg
                .label_ids
                .as_ref()
                .is_some_and(|labels| labels.iter().any(|l| l == "INBOX")),
            reply_to_id: msg
                .get_header("In-Reply-To")
                .map(|v| format!("{}-{v}", account_id)),
            media_type: None,
            metadata,
            context_id: Some(format!("{}-thread-{}", account_id, thread_id)),
            context: None,
        };
        db.upsert_message(&message)?;
        Ok(())
    }

    pub async fn search_api(
        &self,
        query: &str,
        max_results: u32,
    ) -> anyhow::Result<Vec<crate::api::GmailMessage>> {
        let api = self.get_client().await?;
        let resp = api
            .list_messages(max_results, None, None, Some(query))
            .await?;
        let mut messages = Vec::new();
        if let Some(refs) = resp.messages {
            for r in &refs {
                match api.get_message(&r.id).await {
                    Ok(msg) => messages.push(msg),
                    Err(e) => warn!(message_id = %r.id, "failed to fetch: {e}"),
                }
            }
        }
        Ok(messages)
    }

    pub async fn get_thread(&self, thread_id: &str) -> anyhow::Result<crate::api::GmailThread> {
        let api = self.get_client().await?;
        api.get_thread(thread_id).await
    }

    pub async fn get_attachment_data(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> anyhow::Result<Vec<u8>> {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;

        let api = self.get_client().await?;
        let resp = api.get_attachment(message_id, attachment_id).await?;
        let data = resp
            .data
            .ok_or_else(|| anyhow::anyhow!("attachment has no data"))?;
        URL_SAFE_NO_PAD
            .decode(&data)
            .map_err(|e| anyhow::anyhow!("failed to decode attachment: {e}"))
    }

    pub async fn list_labels(&self) -> anyhow::Result<Vec<crate::api::GmailLabel>> {
        let api = self.get_client().await?;
        let resp = api.list_labels().await?;
        Ok(resp.labels.unwrap_or_default())
    }

    pub async fn modify_thread_labels(
        &self,
        thread_id: &str,
        add: &[&str],
        remove: &[&str],
    ) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        api.modify_thread(thread_id, add, remove).await?;
        Ok(())
    }

    pub async fn batch_modify(
        &self,
        message_ids: &[&str],
        add: &[&str],
        remove: &[&str],
    ) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        api.batch_modify_messages(message_ids, add, remove).await
    }

    pub async fn list_drafts(
        &self,
        max_results: u32,
    ) -> anyhow::Result<Vec<crate::api::GmailDraft>> {
        let api = self.get_client().await?;
        let resp = api.list_drafts(max_results).await?;
        let mut drafts = Vec::new();
        if let Some(refs) = resp.drafts {
            for r in &refs {
                match api.get_draft(&r.id).await {
                    Ok(d) => drafts.push(d),
                    Err(e) => warn!(draft_id = %r.id, "failed to fetch draft: {e}"),
                }
            }
        }
        Ok(drafts)
    }

    pub async fn create_draft(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        reply_to_message_id: Option<&str>,
        thread_id: Option<&str>,
    ) -> anyhow::Result<crate::api::GmailDraft> {
        let api = self.get_client().await?;

        let mut headers = format!(
            "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=utf-8\r\n"
        );
        if let Some(ref_id) = reply_to_message_id {
            headers.push_str(&format!(
                "In-Reply-To: {ref_id}\r\nReferences: {ref_id}\r\n"
            ));
        }
        headers.push_str(&format!("\r\n{body}"));

        let encoded = URL_SAFE_NO_PAD.encode(headers.as_bytes());
        api.create_draft(&encoded, thread_id).await
    }

    pub async fn update_draft(
        &self,
        draft_id: &str,
        to: &str,
        subject: &str,
        body: &str,
    ) -> anyhow::Result<crate::api::GmailDraft> {
        let api = self.get_client().await?;

        let raw = format!(
            "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}"
        );
        let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());
        api.update_draft(draft_id, &encoded).await
    }

    pub async fn delete_draft(&self, draft_id: &str) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        api.delete_draft(draft_id).await
    }

    pub fn gmail_url(thread_id: &str) -> String {
        format!("https://mail.google.com/mail/u/0/#inbox/{thread_id}")
    }
}

#[async_trait]
impl Connector for GmailConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::Gmail
    }

    fn account_id(&self) -> &str {
        &self.config_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let creds = auth::load_client_credentials(self.credentials_file.as_deref())?;
        let token_path = self.token_path();

        let cache = auth::authorize_interactive(&creds, None).await?;
        cache.save(&token_path)?;

        let api = crate::api::GmailApiClient::new(&cache.access_token);
        let profile = api.get_profile().await?;
        info!(
            email = profile.email_address.as_deref().unwrap_or("?"),
            "Gmail authenticated"
        );
        eprintln!(
            "Authenticated as {}",
            profile.email_address.unwrap_or_else(|| "?".into())
        );
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        self.initial_sync(&db).await?;

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!(account_id = %self.config_id, "Gmail sync cancelled");
                    break;
                }
                _ = interval.tick() => {
                    if let Err(e) = self.incremental_sync(&db).await {
                        error!(account_id = %self.config_id, "incremental sync error: {e}");
                    }
                }
            }
        }
        Ok(())
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        match self.get_client().await {
            Ok(api) => match api.get_profile().await {
                Ok(profile) => Ok(HealthStatus {
                    account_id: self.config_id.clone(),
                    connector_type: ConnectorType::Gmail,
                    ok: true,
                    message: format!(
                        "Authenticated as {}",
                        profile.email_address.unwrap_or_else(|| "?".into())
                    ),
                    last_sync: None,
                    message_count: None,
                }),
                Err(e) => {
                    warn!(account_id = %self.config_id, error = %e, "Gmail health check API error");
                    Ok(HealthStatus {
                        account_id: self.config_id.clone(),
                        connector_type: ConnectorType::Gmail,
                        ok: false,
                        message: format!("API error: {e}"),
                        last_sync: None,
                        message_count: None,
                    })
                }
            },
            Err(e) => {
                warn!(account_id = %self.config_id, error = %e, "Gmail health check auth error");
                Ok(HealthStatus {
                    account_id: self.config_id.clone(),
                    connector_type: ConnectorType::Gmail,
                    ok: false,
                    message: format!("Auth error: {e}"),
                    last_sync: None,
                    message_count: None,
                })
            }
        }
    }

    async fn send_message(&self, to: &str, content: MessageContent) -> anyhow::Result<String> {
        let (subject, body) = match &content {
            MessageContent::Text(t) => ("(no subject)".to_string(), t.clone()),
            MessageContent::File { caption, .. } => (
                "(attachment)".to_string(),
                caption.clone().unwrap_or_default(),
            ),
        };

        info!(recipient = %to, subject = %subject, "sending Gmail message");

        let raw = compose_rfc2822(to, &subject, &body);
        let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());

        let api = self.get_client().await?;
        let resp = api.send_message(&encoded).await?;
        let message_id = resp.id.clone().unwrap_or_default();
        debug!(message_id = %message_id, "Gmail message sent");
        Ok(message_id)
    }

    async fn mark_read(
        &self,
        external_id: &str,
        _conversation_external_id: &str,
    ) -> anyhow::Result<()> {
        info!(message_id = %external_id, "marking Gmail message as read");
        let api = self.get_client().await?;
        api.modify_message(external_id, &[], &["UNREAD"]).await?;
        Ok(())
    }

    async fn archive(
        &self,
        external_id: &str,
        _conversation_external_id: &str,
    ) -> anyhow::Result<()> {
        info!(message_id = %external_id, "archiving Gmail message");
        let api = self.get_client().await?;
        api.modify_message(external_id, &[], &["INBOX"]).await?;
        Ok(())
    }

    async fn reply(
        &self,
        message_id: &str,
        content: MessageContent,
        in_thread: bool,
    ) -> anyhow::Result<String> {
        info!(message_id = %message_id, in_thread = in_thread, "sending Gmail reply");

        let body = match &content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::File { caption, .. } => caption.clone().unwrap_or_default(),
        };

        let raw = format!(
            "In-Reply-To: {message_id}\r\nReferences: {message_id}\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\n{body}"
        );
        let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());

        let api = self.get_client().await?;
        let resp = api.send_message(&encoded).await?;
        let reply_id = resp.id.clone().unwrap_or_default();
        debug!(reply_id = %reply_id, "Gmail reply sent");
        Ok(reply_id)
    }
}

fn compose_rfc2822(to: &str, subject: &str, body: &str) -> String {
    format!(
        "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}"
    )
}

fn parse_email_address(from: &str) -> String {
    if let Some(start) = from.find('<') {
        from[start + 1..].trim_end_matches('>').trim().to_string()
    } else {
        from.trim().to_string()
    }
}

fn parse_email_name(from: &str) -> String {
    if let Some(start) = from.find('<') {
        from[..start].trim().trim_matches('"').to_string()
    } else {
        from.to_string()
    }
}

fn looks_like_html(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("<!DOCTYPE")
        || trimmed.starts_with("<!doctype")
        || trimmed.starts_with("<html")
        || trimmed.starts_with("<HTML")
        || (trimmed.contains("<div") && trimmed.contains("</div>"))
        || (trimmed.contains("<table") && trimmed.contains("</table>"))
        || (trimmed.contains("<body") && trimmed.contains("</body>"))
}

fn html_to_markdown(html: &str) -> String {
    html_to_markdown_rs::convert(html, None).unwrap_or_else(|_| html.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::GmailApiClient;
    use void_core::db::Database;
    use void_core::models::{Conversation, ConversationKind, Message};
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn api_list_messages_paginates() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "messages": [
                    {"id": "m1", "threadId": "t1"},
                    {"id": "m2", "threadId": "t1"}
                ],
                "nextPageToken": "page2"
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages"))
            .and(query_param("pageToken", "page2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "messages": [
                    {"id": "m3", "threadId": "t2"}
                ]
            })))
            .mount(&server)
            .await;

        let api = GmailApiClient::with_base_url("test-token", &server.uri());

        let mut all_messages = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let resp = api
                .list_messages(100, page_token.as_deref(), Some(&["INBOX"]), None)
                .await
                .unwrap();
            if let Some(msgs) = resp.messages {
                all_messages.extend(msgs);
            }
            page_token = resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        assert_eq!(all_messages.len(), 3);
        assert_eq!(all_messages[0].id, "m1");
        assert_eq!(all_messages[1].id, "m2");
        assert_eq!(all_messages[2].id, "m3");
    }

    #[tokio::test]
    async fn initial_sync_saves_history_id() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/profile"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "emailAddress": "test@example.com",
                "historyId": "12345"
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "messages": [{"id": "m1", "threadId": "t1"}]
            })))
            .mount(&server)
            .await;

        let full_message = serde_json::json!({
            "id": "m1",
            "threadId": "t1",
            "snippet": "Hello",
            "internalDate": "1741700000000",
            "labelIds": ["INBOX"],
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    {"name": "From", "value": "sender@example.com"},
                    {"name": "Subject", "value": "Test Subject"},
                    {"name": "Date", "value": "Wed, 11 Mar 2026 10:00:00 +0000"}
                ],
                "body": {"data": "SGVsbG8gV29ybGQ", "size": 11}
            }
        });

        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages/m1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(full_message))
            .mount(&server)
            .await;

        let api = GmailApiClient::with_base_url("test-token", &server.uri());
        let db = Database::open_in_memory().unwrap();

        let config_id = "test-gmail";
        let profile = api.get_profile().await.unwrap();

        if let Some(history_id) = &profile.history_id {
            db.set_sync_state(config_id, "history_id", history_id)
                .unwrap();
        }

        let mut page_token: Option<String> = None;
        loop {
            let resp = api
                .list_messages(100, page_token.as_deref(), Some(&["INBOX"]), None)
                .await
                .unwrap();
            if let Some(msgs) = resp.messages {
                for msg_ref in &msgs {
                    let msg = api.get_message(&msg_ref.id).await.unwrap();
                    let msg_id = msg.id.as_deref().unwrap_or("");
                    let thread_id = msg.thread_id.as_deref().unwrap_or(msg_id);
                    let from = msg.get_header("From").unwrap_or_default();
                    let account_id = profile
                        .email_address
                        .as_deref()
                        .unwrap_or(config_id)
                        .to_string();
                    let conv_id = format!("{}-{}", account_id, thread_id);
                    let subject = msg
                        .get_header("Subject")
                        .unwrap_or_else(|| "(no subject)".into());

                    let conversation = Conversation {
                        id: conv_id.clone(),
                        account_id: account_id.clone(),
                        connector: "gmail".into(),
                        external_id: thread_id.to_string(),
                        name: Some(subject),
                        kind: ConversationKind::Thread,
                        last_message_at: msg
                            .internal_date
                            .as_deref()
                            .and_then(|d| d.parse().ok())
                            .map(|ms: i64| ms / 1000),
                        unread_count: 0,
                        is_muted: false,
                        metadata: None,
                    };
                    db.upsert_conversation(&conversation).unwrap();

                    let message = Message {
                        id: format!("{}-{}", account_id, msg_id),
                        conversation_id: conv_id,
                        account_id: account_id.clone(),
                        connector: "gmail".into(),
                        external_id: msg_id.to_string(),
                        sender: from
                            .find('<')
                            .map(|i| from[i + 1..].trim_end_matches('>').trim().to_string())
                            .unwrap_or_else(|| from.clone()),
                        sender_name: None,
                        body: msg.text_body().or(msg.snippet.clone()),
                        timestamp: msg
                            .internal_date
                            .as_deref()
                            .and_then(|d| d.parse().ok())
                            .map(|ms: i64| ms / 1000)
                            .unwrap_or(0),
                        synced_at: None,
                        is_archived: false,
                        reply_to_id: None,
                        media_type: None,
                        metadata: None,
                        context_id: Some(format!("{}-thread-{}", account_id, thread_id)),
                        context: None,
                    };
                    db.upsert_message(&message).unwrap();
                }
            }
            page_token = resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        let history_id = db.get_sync_state(config_id, "history_id").unwrap();
        assert_eq!(history_id, Some("12345".to_string()));

        let msg = db
            .get_message("test@example.com-m1")
            .unwrap()
            .expect("message should be stored");
        assert_eq!(msg.external_id, "m1");
        assert_eq!(msg.body.as_deref(), Some("Hello World"));
    }

    #[tokio::test]
    async fn incremental_sync_uses_history_id() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/history"))
            .and(query_param("startHistoryId", "12345"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "history": [{
                    "messagesAdded": [{
                        "message": {"id": "m2", "threadId": "t2"}
                    }]
                }],
                "historyId": "12346"
            })))
            .mount(&server)
            .await;

        let full_message = serde_json::json!({
            "id": "m2",
            "threadId": "t2",
            "snippet": "New message",
            "internalDate": "1741700001000",
            "labelIds": ["INBOX"],
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    {"name": "From", "value": "other@example.com"},
                    {"name": "Subject", "value": "Re: Test"},
                    {"name": "Date", "value": "Wed, 11 Mar 2026 10:01:00 +0000"}
                ],
                "body": {"data": "TmV3IG1lc3NhZ2U=", "size": 11}
            }
        });

        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages/m2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(full_message))
            .mount(&server)
            .await;

        let api = GmailApiClient::with_base_url("test-token", &server.uri());
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("test-gmail", "history_id", "12345")
            .unwrap();

        let config_id = "test-gmail";
        let history_id = db.get_sync_state(config_id, "history_id").unwrap();
        let history_id = history_id.expect("history_id should be set");

        let resp = api.list_history(&history_id).await.unwrap();

        if let Some(records) = resp.history {
            for record in &records {
                if let Some(added) = &record.messages_added {
                    for item in added {
                        let msg = api.get_message(&item.message.id).await.unwrap();
                        let msg_id = msg.id.as_deref().unwrap_or("");
                        let thread_id = msg.thread_id.as_deref().unwrap_or(msg_id);
                        let account_id = "test-gmail".to_string();
                        let conv_id = format!("{}-{}", account_id, thread_id);

                        let conversation = Conversation {
                            id: conv_id.clone(),
                            account_id: account_id.clone(),
                            connector: "gmail".into(),
                            external_id: thread_id.to_string(),
                            name: Some(
                                msg.get_header("Subject")
                                    .unwrap_or_else(|| "(no subject)".into()),
                            ),
                            kind: ConversationKind::Thread,
                            last_message_at: msg
                                .internal_date
                                .as_deref()
                                .and_then(|d| d.parse().ok())
                                .map(|ms: i64| ms / 1000),
                            unread_count: 0,
                            is_muted: false,
                            metadata: None,
                        };
                        db.upsert_conversation(&conversation).unwrap();

                        let from = msg.get_header("From").unwrap_or_default();
                        let message = Message {
                            id: format!("{}-{}", account_id, msg_id),
                            conversation_id: conv_id.clone(),
                            account_id: account_id.clone(),
                            connector: "gmail".into(),
                            external_id: msg_id.to_string(),
                            sender: from
                                .find('<')
                                .map(|i| from[i + 1..].trim_end_matches('>').trim().to_string())
                                .unwrap_or_else(|| from.clone()),
                            sender_name: None,
                            body: msg.text_body().or(msg.snippet.clone()),
                            timestamp: msg
                                .internal_date
                                .as_deref()
                                .and_then(|d| d.parse().ok())
                                .map(|ms: i64| ms / 1000)
                                .unwrap_or(0),
                            synced_at: None,
                            is_archived: false,
                            reply_to_id: None,
                            media_type: None,
                            metadata: None,
                            context_id: Some(format!("{}-thread-{}", account_id, thread_id)),
                            context: None,
                        };
                        db.upsert_message(&message).unwrap();
                    }
                }
            }
        }

        if let Some(new_id) = resp.history_id {
            db.set_sync_state(config_id, "history_id", &new_id).unwrap();
        }

        let updated = db.get_sync_state(config_id, "history_id").unwrap();
        assert_eq!(updated, Some("12346".to_string()));

        let msg = db
            .get_message("test-gmail-m2")
            .unwrap()
            .expect("message should be stored");
        assert_eq!(msg.external_id, "m2");
        assert_eq!(msg.body.as_deref(), Some("New message"));
    }

    #[tokio::test]
    async fn initial_sync_respects_max_pages() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "messages": [{"id": "m1", "threadId": "t1"}],
                "nextPageToken": "next"
            })))
            .expect(5)
            .named("list_messages pages")
            .mount(&server)
            .await;

        let api = GmailApiClient::with_base_url("test-token", &server.uri());

        let max_pages: u64 = 5;
        let mut page_token: Option<String> = None;
        let mut page_count = 0u64;

        while page_count < max_pages {
            let resp = api
                .list_messages(100, page_token.as_deref(), Some(&["INBOX"]), None)
                .await
                .unwrap();
            page_count += 1;
            page_token = resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        assert_eq!(page_count, 5);
        drop(server);
    }

    #[test]
    fn parse_email_address_extracts_email() {
        assert_eq!(
            parse_email_address("Alice <alice@example.com>"),
            "alice@example.com"
        );
        assert_eq!(
            parse_email_address("\"Bob Smith\" <bob@example.com>"),
            "bob@example.com"
        );
        assert_eq!(
            parse_email_address("charlie@example.com"),
            "charlie@example.com"
        );
    }

    #[test]
    fn parse_email_name_with_brackets() {
        assert_eq!(parse_email_name("Alice <alice@example.com>"), "Alice");
        assert_eq!(
            parse_email_name("\"Bob Smith\" <bob@example.com>"),
            "Bob Smith"
        );
        assert_eq!(
            parse_email_name("charlie@example.com"),
            "charlie@example.com"
        );
    }

    #[test]
    fn compose_rfc2822_basic() {
        let raw = compose_rfc2822("alice@example.com", "Test Subject", "Hello, Alice!");
        assert!(raw.contains("To: alice@example.com"));
        assert!(raw.contains("Subject: Test Subject"));
        assert!(raw.contains("Hello, Alice!"));
    }

    #[test]
    fn html_to_markdown_strips_tags() {
        let md = html_to_markdown("<p>Hello <b>world</b></p>");
        assert!(md.contains("Hello"));
        assert!(md.contains("**world**"));
        assert!(!md.contains("<p>"));
        assert!(!md.contains("<b>"));
    }

    #[test]
    fn html_to_markdown_preserves_links() {
        let md = html_to_markdown(r#"<p>Click <a href="https://example.com">here</a></p>"#);
        assert!(md.contains("[here](https://example.com)"));
    }

    #[test]
    fn html_to_markdown_preserves_headings() {
        let md = html_to_markdown("<h1>Title</h1><h2>Subtitle</h2>");
        assert!(md.contains("# Title"));
        assert!(md.contains("## Subtitle"));
    }

    #[test]
    fn html_to_markdown_handles_lists() {
        let md = html_to_markdown("<ul><li>Item 1</li><li>Item 2</li></ul>");
        assert!(md.contains("Item 1"));
        assert!(md.contains("Item 2"));
    }

    #[test]
    fn html_to_markdown_real_email() {
        let html = r#"
        <html>
        <body>
            <div style="font-family:Arial">
                <p>Hi Maxime,</p>
                <p>Your order <b>#12345</b> has been shipped.</p>
                <p>Track it <a href="https://track.example.com/12345">here</a>.</p>
                <br>
                <p>Thanks,<br>The Team</p>
            </div>
        </body>
        </html>
        "#;
        let md = html_to_markdown(html);
        assert!(md.contains("Hi Maxime"));
        assert!(md.contains("**#12345**"));
        assert!(md.contains("has been shipped"));
        assert!(md.contains("[here](https://track.example.com/12345)"));
        assert!(md.contains("Thanks"));
        assert!(!md.contains("<div"));
        assert!(!md.contains("font-family"));
    }

    #[test]
    fn html_to_markdown_empty() {
        let md = html_to_markdown("");
        assert!(md.trim().is_empty());
    }

    #[test]
    fn looks_like_html_detects_doctype() {
        assert!(looks_like_html(
            "<!DOCTYPE html><html><body>Hi</body></html>"
        ));
        assert!(looks_like_html("  <!DOCTYPE html>\n<html>"));
    }

    #[test]
    fn looks_like_html_detects_html_tag() {
        assert!(looks_like_html("<html><body>Hello</body></html>"));
        assert!(looks_like_html("<HTML><BODY>Hello</BODY></HTML>"));
    }

    #[test]
    fn looks_like_html_detects_div_table_body() {
        assert!(looks_like_html("<div class=\"wrapper\">Content</div>"));
        assert!(looks_like_html("<table><tr><td>cell</td></tr></table>"));
        assert!(looks_like_html("<body>hello</body>"));
    }

    #[test]
    fn looks_like_html_plain_text_is_false() {
        assert!(!looks_like_html("Hello, this is a plain text email."));
        assert!(!looks_like_html("Hi Maxime,\n\nSee you tomorrow.\nAlice"));
        assert!(!looks_like_html(""));
    }

    #[test]
    fn gmail_url_formats_correctly() {
        let url = GmailConnector::gmail_url("thread123");
        assert_eq!(url, "https://mail.google.com/mail/u/0/#inbox/thread123");
    }
}
