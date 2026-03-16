//! Slack connector: struct, Connector trait impl, action methods.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use tracing::{debug, info, warn};

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

use crate::api::SlackApiClient;

mod mapping;
mod socket_mode;
mod sync;

#[allow(unused_imports)] // used by tests
pub(crate) use mapping::{build_metadata, map_conversation, parse_ts};

pub struct SlackConnector {
    pub(crate) account_id: String,
    pub(crate) api: SlackApiClient,
    pub(crate) app_token: String,
    pub(crate) exclude_channels: Vec<String>,
}

impl SlackConnector {
    pub fn new(
        account_id: &str,
        user_token: &str,
        app_token: &str,
        exclude_channels: Vec<String>,
    ) -> Self {
        Self {
            account_id: account_id.to_string(),
            api: SlackApiClient::new(user_token),
            app_token: app_token.to_string(),
            exclude_channels,
        }
    }

    pub async fn react(&self, channel: &str, ts: &str, emoji: &str) -> anyhow::Result<()> {
        self.api.reactions_add(channel, ts, emoji).await
    }

    pub async fn edit_message(
        &self,
        channel: &str,
        ts: &str,
        text: &str,
    ) -> anyhow::Result<String> {
        let resp = self.api.chat_update(channel, ts, text).await?;
        Ok(resp.ts.unwrap_or_default())
    }

    pub async fn schedule_message(
        &self,
        channel: &str,
        text: &str,
        post_at: i64,
        thread_ts: Option<&str>,
    ) -> anyhow::Result<String> {
        let resp = self
            .api
            .chat_schedule_message(channel, text, post_at, thread_ts)
            .await?;
        Ok(resp.scheduled_message_id.unwrap_or_default())
    }

    pub async fn open_conversation(&self, users: &[&str]) -> anyhow::Result<String> {
        let resp = self.api.conversations_open(users).await?;
        Ok(resp.channel.id)
    }

    /// Resolve a target to a proper channel ID for file uploads.
    /// `files.completeUploadExternal` requires a channel/DM ID, not a user ID.
    async fn resolve_channel_for_file(&self, to: &str) -> anyhow::Result<String> {
        if to.contains(',') {
            let users: Vec<&str> = to.split(',').map(|s| s.trim()).collect();
            self.open_conversation(&users).await
        } else if to.starts_with('U') || to.starts_with('W') {
            self.open_conversation(&[to]).await
        } else {
            Ok(to.to_string())
        }
    }

    pub async fn upload_file(
        &self,
        channel: &str,
        file_path: &str,
        caption: Option<&str>,
        thread_ts: Option<&str>,
    ) -> anyhow::Result<String> {
        let data = std::fs::read(file_path)
            .with_context(|| format!("failed to read file {}", file_path))?;
        let filename = Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let upload_info = self
            .api
            .files_get_upload_url_external(filename, data.len() as u64)
            .await
            .context("files.getUploadURLExternal failed")?;
        self.api
            .post_file_to_url(&upload_info.upload_url, data, filename)
            .await
            .context("file upload to URL failed")?;
        self.api
            .files_complete_upload_external(
                &upload_info.file_id,
                filename,
                Some(channel),
                caption,
                thread_ts,
            )
            .await
            .context("files.completeUploadExternal failed")?;
        Ok(upload_info.file_id)
    }
}

