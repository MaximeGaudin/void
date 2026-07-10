use clap::Args;
use tracing::info;

use crate::service::writes::{self, ReplyParams};

#[derive(Debug, Args)]
pub struct ReplyArgs {
    /// Message ID to reply to
    pub message_id: String,
    /// Message text
    #[arg(long)]
    pub message: String,
    /// File to attach
    #[arg(long)]
    pub file: Option<String>,
    /// Reply in thread (Slack) or as quote (WhatsApp)
    #[arg(long)]
    pub in_thread: bool,
    /// Schedule for later — "HH:MM", "YYYY-MM-DD HH:MM", or Unix timestamp (Slack only)
    #[arg(long)]
    pub at: Option<String>,
}

pub async fn run(args: &ReplyArgs) -> anyhow::Result<()> {
    info!(message_id = %args.message_id, "reply");
    let cfg = crate::context::void_config();
    let db = crate::context::open_db()?;
    let store_path = crate::context::store_path();

    let params = ReplyParams {
        message_id: &args.message_id,
        message: &args.message,
        file: args.file.as_deref(),
        in_thread: args.in_thread,
        at: args.at.as_deref(),
    };

    let sent_id = writes::reply(&db, cfg, &store_path, params).await?;

    if args.at.is_some() {
        let at_str = args.at.as_deref().unwrap_or("");
        let post_at = crate::commands::slack::parse_schedule_time(at_str)?;
        let dt = chrono::DateTime::from_timestamp(post_at, 0)
            .map(|utc| utc.with_timezone(&chrono::Local))
            .map(|local| local.format("%Y-%m-%d %H:%M %Z").to_string())
            .unwrap_or_else(|| post_at.to_string());
        eprintln!("Reply scheduled for {dt} (id: {sent_id})");
    } else {
        eprintln!("Reply sent (id: {sent_id})");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use void_core::models::ConnectorType;

    use crate::connectors;

    #[test]
    fn build_reply_id_linkedin_joins_conv_and_message_external_ids() {
        let id = connectors::build_reply_id(
            ConnectorType::from_static("linkedin"),
            "linkedin_linkedin_chat-abc",
            "linkedin_linkedin_msg-xyz",
        );
        assert_eq!(id, "linkedin_linkedin_chat-abc:linkedin_linkedin_msg-xyz");
    }

    #[test]
    fn build_reply_id_whatsapp_joins_conv_and_message_external_ids() {
        let id = connectors::build_reply_id(
            ConnectorType::from_static("whatsapp"),
            "120363@g.us",
            "msg-abc",
        );
        assert_eq!(id, "120363@g.us:msg-abc");
    }

    #[test]
    fn build_reply_id_slack_joins_conv_and_message_external_ids() {
        let id = connectors::build_reply_id(
            ConnectorType::from_static("slack"),
            "C08UDH5JE57",
            "1776936528.857609",
        );
        assert_eq!(id, "C08UDH5JE57:1776936528.857609");
    }

    #[test]
    fn build_reply_id_telegram_joins_conv_and_message_external_ids() {
        let id =
            connectors::build_reply_id(ConnectorType::from_static("telegram"), "chat-42", "msg-99");
        assert_eq!(id, "chat-42:msg-99");
    }

    #[test]
    fn build_reply_id_gmail_uses_message_external_id_only() {
        let id = connectors::build_reply_id(
            ConnectorType::from_static("gmail"),
            "thread-ignored",
            "msg-rfc822-id",
        );
        assert_eq!(id, "msg-rfc822-id");
    }

    #[test]
    fn build_reply_id_hackernews_uses_message_external_id_only() {
        let id = connectors::build_reply_id(
            ConnectorType::from_static("hackernews"),
            "story-1",
            "comment-42",
        );
        assert_eq!(id, "comment-42");
    }

    #[test]
    fn build_reply_id_googlenews_uses_message_external_id_only() {
        let id = connectors::build_reply_id(
            ConnectorType::from_static("googlenews"),
            "feed",
            "article-7",
        );
        assert_eq!(id, "article-7");
    }

    #[test]
    fn build_reply_id_github_uses_message_external_id_only() {
        let id = connectors::build_reply_id(
            ConnectorType::from_static("github"),
            "github_github_owner_repo",
            "github_github_notification_1",
        );
        assert_eq!(id, "github_github_notification_1");
    }

    #[test]
    fn build_reply_id_reddit_joins_conv_and_message_external_ids() {
        let id = connectors::build_reply_id(
            ConnectorType::from_static("reddit"),
            "reddit_reddit_post_abc123",
            "reddit_reddit_comment_c1",
        );
        assert_eq!(id, "reddit_reddit_post_abc123:reddit_reddit_comment_c1");
    }
}
