use serde::{Deserialize, Serialize};

use super::connector::ConnectorType;
use super::serde_ts::epoch_iso8601_opt;

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    File {
        path: std::path::PathBuf,
        caption: Option<String>,
        mime_type: Option<String>,
    },
}

impl MessageContent {
    /// The textual payload to send: the body for [`Text`](Self::Text), or the
    /// caption (empty when absent) for [`File`](Self::File).
    pub fn text(&self) -> &str {
        match self {
            MessageContent::Text(text) => text.as_str(),
            MessageContent::File { caption, .. } => caption.as_deref().unwrap_or(""),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub connection_id: String,
    pub connector_type: ConnectorType,
    pub ok: bool,
    pub message: String,
    #[serde(with = "epoch_iso8601_opt")]
    pub last_sync: Option<i64>,
    pub message_count: Option<i64>,
}
