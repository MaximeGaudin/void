//! Slack Web API response types.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::error;

use crate::error::SlackError;

/// Generic Slack response envelope (`{ ok, error, ...data }`).
#[derive(Debug, Deserialize)]
pub(crate) struct SlackResponse<T> {
    ok: bool,
    pub(crate) error: Option<String>,
    #[serde(flatten)]
    data: Option<T>,
}

impl<T> SlackResponse<T> {
    pub(crate) fn into_result(self) -> Result<T, SlackError> {
        if self.ok {
            self.data.ok_or_else(|| {
                error!("slack: ok=true but no data");
                SlackError::Api("Slack returned ok=true but no data".into())
            })
        } else {
            let err = self.error.unwrap_or_else(|| "unknown".into());
            error!(error = %err, "slack: API error");
            Err(SlackError::Api(format!("Slack API error: {err}")))
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AuthTestResponse {
    pub url: Option<String>,
    pub team: Option<String>,
    pub user: Option<String>,
    pub team_id: Option<String>,
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConversationsListResponse {
    pub channels: Vec<SlackConversation>,
    pub response_metadata: Option<ResponseMetadata>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseMetadata {
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackConversation {
    pub id: String,
    pub name: Option<String>,
    pub is_channel: Option<bool>,
    pub is_group: Option<bool>,
    pub is_im: Option<bool>,
    pub is_mpim: Option<bool>,
    pub is_private: Option<bool>,
    pub user: Option<String>,
    pub updated: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ConversationInfoResponse {
    pub channel: SlackConversation,
}

#[derive(Debug, Deserialize)]
pub struct ConversationsOpenResponse {
    pub channel: SlackConversation,
}

#[derive(Debug, Deserialize)]
pub struct ConversationsHistoryResponse {
    pub messages: Vec<SlackMessage>,
    pub has_more: Option<bool>,
    pub response_metadata: Option<ResponseMetadata>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackMessage {
    pub ts: String,
    pub user: Option<String>,
    pub text: Option<String>,
    pub thread_ts: Option<String>,
    #[serde(rename = "type")]
    pub msg_type: Option<String>,
    pub subtype: Option<String>,
    /// Number of thread replies. Present (and > 0) on thread parents when
    /// returned by `conversations.history`. Absent on replies themselves.
    #[serde(default)]
    pub reply_count: Option<u32>,
    #[serde(default)]
    pub reactions: Vec<SlackReaction>,
    #[serde(default)]
    pub files: Vec<SlackFile>,
    #[serde(default)]
    pub attachments: Vec<SlackAttachment>,
}

impl SlackMessage {
    /// `true` iff this message is the head of a thread with replies.
    /// `conversations.history` returns thread parents with
    /// `thread_ts == ts` and `reply_count > 0`; the replies themselves are
    /// only exposed via `conversations.replies`.
    pub fn is_thread_parent_with_replies(&self) -> bool {
        matches!(self.thread_ts.as_deref(), Some(tts) if tts == self.ts)
            && self.reply_count.unwrap_or(0) > 0
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackFile {
    pub id: String,
    pub name: Option<String>,
    pub title: Option<String>,
    pub mimetype: Option<String>,
    pub filetype: Option<String>,
    pub size: Option<u64>,
    pub url_private: Option<String>,
    #[serde(default)]
    pub url_private_download: Option<String>,
    pub permalink: Option<String>,
    #[serde(default)]
    pub is_external: Option<bool>,
    #[serde(default)]
    pub external_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackAttachment {
    pub fallback: Option<String>,
    pub title: Option<String>,
    pub text: Option<String>,
    pub image_url: Option<String>,
    pub from_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackReaction {
    pub name: String,
    pub count: u32,
    #[serde(default)]
    pub users: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UserInfoResponse {
    pub user: Option<SlackUser>,
}

#[derive(Debug, Deserialize)]
pub struct UsersListResponse {
    pub members: Vec<SlackUser>,
    pub response_metadata: Option<ResponseMetadata>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackUser {
    pub id: String,
    pub name: String,
    pub real_name: Option<String>,
    pub profile: Option<SlackUserProfile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlackUserProfile {
    pub display_name: Option<String>,
    pub real_name: Option<String>,
    pub image_72: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatPostMessageResponse {
    pub channel: Option<String>,
    pub ts: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatUpdateResponse {
    pub channel: Option<String>,
    pub ts: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatScheduleMessageResponse {
    pub channel: Option<String>,
    pub scheduled_message_id: Option<String>,
    pub post_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ConnectionsOpenResponse {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct FilesUploadUrlResponse {
    pub upload_url: String,
    pub file_id: String,
}

/// One entry of `files.completeUploadExternal`'s `files` array.
///
/// Note on `shares`: it is **not** usable as proof of delivery from this
/// response. Measured against a live workspace, the immediate reply carries
/// `shares: {}` with empty `ims`/`channels`, and the share record only appears
/// on `files.info` a moment later, because Slack shares the file
/// asynchronously after the upload is completed. Keying a send's success on
/// `shares` therefore fails every successful send. It is parsed only so the
/// `ts` can be logged when Slack does happen to include it.
#[derive(Debug, Deserialize)]
pub struct CompletedUploadFile {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub shares: Option<HashMap<String, HashMap<String, Vec<ShareRecord>>>>,
}

/// A single share of a file into one channel.
#[derive(Debug, Deserialize)]
pub struct ShareRecord {
    /// Timestamp of the message carrying the file in that channel.
    #[serde(default)]
    pub ts: Option<String>,
}

impl CompletedUploadFile {
    /// The message `ts` for this file in `channel`, when Slack already knows it.
    ///
    /// Often `None` on the immediate `completeUploadExternal` reply (the share is
    /// applied asynchronously), so this is for logging, never for deciding
    /// whether the send succeeded.
    pub fn share_ts_in(&self, channel: &str) -> Option<&str> {
        self.shares
            .as_ref()?
            .values()
            .filter_map(|by_channel| by_channel.get(channel))
            .flatten()
            .find_map(|share| share.ts.as_deref())
    }
}

/// Response of `files.completeUploadExternal`. An `ok: true` with an empty
/// `files` array means Slack acknowledged nothing, so it must not count as a send.
#[derive(Debug, Deserialize)]
pub struct FilesCompleteUploadResponse {
    #[serde(default)]
    pub files: Vec<CompletedUploadFile>,
}

#[derive(Debug, Deserialize)]
pub struct SearchMessagesResponse {
    pub messages: SearchMessagesMatches,
    pub response_metadata: Option<ResponseMetadata>,
}

#[derive(Debug, Deserialize)]
pub struct SearchMessagesMatches {
    pub matches: Vec<SearchMatch>,
    pub pagination: Option<SearchPagination>,
}

#[derive(Debug, Deserialize)]
pub struct SearchMatch {
    pub channel: SearchMatchChannel,
    pub ts: String,
    pub text: Option<String>,
    pub user: Option<String>,
    pub permalink: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchMatchChannel {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchPagination {
    pub next_cursor: Option<String>,
}
