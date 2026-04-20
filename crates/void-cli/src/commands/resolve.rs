use void_core::db::Database;
use void_core::links::SlackLink;
use void_core::models::Message;

/// Resolve a user-supplied identifier to a message.
///
/// Accepts:
/// - A Slack permalink URL (parsed and looked up by constructed ID)
/// - A void internal message ID (exact or suffix match via `get_message`)
pub fn resolve_message(db: &Database, input: &str) -> anyhow::Result<Message> {
    if let Some(link) = SlackLink::parse(input) {
        let id = link.to_message_id();
        return db.get_message(&id)?.ok_or_else(|| {
            anyhow::anyhow!("Message not found for Slack link (resolved id: {id})")
        });
    }

    db.get_message(input)?
        .ok_or_else(|| anyhow::anyhow!("Message not found: {input}"))
}

/// Pick the connection to use for a forwarded message: explicit `--connection`
/// flag wins, otherwise fall back to the message's original connection.
pub fn resolve_forward_connection<'a>(
    explicit: Option<&'a str>,
    message_connection: &'a str,
) -> &'a str {
    explicit.unwrap_or(message_connection)
}

/// Ensure a message belongs to the expected connector, or bail with a
/// descriptive error mentioning both the actual and expected connectors.
pub fn check_forward_connector(
    message_id: &str,
    actual: &str,
    expected: &str,
) -> anyhow::Result<()> {
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

/// Resolve a user-supplied identifier for the `messages` command.
///
/// If the input is a Slack link, returns `Link { message_id, conversation_id }`.
/// Otherwise, returns `ConversationId` for listing.
#[derive(Debug, PartialEq, Eq)]
pub enum MessagesTarget {
    Link {
        message_id: String,
        conversation_id: String,
    },
    ConversationId(String),
}

pub fn resolve_messages_target(input: &str) -> MessagesTarget {
    if let Some(link) = SlackLink::parse(input) {
        MessagesTarget::Link {
            message_id: link.to_message_id(),
            conversation_id: link.to_conversation_id(),
        }
    } else {
        MessagesTarget::ConversationId(input.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_connection_prefers_explicit() {
        assert_eq!(
            resolve_forward_connection(Some("explicit"), "msg-conn"),
            "explicit"
        );
    }

    #[test]
    fn forward_connection_falls_back_to_message() {
        assert_eq!(resolve_forward_connection(None, "msg-conn"), "msg-conn");
    }

    #[test]
    fn forward_connector_guard_accepts_match() {
        assert!(check_forward_connector("id1", "slack", "slack").is_ok());
    }

    #[test]
    fn forward_connector_guard_rejects_mismatch() {
        let err = check_forward_connector("id1", "gmail", "slack")
            .unwrap_err()
            .to_string();
        assert!(err.contains("gmail"));
        assert!(err.contains("slack"));
    }

    #[test]
    fn resolve_messages_target_slack_link() {
        let url = "https://gladiaio.slack.com/archives/D09R63ASNEL/p1773903727112369";
        match resolve_messages_target(url) {
            MessagesTarget::Link {
                message_id,
                conversation_id,
            } => {
                assert_eq!(message_id, "gladiaio-1773903727.112369");
                assert_eq!(conversation_id, "gladiaio-D09R63ASNEL");
            }
            MessagesTarget::ConversationId(_) => panic!("expected Link variant"),
        }
    }

    #[test]
    fn resolve_messages_target_plain_conversation_id() {
        assert_eq!(
            resolve_messages_target("slack-uuid-C123"),
            MessagesTarget::ConversationId("slack-uuid-C123".into())
        );
    }
}
