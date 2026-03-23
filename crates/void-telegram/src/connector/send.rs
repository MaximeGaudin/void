use grammers_client::client::Client;
use grammers_client::message::InputMessage;
use grammers_client::peer::Peer;
use void_core::models::MessageContent;

fn text_for_message_content(content: &MessageContent) -> &str {
    match content {
        MessageContent::Text(text) => text.as_str(),
        MessageContent::File { caption, .. } => caption.as_deref().unwrap_or(""),
    }
}

pub(crate) fn build_input_message(content: &MessageContent) -> InputMessage {
    InputMessage::new().text(text_for_message_content(content))
}

pub(crate) fn parse_reply_id(message_id: &str) -> anyhow::Result<(String, String)> {
    let (conv, msg) = message_id
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid reply ID format: {message_id}"))?;
    Ok((conv.to_string(), msg.to_string()))
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
    fn parse_reply_id_valid() {
        let (conv, msg) = parse_reply_id("telegram_conn_1_-100123:telegram_conn_1_42").unwrap();
        assert_eq!(conv, "telegram_conn_1_-100123");
        assert_eq!(msg, "telegram_conn_1_42");
    }

    #[test]
    fn parse_reply_id_splits_on_first_colon_only() {
        let (a, b) = parse_reply_id("left:mid:right").unwrap();
        assert_eq!(a, "left");
        assert_eq!(b, "mid:right");
    }

    #[test]
    fn parse_reply_id_invalid_no_colon() {
        let err = parse_reply_id("no-separator-here").unwrap_err();
        assert!(
            err.to_string().contains("invalid reply ID format"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn text_for_message_content_text() {
        assert_eq!(
            text_for_message_content(&MessageContent::Text("hello".into())),
            "hello"
        );
    }

    #[test]
    fn text_for_message_content_file_with_caption() {
        let path = std::env::temp_dir().join("x.png");
        assert_eq!(
            text_for_message_content(&MessageContent::File {
                path,
                caption: Some("see this".into()),
                mime_type: None,
            }),
            "see this"
        );
    }

    #[test]
    fn text_for_message_content_file_without_caption() {
        let path = std::env::temp_dir().join("x.png");
        assert_eq!(
            text_for_message_content(&MessageContent::File {
                path,
                caption: None,
                mime_type: None,
            }),
            ""
        );
    }
}
