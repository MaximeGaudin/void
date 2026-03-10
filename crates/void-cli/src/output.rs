use chrono::{DateTime, TimeZone, Utc};
use tabled::settings::Style;
use tabled::{Table, Tabled};
use void_core::models::{CalendarEvent, ChannelType, Conversation, HealthStatus, Message};

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
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt: DateTime<Utc>| dt.format("%Y-%m-%d %H:%M").to_string())
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
            channel: badge_from_account_id(&c.account_id),
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
            channel: badge_from_account_id(&m.account_id),
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

/// Derive a short channel badge from the account_id convention (e.g. "wa_..." -> "[WA]").
fn badge_from_account_id(account_id: &str) -> String {
    let id = account_id.to_lowercase();
    if id.starts_with("wa") || id.contains("whatsapp") {
        "[WA]".into()
    } else if id.contains("slack") || id.starts_with("sl") {
        "[SL]".into()
    } else if id.contains("gmail") || id.contains("email") || id.starts_with("gm") {
        "[GM]".into()
    } else if id.contains("calendar") || id.contains("cal") {
        "[CA]".into()
    } else {
        format!("[{}]", truncate(account_id, 4))
    }
}
