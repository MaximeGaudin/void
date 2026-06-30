use void_core::models::{
    CalendarEvent, ConnectorType, Contact, Conversation, HealthStatus, Message,
};

use crate::connectors;

pub struct OutputFormatter;

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct PaginationMeta {
    pub current_page: i64,
    pub page_size: i64,
    pub total_elements: i64,
    pub total_pages: i64,
}

impl Default for OutputFormatter {
    fn default() -> Self {
        Self::new()
    }
}

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

    pub fn print_paginated<T: serde::Serialize>(
        &self,
        data: T,
        pagination: PaginationMeta,
    ) -> anyhow::Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&json_wrap_paginated(data, pagination))?
        );
        Ok(())
    }
}

pub(crate) fn json_wrap<T: serde::Serialize>(data: T) -> serde_json::Value {
    serde_json::json!({ "data": data, "error": null })
}

pub(crate) fn json_wrap_paginated<T: serde::Serialize>(
    data: T,
    pagination: PaginationMeta,
) -> serde_json::Value {
    serde_json::json!({ "data": data, "pagination": pagination, "error": null })
}

pub fn parse_connector_type(s: &str) -> Option<ConnectorType> {
    connectors::connector_type_from_alias(s)
}

pub fn known_connectors_csv() -> String {
    connectors::known_ids_csv()
}

/// Placeholder for clap `#[arg(help)]`; runtime `--help` is patched in [`patch_connector_arg_help`].
pub const CONNECTOR_FILTER_HELP: &str = "Filter by connector";

pub fn connector_filter_help() -> String {
    format!("Filter by connector ({})", known_connectors_csv())
}

/// Patch `--connector` flag help on all subcommands to list registered connector ids.
pub fn patch_connector_arg_help(cmd: &mut clap::Command) {
    let help = connector_filter_help();
    patch_connector_arg_help_recursive(cmd, help);
}

fn patch_connector_arg_help_recursive(cmd: &mut clap::Command, help: String) {
    if cmd.get_arguments().any(|a| a.get_id() == "connector") {
        let help = help.clone();
        *cmd = cmd.clone().mut_arg("connector", move |arg| arg.help(help));
    }
    for sub in cmd.get_subcommands_mut() {
        patch_connector_arg_help_recursive(sub, help.clone());
    }
}

pub fn resolve_connector_filter(raw: Option<&str>) -> anyhow::Result<Option<String>> {
    match raw {
        None => Ok(None),
        Some(s) => {
            let ct = parse_connector_type(s).ok_or_else(|| {
                anyhow::anyhow!(
                    "Unknown connector \"{s}\". Valid connectors: {}",
                    known_connectors_csv()
                )
            })?;
            Ok(Some(ct.to_string()))
        }
    }
}

