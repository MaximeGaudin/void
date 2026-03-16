use chrono::{Local, TimeZone};
use tabled::settings::Style;
use tabled::{Table, Tabled};
use void_core::models::{
    CalendarEvent, ConnectorType, Contact, Conversation, HealthStatus, Message,
};

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

        let has_context = messages.iter().any(|m| m.context.is_some());
        if has_context {
            print_messages_with_context(messages);
        } else {
            let rows: Vec<MessageRow> = messages.iter().map(MessageRow::from).collect();
            let table = Table::new(rows).with(Style::rounded()).to_string();
            println!("{table}");
        }
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

fn format_time_short(ts: i64) -> String {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_else(|| ts.to_string())
}

fn print_messages_with_context(messages: &[Message]) {
    const CH_W: usize = 4;
    const TIME_W: usize = 16;
    const SENDER_W: usize = 14;
    const MSG_W: usize = 52;
    const ID_W: usize = 14;

    let sep = format!(
        "├{:─<w1$}┼{:─<w2$}┼{:─<w3$}┼{:─<w4$}┼{:─<w5$}┤",
        "",
        "",
        "",
        "",
        "",
        w1 = CH_W + 2,
        w2 = TIME_W + 2,
        w3 = SENDER_W + 2,
        w4 = MSG_W + 2,
        w5 = ID_W + 2,
    );
    let top = format!(
        "┌{:─<w1$}┬{:─<w2$}┬{:─<w3$}┬{:─<w4$}┬{:─<w5$}┐",
        "",
        "",
        "",
        "",
        "",
        w1 = CH_W + 2,
        w2 = TIME_W + 2,
        w3 = SENDER_W + 2,
        w4 = MSG_W + 2,
        w5 = ID_W + 2,
    );
    let bottom = format!(
        "└{:─<w1$}┴{:─<w2$}┴{:─<w3$}┴{:─<w4$}┴{:─<w5$}┘",
        "",
        "",
        "",
        "",
        "",
        w1 = CH_W + 2,
        w2 = TIME_W + 2,
        w3 = SENDER_W + 2,
        w4 = MSG_W + 2,
        w5 = ID_W + 2,
    );
    let header = format!(
        "│ {:<CH_W$} │ {:<TIME_W$} │ {:<SENDER_W$} │ {:<MSG_W$} │ {:<ID_W$} │",
        "CH", "Time", "Sender", "Message", "ID"
    );

    println!("{top}");
    println!("{header}");

    for msg in messages {
        println!("{sep}");

        let badge = badge_from_connector(&msg.connector);
        let time = format_ts(msg.timestamp);
        let sender = msg
            .sender_name
            .clone()
            .unwrap_or_else(|| truncate(&msg.sender, SENDER_W));
        let body = truncate(msg.body.as_deref().unwrap_or(""), MSG_W);
        let id = truncate(&msg.id, ID_W);

        println!(
            "│ {:<CH_W$} │ {:<TIME_W$} │ {:<SENDER_W$} │ {:<MSG_W$} │ {:<ID_W$} │",
            badge,
            time,
            truncate(&sender, SENDER_W),
            body,
            id,
        );

        if let Some(ctx) = &msg.context {
            for ctx_msg in ctx {
                let marker = if ctx_msg.id == msg.id { " *" } else { "" };
                let ctx_time = format_time_short(ctx_msg.timestamp);
                let ctx_sender = ctx_msg
                    .sender_name
                    .clone()
                    .unwrap_or_else(|| truncate(&ctx_msg.sender, SENDER_W));
                let body_text = ctx_msg.body.as_deref().unwrap_or("");
                let available_msg_w = MSG_W.saturating_sub(marker.len() + 2);
                let ctx_body = format!("  {}{}", truncate(body_text, available_msg_w), marker);

                println!(
                    "│ {:<CH_W$} │   {:<w$} │ {:<SENDER_W$} │ {:<MSG_W$} │ {:<ID_W$} │",
                    "",
                    ctx_time,
                    truncate(&ctx_sender, SENDER_W),
                    ctx_body,
                    "",
                    w = TIME_W - 2,
                );
            }
        }
    }

    println!("{bottom}");
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
        let end = max.saturating_sub(3);
        let boundary = s
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= end)
            .last()
            .unwrap_or(0);
        format!("{}...", &s[..boundary])
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
    #[tabled(rename = "Event ID")]
    event_id: String,
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
            event_id: truncate(&e.external_id, 30),
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
    connector_type: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Message")]
    message: String,
}

impl From<&HealthStatus> for HealthRow {
    fn from(h: &HealthStatus) -> Self {
        Self {
            account: h.account_id.clone(),
            connector_type: format!("[{}]", h.connector_type.badge()),
            status: if h.ok { "OK".into() } else { "ERROR".into() },
            message: h.message.clone(),
        }
    }
}

pub fn parse_connector_type(s: &str) -> Option<ConnectorType> {
    match s.to_lowercase().as_str() {
        "whatsapp" | "wa" => Some(ConnectorType::WhatsApp),
        "slack" | "sl" => Some(ConnectorType::Slack),
        "gmail" | "gm" | "email" => Some(ConnectorType::Gmail),
        "calendar" | "cal" | "ca" => Some(ConnectorType::Calendar),
        "telegram" | "tg" => Some(ConnectorType::Telegram),
        _ => None,
    }
}

fn badge_from_connector(connector: &str) -> String {
    match connector {
        "whatsapp" => "[WA]".into(),
        "slack" => "[SL]".into(),
        "gmail" => "[GM]".into(),
        "calendar" => "[CA]".into(),
        "telegram" => "[TG]".into(),
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
    fn parse_connector_type_whatsapp() {
        assert_eq!(
            parse_connector_type("whatsapp"),
            Some(ConnectorType::WhatsApp)
        );
        assert_eq!(parse_connector_type("wa"), Some(ConnectorType::WhatsApp));
        assert_eq!(parse_connector_type("WA"), Some(ConnectorType::WhatsApp));
    }

    #[test]
    fn parse_connector_type_slack() {
        assert_eq!(parse_connector_type("slack"), Some(ConnectorType::Slack));
        assert_eq!(parse_connector_type("sl"), Some(ConnectorType::Slack));
    }

    #[test]
    fn parse_connector_type_gmail() {
        assert_eq!(parse_connector_type("gmail"), Some(ConnectorType::Gmail));
        assert_eq!(parse_connector_type("gm"), Some(ConnectorType::Gmail));
        assert_eq!(parse_connector_type("email"), Some(ConnectorType::Gmail));
    }

    #[test]
    fn parse_connector_type_calendar() {
        assert_eq!(
            parse_connector_type("calendar"),
            Some(ConnectorType::Calendar)
        );
        assert_eq!(parse_connector_type("cal"), Some(ConnectorType::Calendar));
        assert_eq!(parse_connector_type("ca"), Some(ConnectorType::Calendar));
    }

    #[test]
    fn parse_connector_type_unknown_returns_none() {
        assert_eq!(parse_connector_type("unknown"), None);
        assert_eq!(parse_connector_type(""), None);
    }
}
