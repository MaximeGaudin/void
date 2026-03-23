//! Slack connector: struct, Connector trait impl, action methods.

use std::path::Path;

use anyhow::Context;

use crate::api::SlackApiClient;
use crate::error::SlackError;

mod connector_trait;
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
    pub(crate) exclude_channels: Vec<String>,
}

impl SlackConnector {
    pub fn new(
        connection_id: &str,
        user_token: &str,
        app_token: &str,
        exclude_channels: Vec<String>,
    ) -> Result<Self, SlackError> {
        Ok(Self {
            connection_id: connection_id.to_string(),
            api: SlackApiClient::new(user_token)?,
            app_token: app_token.to_string(),
            exclude_channels,
        })
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
