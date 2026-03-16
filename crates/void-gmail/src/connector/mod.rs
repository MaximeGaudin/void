mod api_methods;
mod compose;
mod sync;

use std::sync::Arc;

use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

use crate::api::GmailApiClient;
use crate::auth;

pub use compose::{
    compose_rfc2822, compose_rfc2822_with_attachment, html_to_markdown, looks_like_html,
    parse_email_address, parse_email_name,
};

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

        let api = GmailApiClient::new(&cache.access_token);
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
        let raw = match &content {
            MessageContent::Text(t) => {
                let subject = "(no subject)";
                info!(recipient = %to, subject = %subject, "sending Gmail message");
                compose_rfc2822(to, subject, t)
            }
            MessageContent::File {
                path,
                caption,
                mime_type,
            } => {
                let subject = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("(attachment)");
                let body = caption.clone().unwrap_or_default();
                info!(recipient = %to, subject = %subject, "sending Gmail message with attachment");
                compose_rfc2822_with_attachment(
                    to,
                    subject,
                    &body,
                    path,
                    mime_type.as_deref(),
                    None,
                    None,
                )?
            }
        };

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

        let api = self.get_client().await?;

        let raw = match &content {
            MessageContent::Text(t) => format!(
                "In-Reply-To: {message_id}\r\nReferences: {message_id}\r\n\
                 Content-Type: text/plain; charset=utf-8\r\n\r\n{t}"
            ),
            MessageContent::File {
                path,
                caption,
                mime_type,
            } => {
                let orig = api.get_message(message_id).await?;
                let to = orig.get_header("From").unwrap_or_default();
                let subj = orig
                    .get_header("Subject")
                    .unwrap_or_else(|| "(no subject)".into());
                let subject = if subj.starts_with("Re:") {
                    subj
                } else {
                    format!("Re: {subj}")
                };
                let in_reply_to = orig.get_header("Message-ID");
                let references = in_reply_to.as_deref();
                let body = caption.clone().unwrap_or_default();
                compose_rfc2822_with_attachment(
                    &to,
                    &subject,
                    &body,
                    path,
                    mime_type.as_deref(),
                    in_reply_to.as_deref(),
                    references,
                )?
            }
        };

        let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());

        let resp = api.send_message(&encoded).await?;
        let reply_id = resp.id.clone().unwrap_or_default();
        debug!(reply_id = %reply_id, "Gmail reply sent");
        Ok(reply_id)
    }

    async fn forward(
        &self,
        external_id: &str,
        _conversation_external_id: &str,
        to: &str,
        comment: Option<&str>,
    ) -> anyhow::Result<String> {
        info!(message_id = %external_id, to = %to, "forwarding Gmail message");

        let api = self.get_client().await?;
        let orig = api.get_message(external_id).await?;

        let orig_from = orig.get_header("From").unwrap_or_else(|| "unknown".into());
        let orig_to = orig.get_header("To").unwrap_or_default();
        let orig_date = orig.get_header("Date").unwrap_or_default();
        let orig_subject = orig
            .get_header("Subject")
            .unwrap_or_else(|| "(no subject)".into());

        let subject = if orig_subject.starts_with("Fwd:") || orig_subject.starts_with("Fw:") {
            orig_subject.clone()
        } else {
            format!("Fwd: {orig_subject}")
        };

        let orig_body = orig.text_body().unwrap_or_default();

        let mut body = String::new();
        if let Some(c) = comment {
            body.push_str(c);
            body.push_str("\r\n\r\n");
        }
        body.push_str("---------- Forwarded message ---------\r\n");
        body.push_str(&format!("From: {orig_from}\r\n"));
        body.push_str(&format!("Date: {orig_date}\r\n"));
        body.push_str(&format!("Subject: {orig_subject}\r\n"));
        body.push_str(&format!("To: {orig_to}\r\n"));
        body.push_str("\r\n");
        body.push_str(&orig_body);

        let raw = compose_rfc2822(to, &subject, &body);
        let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());

        let resp = api.send_message(&encoded).await?;
        let fwd_id = resp.id.clone().unwrap_or_default();
        debug!(fwd_id = %fwd_id, "Gmail message forwarded");
        Ok(fwd_id)
    }
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
    fn compose_rfc2822_with_attachment_creates_multipart() {
        let dir = std::env::temp_dir();
        let name = format!("void_gmail_test_{}.txt", uuid::Uuid::new_v4());
        let path = dir.join(&name);
        std::fs::write(&path, "test content").unwrap();
        let result =
            compose_rfc2822_with_attachment("a@b.com", "Subj", "body", &path, None, None, None)
                .unwrap();
        std::fs::remove_file(&path).ok();
        assert!(result.contains("void_boundary_001"));
        assert!(result.contains("Content-Type: multipart/mixed"));
        assert!(result.contains("Content-Transfer-Encoding: base64"));
        assert!(result.contains("dGVzdCBjb250ZW50"));
        assert!(result.contains(&name));
        assert!(result.contains("To: a@b.com"));
        assert!(result.contains("Subject: Subj"));
        assert!(result.contains("Content-Disposition: attachment"));
    }

    #[test]
    fn compose_rfc2822_with_attachment_uses_provided_mime_type() {
        let dir = std::env::temp_dir();
        let name = format!("void_gmail_test_{}.pdf", uuid::Uuid::new_v4());
        let path = dir.join(&name);
        std::fs::write(&path, "PDF bytes").unwrap();
        let result = compose_rfc2822_with_attachment(
            "x@y.com",
            "Doc",
            "See attached",
            &path,
            Some("application/pdf"),
            None,
            None,
        )
        .unwrap();
        std::fs::remove_file(&path).ok();
        assert!(result.contains("Content-Type: application/pdf"));
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
