use serde::{Deserialize, Serialize};

use super::serde_ts::epoch_iso8601;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub connection_id: String,
    pub connector: String,
    pub external_id: String,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    #[serde(with = "epoch_iso8601")]
    pub start_at: i64,
    #[serde(with = "epoch_iso8601")]
    pub end_at: i64,
    pub all_day: bool,
    pub attendees: Option<serde_json::Value>,
    pub status: Option<String>,
    pub calendar_name: Option<String>,
    pub meet_link: Option<String>,
    pub metadata: Option<serde_json::Value>,
}
