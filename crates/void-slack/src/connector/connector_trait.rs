use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use tracing::{debug, info, warn};

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

use super::SlackConnector;

#[async_trait]
impl Connector for SlackConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::Slack
    }

    fn connection_id(&self) -> &str {
        &self.connection_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let resp = self.api.auth_test().await?;
        info!(
            user = resp.user.as_deref().unwrap_or("?"),
            team = resp.team.as_deref().unwrap_or("?"),
            "Slack authenticated"
        );
        Ok(())
    }

    async fn start_sync(
        &self,
        db: Arc<Database>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<()> {
        if let Some(app_id) = &self.app_id {
            let token_path = self.config_token_path();
            if token_path.exists() {
                if let Err(e) = crate::manifest::ensure_event_subscriptions(
                    &token_path,
                    app_id,
                    &self.connection_id,
                )
                .await
                {
                    warn!(
                        connection_id = %self.connection_id,
                        error = %e,
                        "event subscription check failed (non-fatal, continuing sync)"
                    );
                    eprintln!(
                        "[slack:{}] Warning: event subscription check failed: {e}",
                        self.connection_id
                    );
                }
            } else {
                debug!(
                    connection_id = %self.connection_id,
                    "no config token file found, skipping event subscription check"
                );
            }
        }

        let needs_backfill = db
            .get_sync_state(&self.connection_id, "backfill_done")?
            .is_none();

        // Start Socket Mode immediately alongside backfill/catch-up so that
        // real-time messages arriving during backfill are not lost.
        let backfill_task = async {
            if needs_backfill {
                match self.backfill(&db).await {
                    Ok(()) => {
                        db.set_sync_state(&self.connection_id, "backfill_done", "1")
                            .ok();
                    }
                    Err(e) => {
                        warn!(connection_id = %self.connection_id, error = %e, "Slack backfill failed")
                    }
                }
            } else {
                info!(
                    connection_id = %self.connection_id,
                    "Slack backfill already complete, catching up missed messages"
                );
                if let Err(e) = self.catch_up(&db).await {
                    warn!(connection_id = %self.connection_id, error = %e, "Slack catch-up failed");
                }
            }
        };

        let (_, socket_result) = tokio::join!(backfill_task, self.run_socket_mode(&db, &cancel));
        socket_result
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        let auth_resp = match self.api.auth_test().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!(connection_id = %self.connection_id, error = %e, "Slack health check failed");
                return Ok(HealthStatus {
                    connection_id: self.connection_id.clone(),
                    connector_type: ConnectorType::Slack,
                    ok: false,
                    message: format!("Auth failed: {e}"),
                    last_sync: None,
                    message_count: None,
                });
            }
        };

        if let Some(app_id) = &self.app_id {
            let token_path = self.config_token_path();
            if token_path.exists() {
                if let Err(e) = crate::manifest::ensure_event_subscriptions(
                    &token_path,
                    app_id,
                    &self.connection_id,
                )
                .await
                {
                    warn!(
                        connection_id = %self.connection_id,
                        error = %e,
                        "Slack health check: event subscription check failed"
                    );
                    return Ok(HealthStatus {
                        connection_id: self.connection_id.clone(),
                        connector_type: ConnectorType::Slack,
                        ok: false,
                        message: format!(
                            "Auth OK, but event subscription repair token is invalid: {e}"
                        ),
                        last_sync: None,
                        message_count: None,
                    });
                }
            } else {
                return Ok(HealthStatus {
                    connection_id: self.connection_id.clone(),
                    connector_type: ConnectorType::Slack,
                    ok: false,
                    message: "Auth OK, but missing config token file (run void setup again to restore auto-repair)".to_string(),
                    last_sync: None,
                    message_count: None,
                });
            }
        }

        Ok(HealthStatus {
            connection_id: self.connection_id.clone(),
            connector_type: ConnectorType::Slack,
            ok: true,
            message: format!(
                "Authenticated as {} in {}",
                auth_resp.user.as_deref().unwrap_or("?"),
                auth_resp.team.as_deref().unwrap_or("?")
            ),
            last_sync: None,
            message_count: None,
        })
    }

    async fn mark_read(
        &self,
        external_id: &str,
        conversation_external_id: &str,
    ) -> anyhow::Result<()> {
        info!(connection_id = %self.connection_id, ts = %external_id, channel = %conversation_external_id, "marking Slack message as read");
        self.api
            .conversations_mark(conversation_external_id, external_id)
            .await?;
        Ok(())
    }

    async fn send_message(&self, to: &str, content: MessageContent) -> anyhow::Result<String> {
        match &content {
            MessageContent::File { path, caption, .. } => {
                let path_str = path.to_str().context("file path is not valid UTF-8")?;
                let channel = self.resolve_channel_for_file(to).await?;
                self.upload_file(&channel, path_str, caption.as_deref(), None)
                    .await
            }
            MessageContent::Text(t) => {
                let channel = if to.contains(',') {
                    let users: Vec<&str> = to.split(',').map(|s| s.trim()).collect();
                    let channel_id = self.open_conversation(&users).await?;
                    info!(users = ?users, channel_id = %channel_id, "opened group conversation");
                    channel_id
                } else {
                    to.to_string()
                };
                let resp = self.api.chat_post_message(&channel, t, None).await?;
                Ok(resp.ts.unwrap_or_default())
            }
        }
    }

    async fn reply(
        &self,
        message_id: &str,
        content: MessageContent,
        in_thread: bool,
    ) -> anyhow::Result<String> {
        info!(connection_id = %self.connection_id, message_id = %message_id, in_thread, "sending Slack reply");

        let parts: Vec<&str> = message_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            anyhow::bail!("invalid message_id format, expected 'channel_id:ts'");
        }
        let (channel_id, ts) = (parts[0], parts[1]);

        match &content {
            MessageContent::File { path, caption, .. } => {
                let path_str = path.to_str().context("file path is not valid UTF-8")?;
                let thread_ts = if in_thread { Some(ts) } else { None };
                self.upload_file(channel_id, path_str, caption.as_deref(), thread_ts)
                    .await
            }
            MessageContent::Text(t) => {
                let thread_ts = if in_thread { Some(ts) } else { None };
                let resp = self.api.chat_post_message(channel_id, t, thread_ts).await?;
                let reply_ts = resp.ts.clone().unwrap_or_default();
                debug!(connection_id = %self.connection_id, ts = %reply_ts, "Slack reply sent");
                Ok(reply_ts)
            }
        }
    }

    async fn forward(
        &self,
        external_id: &str,
        conversation_external_id: &str,
        to: &str,
        comment: Option<&str>,
    ) -> anyhow::Result<String> {
        info!(
            connection_id = %self.connection_id,
            message_ts = %external_id,
            channel = %conversation_external_id,
            to = %to,
            "forwarding Slack message"
        );

        let orig = self
            .api
            .get_single_message(conversation_external_id, external_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Original message not found (ts={external_id})"))?;

        let sender_name = if let Some(ref user_id) = orig.user {
            self.api
                .users_info(user_id)
                .await
                .ok()
                .and_then(|r| r.user)
                .map(|u| u.real_name.unwrap_or(u.name))
                .unwrap_or_else(|| user_id.clone())
        } else {
            "someone".into()
        };

        let orig_text = orig.text.as_deref().unwrap_or("");

        let mut forwarded = String::new();
        if let Some(c) = comment {
            forwarded.push_str(c);
            forwarded.push_str("\n\n");
        }
        forwarded.push_str(&format!("_Forwarded from {sender_name}:_\n"));
        for line in orig_text.lines() {
            forwarded.push_str(&format!("> {line}\n"));
        }

        let target = if to.contains(',') {
            let users: Vec<&str> = to.split(',').map(|s| s.trim()).collect();
            self.open_conversation(&users).await?
        } else {
            to.to_string()
        };

        let resp = self
            .api
            .chat_post_message(&target, &forwarded, None)
            .await?;
        let ts = resp.ts.unwrap_or_default();
        debug!(connection_id = %self.connection_id, ts = %ts, "Slack message forwarded");
        Ok(ts)
    }
}
