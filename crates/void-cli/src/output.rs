use chrono::{Local, TimeZone};
use tabled::settings::Style;
use tabled::{Table, Tabled};
use void_core::models::{CalendarEvent, ChannelType, Contact, Conversation, HealthStatus, Message};

pub struct OutputFormatter {
    json: bool,
}

impl OutputFormatter {
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    pub fn print_conversations(&self, conversations: &[Conversation]) -> anyhow::Result<()> {
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json_wrap(conversations))?
            );
            return Ok(());
        }

        if conversations.is_empty() {
            eprintln!("No conversations found.");
            return Ok(());
        }

        let rows: Vec<ConversationRow> = conversations.iter().map(ConversationRow::from).collect();
        let table = Table::new(rows).with(Style::rounded()).to_string();
        println!("{table}");
        Ok(())
    }

    pub fn print_messages(&self, messages: &[Message]) -> anyhow::Result<()> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(&json_wrap(messages))?);
            return Ok(());
        }

        if messages.is_empty() {
            eprintln!("No messages found.");
            return Ok(());
        }

        let rows: Vec<MessageRow> = messages.iter().map(MessageRow::from).collect();
        let table = Table::new(rows).with(Style::rounded()).to_string();
        println!("{table}");
        Ok(())
    }

    pub fn print_events(&self, events: &[CalendarEvent]) -> anyhow::Result<()> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(&json_wrap(events))?);
            return Ok(());
        }

        if events.is_empty() {
            eprintln!("No events found.");
            return Ok(());
        }

        let rows: Vec<EventRow> = events.iter().map(EventRow::from).collect();
        let table = Table::new(rows).with(Style::rounded()).to_string();
        println!("{table}");
        Ok(())
    }

    pub fn print_contacts(&self, contacts: &[Contact]) -> anyhow::Result<()> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(&json_wrap(contacts))?);
            return Ok(());
        }

        if contacts.is_empty() {
            eprintln!("No contacts found.");
            return Ok(());
        }

        let rows: Vec<ContactRow> = contacts.iter().map(ContactRow::from).collect();
        let table = Table::new(rows).with(Style::rounded()).to_string();
        println!("{table}");
        Ok(())
    }

    pub fn print_health(&self, statuses: &[HealthStatus]) -> anyhow::Result<()> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(&json_wrap(statuses))?);
            return Ok(());
        }

        let rows: Vec<HealthRow> = statuses.iter().map(HealthRow::from).collect();
        let table = Table::new(rows).with(Style::rounded()).to_string();
        println!("{table}");
        Ok(())
    }
}

fn json_wrap<T: serde::Serialize>(data: T) -> serde_json::Value {
    serde_json::json!({ "data": data, "error": null })
}

