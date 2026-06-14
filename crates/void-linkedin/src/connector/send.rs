use void_core::models::MessageContent;

pub(crate) fn file_path_for_message_content(content: &MessageContent) -> Option<&std::path::Path> {
    match content {
        MessageContent::File { path, .. } => Some(path.as_path()),
        _ => None,
    }
}

pub(crate) fn is_post_conversation(conv_external_id: &str) -> bool {
    conv_external_id.contains("_post_")
}

/// Numeric post id from `linkedin_{connection_id}_post_{post_id}`.
pub(crate) fn post_id_from_conv_external(
    connection_id: &str,
    conv_external_id: &str,
) -> anyhow::Result<String> {
    let prefix = format!("linkedin_{connection_id}_post_");
    conv_external_id
        .strip_prefix(&prefix)
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("invalid post conversation external id: {conv_external_id}"))
}

/// Comment id from `linkedin_{connection_id}_comment_{comment_id}`.
pub(crate) fn comment_id_from_msg_external(
    connection_id: &str,
    msg_external_id: &str,
) -> anyhow::Result<String> {
    let prefix = format!("linkedin_{connection_id}_comment_");
    msg_external_id
        .strip_prefix(&prefix)
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("invalid comment message external id: {msg_external_id}"))
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
    fn chat_id_from_conv_external_strips_prefix() {
        let id = chat_id_from_conv_external("li", "linkedin_li_abc").unwrap();
        assert_eq!(id, "abc");
    }

    #[test]
    fn post_and_comment_external_id_parsing() {
        assert!(is_post_conversation("linkedin_li_post_123"));
        let post_id = post_id_from_conv_external("li", "linkedin_li_post_123").unwrap();
        assert_eq!(post_id, "123");
        let cid = comment_id_from_msg_external("li", "linkedin_li_comment_456").unwrap();
        assert_eq!(cid, "456");
    }
}
