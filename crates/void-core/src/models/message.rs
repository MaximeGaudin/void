use serde::{Deserialize, Serialize};

use super::serde_ts::{epoch_iso8601, epoch_iso8601_opt};

/// Parse a connector reply ID of the form `{conversation_external_id}:{message_external_id}`.
///
/// Connectors address a reply target as `conv:msg`; this splits on the first
/// `:` only, so external IDs that themselves contain `:` are preserved in the
/// message portion. Returns an error (including the offending input) when no
/// `:` is present.
pub fn parse_reply_id(message_id: &str) -> anyhow::Result<(String, String)> {
    let (conv, msg) = message_id.split_once(':').ok_or_else(|| {
        anyhow::anyhow!(
            "invalid reply ID format, expected 'conversation_id:message_id': {message_id}"
        )
    })?;
    Ok((conv.to_string(), msg.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub connection_id: String,
    pub connector: String,
    pub external_id: String,
    pub sender: String,
    pub sender_name: Option<String>,
    pub sender_avatar_url: Option<String>,
    pub body: Option<String>,
    /// When the message was originally sent (ISO 8601 in JSON, epoch seconds internally).
    #[serde(with = "epoch_iso8601")]
    pub timestamp: i64,
    /// When we first synced this message (ISO 8601 in JSON, epoch seconds internally).
    #[serde(with = "epoch_iso8601_opt")]
    pub synced_at: Option<i64>,
    pub is_archived: bool,
    #[serde(default)]
    pub is_saved: bool,
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
///
/// NOTE: This in-memory dedup is superseded by SQL-level context dedup in queries
/// (see `DEDUP_CONTEXT_CLAUSE` in `db::messages`). Retained for unit tests.
#[cfg(test)]
pub(crate) fn dedup_context_messages(messages: Vec<Message>) -> Vec<Message> {
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
