use serde::{Deserialize, Serialize};

use super::serde_ts::epoch_iso8601_opt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConversationKind {
    Dm,
    Group,
    Channel,
    Thread,
}

impl std::fmt::Display for ConversationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dm => write!(f, "dm"),
            Self::Group => write!(f, "group"),
            Self::Channel => write!(f, "channel"),
            Self::Thread => write!(f, "thread"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub connection_id: String,
    pub connector: String,
    pub external_id: String,
    pub name: Option<String>,
    pub kind: ConversationKind,
    #[serde(with = "epoch_iso8601_opt")]
    pub last_message_at: Option<i64>,
    pub unread_count: i64,
    pub is_muted: bool,
    pub metadata: Option<serde_json::Value>,
}
