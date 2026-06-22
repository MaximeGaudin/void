use clap::Args;
use tracing::{debug, info};

use void_core::models::MessageContent;
use void_core::sync::is_daemon_running;

use crate::commands::connector_factory;
use crate::connectors;
use crate::output::parse_connector_type;

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

    let msg = super::resolve::resolve_message(&db, &args.message_id)?;

    let conv = db
        .get_conversation(&msg.conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", msg.conversation_id))?;

    debug!("message and conversation found");

    let connection = cfg
        .find_connection_by_connector(&msg.connector)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No {} connection found in config.toml for message {}",
                msg.connector,
                msg.id
            )
        })?;

    let plugin = connectors::by_id(connection.connector_type.as_str())
        .ok_or_else(|| anyhow::anyhow!("Unknown connector type: {}", connection.connector_type))?;

    if let Some(ref at_str) = args.at {
        if !plugin.supports_scheduling {
            anyhow::bail!("Scheduled sending (--at) is only supported for Slack.");
        }
        return run_slack_scheduled_reply(
            connection,
            &conv.external_id,
            &msg.external_id,
            &args.message,
            at_str,
        )
        .await;
    }

    let connector_type = parse_connector_type(&connection.connector_type.to_string())
        .ok_or_else(|| anyhow::anyhow!("Unknown connector type: {}", connection.connector_type))?;

    let store_path = crate::context::store_path();
    let reply_id = connectors::build_reply_id(connector_type, &conv.external_id, &msg.external_id);

    let content = if let Some(ref path) = args.file {
        MessageContent::File {
            path: path.into(),
            caption: Some(args.message.clone()),
            mime_type: None,
            subject: None,
        }
    } else {
        MessageContent::from_text(args.message.clone())
    };

    let sent_id = if plugin.uses_daemon_rpc && is_daemon_running(&store_path) {
        void_whatsapp::rpc::reply_message(
            &store_path,
            &connection.id,
            &reply_id,
            content,
            args.in_thread,
        )
        .await?
    } else {
        let conn = connector_factory::build_connector(connection, &store_path)?;
        conn.reply(&reply_id, content, args.in_thread).await?
    };

    eprintln!("Reply sent (id: {sent_id})");
    Ok(())
}

async fn run_slack_scheduled_reply(
    connection: &void_core::config::ConnectionConfig,
    channel_id: &str,
    thread_ts: &str,
    message: &str,
    at_str: &str,
) -> anyhow::Result<()> {
    use super::slack::parse_schedule_time;

    let post_at = parse_schedule_time(at_str)?;
    let now = chrono::Utc::now().timestamp();
    if post_at <= now {
        anyhow::bail!("Scheduled time must be in the future.");
    }

    let user_token = void_core::config::settings_string(&connection.settings, "user_token")
        .ok_or_else(|| anyhow::anyhow!("missing user_token"))?;
    let app_token = void_core::config::settings_string(&connection.settings, "app_token")
        .ok_or_else(|| anyhow::anyhow!("missing app_token"))?;

    let connector = void_slack::connector::SlackConnector::new(
        &connection.id,
        &user_token,
        &app_token,
        None,
        None,
        std::env::temp_dir().as_path(),
        None,
    )?;

    let scheduled_id = connector
        .schedule_message(channel_id, message, post_at, Some(thread_ts))
        .await?;

    let dt = chrono::DateTime::from_timestamp(post_at, 0)
        .map(|utc| utc.with_timezone(&chrono::Local))
        .map(|local| local.format("%Y-%m-%d %H:%M %Z").to_string())
        .unwrap_or_else(|| post_at.to_string());

    eprintln!("Reply scheduled for {dt} (id: {scheduled_id})");
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
}
