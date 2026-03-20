use clap::Args;
use tracing::{debug, info};

use void_core::config::{self, VoidConfig};
use void_core::db::Database;
use void_core::models::ConnectorType;
use void_core::models::MessageContent;

use crate::commands::connector_factory;
use crate::output::parse_connector_type;

#[derive(Debug, Args)]
pub struct ReplyArgs {
    /// Message ID to reply to
    pub message_id: String,
    /// Message text
    #[arg(long)]
    pub message: String,
    /// Reply in thread (Slack) or as quote (WhatsApp)
    #[arg(long)]
    pub in_thread: bool,
    /// Schedule for later — "HH:MM", "YYYY-MM-DD HH:MM", or Unix timestamp (Slack only)
    #[arg(long)]
    pub at: Option<String>,
}

pub async fn run(args: &ReplyArgs) -> anyhow::Result<()> {
    info!(message_id = %args.message_id, "reply");
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let db = Database::open(&cfg.db_path())?;

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

    if let Some(ref at_str) = args.at {
        if connection.connector_type != ConnectorType::Slack {
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

    let store_path = cfg.store_path();
    let conn = connector_factory::build_connector(connection, &store_path)?;

    let reply_id = build_reply_id(connector_type, &conv.external_id, &msg.external_id);

    let content = MessageContent::Text(args.message.clone());
    let sent_id = conn.reply(&reply_id, content, args.in_thread).await?;

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

    let (user_token, app_token, exclude_channels) = match &connection.settings {
        void_core::config::ConnectionSettings::Slack {
            user_token,
            app_token,
            exclude_channels,
        } => (
            user_token.clone(),
            app_token.clone(),
            exclude_channels.clone(),
        ),
        _ => anyhow::bail!("Mismatched settings for Slack connection"),
    };

    let connector = void_slack::connector::SlackConnector::new(
        &connection.id,
        &user_token,
        &app_token,
        exclude_channels,
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

fn build_reply_id(
    connector_type: void_core::models::ConnectorType,
    conv_external_id: &str,
    msg_external_id: &str,
) -> String {
    use void_core::models::ConnectorType;
    match connector_type {
        ConnectorType::WhatsApp => format!("{conv_external_id}:{msg_external_id}"),
        ConnectorType::Slack => format!("{conv_external_id}:{msg_external_id}"),
        ConnectorType::Telegram => format!("{conv_external_id}:{msg_external_id}"),
        ConnectorType::Gmail => msg_external_id.to_string(),
        ConnectorType::Calendar => msg_external_id.to_string(),
        ConnectorType::HackerNews => msg_external_id.to_string(),
    }
}
