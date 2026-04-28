use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Wall-clock threshold to detect hibernation gaps (same rationale as Slack:
/// `SystemTime` survives macOS sleep where the monotonic clock pauses).
const IDLE_THRESHOLD: Duration = Duration::from_secs(3 * 60);

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

use crate::api::GmailApiClient;
use crate::auth;

use super::compose::{compose_rfc2822, compose_rfc2822_with_attachment};
use super::GmailConnector;

#[async_trait]
impl Connector for GmailConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::Gmail
    }

    fn connection_id(&self) -> &str {
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

        let mut interval = tokio::time::interval(Duration::from_secs(30));
        let mut last_sync = SystemTime::now();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!(connection_id = %self.config_id, "Gmail sync cancelled");
                    break;
                }
                _ = interval.tick() => {
                    let elapsed = last_sync.elapsed().unwrap_or_default();
                    if elapsed > IDLE_THRESHOLD {
                        warn!(
                            connection_id = %self.config_id,
                            idle_secs = elapsed.as_secs(),
                            "Gmail sync was idle, refreshing inbox"
                        );
                        void_core::status!(
                            "[gmail:{}] sync idle for {}s, refreshing inbox",
                            self.config_id,
                            elapsed.as_secs(),
                        );
                        if let Err(e) = self.refresh_inbox(&db).await {
                            error!(connection_id = %self.config_id, "inbox refresh after idle failed: {e}");
                        }
                    }
                    if let Err(e) = self.incremental_sync(&db).await {
                        error!(connection_id = %self.config_id, "incremental sync error: {e}");
                    }
                    last_sync = SystemTime::now();
                }
            }
        }
        Ok(())
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        match self.get_client().await {
            Ok(api) => match api.get_profile().await {
                Ok(profile) => Ok(HealthStatus {
                    connection_id: self.config_id.clone(),
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
                    warn!(connection_id = %self.config_id, error = %e, "Gmail health check API error");
                    Ok(HealthStatus {
                        connection_id: self.config_id.clone(),
                        connector_type: ConnectorType::Gmail,
                        ok: false,
                        message: format!("API error: {e}"),
                        last_sync: None,
                        message_count: None,
                    })
                }
            },
            Err(e) => {
                warn!(connection_id = %self.config_id, error = %e, "Gmail health check auth error");
                Ok(HealthStatus {
                    connection_id: self.config_id.clone(),
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
                compose_rfc2822(to, subject, t, None, None)
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

        let raw = match &content {
            MessageContent::Text(t) => {
                compose_rfc2822(&to, &subject, t, in_reply_to.as_deref(), references)
            }
            MessageContent::File {
                path,
                caption,
                mime_type,
            } => {
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

        let raw = compose_rfc2822(to, &subject, &body, None, None);
        let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());

        let resp = api.send_message(&encoded).await?;
        let fwd_id = resp.id.clone().unwrap_or_default();
        debug!(fwd_id = %fwd_id, "Gmail message forwarded");
        Ok(fwd_id)
    }
}
