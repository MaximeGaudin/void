//! Gmail CLI subcommands (search, thread, labels, drafts, attachments).

mod args;
mod handlers;

pub use args::*;

use tracing::debug;
use void_core::config::{self, expand_tilde, VoidConfig};
use void_core::db::Database;
use void_core::models::ConnectorType;

pub async fn run(args: &GmailArgs) -> anyhow::Result<()> {
    handlers::dispatch(args).await
}

/// Strip the void internal ID prefix from a Gmail message or thread ID.
///
/// Void stores IDs as `{connection_id}-{external_id}`, e.g.
/// `mgaudin@gladia.io-19c9ae5982d4b217`. Gmail IDs are pure hex and
/// never contain `@`, so the presence of `@` is an unambiguous indicator
/// that the void prefix must be stripped before passing the ID to the API.
fn strip_void_id_prefix(id: &str) -> &str {
    if let Some(at_pos) = id.find('@') {
        if let Some(dash_offset) = id[at_pos..].find('-') {
            return &id[at_pos + dash_offset + 1..];
        }
    }
    id
}

fn resolve_forward_connection<'a>(
    explicit: Option<&'a str>,
    message_connection: &'a str,
) -> &'a str {
    explicit.unwrap_or(message_connection)
}

fn resolve_forward_connector(
    message_id: &str,
    db: &Database,
    expected: &str,
) -> anyhow::Result<void_core::models::Message> {
    let msg = super::resolve::resolve_message(db, message_id)?;
    check_forward_connector(message_id, &msg.connector, expected)?;
    Ok(msg)
}

fn check_forward_connector(message_id: &str, actual: &str, expected: &str) -> anyhow::Result<()> {
    if actual != expected {
        anyhow::bail!(
            "Message {} is from connector '{}', not {}.",
            message_id,
            actual,
            expected
        );
    }
    Ok(())
}

fn build_gmail_connector(
    connection_filter: Option<&str>,
) -> anyhow::Result<void_gmail::connector::GmailConnector> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let connection = cfg
        .connections
        .iter()
        .find(|a| {
            let is_gmail = a.connector_type == ConnectorType::Gmail;
            let name_matches = connection_filter.map_or(true, |n| a.id == n);
            is_gmail && name_matches
        })
        .ok_or_else(|| {
            anyhow::anyhow!("No Gmail connection found in config. Run `void setup` to add one.")
        })?;

    let credentials_file = match &connection.settings {
        void_core::config::ConnectionSettings::Gmail { credentials_file } => {
            credentials_file.clone()
        }
        _ => anyhow::bail!(
            "Mismatched connection settings for Gmail connection '{}'",
            connection.id
        ),
    };

    let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
    let store_path = cfg.store_path();
    debug!(connection_id = %connection.id, "building Gmail connector for CLI");
    Ok(void_gmail::connector::GmailConnector::new(
        &connection.id,
        cred_path.as_deref().and_then(|p| p.to_str()),
        &store_path,
    ))
}

#[cfg(test)]
mod tests {
    use super::{check_forward_connector, resolve_forward_connection, strip_void_id_prefix};

    #[test]
    fn forward_connection_prefers_explicit_connection() {
        assert_eq!(
            resolve_forward_connection(Some("explicit-conn"), "msg-conn"),
            "explicit-conn"
        );
    }

    #[test]
    fn forward_connection_defaults_to_message_connection() {
        assert_eq!(resolve_forward_connection(None, "msg-conn"), "msg-conn");
    }

    #[test]
    fn forward_connector_guard_accepts_gmail() {
        assert!(check_forward_connector("id1", "gmail", "gmail").is_ok());
    }

    #[test]
    fn forward_connector_guard_rejects_non_gmail() {
        assert!(check_forward_connector("id1", "slack", "gmail").is_err());
    }

    #[test]
    fn forward_connector_guard_error_mentions_actual_connector() {
        let err = check_forward_connector("id1", "slack", "gmail")
            .unwrap_err()
            .to_string();
        assert!(err.contains("slack"), "error should mention 'slack': {err}");
        assert!(err.contains("gmail"), "error should mention 'gmail': {err}");
    }

    #[test]
    fn strip_void_prefix_removes_connection_prefix() {
        assert_eq!(
            strip_void_id_prefix("mgaudin@gladia.io-19c9ae5982d4b217"),
            "19c9ae5982d4b217"
        );
    }

    #[test]
    fn strip_void_prefix_handles_personal_email() {
        assert_eq!(
            strip_void_id_prefix("me@maxime.ly-abcdef1234567890"),
            "abcdef1234567890"
        );
    }

    #[test]
    fn strip_void_prefix_passthrough_raw_gmail_id() {
        assert_eq!(strip_void_id_prefix("19c9ae5982d4b217"), "19c9ae5982d4b217");
    }

    #[test]
    fn strip_void_prefix_passthrough_when_no_dash_after_at() {
        // Malformed input with @ but no dash — return as-is rather than panic.
        assert_eq!(strip_void_id_prefix("weird@nodash"), "weird@nodash");
    }

    #[test]
    fn strip_void_prefix_empty_string() {
        assert_eq!(strip_void_id_prefix(""), "");
    }
}