pub fn resolve_connector_list(raw: Option<&str>) -> anyhow::Result<Option<Vec<String>>> {
    match raw {
        None => Ok(None),
        Some(s) => {
            let mut resolved = Vec::new();
            for part in s.split(',') {
                let trimmed = part.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let ct = parse_connector_type(trimmed).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Unknown connector \"{trimmed}\". Valid connectors: {}",
                        known_connectors_csv()
                    )
                })?;
                resolved.push(ct.to_string());
            }
            if resolved.is_empty() {
                Ok(None)
            } else {
                Ok(Some(resolved))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_connector_type_whatsapp() {
        assert_eq!(
            parse_connector_type("whatsapp"),
            Some(ConnectorType::from_static("whatsapp"))
        );
        assert_eq!(
            parse_connector_type("wa"),
            Some(ConnectorType::from_static("whatsapp"))
        );
        assert_eq!(
            parse_connector_type("WA"),
            Some(ConnectorType::from_static("whatsapp"))
        );
    }

    #[test]
    fn parse_connector_type_slack() {
        assert_eq!(
            parse_connector_type("slack"),
            Some(ConnectorType::from_static("slack"))
        );
        assert_eq!(
            parse_connector_type("sl"),
            Some(ConnectorType::from_static("slack"))
        );
    }

    #[test]
    fn parse_connector_type_gmail() {
        assert_eq!(
            parse_connector_type("gmail"),
            Some(ConnectorType::from_static("gmail"))
        );
        assert_eq!(
            parse_connector_type("gm"),
            Some(ConnectorType::from_static("gmail"))
        );
        assert_eq!(
            parse_connector_type("email"),
            Some(ConnectorType::from_static("gmail"))
        );
    }

    #[test]
    fn parse_connector_type_calendar() {
        assert_eq!(
            parse_connector_type("calendar"),
            Some(ConnectorType::from_static("calendar"))
        );
        assert_eq!(
            parse_connector_type("cal"),
            Some(ConnectorType::from_static("calendar"))
        );
        assert_eq!(
            parse_connector_type("ca"),
            Some(ConnectorType::from_static("calendar"))
        );
    }

    #[test]
    fn parse_connector_type_linkedin() {
        assert_eq!(
            parse_connector_type("linkedin"),
            Some(ConnectorType::from_static("linkedin"))
        );
        assert_eq!(
            parse_connector_type("li"),
            Some(ConnectorType::from_static("linkedin"))
        );
        assert_eq!(
            parse_connector_type("LI"),
            Some(ConnectorType::from_static("linkedin"))
        );
    }

    #[test]
    fn parse_connector_type_github() {
        assert_eq!(
            parse_connector_type("github"),
            Some(ConnectorType::from_static("github"))
        );
        assert_eq!(
            parse_connector_type("gh"),
            Some(ConnectorType::from_static("github"))
        );
        assert_eq!(
            parse_connector_type("GH"),
            Some(ConnectorType::from_static("github"))
        );
    }

    #[test]
    fn parse_connector_type_googlenews() {
        assert_eq!(
            parse_connector_type("googlenews"),
            Some(ConnectorType::from_static("googlenews"))
        );
        assert_eq!(
            parse_connector_type("gn"),
            Some(ConnectorType::from_static("googlenews"))
        );
    }

    #[test]
    fn connector_filter_help_lists_all_plugins() {
        let help = connector_filter_help();
        for p in crate::connectors::all() {
            assert!(help.contains(p.id), "help missing plugin id {}", p.id);
        }
    }

    #[test]
    fn parse_connector_type_unknown_returns_none() {
        assert_eq!(parse_connector_type("unknown"), None);
        assert_eq!(parse_connector_type(""), None);
    }

    #[test]
    fn parse_connector_type_telegram() {
        assert_eq!(
            parse_connector_type("telegram"),
            Some(ConnectorType::from_static("telegram"))
        );
        assert_eq!(
            parse_connector_type("tg"),
            Some(ConnectorType::from_static("telegram"))
        );
    }

    #[test]
    fn parse_connector_type_hackernews() {
        assert_eq!(
            parse_connector_type("hackernews"),
            Some(ConnectorType::from_static("hackernews"))
        );
        assert_eq!(
            parse_connector_type("hn"),
            Some(ConnectorType::from_static("hackernews"))
        );
    }

    #[test]
    fn resolve_connector_filter_none_returns_ok_none() {
        assert_eq!(resolve_connector_filter(None).unwrap(), None);
    }

    #[test]
    fn resolve_connector_filter_valid_connector() {
        assert_eq!(
            resolve_connector_filter(Some("slack")).unwrap(),
            Some("slack".to_string())
        );
        assert_eq!(
            resolve_connector_filter(Some("GM")).unwrap(),
            Some("gmail".to_string())
        );
    }

    #[test]
    fn resolve_connector_filter_unknown_returns_err() {
        let err = resolve_connector_filter(Some("twitter")).unwrap_err();
        assert!(err.to_string().contains("Unknown connector"));
    }

    #[test]
    fn resolve_connector_list_none_returns_ok_none() {
        assert_eq!(resolve_connector_list(None).unwrap(), None);
    }

    #[test]
    fn resolve_connector_list_comma_separated() {
        assert_eq!(
            resolve_connector_list(Some("slack, gmail, wa")).unwrap(),
            Some(vec![
                "slack".to_string(),
                "gmail".to_string(),
                "whatsapp".to_string(),
            ])
        );
    }

    #[test]
    fn resolve_connector_list_empty_parts_skipped() {
        assert_eq!(
            resolve_connector_list(Some("slack,,gmail")).unwrap(),
            Some(vec!["slack".to_string(), "gmail".to_string()])
        );
    }

    #[test]
    fn resolve_connector_list_all_empty_returns_none() {
        assert_eq!(resolve_connector_list(Some(" , ")).unwrap(), None);
    }

    #[test]
    fn resolve_connector_list_unknown_connector_err() {
        let err = resolve_connector_list(Some("slack,unknown")).unwrap_err();
        assert!(err.to_string().contains("Unknown connector"));
    }
}
