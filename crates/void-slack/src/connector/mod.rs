//! Slack connector: struct, Connector trait impl, action methods.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Context;

use void_core::config::{default_config_path, VoidConfig};

use crate::api::SlackApiClient;
use crate::error::SlackError;

mod connector_trait;
mod files;
mod mapping;
mod socket_mode;
mod sync;

#[cfg(test)]
mod tests;

#[allow(unused_imports)] // used by tests
pub(crate) use mapping::{build_metadata, map_conversation, parse_ts};

pub struct SlackConnector {
    pub(crate) connection_id: String,
    pub(crate) api: SlackApiClient,
    pub(crate) app_token: String,
    pub(crate) app_id: Option<String>,
    pub(crate) config_refresh_token: Mutex<Option<String>>,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) store_path: PathBuf,
}

fn is_invalid_refresh_token_err(result: &anyhow::Result<()>) -> bool {
    result
        .as_ref()
        .err()
        .is_some_and(|e| e.to_string().contains("invalid_refresh_token"))
}

impl SlackConnector {
    pub fn new(
        connection_id: &str,
        user_token: &str,
        app_token: &str,
        app_id: Option<&str>,
        config_refresh_token: Option<&str>,
        store_path: &Path,
        config_path: Option<&Path>,
    ) -> Result<Self, SlackError> {
        Ok(Self {
            connection_id: connection_id.to_string(),
            api: SlackApiClient::new(user_token)?,
            app_token: app_token.to_string(),
            app_id: app_id.map(|s| s.to_string()),
            config_refresh_token: Mutex::new(config_refresh_token.map(|token| token.to_string())),
            config_path: config_path.map(Path::to_path_buf),
            store_path: store_path.to_path_buf(),
        })
    }

    pub(crate) async fn run_event_subscription_check(&self) -> anyhow::Result<()> {
        let Some(app_id) = &self.app_id else {
            return Ok(());
        };

        self.reload_config_refresh_token_from_disk();
        let result = self.try_event_subscription_check(app_id).await;
        if result.is_err() && is_invalid_refresh_token_err(&result) {
            self.reload_config_refresh_token_from_disk();
            return self.try_event_subscription_check(app_id).await;
        }
        result
    }

    async fn try_event_subscription_check(&self, app_id: &str) -> anyhow::Result<()> {
        let mut token = self
            .config_refresh_token
            .lock()
            .map_err(|_| anyhow::anyhow!("Slack config refresh token lock poisoned"))?
            .clone();
        if token.is_none() {
            return Ok(());
        }

        let used = token.clone();
        let result =
            crate::manifest::ensure_event_subscriptions(&mut token, app_id, &self.connection_id)
                .await;

        match &result {
            Ok(()) if token.as_deref() != used.as_deref() => {
                if let Some(new_token) = token {
                    *self.config_refresh_token.lock().map_err(|_| {
                        anyhow::anyhow!("Slack config refresh token lock poisoned")
                    })? = Some(new_token.clone());
                    self.persist_config_refresh_token(&new_token);
                }
            }
            Err(_) => {
                self.reload_config_refresh_token_from_disk();
            }
            _ => {}
        }

        result
    }

    fn reload_config_refresh_token_from_disk(&self) {
        let path = self.config_path.clone().unwrap_or_else(default_config_path);
        let Some(token) = Self::read_config_refresh_token(&path, &self.connection_id) else {
            return;
        };
        if let Ok(mut guard) = self.config_refresh_token.lock() {
            *guard = token;
        }
    }

    fn read_config_refresh_token(
        config_path: &Path,
        connection_id: &str,
    ) -> Option<Option<String>> {
        let cfg = VoidConfig::load(config_path).ok()?;
        let conn = cfg.connections.iter().find(|c| c.id == connection_id)?;
        let token = void_core::config::settings_string(&conn.settings, "config_refresh_token");
        Some(token)
    }

    fn persist_config_refresh_token(&self, token: &str) {
        let path = self.config_path.clone().unwrap_or_else(default_config_path);
        if let Ok(mut cfg) = VoidConfig::load(&path) {
            if cfg.set_slack_config_refresh_token(&self.connection_id, Some(token.to_string())) {
                let _ = cfg.save(&path);
            }
        }
    }

    pub(crate) fn has_config_refresh_token(&self) -> bool {
        self.reload_config_refresh_token_from_disk();
        self.config_refresh_token
            .lock()
            .map(|token| token.is_some())
            .unwrap_or(false)
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
        } else if let Some(channel_name) = to.strip_prefix('#') {
            self.api.resolve_channel_id_by_name(channel_name).await
        } else {
            Ok(to.to_string())
        }
    }

    pub(crate) fn files_dir(&self) -> PathBuf {
        self.store_path
            .join(format!("files/slack-{}", self.connection_id))
    }

    /// Download all Slack files referenced in `metadata.files` for each message
    /// and add a `local_path` field pointing to the cached copy on disk.
    pub(crate) async fn download_message_files(&self, messages: &mut [void_core::models::Message]) {
        let dir = self.files_dir();
        for msg in messages.iter_mut() {
            let files = match msg
                .metadata
                .as_mut()
                .and_then(|m| m.get_mut("files"))
                .and_then(|f| f.as_array_mut())
            {
                Some(f) => f,
                None => continue,
            };
            for file in files.iter_mut() {
                let file_id = file.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");

                if let Some(reason) = files::skip_download_reason(file) {
                    tracing::debug!(
                        file_id,
                        reason,
                        "skipping Slack file download (not cacheable locally)"
                    );
                    files::mark_download_skipped(file, reason);
                    continue;
                }

                let url = match files::resolve_download_url(file) {
                    Some(u) => u.to_string(),
                    None => continue,
                };
                let file_name = file.get("name").and_then(|v| v.as_str()).unwrap_or("file");
                let local_name = format!("{file_id}_{file_name}");
                let dest = dir.join(&local_name);

                if dest.exists() {
                    file["local_path"] =
                        serde_json::Value::String(dest.to_string_lossy().into_owned());
                    continue;
                }

                match self.api.download_file(&url).await {
                    Ok(data) => {
                        if let Err(e) = std::fs::create_dir_all(&dir) {
                            tracing::warn!(error = %e, "failed to create files cache dir");
                            continue;
                        }
                        if let Err(e) = std::fs::write(&dest, &data) {
                            tracing::warn!(file_id, error = %e, "failed to write cached file");
                            continue;
                        }
                        file["local_path"] =
                            serde_json::Value::String(dest.to_string_lossy().into_owned());
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        if err_str.contains("HTTP 404") {
                            tracing::debug!(
                                file_id,
                                error = %e,
                                "Slack file no longer available for download"
                            );
                            files::mark_download_skipped(file, "not_found");
                        } else {
                            tracing::warn!(file_id, error = %e, "failed to download Slack file");
                        }
                    }
                }
            }
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
