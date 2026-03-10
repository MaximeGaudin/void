use std::sync::Arc;

use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use void_core::channel::Channel;
use void_core::db::Database;
use void_core::models::*;

use crate::api::{GmailApiClient, GmailMessage};
use crate::auth;

pub struct GmailChannel {
    account_id: String,
    credentials_file: String,
    store_path: std::path::PathBuf,
    my_email: std::sync::Mutex<Option<String>>,
}

impl GmailChannel {
    pub fn new(account_id: &str, credentials_file: &str, store_path: &std::path::Path) -> Self {
        Self {
            account_id: account_id.to_string(),
            credentials_file: credentials_file.to_string(),
            store_path: store_path.to_path_buf(),
            my_email: std::sync::Mutex::new(None),
        }
    }

    fn token_path(&self) -> std::path::PathBuf {
        auth::token_cache_path(&self.store_path, &self.account_id)
    }

    async fn get_client(&self) -> anyhow::Result<GmailApiClient> {
        let token_path = self.token_path();
        let mut cache = auth::TokenCache::load(&token_path)?;

        let is_expired = cache
            .expires_at
            .map(|exp| chrono::Utc::now().timestamp() >= exp - 60)
            .unwrap_or(true);

        if is_expired {
            if let Some(ref refresh_token) = cache.refresh_token {
                let creds = auth::load_client_credentials(&self.credentials_file)?;
                let http = reqwest::Client::new();
                cache = auth::refresh_access_token(&http, &creds, refresh_token).await?;
                cache.save(&token_path)?;
            } else {
                anyhow::bail!(
                    "token expired and no refresh token available. Run `void auth gmail {}`",
                    self.account_id
                );
            }
        }

        Ok(GmailApiClient::new(&cache.access_token))
    }

    async fn initial_sync(&self, db: &Database) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        info!(account_id = %self.account_id, "starting Gmail initial sync");

        let profile = api.get_profile().await?;
        if let Some(email) = &profile.email_address {
            *self.my_email.lock().expect("mutex") = Some(email.clone());
        }

        if let Some(history_id) = &profile.history_id {
            db.set_sync_state(&self.account_id, "history_id", history_id)?;
        }

        let mut page_token: Option<String> = None;
        let mut total = 0u32;
        let max_pages = 5;
        let mut pages = 0;

        loop {
            let resp = api.list_messages(100, page_token.as_deref()).await?;
            if let Some(msgs) = resp.messages {
                for msg_ref in &msgs {
                    match api.get_message(&msg_ref.id).await {
                        Ok(msg) => {
                            self.store_message(db, &msg)?;
                            total += 1;
                        }
                        Err(e) => {
                            warn!(message_id = %msg_ref.id, "failed to fetch message: {e}");
                        }
                    }
                }
            }

            pages += 1;
            page_token = resp.next_page_token;
            if page_token.is_none() || pages >= max_pages {
                break;
            }
        }

        info!(account_id = %self.account_id, messages = total, "Gmail initial sync complete");
        Ok(())
    }

    async fn incremental_sync(&self, db: &Database) -> anyhow::Result<()> {
        let Some(history_id) = db.get_sync_state(&self.account_id, "history_id")? else {
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
            db.set_sync_state(&self.account_id, "history_id", &new_id)?;
        }

        Ok(())
    }

    fn store_message(&self, db: &Database, msg: &GmailMessage) -> anyhow::Result<()> {
        let msg_id = msg.id.as_deref().unwrap_or("");
        let thread_id = msg.thread_id.as_deref().unwrap_or(msg_id);

        let conv_id = format!("{}-{}", self.account_id, thread_id);
        let subject = msg
            .get_header("Subject")
            .unwrap_or_else(|| "(no subject)".into());
        let from = msg.get_header("From").unwrap_or_default();

        let conversation = Conversation {
            id: conv_id.clone(),
            account_id: self.account_id.clone(),
            external_id: thread_id.to_string(),
            name: Some(subject),
            kind: ConversationKind::Thread,
            last_message_at: msg
                .internal_date
                .as_deref()
                .and_then(|d| d.parse().ok())
                .map(|ms: i64| ms / 1000),
            unread_count: 0,
            metadata: None,
        };
        db.upsert_conversation(&conversation)?;

        let my_email = self.my_email.lock().expect("mutex").clone();
        let is_from_me = my_email.as_ref().is_some_and(|e| from.contains(e));
        let sender_name = parse_email_name(&from);

        let message = Message {
            id: format!("{}-{}", self.account_id, msg_id),
            conversation_id: conv_id,
            account_id: self.account_id.clone(),
            external_id: msg_id.to_string(),
            sender: from.clone(),
            sender_name: Some(sender_name),
            body: msg.snippet.clone(),
            timestamp: msg
                .internal_date
                .as_deref()
                .and_then(|d| d.parse().ok())
                .map(|ms: i64| ms / 1000)
                .unwrap_or(0),
            is_from_me,
            reply_to_id: msg
                .get_header("In-Reply-To")
                .map(|v| format!("{}-{v}", self.account_id)),
            media_type: None,
            metadata: None,
        };
        db.upsert_message(&message)?;
        Ok(())
    }
}

#[async_trait]
impl Channel for GmailChannel {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Gmail
    }

    fn account_id(&self) -> &str {
        &self.account_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        let profile = api.get_profile().await?;
        info!(
            email = profile.email_address.as_deref().unwrap_or("?"),
            "Gmail authenticated"
        );
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        self.initial_sync(&db).await?;

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!(account_id = %self.account_id, "Gmail sync cancelled");
                    break;
                }
                _ = interval.tick() => {
                    if let Err(e) = self.incremental_sync(&db).await {
                        error!(account_id = %self.account_id, "incremental sync error: {e}");
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
                    account_id: self.account_id.clone(),
                    channel_type: ChannelType::Gmail,
                    ok: true,
                    message: format!(
                        "Authenticated as {}",
                        profile.email_address.unwrap_or_else(|| "?".into())
                    ),
                    last_sync: None,
                    message_count: None,
                }),
                Err(e) => Ok(HealthStatus {
                    account_id: self.account_id.clone(),
                    channel_type: ChannelType::Gmail,
                    ok: false,
                    message: format!("API error: {e}"),
                    last_sync: None,
                    message_count: None,
                }),
            },
            Err(e) => Ok(HealthStatus {
                account_id: self.account_id.clone(),
                channel_type: ChannelType::Gmail,
                ok: false,
                message: format!("Auth error: {e}"),
                last_sync: None,
                message_count: None,
            }),
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

        let raw = compose_rfc2822(to, &subject, &body);
        let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());

        let api = self.get_client().await?;
        let resp = api.send_message(&encoded).await?;
        Ok(resp.id.unwrap_or_default())
    }

    async fn reply(
        &self,
        message_id: &str,
        content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
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
        Ok(resp.id.unwrap_or_default())
    }
}

fn compose_rfc2822(to: &str, subject: &str, body: &str) -> String {
    format!(
        "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}"
    )
}

fn parse_email_name(from: &str) -> String {
    if let Some(start) = from.find('<') {
        from[..start].trim().trim_matches('"').to_string()
    } else {
        from.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
