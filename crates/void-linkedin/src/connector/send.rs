use void_core::models::MessageContent;

pub(crate) fn text_for_message_content(content: &MessageContent) -> &str {
    match content {
        MessageContent::Text(text) => text.as_str(),
        MessageContent::File { caption, .. } => caption.as_deref().unwrap_or(""),
    }
}

pub(crate) fn file_path_for_message_content(content: &MessageContent) -> Option<&std::path::Path> {
    match content {
        MessageContent::File { path, .. } => Some(path.as_path()),
        _ => None,
    }
}

/// Parse reply ID: `{conv_external_id}:{msg_external_id}`.
pub(crate) fn parse_reply_id(message_id: &str) -> anyhow::Result<(String, String)> {
    let (conv, msg) = message_id
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid reply ID format: {message_id}"))?;
    Ok((conv.to_string(), msg.to_string()))
}

/// Extract Unipile chat id from void conversation external id.
pub(crate) fn chat_id_from_conv_external(
    connection_id: &str,
    conv_external_id: &str,
) -> anyhow::Result<String> {
    let prefix = format!("linkedin_{connection_id}_");
    conv_external_id
        .strip_prefix(&prefix)
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("invalid conversation external id: {conv_external_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reply_id_valid() {
        let (conv, msg) = parse_reply_id("linkedin_conn_1_chat123:linkedin_conn_1_msg456").unwrap();
        assert_eq!(conv, "linkedin_conn_1_chat123");
        assert_eq!(msg, "linkedin_conn_1_msg456");
    }

    #[test]
    fn chat_id_from_conv_external_strips_prefix() {
        let id = chat_id_from_conv_external("li", "linkedin_li_abc").unwrap();
        assert_eq!(id, "abc");
    }
}
