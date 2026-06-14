use grammers_client::client::Client;
use grammers_client::message::InputMessage;
use grammers_client::peer::Peer;
use void_core::models::MessageContent;

pub(crate) fn build_input_message(content: &MessageContent) -> InputMessage {
    InputMessage::new().text(content.text())
}

/// Strip the `telegram_<connection_id>_` prefix from an external ID, returning
/// the bare numeric portion. If the prefix is absent, the input is returned
/// unchanged (callers then attempt to parse it directly).
pub(crate) fn strip_telegram_prefix<'a>(raw: &'a str, connection_id: &str) -> &'a str {
    let prefix = format!("telegram_{connection_id}_");
    raw.strip_prefix(&prefix).unwrap_or(raw)
}

/// Resolve a user-provided recipient string to a Telegram peer.
/// Accepts: @username, username, phone number, or numeric chat ID.
pub(crate) async fn resolve_peer(client: &Client, input: &str) -> anyhow::Result<Peer> {
    let input = input.trim();

    if let Ok(id) = input.parse::<i64>() {
        let results = client.search_peer(&id.to_string(), 1).await?;
        if let Some(item) = results.into_iter().next() {
            return Ok(item.into_peer());
        }
        anyhow::bail!("could not resolve numeric peer ID: {input}");
    }

    let username = input.strip_prefix('@').unwrap_or(input);
    match client.resolve_username(username).await {
        Ok(Some(peer)) => return Ok(peer),
        Ok(None) => {}
        Err(e) => {
            tracing::debug!(input, error = %e, "resolve_username failed, falling back to search");
        }
    }

    let results = client.search_peer(input, 5).await?;
    if let Some(item) = results.into_iter().next() {
        return Ok(item.into_peer());
    }

    anyhow::bail!("could not resolve peer: {input}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_telegram_prefix_removes_matching_prefix() {
        assert_eq!(strip_telegram_prefix("telegram_conn1_42", "conn1"), "42");
        assert_eq!(
            strip_telegram_prefix("telegram_conn1_-100987654321", "conn1"),
            "-100987654321"
        );
    }

    #[test]
    fn strip_telegram_prefix_passthrough_when_absent() {
        // Raw numeric IDs with no prefix are returned unchanged.
        assert_eq!(strip_telegram_prefix("42", "conn1"), "42");
        assert_eq!(strip_telegram_prefix("-100123", "conn1"), "-100123");
    }

    #[test]
    fn strip_telegram_prefix_wrong_connection_id_not_stripped() {
        // Prefix for a different connection id must not be stripped.
        assert_eq!(
            strip_telegram_prefix("telegram_other_42", "conn1"),
            "telegram_other_42"
        );
    }

    #[test]
    fn strip_telegram_prefix_roundtrip_with_sync_format() {
        // Mirrors the external_id format built in sync.rs:
        // format!("telegram_{connection_id}_{msg_id}")
        let connection_id = "abc";
        let msg_id: i32 = 12345;
        let external_id = format!("telegram_{connection_id}_{msg_id}");
        let stripped = strip_telegram_prefix(&external_id, connection_id);
        assert_eq!(stripped.parse::<i32>().unwrap(), msg_id);

        let chat_id: i64 = -1001234567890;
        let conv_external_id = format!("telegram_{connection_id}_{chat_id}");
        let stripped_chat = strip_telegram_prefix(&conv_external_id, connection_id);
        assert_eq!(stripped_chat.parse::<i64>().unwrap(), chat_id);
    }

    #[test]
    fn strip_telegram_prefix_only_strips_once() {
        // Only the leading prefix is removed; an embedded repeat stays.
        assert_eq!(
            strip_telegram_prefix("telegram_c_telegram_c_5", "c"),
            "telegram_c_5"
        );
    }
}
