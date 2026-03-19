use void_core::models::{CalendarEvent, ConnectorType, Contact, Conversation, HealthStatus, Message};

pub struct OutputFormatter;

impl OutputFormatter {
    pub fn new() -> Self {
        Self
    }

    pub fn print_conversations(&self, conversations: &[Conversation]) -> anyhow::Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&json_wrap(conversations))?
        );
        Ok(())
    }

    pub fn print_messages(&self, messages: &[Message]) -> anyhow::Result<()> {
        println!("{}", serde_json::to_string_pretty(&json_wrap(messages))?);
        Ok(())
    }

    pub fn print_events(&self, events: &[CalendarEvent]) -> anyhow::Result<()> {
        println!("{}", serde_json::to_string_pretty(&json_wrap(events))?);
        Ok(())
    }

    pub fn print_contacts(&self, contacts: &[Contact]) -> anyhow::Result<()> {
        println!("{}", serde_json::to_string_pretty(&json_wrap(contacts))?);
        Ok(())
    }

    pub fn print_health(&self, statuses: &[HealthStatus]) -> anyhow::Result<()> {
        println!("{}", serde_json::to_string_pretty(&json_wrap(statuses))?);
        Ok(())
    }
}

fn json_wrap<T: serde::Serialize>(data: T) -> serde_json::Value {
    serde_json::json!({ "data": data, "error": null })
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

#[cfg(test)]
mod tests {
    use super::*;

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
