use grammers_client::client::Client;
use grammers_client::message::InputMessage;
use grammers_client::peer::Peer;
use void_core::models::MessageContent;

pub(crate) fn build_input_message(content: &MessageContent) -> InputMessage {
    match content {
        MessageContent::Text(text) => InputMessage::new().text(text),
        MessageContent::File { caption, .. } => {
            InputMessage::new().text(caption.as_deref().unwrap_or(""))
        }
    }
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
