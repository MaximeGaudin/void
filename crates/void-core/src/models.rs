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
    /// Groups related messages (thread, email chain, time-proximity window). Stored in DB.
    pub context_id: Option<String>,
    /// Related messages sharing the same context_id. Populated at query time, never stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Vec<Message>>,
}

/// Remove messages that already appear in another message's context to avoid duplication.
/// For each context group, the most recent message in the top-level list is the anchor;
/// all other messages from that group are removed.
pub fn dedup_context_messages(messages: Vec<Message>) -> Vec<Message> {
    use std::collections::{HashMap, HashSet};

    let mut best_per_context: HashMap<String, (i64, String)> = HashMap::new();
    for msg in &messages {
        if let Some(ctx_id) = &msg.context_id {
            if msg.context.is_some() {
                let entry = best_per_context
                    .entry(ctx_id.clone())
                    .or_insert((0, String::new()));
                if msg.timestamp > entry.0 {
                    *entry = (msg.timestamp, msg.id.clone());
                }
            }
        }
    }

    if best_per_context.is_empty() {
        return messages;
    }

    let mut removable: HashSet<String> = HashSet::new();
    for msg in &messages {
        if let Some(ctx_id) = &msg.context_id {
            if let Some((_, anchor_id)) = best_per_context.get(ctx_id) {
                if msg.id != *anchor_id {
                    removable.insert(msg.id.clone());
                }
            }
        }
    }

    messages
        .into_iter()
        .filter(|m| !removable.contains(&m.id))
        .collect()
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
            context_id: None,
            context: None,
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

    fn make_msg_ts(id: &str, ts: i64, ctx_id: Option<&str>) -> Message {
        Message {
            id: id.into(),
            conversation_id: "c1".into(),
            account_id: "a1".into(),
            connector: "slack".into(),
            external_id: format!("ext-{id}"),
            sender: "user@test".into(),
            sender_name: None,
            body: Some(format!("body of {id}")),
            timestamp: ts,
            synced_at: None,
            is_from_me: false,
            is_read: false,
            is_archived: false,
            reply_to_id: None,
            media_type: None,
            metadata: None,
            context_id: ctx_id.map(|s| s.to_string()),
            context: None,
        }
    }

    fn make_msg(id: &str) -> Message {
        make_msg_ts(id, 1_000, None)
    }

    #[test]
    fn dedup_no_context_returns_all() {
        let messages = vec![make_msg("m1"), make_msg("m2"), make_msg("m3")];
        let result = dedup_context_messages(messages);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn dedup_removes_messages_shown_in_other_context() {
        let m1 = make_msg_ts("m1", 100, Some("ctx1"));
        let m2 = make_msg_ts("m2", 200, Some("ctx1"));
        let mut m3 = make_msg_ts("m3", 300, Some("ctx1"));
        m3.context = Some(vec![m1.clone(), m2.clone(), m3.clone()]);

        let mut m1_with_ctx = m1.clone();
        m1_with_ctx.context = Some(vec![m1.clone(), m2.clone(), m3.clone()]);
        let mut m2_with_ctx = m2.clone();
        m2_with_ctx.context = Some(vec![m1.clone(), m2.clone(), m3.clone()]);

        let messages = vec![m1_with_ctx, m2_with_ctx, m3];
        let result = dedup_context_messages(messages);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "m3");
    }

    #[test]
    fn dedup_keeps_anchor_even_if_in_own_context() {
        let m1 = make_msg_ts("m1", 100, Some("ctx1"));
        let mut m2 = make_msg_ts("m2", 200, Some("ctx1"));
        m2.context = Some(vec![m1.clone(), m2.clone()]);

        let mut m1_with_ctx = m1.clone();
        m1_with_ctx.context = Some(vec![m1.clone(), m2.clone()]);

        let messages = vec![m1_with_ctx, m2];
        let result = dedup_context_messages(messages);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "m2");
        assert!(result[0].context.is_some());
    }

    #[test]
    fn dedup_preserves_messages_without_context_overlap() {
        let m1 = make_msg_ts("m1", 100, Some("ctx1"));
        let mut m2 = make_msg_ts("m2", 200, Some("ctx1"));
        m2.context = Some(vec![m1.clone(), m2.clone()]);

        let mut m1_with_ctx = m1.clone();
        m1_with_ctx.context = Some(vec![m1.clone(), m2.clone()]);

        let standalone = make_msg_ts("m3", 300, None);

        let messages = vec![m1_with_ctx, m2, standalone];
        let result = dedup_context_messages(messages);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "m2");
        assert_eq!(result[1].id, "m3");
    }

    #[test]
    fn dedup_all_same_context_keeps_most_recent() {
        let m1 = make_msg_ts("m1", 100, Some("ctx1"));
        let m2 = make_msg_ts("m2", 200, Some("ctx1"));
        let m3 = make_msg_ts("m3", 300, Some("ctx1"));
        let ctx = vec![m1.clone(), m2.clone(), m3.clone()];

        let mut m1e = m1;
        m1e.context = Some(ctx.clone());
        let mut m2e = m2;
        m2e.context = Some(ctx.clone());
        let mut m3e = m3;
        m3e.context = Some(ctx);

        let messages = vec![m1e, m2e, m3e];
        let result = dedup_context_messages(messages);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "m3");
    }
}
