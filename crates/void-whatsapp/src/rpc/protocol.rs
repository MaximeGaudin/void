//! JSON-RPC-style request/response types for WhatsApp daemon IPC.

use serde::{Deserialize, Serialize};
use void_core::models::MessageContent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcRequest {
    pub id: u64,
    pub connection_id: String,
    #[serde(flatten)]
    pub method: RpcMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum RpcMethod {
    Send {
        to: String,
        content: RpcContent,
    },
    Reply {
        message_id: String,
        content: RpcContent,
        in_thread: bool,
    },
    DownloadMedia {
        #[serde(flatten)]
        params: RpcDownloadParams,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcDownloadParams {
    pub direct_path: String,
    pub media_key: String,
    pub file_sha256: String,
    pub file_enc_sha256: String,
    pub file_length: u64,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcContent {
    Text {
        text: String,
    },
    File {
        path: String,
        caption: Option<String>,
        mime_type: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcResponse {
    pub id: u64,
    #[serde(flatten)]
    pub body: RpcResponseBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "status", content = "data")]
pub enum RpcResponseBody {
    #[serde(rename = "ok")]
    Ok { result: RpcResult },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcResult {
    MessageId { message_id: String },
    MediaBytes { data_base64: String },
}

impl RpcRequest {
    pub fn encode_line(&self) -> anyhow::Result<String> {
        Ok(format!("{}\n", serde_json::to_string(self)?))
    }

    pub fn decode_line(line: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(line.trim())?)
    }
}

impl RpcResponse {
    pub fn encode_line(&self) -> anyhow::Result<String> {
        Ok(format!("{}\n", serde_json::to_string(self)?))
    }

    pub fn decode_line(line: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(line.trim())?)
    }

    pub fn ok(id: u64, result: RpcResult) -> Self {
        Self {
            id,
            body: RpcResponseBody::Ok { result },
        }
    }

    pub fn error(id: u64, error: impl Into<String>) -> Self {
        Self {
            id,
            body: RpcResponseBody::Error {
                error: error.into(),
            },
        }
    }
}

pub fn message_content_to_rpc(content: &MessageContent) -> RpcContent {
    match content {
        MessageContent::Text(text) => RpcContent::Text { text: text.clone() },
        MessageContent::File {
            path,
            caption,
            mime_type,
        } => RpcContent::File {
            path: path.to_string_lossy().into_owned(),
            caption: caption.clone(),
            mime_type: mime_type.clone(),
        },
    }
}

pub fn rpc_to_message_content(content: RpcContent) -> MessageContent {
    match content {
        RpcContent::Text { text } => MessageContent::Text(text),
        RpcContent::File {
            path,
            caption,
            mime_type,
        } => MessageContent::File {
            path: path.into(),
            caption,
            mime_type,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip_send() {
        let req = RpcRequest {
            id: 7,
            connection_id: "whatsapp".into(),
            method: RpcMethod::Send {
                to: "33612345678".into(),
                content: RpcContent::Text {
                    text: "hello".into(),
                },
            },
        };
        let line = req.encode_line().unwrap();
        let parsed = RpcRequest::decode_line(&line).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn response_round_trip_media() {
        let resp = RpcResponse::ok(
            3,
            RpcResult::MediaBytes {
                data_base64: "AQID".into(),
            },
        );
        let line = resp.encode_line().unwrap();
        let parsed = RpcResponse::decode_line(&line).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn response_error_round_trip() {
        let resp = RpcResponse::error(1, "not connected");
        let line = resp.encode_line().unwrap();
        let parsed = RpcResponse::decode_line(&line).unwrap();
        assert_eq!(
            parsed.body,
            RpcResponseBody::Error {
                error: "not connected".into(),
            }
        );
    }

    #[test]
    fn request_round_trip_reply_in_thread() {
        let req = RpcRequest {
            id: 12,
            connection_id: "wa".into(),
            method: RpcMethod::Reply {
                message_id: "120363@g.us:MSG1".into(),
                content: RpcContent::Text { text: "re".into() },
                in_thread: true,
            },
        };
        let parsed = RpcRequest::decode_line(&req.encode_line().unwrap()).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn request_round_trip_download_media() {
        let req = RpcRequest {
            id: 5,
            connection_id: "wa".into(),
            method: RpcMethod::DownloadMedia {
                params: RpcDownloadParams {
                    direct_path: "/v/t62".into(),
                    media_key: "a2V5".into(),
                    file_sha256: "c2hh".into(),
                    file_enc_sha256: "ZW5j".into(),
                    file_length: 4096,
                    media_type: "image".into(),
                },
            },
        };
        let parsed = RpcRequest::decode_line(&req.encode_line().unwrap()).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn decode_line_rejects_malformed_json() {
        assert!(RpcRequest::decode_line("{not json").is_err());
        assert!(RpcResponse::decode_line("garbage").is_err());
    }

    #[test]
    fn message_content_round_trip_text() {
        let original = MessageContent::Text("hello world".into());
        match rpc_to_message_content(message_content_to_rpc(&original)) {
            MessageContent::Text(t) => assert_eq!(t, "hello world"),
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn message_content_round_trip_file_with_caption() {
        let original = MessageContent::File {
            path: "/tmp/pic.jpg".into(),
            caption: Some("look".into()),
            mime_type: Some("image/jpeg".into()),
        };
        match rpc_to_message_content(message_content_to_rpc(&original)) {
            MessageContent::File {
                path,
                caption,
                mime_type,
            } => {
                assert_eq!(path, std::path::PathBuf::from("/tmp/pic.jpg"));
                assert_eq!(caption.as_deref(), Some("look"));
                assert_eq!(mime_type.as_deref(), Some("image/jpeg"));
            }
            other => panic!("expected file, got {other:?}"),
        }
    }

    #[test]
    fn message_content_round_trip_file_without_optionals() {
        let original = MessageContent::File {
            path: "/tmp/doc.pdf".into(),
            caption: None,
            mime_type: None,
        };
        match rpc_to_message_content(message_content_to_rpc(&original)) {
            MessageContent::File {
                path,
                caption,
                mime_type,
            } => {
                assert_eq!(path, std::path::PathBuf::from("/tmp/doc.pdf"));
                assert!(caption.is_none());
                assert!(mime_type.is_none());
            }
            other => panic!("expected file, got {other:?}"),
        }
    }
}
