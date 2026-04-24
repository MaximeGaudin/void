use void_core::db::Database;
use void_core::links::SlackLink;
use void_core::models::Message;

/// Resolve a user-supplied identifier to a message.
///
/// Accepts:
/// - A Slack permalink URL — resolved via `(channel_id, ts)` across Slack
///   connections, independent of the URL's workspace subdomain.
/// - A void internal message ID (exact match).
pub fn resolve_message(db: &Database, input: &str) -> anyhow::Result<Message> {
    if let Some(link) = SlackLink::parse(input) {
        return db
            .find_slack_message_by_link(&link.channel_id, &link.message_ts)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Message not found for Slack link (channel: {}, ts: {})",
                    link.channel_id,
                    link.message_ts
                )
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
/// If the input is a Slack link, resolves the message and its conversation
/// against the local DB, using the Slack-native `(channel_id, ts)` pair.
/// Otherwise, treats the input as a void conversation ID for listing.
#[derive(Debug, PartialEq, Eq)]
pub enum MessagesTarget {
    /// A specific Slack message pulled from a permalink.
    Link {
        message_id: String,
        conversation_id: String,
    },
    /// A conversation to list messages from.
    ConversationId(String),
    /// A Slack permalink that could not be resolved (e.g. not yet synced, or
    /// the channel doesn't exist locally). Callers decide whether to fall
    /// back (list the channel) or surface an error.
    UnresolvedSlackLink {
        channel_id: String,
        message_ts: String,
        /// The workspace subdomain, kept for diagnostics.
        workspace: String,
    },
}

pub fn resolve_messages_target(db: &Database, input: &str) -> anyhow::Result<MessagesTarget> {
    if let Some(link) = SlackLink::parse(input) {
        if let Some(msg) = db.find_slack_message_by_link(&link.channel_id, &link.message_ts)? {
            return Ok(MessagesTarget::Link {
                message_id: msg.id,
                conversation_id: msg.conversation_id,
            });
        }
        // Message not synced — see if we at least know the conversation so
        // the caller can fall back to listing it.
        return Ok(MessagesTarget::UnresolvedSlackLink {
            channel_id: link.channel_id,
            message_ts: link.message_ts,
            workspace: link.workspace,
        });
    }

    Ok(MessagesTarget::ConversationId(input.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use void_core::models::{Conversation, ConversationKind, Message};

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn insert_slack_message(
        db: &Database,
        connection_id: &str,
        channel_ext: &str,
        message_ts: &str,
        body: &str,
    ) -> (String, String) {
        let conv_id = format!("{connection_id}-{channel_ext}");
        let msg_id = format!("{connection_id}-{message_ts}");
        let conv = Conversation {
            id: conv_id.clone(),
            connection_id: connection_id.into(),
            connector: "slack".into(),
            external_id: channel_ext.into(),
            name: Some("channel".into()),
            kind: ConversationKind::Channel,
            last_message_at: Some(1),
            unread_count: 0,
            is_muted: false,
            metadata: None,
        };
        db.upsert_conversation(&conv).unwrap();
        let msg = Message {
            id: msg_id.clone(),
            conversation_id: conv_id.clone(),
            connection_id: connection_id.into(),
            connector: "slack".into(),
            external_id: message_ts.into(),
            sender: "U1".into(),
            sender_name: Some("Alice".into()),
            sender_avatar_url: None,
            body: Some(body.into()),
            timestamp: 1,
            synced_at: None,
            is_archived: false,
            reply_to_id: None,
            media_type: None,
            metadata: None,
            context_id: None,
            context: None,
        };
        db.upsert_message(&msg).unwrap();
        (conv_id, msg_id)
    }

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
    fn resolve_messages_target_slack_link_finds_message_across_connections() {
        // Real scenario: URL uses workspace `gladiaio` but the message is
        // stored under connection `slack`. The resolver must still find it.
        let db = test_db();
        let (conv_id, msg_id) =
            insert_slack_message(&db, "slack", "C08UDH5JE57", "1776936528.857609", "hi");

        let url = "https://gladiaio.slack.com/archives/C08UDH5JE57/p1776936528857609?thread_ts=1776932503.025469";
        let target = resolve_messages_target(&db, url).unwrap();

        match target {
            MessagesTarget::Link {
                message_id,
                conversation_id,
            } => {
                assert_eq!(message_id, msg_id);
                assert_eq!(conversation_id, conv_id);
            }
            other => panic!("expected Link, got {other:?}"),
        }
    }

    #[test]
    fn resolve_messages_target_returns_unresolved_when_not_synced() {
        let db = test_db();
        let url = "https://gladiaio.slack.com/archives/C08UDH5JE57/p1776936528857609";
        let target = resolve_messages_target(&db, url).unwrap();
        assert_eq!(
            target,
            MessagesTarget::UnresolvedSlackLink {
                channel_id: "C08UDH5JE57".into(),
                message_ts: "1776936528.857609".into(),
                workspace: "gladiaio".into(),
            }
        );
    }

    #[test]
    fn resolve_messages_target_plain_conversation_id() {
        let db = test_db();
        let target = resolve_messages_target(&db, "slack-uuid-C123").unwrap();
        assert_eq!(
            target,
            MessagesTarget::ConversationId("slack-uuid-C123".into())
        );
    }

    #[test]
    fn resolve_message_finds_slack_link_across_connections() {
        let db = test_db();
        let (_, msg_id) =
            insert_slack_message(&db, "slack", "C08UDH5JE57", "1776936528.857609", "hi");

        let url = "https://gladiaio.slack.com/archives/C08UDH5JE57/p1776936528857609";
        let msg = resolve_message(&db, url).unwrap();
        assert_eq!(msg.id, msg_id);
    }

    #[test]
    fn resolve_message_returns_error_when_slack_link_unresolved() {
        let db = test_db();
        let url = "https://gladiaio.slack.com/archives/C08UDH5JE57/p1776936528857609";
        let err = resolve_message(&db, url).unwrap_err().to_string();
        assert!(err.contains("C08UDH5JE57"));
        assert!(err.contains("1776936528.857609"));
    }
}