fn format_ts(ts: i64) -> String {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| ts.to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

// -- Table row structs --

#[derive(Tabled)]
struct ConversationRow {
    #[tabled(rename = "CH")]
    channel: String,
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Kind")]
    kind: String,
    #[tabled(rename = "Account")]
    account: String,
    #[tabled(rename = "Last Message")]
    last_message: String,
    #[tabled(rename = "Unread")]
    unread: i64,
}

impl From<&Conversation> for ConversationRow {
    fn from(c: &Conversation) -> Self {
        Self {
            channel: badge_from_connector(&c.connector),
            id: truncate(&c.id, 12),
            name: c.name.clone().unwrap_or_else(|| "-".into()),
            kind: c.kind.to_string(),
            account: c.account_id.clone(),
            last_message: c.last_message_at.map(format_ts).unwrap_or_default(),
            unread: c.unread_count,
        }
    }
}

#[derive(Tabled)]
struct MessageRow {
    #[tabled(rename = "CH")]
    channel: String,
    #[tabled(rename = "Time")]
    time: String,
    #[tabled(rename = "Sender")]
    sender: String,
    #[tabled(rename = "Message")]
    body: String,
    #[tabled(rename = "ID")]
    id: String,
}

impl From<&Message> for MessageRow {
    fn from(m: &Message) -> Self {
        Self {
            channel: badge_from_connector(&m.connector),
            time: format_ts(m.timestamp),
            sender: m
                .sender_name
                .clone()
                .unwrap_or_else(|| truncate(&m.sender, 20)),
            body: truncate(m.body.as_deref().unwrap_or(""), 60),
            id: truncate(&m.id, 12),
        }
    }
}

#[derive(Tabled)]
struct EventRow {
    #[tabled(rename = "Start")]
    start: String,
    #[tabled(rename = "End")]
    end: String,
    #[tabled(rename = "Title")]
    title: String,
    #[tabled(rename = "Location")]
    location: String,
    #[tabled(rename = "Meet")]
    meet: String,
}

impl From<&CalendarEvent> for EventRow {
    fn from(e: &CalendarEvent) -> Self {
        Self {
            start: format_ts(e.start_at),
            end: format_ts(e.end_at),
            title: truncate(&e.title, 40),
            location: e
                .location
                .as_deref()
                .map(|l| truncate(l, 30))
                .unwrap_or_default(),
            meet: e
                .meet_link
                .as_deref()
                .map(|l| truncate(l, 30))
                .unwrap_or_default(),
        }
    }
}

#[derive(Tabled)]
struct ContactRow {
    #[tabled(rename = "CH")]
    channel: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Address")]
    address: String,
    #[tabled(rename = "Account")]
    account: String,
    #[tabled(rename = "Messages")]
    message_count: i64,
    #[tabled(rename = "Last Active")]
    last_active: String,
}

impl From<&Contact> for ContactRow {
    fn from(c: &Contact) -> Self {
        Self {
            channel: badge_from_connector(&c.connector),
            name: c
                .sender_name
                .clone()
                .unwrap_or_else(|| truncate(&c.sender, 30)),
            address: truncate(&c.sender, 40),
            account: c.account_id.clone(),
            message_count: c.message_count,
            last_active: format_ts(c.last_message_at),
        }
    }
}

#[derive(Tabled)]
struct HealthRow {
    #[tabled(rename = "Account")]
    account: String,
    #[tabled(rename = "Type")]
    channel_type: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Message")]
    message: String,
}

impl From<&HealthStatus> for HealthRow {
    fn from(h: &HealthStatus) -> Self {
        Self {
            account: h.account_id.clone(),
            channel_type: format!("[{}]", h.channel_type.badge()),
            status: if h.ok { "OK".into() } else { "ERROR".into() },
            message: h.message.clone(),
        }
    }
}

pub fn parse_channel_type(s: &str) -> Option<ChannelType> {
    match s.to_lowercase().as_str() {
        "whatsapp" | "wa" => Some(ChannelType::WhatsApp),
        "slack" | "sl" => Some(ChannelType::Slack),
        "gmail" | "gm" | "email" => Some(ChannelType::Gmail),
        "calendar" | "cal" | "ca" => Some(ChannelType::Calendar),
        _ => None,
    }
}

fn badge_from_connector(connector: &str) -> String {
    match connector {
        "whatsapp" => "[WA]".into(),
        "slack" => "[SL]".into(),
        "gmail" => "[GM]".into(),
        "calendar" => "[CA]".into(),
        other => format!("[{}]", truncate(other, 4)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ts_uses_local_timezone() {
        let ts = 1_700_000_000i64; // 2023-11-14 21:13:20 UTC
        let formatted = format_ts(ts);
        let local_dt = Local.timestamp_opt(ts, 0).single().unwrap();
        let expected = local_dt.format("%Y-%m-%d %H:%M").to_string();
        assert_eq!(formatted, expected);
        assert!(formatted.starts_with("2023-11-1"));
    }

    #[test]
    fn format_ts_zero_epoch() {
        let formatted = format_ts(0);
        assert!(
            formatted.contains("1970-01-0") || formatted.contains("1969-12-31"),
            "epoch 0 should format to a 1970/1969 date, got: {formatted}"
        );
    }

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string_adds_ellipsis() {
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn badge_from_whatsapp() {
        assert_eq!(badge_from_connector("whatsapp"), "[WA]");
    }

    #[test]
    fn badge_from_slack() {
        assert_eq!(badge_from_connector("slack"), "[SL]");
    }

    #[test]
    fn badge_from_gmail() {
        assert_eq!(badge_from_connector("gmail"), "[GM]");
    }

    #[test]
    fn badge_from_calendar() {
        assert_eq!(badge_from_connector("calendar"), "[CA]");
    }

    #[test]
    fn badge_from_unknown() {
        let badge = badge_from_connector("custom");
        assert!(badge.starts_with('['));
        assert!(badge.ends_with(']'));
    }

    #[test]
    fn parse_channel_type_whatsapp() {
        assert_eq!(parse_channel_type("whatsapp"), Some(ChannelType::WhatsApp));
        assert_eq!(parse_channel_type("wa"), Some(ChannelType::WhatsApp));
        assert_eq!(parse_channel_type("WA"), Some(ChannelType::WhatsApp));
    }

    #[test]
    fn parse_channel_type_slack() {
        assert_eq!(parse_channel_type("slack"), Some(ChannelType::Slack));
        assert_eq!(parse_channel_type("sl"), Some(ChannelType::Slack));
    }

    #[test]
    fn parse_channel_type_gmail() {
        assert_eq!(parse_channel_type("gmail"), Some(ChannelType::Gmail));
        assert_eq!(parse_channel_type("gm"), Some(ChannelType::Gmail));
        assert_eq!(parse_channel_type("email"), Some(ChannelType::Gmail));
    }

    #[test]
    fn parse_channel_type_calendar() {
        assert_eq!(parse_channel_type("calendar"), Some(ChannelType::Calendar));
        assert_eq!(parse_channel_type("cal"), Some(ChannelType::Calendar));
        assert_eq!(parse_channel_type("ca"), Some(ChannelType::Calendar));
    }

    #[test]
    fn parse_channel_type_unknown_returns_none() {
        assert_eq!(parse_channel_type("unknown"), None);
        assert_eq!(parse_channel_type(""), None);
    }
}
