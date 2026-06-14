//! `chat.*`, `reactions.*`, `connections.*`, and `files.*` endpoint wrappers.

use tracing::debug;

use super::types::*;
use super::SlackApiClient;
use crate::error::SlackError;

impl SlackApiClient {
    pub async fn chat_post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<ChatPostMessageResponse, SlackError> {
        debug!(channel, thread_ts, "slack: chat.postMessage");
        let mut body = serde_json::json!({
            "channel": channel,
            "text": text,
        });
        if let Some(ts) = thread_ts {
            body["thread_ts"] = serde_json::Value::String(ts.to_string());
        }
        let result: ChatPostMessageResponse = self
            .post_with_retry(
                &format!("{}/chat.postMessage", self.base_url),
                &body,
                "chat.postMessage",
            )
            .await?;
        debug!(ts = ?result.ts, "slack: chat.postMessage success");
        Ok(result)
    }

    pub async fn chat_update(
        &self,
        channel: &str,
        ts: &str,
        text: &str,
    ) -> Result<ChatUpdateResponse, SlackError> {
        debug!(channel, ts, "slack: chat.update");
        let body = serde_json::json!({
            "channel": channel,
            "ts": ts,
            "text": text,
        });
        let result: ChatUpdateResponse = self
            .post_with_retry(
                &format!("{}/chat.update", self.base_url),
                &body,
                "chat.update",
            )
            .await?;
        debug!(ts = ?result.ts, "slack: chat.update success");
        Ok(result)
    }

    /// Call apps.connections.open with an app-level token to get a WebSocket URL for Socket Mode.
    pub async fn connections_open(
        &self,
        app_token: &str,
    ) -> Result<ConnectionsOpenResponse, SlackError> {
        let resp = self
            .http
            .post(format!("{}/apps.connections.open", self.base_url))
            .bearer_auth(app_token)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .await?;

        let slack_resp: SlackResponse<ConnectionsOpenResponse> = resp.json().await?;
        slack_resp.into_result()
    }

    pub async fn chat_schedule_message(
        &self,
        channel: &str,
        text: &str,
        post_at: i64,
        thread_ts: Option<&str>,
    ) -> anyhow::Result<ChatScheduleMessageResponse> {
        debug!(channel, post_at, "slack: chat.scheduleMessage");
        let mut body = serde_json::json!({
            "channel": channel,
            "text": text,
            "post_at": post_at,
        });
        if let Some(ts) = thread_ts {
            body["thread_ts"] = serde_json::Value::String(ts.to_string());
        }
        let result: ChatScheduleMessageResponse = self
            .post_with_retry(
                &format!("{}/chat.scheduleMessage", self.base_url),
                &body,
                "chat.scheduleMessage",
            )
            .await?;
        debug!(scheduled_message_id = ?result.scheduled_message_id, "slack: chat.scheduleMessage success");
        Ok(result)
    }

    pub async fn reactions_add(&self, channel: &str, ts: &str, emoji: &str) -> anyhow::Result<()> {
        debug!(channel, ts, emoji, "slack: reactions.add");
        let body = serde_json::json!({
            "channel": channel,
            "timestamp": ts,
            "name": emoji,
        });
        let _: serde_json::Value = self
            .post_with_retry(
                &format!("{}/reactions.add", self.base_url),
                &body,
                "reactions.add",
            )
            .await?;
        debug!(emoji, "slack: reactions.add success");
        Ok(())
    }

    pub async fn files_get_upload_url_external(
        &self,
        filename: &str,
        length: u64,
    ) -> anyhow::Result<FilesUploadUrlResponse> {
        debug!(filename, length, "slack: files.getUploadURLExternal");
        let params = [
            ("filename", filename.to_string()),
            ("length", length.to_string()),
        ];
        let result: FilesUploadUrlResponse = self
            .get_with_retry(
                &format!("{}/files.getUploadURLExternal", self.base_url),
                &params,
                "files.getUploadURLExternal",
            )
            .await?;
        debug!(file_id = %result.file_id, "slack: files.getUploadURLExternal success");
        Ok(result)
    }

    /// Upload file bytes to a pre-signed URL (from files.getUploadURLExternal).
    /// Slack requires multipart/form-data with the file in a field named "file".
    pub async fn post_file_to_url(
        &self,
        url: &str,
        data: Vec<u8>,
        filename: &str,
    ) -> anyhow::Result<()> {
        let part = reqwest::multipart::Part::bytes(data)
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")?;
        let form = reqwest::multipart::Form::new().part("file", part);
        let resp = self.http.post(url).multipart(form).send().await?;
        resp.error_for_status()?;
        Ok(())
    }

    /// Download a file from a Slack `url_private` URL using bearer-token auth.
    pub async fn download_file(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.user_token)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("Slack file download failed (HTTP {status}): {url}");
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn files_complete_upload_external(
        &self,
        file_id: &str,
        title: &str,
        channel_id: Option<&str>,
        initial_comment: Option<&str>,
        thread_ts: Option<&str>,
    ) -> anyhow::Result<()> {
        debug!(
            file_id,
            title, channel_id, "slack: files.completeUploadExternal"
        );
        let mut body = serde_json::json!({
            "files": [{"id": file_id, "title": title}],
        });
        if let Some(c) = channel_id {
            body["channel_id"] = serde_json::Value::String(c.to_string());
        }
        if let Some(comment) = initial_comment {
            body["initial_comment"] = serde_json::Value::String(comment.to_string());
        }
        if let Some(ts) = thread_ts {
            body["thread_ts"] = serde_json::Value::String(ts.to_string());
        }
        let _: serde_json::Value = self
            .post_with_retry(
                &format!("{}/files.completeUploadExternal", self.base_url),
                &body,
                "files.completeUploadExternal",
            )
            .await?;
        debug!("slack: files.completeUploadExternal success");
        Ok(())
    }
}