#[async_trait]
impl Connector for SlackConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::Slack
    }

    fn account_id(&self) -> &str {
        &self.account_id
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
        let needs_backfill = db
            .get_sync_state(&self.account_id, "backfill_done")?
            .is_none();
        if needs_backfill {
            self.backfill(&db).await?;
            db.set_sync_state(&self.account_id, "backfill_done", "1")?;
        } else {
            info!(
                account_id = %self.account_id,
                "Slack backfill already complete, catching up missed messages"
            );
            self.catch_up(&db).await?;
        }

        self.run_socket_mode(&db, &cancel).await
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        match self.api.auth_test().await {
            Ok(resp) => Ok(HealthStatus {
                account_id: self.account_id.clone(),
                connector_type: ConnectorType::Slack,
                ok: true,
                message: format!(
                    "Authenticated as {} in {}",
                    resp.user.as_deref().unwrap_or("?"),
                    resp.team.as_deref().unwrap_or("?")
                ),
                last_sync: None,
                message_count: None,
            }),
            Err(e) => {
                warn!(account_id = %self.account_id, error = %e, "Slack health check failed");
                Ok(HealthStatus {
                    account_id: self.account_id.clone(),
                    connector_type: ConnectorType::Slack,
                    ok: false,
                    message: format!("Auth failed: {e}"),
                    last_sync: None,
                    message_count: None,
                })
            }
        }
    }

    async fn mark_read(
        &self,
        external_id: &str,
        conversation_external_id: &str,
    ) -> anyhow::Result<()> {
        info!(account_id = %self.account_id, ts = %external_id, channel = %conversation_external_id, "marking Slack message as read");
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
        info!(account_id = %self.account_id, message_id = %message_id, in_thread, "sending Slack reply");

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
                debug!(account_id = %self.account_id, ts = %reply_ts, "Slack reply sent");
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
            account_id = %self.account_id,
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
        debug!(account_id = %self.account_id, ts = %ts, "Slack message forwarded");
        Ok(ts)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::api::{SlackConversation, SlackReaction};

    #[test]
    fn map_conversation_dm() {
        let conv = SlackConversation {
            id: "D123".into(),
            name: None,
            is_channel: Some(false),
            is_group: Some(false),
            is_im: Some(true),
            is_mpim: Some(false),
            is_private: Some(true),
            user: Some("U456".into()),
            updated: None,
        };
        let mut cache = HashMap::new();
        cache.insert("U456".to_string(), "Alice".to_string());
        let result = map_conversation(&conv, "work-slack", &cache);
        assert_eq!(result.kind, ConversationKind::Dm);
        assert_eq!(result.connector, "slack");
        assert_eq!(result.name.as_deref(), Some("Alice"));
        assert_eq!(result.external_id, "D123");
    }

    #[test]
    fn map_conversation_channel() {
        let conv = SlackConversation {
            id: "C789".into(),
            name: Some("general".into()),
            is_channel: Some(true),
            is_group: Some(false),
            is_im: Some(false),
            is_mpim: Some(false),
            is_private: Some(false),
            user: None,
            updated: None,
        };
        let result = map_conversation(&conv, "work-slack", &HashMap::new());
        assert_eq!(result.kind, ConversationKind::Channel);
        assert_eq!(result.connector, "slack");
        assert_eq!(result.name.as_deref(), Some("general"));
    }

    #[test]
    fn parse_slack_ts() {
        assert_eq!(parse_ts("1700000000.123456"), Some(1_700_000_000));
        assert_eq!(parse_ts("invalid"), None);
    }

    #[test]
    fn build_metadata_channel_no_reactions() {
        let conv = SlackConversation {
            id: "C789".into(),
            name: Some("general".into()),
            is_channel: Some(true),
            is_group: Some(false),
            is_im: Some(false),
            is_mpim: Some(false),
            is_private: Some(false),
            user: None,
            updated: None,
        };
        let meta = build_metadata(&conv, &[], &HashMap::new()).unwrap();
        assert_eq!(meta["channel_id"], "C789");
        assert_eq!(meta["channel_name"], "general");
        assert_eq!(meta["channel_kind"], "channel");
        assert_eq!(meta["is_private"], false);
        assert!(meta.get("reactions").is_none());
    }

    #[test]
    fn build_metadata_dm_with_reactions() {
        let conv = SlackConversation {
            id: "D123".into(),
            name: None,
            is_channel: Some(false),
            is_group: Some(false),
            is_im: Some(true),
            is_mpim: Some(false),
            is_private: Some(true),
            user: Some("U456".into()),
            updated: None,
        };
        let reactions = vec![
            SlackReaction {
                name: "thumbsup".into(),
                count: 3,
                users: vec![],
            },
            SlackReaction {
                name: "heart".into(),
                count: 1,
                users: vec![],
            },
        ];
        let mut cache = HashMap::new();
        cache.insert("U456".to_string(), "Bob".to_string());
        let meta = build_metadata(&conv, &reactions, &cache).unwrap();
        assert_eq!(meta["channel_id"], "D123");
        assert_eq!(meta["channel_name"], "Bob");
        assert_eq!(meta["channel_kind"], "dm");
        assert_eq!(meta["is_private"], true);
        let r = meta["reactions"].as_array().unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0]["name"], "thumbsup");
        assert_eq!(r[0]["count"], 3);
        assert_eq!(r[1]["name"], "heart");
    }

    #[test]
    fn build_metadata_private_channel() {
        let conv = SlackConversation {
            id: "G111".into(),
            name: Some("secret-project".into()),
            is_channel: Some(false),
            is_group: Some(true),
            is_im: Some(false),
            is_mpim: Some(false),
            is_private: Some(true),
            user: None,
            updated: None,
        };
        let meta = build_metadata(&conv, &[], &HashMap::new()).unwrap();
        assert_eq!(meta["channel_kind"], "private_channel");
        assert_eq!(meta["is_private"], true);
        assert_eq!(meta["channel_name"], "secret-project");
    }

    // --- Integration tests (wiremock) ---

    #[tokio::test]
    async fn backfill_stores_conversations_and_messages() {
        let server = wiremock::MockServer::start().await;

        let users = serde_json::json!({
            "ok": true,
            "members": [
                {
                    "id": "U1",
                    "name": "alice",
                    "real_name": "Alice",
                    "profile": {"display_name": "Alice", "real_name": "Alice"}
                }
            ]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
            .mount(&server)
            .await;

        let channels = serde_json::json!({
            "ok": true,
            "channels": [
                {
                    "id": "C1",
                    "name": "general",
                    "is_channel": true,
                    "is_group": false,
                    "is_im": false,
                    "is_mpim": false,
                    "is_private": false
                }
            ]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(channels))
            .mount(&server)
            .await;

        let history = serde_json::json!({
            "ok": true,
            "messages": [
                {
                    "ts": "1741700000.000100",
                    "user": "U1",
                    "text": "Hello world"
                }
            ]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.history"))
            .and(wiremock::matchers::query_param("channel", "C1"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history))
            .mount(&server)
            .await;

        let connector = SlackConnector {
            account_id: "test-slack".to_string(),
            api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()),
            app_token: "xapp-test".to_string(),
            exclude_channels: vec![],
        };

        let db = void_core::db::Database::open_in_memory().unwrap();
        connector.backfill(&db).await.unwrap();

        let conv = db.get_conversation("test-slack-C1").unwrap().unwrap();
        assert_eq!(conv.name.as_deref(), Some("general"));

        let msg = db
            .get_message("test-slack-1741700000.000100")
            .unwrap()
            .unwrap();
        assert_eq!(msg.body.as_deref(), Some("Hello world"));
    }

    #[tokio::test]
    async fn backfill_saves_done_state() {
        let server = wiremock::MockServer::start().await;

        let users = serde_json::json!({"ok": true, "members": []});
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
            .mount(&server)
            .await;

        let channels = serde_json::json!({
            "ok": true,
            "channels": [
                {
                    "id": "C1",
                    "name": "general",
                    "is_channel": true,
                    "is_group": false,
                    "is_im": false,
                    "is_mpim": false,
                    "is_private": false
                }
            ]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(channels))
            .mount(&server)
            .await;

        let history = serde_json::json!({"ok": true, "messages": []});
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.history"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history))
            .mount(&server)
            .await;

        let connector = SlackConnector {
            account_id: "test-slack".to_string(),
            api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()),
            app_token: "xapp-test".to_string(),
            exclude_channels: vec![],
        };

        let db = void_core::db::Database::open_in_memory().unwrap();
        connector.backfill(&db).await.unwrap();
        db.set_sync_state("test-slack", "backfill_done", "1")
            .unwrap();

        assert_eq!(
            db.get_sync_state("test-slack", "backfill_done").unwrap(),
            Some("1".to_string())
        );
    }

    #[tokio::test]
    async fn start_sync_skips_backfill_when_already_done() {
        let server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users.list"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .expect(0)
            .named("users.list")
            .mount(&server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.list"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .expect(0)
            .named("conversations.list")
            .mount(&server)
            .await;

        let db = void_core::db::Database::open_in_memory().unwrap();
        db.set_sync_state("test-slack", "backfill_done", "1")
            .unwrap();

        let connector = SlackConnector {
            account_id: "test-slack".to_string(),
            api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()),
            app_token: "xapp-test".to_string(),
            exclude_channels: vec![],
        };

        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        connector
            .start_sync(std::sync::Arc::new(db), cancel)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn backfill_paginates_conversations() {
        let server = wiremock::MockServer::start().await;

        let users = serde_json::json!({"ok": true, "members": []});
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
            .mount(&server)
            .await;

        let page1 = serde_json::json!({
            "ok": true,
            "channels": [
                {
                    "id": "C1",
                    "name": "ch1",
                    "is_channel": true,
                    "is_group": false,
                    "is_im": false,
                    "is_mpim": false,
                    "is_private": false
                }
            ],
            "response_metadata": {"next_cursor": "cursor2"}
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.list"))
            .and(wiremock::matchers::query_param("cursor", "cursor2"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "ok": true,
                    "channels": [
                        {
                            "id": "C2",
                            "name": "ch2",
                            "is_channel": true,
                            "is_group": false,
                            "is_im": false,
                            "is_mpim": false,
                            "is_private": false
                        }
                    ]
                })),
            )
            .with_priority(1)
            .mount(&server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(page1))
            .with_priority(2)
            .mount(&server)
            .await;

        let history_empty = serde_json::json!({"ok": true, "messages": []});
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.history"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history_empty))
            .mount(&server)
            .await;

        let connector = SlackConnector {
            account_id: "test-slack".to_string(),
            api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()),
            app_token: "xapp-test".to_string(),
            exclude_channels: vec![],
        };

        let db = void_core::db::Database::open_in_memory().unwrap();
        connector.backfill(&db).await.unwrap();

        assert!(db.get_conversation("test-slack-C1").unwrap().is_some());
        assert!(db.get_conversation("test-slack-C2").unwrap().is_some());
    }

    #[tokio::test]
    async fn backfill_excludes_channels() {
        let server = wiremock::MockServer::start().await;

        let users = serde_json::json!({"ok": true, "members": []});
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
            .mount(&server)
            .await;

        let channels = serde_json::json!({
            "ok": true,
            "channels": [
                {
                    "id": "C1",
                    "name": "general",
                    "is_channel": true,
                    "is_group": false,
                    "is_im": false,
                    "is_mpim": false,
                    "is_private": false
                },
                {
                    "id": "C2",
                    "name": "random",
                    "is_channel": true,
                    "is_group": false,
                    "is_im": false,
                    "is_mpim": false,
                    "is_private": false
                }
            ]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(channels))
            .mount(&server)
            .await;

        let history = serde_json::json!({"ok": true, "messages": []});
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.history"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history))
            .mount(&server)
            .await;

        let connector = SlackConnector {
            account_id: "test-slack".to_string(),
            api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()),
            app_token: "xapp-test".to_string(),
            exclude_channels: vec!["random".to_string()],
        };

        let db = void_core::db::Database::open_in_memory().unwrap();
        connector.backfill(&db).await.unwrap();

        assert!(db.get_conversation("test-slack-C1").unwrap().is_some());
        assert!(db.get_conversation("test-slack-C2").unwrap().is_none());
    }

    #[tokio::test]
    async fn upload_file_calls_three_step_flow() {
        let server = wiremock::MockServer::start().await;

        let file_content = b"hello world";
        let upload_path = format!("/upload-{}", std::process::id());
        let upload_url = format!("{}{}", server.uri(), upload_path);

        let get_upload_url_resp = serde_json::json!({
            "ok": true,
            "upload_url": upload_url,
            "file_id": "F12345"
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path_regex(
                r"^/files\.getUploadURLExternal",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(get_upload_url_resp))
            .mount(&server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path(upload_path))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/files.completeUploadExternal"))
            .and(wiremock::matchers::body_json(serde_json::json!({
                "files": [{"id": "F12345", "title": "test.txt"}],
                "channel_id": "C1",
                "initial_comment": "my caption",
                "thread_ts": "123.456"
            })))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})),
            )
            .mount(&server)
            .await;

        let temp_dir = std::env::temp_dir().join(format!("void-slack-{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("test.txt");
        std::fs::write(&file_path, file_content).unwrap();

        let connector = SlackConnector {
            account_id: "test-slack".to_string(),
            api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()),
            app_token: "xapp-test".to_string(),
            exclude_channels: vec![],
        };

        let file_id = connector
            .upload_file(
                "C1",
                file_path.to_str().unwrap(),
                Some("my caption"),
                Some("123.456"),
            )
            .await
            .unwrap();

        assert_eq!(file_id, "F12345");
    }

    #[tokio::test]
    async fn catch_up_fetches_messages_since_latest() {
        let server = wiremock::MockServer::start().await;

        let users = serde_json::json!({
            "ok": true,
            "members": [
                {
                    "id": "U1",
                    "name": "alice",
                    "real_name": "Alice",
                    "profile": {"display_name": "Alice", "real_name": "Alice"}
                }
            ]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
            .mount(&server)
            .await;

        let channels = serde_json::json!({
            "ok": true,
            "channels": [
                {
                    "id": "C1",
                    "name": "general",
                    "is_channel": true,
                    "is_group": false,
                    "is_im": false,
                    "is_mpim": false,
                    "is_private": false
                }
            ]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(channels))
            .mount(&server)
            .await;

        let history = serde_json::json!({
            "ok": true,
            "messages": [
                {
                    "ts": "1741800000.000200",
                    "user": "U1",
                    "text": "Caught up message"
                }
            ]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.history"))
            .and(wiremock::matchers::query_param("channel", "C1"))
            .and(wiremock::matchers::query_param("oldest", "1741700000"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history))
            .mount(&server)
            .await;

        let db = void_core::db::Database::open_in_memory().unwrap();

        let existing_conv = Conversation {
            id: "test-slack-C1".into(),
            account_id: "test-slack".into(),
            connector: "slack".into(),
            external_id: "C1".into(),
            name: Some("general".into()),
            kind: ConversationKind::Channel,
            last_message_at: Some(1_741_700_000),
            unread_count: 0,
            is_muted: false,
            metadata: None,
        };
        db.upsert_conversation(&existing_conv).unwrap();

        let existing_msg = Message {
            id: "test-slack-1741700000.000100".into(),
            conversation_id: "test-slack-C1".into(),
            account_id: "test-slack".into(),
            connector: "slack".into(),
            external_id: "1741700000.000100".into(),
            sender: "U1".into(),
            sender_name: Some("Alice".into()),
            body: Some("Old message".into()),
            timestamp: 1_741_700_000,
            synced_at: None,
            is_archived: false,
            reply_to_id: None,
            media_type: None,
            metadata: None,
            context_id: None,
            context: None,
        };
        db.upsert_message(&existing_msg).unwrap();

        let connector = SlackConnector {
            account_id: "test-slack".to_string(),
            api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()),
            app_token: "xapp-test".to_string(),
            exclude_channels: vec![],
        };

        connector.catch_up(&db).await.unwrap();

        let new_msg = db
            .get_message("test-slack-1741800000.000200")
            .unwrap()
            .unwrap();
        assert_eq!(new_msg.body.as_deref(), Some("Caught up message"));
        assert_eq!(new_msg.sender_name.as_deref(), Some("Alice"));
    }

    #[tokio::test]
    async fn start_sync_runs_catch_up_when_backfill_done() {
        let server = wiremock::MockServer::start().await;

        let users = serde_json::json!({"ok": true, "members": []});
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
            .mount(&server)
            .await;

        let channels = serde_json::json!({
            "ok": true,
            "channels": [
                {
                    "id": "C1",
                    "name": "general",
                    "is_channel": true,
                    "is_group": false,
                    "is_im": false,
                    "is_mpim": false,
                    "is_private": false
                }
            ]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(channels))
            .mount(&server)
            .await;

        let history = serde_json::json!({
            "ok": true,
            "messages": [
                {
                    "ts": "1741800000.000200",
                    "user": "U1",
                    "text": "New message after restart"
                }
            ]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/conversations.history"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history))
            .mount(&server)
            .await;

        let db = std::sync::Arc::new(void_core::db::Database::open_in_memory().unwrap());
        db.set_sync_state("test-slack", "backfill_done", "1")
            .unwrap();

        let existing_conv = Conversation {
            id: "test-slack-C1".into(),
            account_id: "test-slack".into(),
            connector: "slack".into(),
            external_id: "C1".into(),
            name: Some("general".into()),
            kind: ConversationKind::Channel,
            last_message_at: Some(1_741_700_000),
            unread_count: 0,
            is_muted: false,
            metadata: None,
        };
        db.upsert_conversation(&existing_conv).unwrap();

        let existing_msg = Message {
            id: "test-slack-1741700000.000100".into(),
            conversation_id: "test-slack-C1".into(),
            account_id: "test-slack".into(),
            connector: "slack".into(),
            external_id: "1741700000.000100".into(),
            sender: "U1".into(),
            sender_name: Some("Alice".into()),
            body: Some("Old message".into()),
            timestamp: 1_741_700_000,
            synced_at: None,
            is_archived: false,
            reply_to_id: None,
            media_type: None,
            metadata: None,
            context_id: None,
            context: None,
        };
        db.upsert_message(&existing_msg).unwrap();

        let connector = SlackConnector {
            account_id: "test-slack".to_string(),
            api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()),
            app_token: "xapp-test".to_string(),
            exclude_channels: vec![],
        };

        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        connector.start_sync(db.clone(), cancel).await.unwrap();

        let new_msg = db
            .get_message("test-slack-1741800000.000200")
            .unwrap()
            .unwrap();
        assert_eq!(new_msg.body.as_deref(), Some("New message after restart"));
    }
}
