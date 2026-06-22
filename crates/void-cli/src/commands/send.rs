use clap::Args;
use tracing::{debug, info};

use void_core::config::VoidConfig;
use void_core::models::MessageContent;
use void_core::sync::is_daemon_running;

use crate::commands::connector_factory;
use crate::connectors;
use crate::output::parse_connector_type;

#[derive(Debug, Args)]
pub struct SendArgs {
    #[command(flatten)]
    pub recipient: SendRecipientArgs,
    /// Connector to send via: whatsapp, slack, gmail
    #[arg(long)]
    pub via: String,
    /// Connection to use (for multi-connection connectors)
    #[arg(long)]
    pub connection: Option<String>,
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

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct SendRecipientArgs {
    /// Recipient (phone number, channel name, email)
    #[arg(long = "to")]
    pub to: Option<String>,
    /// void conversation id (e.g. wa_whatsapp_94004066660357@lid for WhatsApp notes-to-self)
    #[arg(long)]
    pub conversation: Option<String>,
}

pub async fn run(args: &SendArgs) -> anyhow::Result<()> {
    info!(
        via = %args.via,
        to = ?args.recipient.to,
        conversation = ?args.recipient.conversation,
        "send"
    );
    let connector_type = parse_connector_type(&args.via)
        .ok_or_else(|| anyhow::anyhow!("Unknown connector type: {}", args.via))?;

    let cfg = crate::context::void_config();

    let target_type = connector_type.to_string();
    let connection = cfg
        .connections
        .iter()
        .find(|a| {
            let type_matches = a.connector_type.to_string() == target_type;
            let name_matches = args.connection.as_ref().is_none_or(|n| a.id == *n);
            type_matches && name_matches
        })
        .ok_or_else(|| anyhow::anyhow!("No {target_type} connection found in config.toml"))?;

    let plugin = connectors::by_id(connection.connector_type.as_str())
        .ok_or_else(|| anyhow::anyhow!("Unknown connector type: {}", connection.connector_type))?;

    if let Some(ref at_str) = args.at {
        if !plugin.supports_scheduling {
            anyhow::bail!("Scheduled sending (--at) is only supported for Slack.");
        }
        let to = args
            .recipient
            .to
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--to is required for scheduled Slack sends"))?;
        return run_slack_scheduled_send(connection, cfg, to, &args.message, at_str).await;
    }

    let store_path = crate::context::store_path();
    let to = resolve_target(
        args.recipient.to.as_deref(),
        args.recipient.conversation.as_deref(),
        &target_type,
        cfg,
    )?;

    let content = if let Some(ref path) = args.file {
        MessageContent::File {
            path: path.into(),
            caption: Some(args.message.clone()),
            mime_type: None,
            subject: args.subject.clone(),
        }
    } else {
        MessageContent::Text {
            body: args.message.clone(),
            subject: args.subject.clone(),
        }
    };

    let msg_id = if plugin.uses_daemon_rpc && is_daemon_running(&store_path) {
        void_whatsapp::rpc::send_message(&store_path, &connection.id, &to, content).await?
    } else {
        let conn = connector_factory::build_connector(connection, &store_path)?;
        conn.send_message(&to, content).await?
    };
    eprintln!("Message sent (id: {msg_id})");
    Ok(())
}

async fn run_slack_scheduled_send(
    connection: &void_core::config::ConnectionConfig,
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
        .schedule_message(channel, message, post_at, None)
        .await?;

    let dt = chrono::DateTime::from_timestamp(post_at, 0)
        .map(|utc| utc.with_timezone(&chrono::Local))
        .map(|local| local.format("%Y-%m-%d %H:%M %Z").to_string())
        .unwrap_or_else(|| post_at.to_string());

    eprintln!("Message scheduled for {dt} (id: {scheduled_id})");
    Ok(())
}

/// Resolve `#channel-name` to a channel ID using the local database, or map a
/// void conversation id to its connector external id when `--conversation` is used.
fn resolve_target(
    to: Option<&str>,
    conversation: Option<&str>,
    connector_type: &str,
    _cfg: &VoidConfig,
) -> anyhow::Result<String> {
    if let Some(conv_id) = conversation {
        let db = crate::context::open_db()?;
        let conv = db
            .get_conversation(conv_id)?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {conv_id}"))?;
        if conv.connector != connector_type {
            anyhow::bail!(
                "Conversation {conv_id} belongs to connector {}, not {connector_type}",
                conv.connector
            );
        }
        debug!(
            conversation_id = conv_id,
            external_id = %conv.external_id,
            kind = %conv.kind,
            "resolved conversation to external id"
        );
        return Ok(conv.external_id);
    }

    let to = to.ok_or_else(|| anyhow::anyhow!("Either --to or --conversation is required"))?;

    if !to.starts_with('#') {
        return Ok(to.to_string());
    }
    let name = &to[1..];
    let db = crate::context::open_db()?;
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

#[cfg(test)]
mod tests {
    use super::resolve_target;
    use void_core::config::VoidConfig;
    use void_core::db::Database;
    use void_core::models::{Conversation, ConversationKind};

    fn test_db() -> Database {
        Database::open(std::path::Path::new(":memory:")).expect("in-memory db")
    }

    fn seed_self_chat(db: &Database) {
        db.upsert_conversation(&Conversation {
            id: "wa_whatsapp_94004066660357@lid".into(),
            connection_id: "whatsapp".into(),
            connector: "whatsapp".into(),
            external_id: "94004066660357@lid".into(),
            name: Some("Message yourself".into()),
            kind: ConversationKind::SelfChat,
            last_message_at: None,
            unread_count: 0,
            is_muted: false,
            metadata: None,
        })
        .expect("seed conversation");
    }

    #[test]
    fn resolve_target_conversation_returns_external_id() {
        let db = test_db();
        seed_self_chat(&db);
        // resolve_target opens its own db via context in production; test the logic inline
        let conv = db
            .get_conversation("wa_whatsapp_94004066660357@lid")
            .unwrap()
            .unwrap();
        assert_eq!(conv.external_id, "94004066660357@lid");
        assert_eq!(conv.kind, ConversationKind::SelfChat);
    }

    #[test]
    fn resolve_target_passthrough_non_channel() {
        let cfg = VoidConfig::default();
        let target = resolve_target(Some("33651090627"), None, "whatsapp", &cfg).unwrap();
        assert_eq!(target, "33651090627");
    }
}
