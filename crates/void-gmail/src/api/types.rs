use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailProfile {
    pub email_address: Option<String>,
    pub history_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageListResponse {
    pub messages: Option<Vec<MessageRef>>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MessageRef {
    pub id: String,
    #[serde(rename = "threadId")]
    pub thread_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailMessage {
    pub id: Option<String>,
    pub thread_id: Option<String>,
    pub snippet: Option<String>,
    pub internal_date: Option<String>,
    pub label_ids: Option<Vec<String>>,
    pub payload: Option<MessagePayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePayload {
    pub mime_type: Option<String>,
    pub headers: Option<Vec<MessageHeader>>,
    pub body: Option<MessagePartBody>,
    pub parts: Option<Vec<MessagePart>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePart {
    pub mime_type: Option<String>,
    pub filename: Option<String>,
    pub headers: Option<Vec<MessageHeader>>,
    pub body: Option<MessagePartBody>,
    pub parts: Option<Vec<MessagePart>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePartBody {
    pub data: Option<String>,
    pub size: Option<u64>,
    pub attachment_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MessageHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileAttachment {
    pub filename: String,
    pub mime_type: Option<String>,
    pub size: Option<u64>,
    pub attachment_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryListResponse {
    pub history: Option<Vec<HistoryRecord>>,
    pub history_id: Option<String>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    pub messages_added: Option<Vec<HistoryMessageAdded>>,
    pub labels_added: Option<Vec<HistoryLabelChange>>,
    pub labels_removed: Option<Vec<HistoryLabelChange>>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryMessageAdded {
    pub message: MessageRef,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryLabelChange {
    pub message: MessageRef,
    pub label_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailThread {
    pub id: Option<String>,
    pub snippet: Option<String>,
    pub messages: Option<Vec<GmailMessage>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentResponse {
    pub data: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct LabelListResponse {
    pub labels: Option<Vec<GmailLabel>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailLabel {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub label_type: Option<String>,
    pub messages_total: Option<u64>,
    pub messages_unread: Option<u64>,
    pub threads_total: Option<u64>,
    pub threads_unread: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct DraftListResponse {
    pub drafts: Option<Vec<DraftRef>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftRef {
    pub id: String,
    pub message: Option<MessageRef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailDraft {
    pub id: Option<String>,
    pub message: Option<GmailMessage>,
}
