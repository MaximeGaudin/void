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
        return db
            .get_message(&id)?
            .ok_or_else(|| anyhow::anyhow!("Message not found for Slack link (resolved id: {id})"));
    }

    db.get_message(input)?
        .ok_or_else(|| anyhow::anyhow!("Message not found: {input}"))
}

/// Resolve a user-supplied identifier for the `messages` command.
///
/// If the input is a Slack link, returns `Link { message_id, conversation_id }`.
/// Otherwise, returns `ConversationId` for listing.
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
