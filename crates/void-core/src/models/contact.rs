use serde::{Deserialize, Serialize};

use super::serde_ts::epoch_iso8601;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub sender: String,
    pub sender_name: Option<String>,
    pub avatar_url: Option<String>,
    pub connection_id: String,
    pub connector: String,
    pub message_count: i64,
    #[serde(with = "epoch_iso8601")]
    pub last_message_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub connection_id: String,
    pub key: String,
    pub value: String,
}
