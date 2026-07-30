use serde::{Deserialize, Serialize};

use super::connector::ConnectorType;
use super::serde_ts::epoch_iso8601_opt;

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text {
        body: String,
        /// Email subject (Gmail only).
        subject: Option<String>,
        /// Append the account Gmail HTML signature (Gmail only).
        append_signature: bool,
        /// Send-as alias whose signature to use (Gmail only; requires `append_signature`).
        signature_from: Option<String>,
        /// Cc recipient(s), comma-separated (Gmail only).
        cc: Option<String>,
        /// Bcc recipient(s), comma-separated (Gmail only).
        bcc: Option<String>,
    },
    File {
        path: std::path::PathBuf,
        caption: Option<String>,
        mime_type: Option<String>,
        /// Email subject (Gmail only). When absent, attachment sends use the filename.
        subject: Option<String>,
        /// Append the account Gmail HTML signature (Gmail only).
        append_signature: bool,
        /// Send-as alias whose signature to use (Gmail only; requires `append_signature`).
        signature_from: Option<String>,
        /// Cc recipient(s), comma-separated (Gmail only).
        cc: Option<String>,
        /// Bcc recipient(s), comma-separated (Gmail only).
        bcc: Option<String>,
    },
}

impl MessageContent {
    pub fn from_text(body: impl Into<String>) -> Self {
        Self::Text {
            body: body.into(),
            subject: None,
            append_signature: false,
            signature_from: None,
            cc: None,
            bcc: None,
        }
    }

    /// The textual payload to send: the body for [`Text`](Self::Text), or the
    /// caption (empty when absent) for [`File`](Self::File).
    pub fn text(&self) -> &str {
        match self {
            MessageContent::Text { body, .. } => body.as_str(),
            MessageContent::File { caption, .. } => caption.as_deref().unwrap_or(""),
        }
    }

    /// Email subject when sending via Gmail.
    pub fn subject(&self) -> Option<&str> {
        match self {
            MessageContent::Text { subject, .. } | MessageContent::File { subject, .. } => {
                subject.as_deref()
            }
        }
    }

    /// Whether to append a Gmail HTML signature (ignored by other connectors).
    pub fn append_signature(&self) -> bool {
        match self {
            MessageContent::Text {
                append_signature, ..
            }
            | MessageContent::File {
                append_signature, ..
            } => *append_signature,
        }
    }

    /// Optional send-as alias for the Gmail signature (ignored by other connectors).
    pub fn signature_from(&self) -> Option<&str> {
        match self {
            MessageContent::Text { signature_from, .. }
            | MessageContent::File { signature_from, .. } => signature_from.as_deref(),
        }
    }

    /// Optional Cc recipients for Gmail (ignored by other connectors).
    pub fn cc(&self) -> Option<&str> {
        match self {
            MessageContent::Text { cc, .. } | MessageContent::File { cc, .. } => cc.as_deref(),
        }
    }

    /// Optional Bcc recipients for Gmail (ignored by other connectors).
    pub fn bcc(&self) -> Option<&str> {
        match self {
            MessageContent::Text { bcc, .. } | MessageContent::File { bcc, .. } => bcc.as_deref(),
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
