use clap::Args;
use tracing::{debug, info};

use void_core::config::{self, AccountType, VoidConfig};
use void_core::db::Database;
use void_core::models::MessageContent;

use crate::commands::connector_factory;
use crate::output::parse_connector_type;

#[derive(Debug, Args)]
pub struct SendArgs {
    /// Recipient (phone number, channel name, email)
    #[arg(long)]
    pub to: String,
    /// Connector to send via: whatsapp, slack, gmail
    #[arg(long)]
    pub via: String,
    /// Account to use (for multi-account connectors)
    #[arg(long)]
    pub account: Option<String>,
    /// Message text
    #[arg(long)]
    pub message: String,
    /// Email subject (gmail only)
    #[arg(long)]
    pub subject: Option<String>,
    /// File to attach
    #[arg(long)]
    pub file: Option<String>,
    /// Schedule for later — "HH:MM", "YYYY-MM-DD HH:MM", or Unix timestamp (Slack only)
    #[arg(long)]
    pub at: Option<String>,
}

pub async fn run(args: &SendArgs) -> anyhow::Result<()> {
    info!(via = %args.via, to = %args.to, "send");
    let connector_type = parse_connector_type(&args.via)
        .ok_or_else(|| anyhow::anyhow!("Unknown connector type: {}", args.via))?;

    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let target_type = connector_type.to_string();
    let account = cfg
        .accounts
        .iter()
        .find(|a| {
            let type_matches = a.account_type.to_string() == target_type;
            let name_matches = args.account.as_ref().map_or(true, |n| a.id == *n);
            type_matches && name_matches
        })
        .ok_or_else(|| anyhow::anyhow!("No {target_type} account found in config.toml"))?;

    if let Some(ref at_str) = args.at {
        if account.account_type != AccountType::Slack {
            anyhow::bail!("Scheduled sending (--at) is only supported for Slack.");
        }
        return run_slack_scheduled_send(account, &cfg, &args.to, &args.message, at_str).await;
    }

    let store_path = cfg.store_path();
    let conn = connector_factory::build_connector(account, &store_path)?;
    debug!("connector built");

    let to = resolve_target(&args.to, &target_type, &cfg)?;

    let content = if let Some(ref path) = args.file {
        MessageContent::File {
            path: path.into(),
            caption: Some(args.message.clone()),
            mime_type: None,
        }
    } else {
        MessageContent::Text(args.message.clone())
    };

    let msg_id = conn.send_message(&to, content).await?;
    eprintln!("Message sent (id: {msg_id})");
    Ok(())
}

async fn run_slack_scheduled_send(
    account: &void_core::config::AccountConfig,
    _cfg: &VoidConfig,
    channel: &str,
    message: &str,
    at_str: &str,
) -> anyhow::Result<()> {
    use super::slack::parse_schedule_time;

    let post_at = parse_schedule_time(at_str)?;
    let now = chrono::Utc::now().timestamp();
    if post_at <= now {
        anyhow::bail!("Scheduled time must be in the future.");
    }

    let (user_token, app_token, exclude_channels) = match &account.settings {
        void_core::config::AccountSettings::Slack {
            user_token,
            app_token,
            exclude_channels,
        } => (
            user_token.clone(),
            app_token.clone(),
            exclude_channels.clone(),
        ),
        _ => anyhow::bail!("Mismatched settings for Slack account"),
    };

    let connector = void_slack::connector::SlackConnector::new(
        &account.id,
        &user_token,
        &app_token,
        exclude_channels,
    );

    let scheduled_id = connector
        .schedule_message(channel, message, post_at, None)
        .await?;

    let dt = chrono::DateTime::from_timestamp(post_at, 0)
        .map(|utc| utc.with_timezone(&chrono::Local))
        .map(|local| local.format("%Y-%m-%d %H:%M %Z").to_string())
        .unwrap_or_else(|| post_at.to_string());

    eprintln!("Message scheduled for {dt} (id: {scheduled_id})");
    Ok(())
}

/// Resolve `#channel-name` to a channel ID using the local database.
/// Returns the original value if not a `#name` target or not found (the
/// connector will handle the final resolution via the Slack API).
fn resolve_target(to: &str, connector_type: &str, cfg: &VoidConfig) -> anyhow::Result<String> {
    if !to.starts_with('#') {
        return Ok(to.to_string());
    }
    let name = &to[1..];
    let db = Database::open(&cfg.db_path())?;
    if let Some(conv) = db.find_conversation_by_name(name, connector_type)? {
        debug!(name, external_id = %conv.external_id, "resolved channel name to ID from DB");
        Ok(conv.external_id)
    } else {
        debug!(
            name,
            "channel not in local DB, passing through for API resolution"
        );
        Ok(to.to_string())
    }
}
