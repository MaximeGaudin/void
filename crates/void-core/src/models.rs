use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorType {
    WhatsApp,
    Slack,
    Gmail,
    Calendar,
}

impl std::fmt::Display for ConnectorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WhatsApp => write!(f, "whatsapp"),
            Self::Slack => write!(f, "slack"),
            Self::Gmail => write!(f, "gmail"),
            Self::Calendar => write!(f, "calendar"),
        }
    }
}

impl ConnectorType {
    /// Short badge for display in unified views (e.g. "[WA]", "[SL]").
    pub fn badge(&self) -> &'static str {
        match self {
            Self::WhatsApp => "WA",
            Self::Slack => "SL",
            Self::Gmail => "GM",
            Self::Calendar => "CA",
        }
    }
}

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
    pub account_id: String,
    /// Connector type: "slack", "gmail", "whatsapp", "calendar"
    pub connector: String,
    pub external_id: String,
    pub name: Option<String>,
    pub kind: ConversationKind,
    pub last_message_at: Option<i64>,
    pub unread_count: i64,
    pub is_muted: bool,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub account_id: String,
    /// Connector type: "slack", "gmail", "whatsapp", "calendar"
    pub connector: String,
    pub external_id: String,
    pub sender: String,
    pub sender_name: Option<String>,
    pub body: Option<String>,
    /// When the message was originally sent (UTC epoch seconds).
    pub timestamp: i64,
    /// When we first synced this message (UTC epoch seconds).
    pub synced_at: Option<i64>,
    pub is_from_me: bool,
    pub is_read: bool,
    pub is_archived: bool,
    pub reply_to_id: Option<String>,
    pub media_type: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub account_id: String,
    /// Connector type: "slack", "gmail", "whatsapp", "calendar"
    pub connector: String,
    pub external_id: String,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: i64,
    pub end_at: i64,
    pub all_day: bool,
    pub attendees: Option<serde_json::Value>,
    pub status: Option<String>,
    pub calendar_name: Option<String>,
    pub meet_link: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub sender: String,
    pub sender_name: Option<String>,
    pub account_id: String,
    /// Connector type: "slack", "gmail", "whatsapp", "calendar"
    pub connector: String,
    pub message_count: i64,
    pub last_message_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub account_id: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    File {
        path: std::path::PathBuf,
        caption: Option<String>,
        mime_type: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub account_id: String,
    pub connector_type: ConnectorType,
    pub ok: bool,
    pub message: String,
    pub last_sync: Option<i64>,
    pub message_count: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_type_display() {
        assert_eq!(ConnectorType::WhatsApp.to_string(), "whatsapp");
        assert_eq!(ConnectorType::Slack.to_string(), "slack");
        assert_eq!(ConnectorType::Gmail.to_string(), "gmail");
        assert_eq!(ConnectorType::Calendar.to_string(), "calendar");
    }

    #[test]
    fn connector_type_badges() {
        assert_eq!(ConnectorType::WhatsApp.badge(), "WA");
        assert_eq!(ConnectorType::Slack.badge(), "SL");
        assert_eq!(ConnectorType::Gmail.badge(), "GM");
        assert_eq!(ConnectorType::Calendar.badge(), "CA");
    }

    #[test]
    fn conversation_kind_display() {
        assert_eq!(ConversationKind::Dm.to_string(), "dm");
        assert_eq!(ConversationKind::Group.to_string(), "group");
        assert_eq!(ConversationKind::Channel.to_string(), "channel");
        assert_eq!(ConversationKind::Thread.to_string(), "thread");
    }

    #[test]
    fn message_serialization_roundtrip() {
        let msg = Message {
            id: "m1".into(),
            conversation_id: "c1".into(),
            account_id: "a1".into(),
            connector: "slack".into(),
            external_id: "ext1".into(),
            sender: "user@example.com".into(),
            sender_name: Some("Alice".into()),
            body: Some("Hello world".into()),
            timestamp: 1_700_000_000,
            synced_at: Some(1_700_000_010),
            is_from_me: false,
            is_read: false,
            is_archived: false,
            reply_to_id: None,
            media_type: None,
            metadata: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "m1");
        assert_eq!(deserialized.body.as_deref(), Some("Hello world"));
    }

    #[test]
    fn calendar_event_serialization() {
        let event = CalendarEvent {
            id: "e1".into(),
            account_id: "cal1".into(),
            connector: "calendar".into(),
            external_id: "goog123".into(),
            title: "Standup".into(),
            description: None,
            location: None,
            start_at: 1_700_000_000,
            end_at: 1_700_001_800,
            all_day: false,
            attendees: Some(serde_json::json!(["alice@co.com"])),
            status: Some("confirmed".into()),
            calendar_name: Some("primary".into()),
            meet_link: Some("https://meet.google.com/abc-defg-hij".into()),
            metadata: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("meet.google.com"));
    }
}
