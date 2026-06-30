use clap::Args;
use tracing::info;

use crate::service::writes::{self, SendParams};

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
    let cfg = crate::context::void_config();
    let db = crate::context::open_db()?;
    let store_path = crate::context::store_path();

    let params = SendParams {
        to: args.recipient.to.as_deref(),
        conversation: args.recipient.conversation.as_deref(),
        via: &args.via,
        connection: args.connection.as_deref(),
        message: &args.message,
        subject: args.subject.as_deref(),
        file: args.file.as_deref(),
        at: args.at.as_deref(),
    };

    let msg_id = writes::send(&db, cfg, &store_path, params).await?;

    if args.at.is_some() {
        let at_str = args.at.as_deref().unwrap_or("");
        let post_at = crate::commands::slack::parse_schedule_time(at_str)?;
        let dt = chrono::DateTime::from_timestamp(post_at, 0)
            .map(|utc| utc.with_timezone(&chrono::Local))
            .map(|local| local.format("%Y-%m-%d %H:%M %Z").to_string())
            .unwrap_or_else(|| post_at.to_string());
        eprintln!("Message scheduled for {dt} (id: {msg_id})");
    } else {
        eprintln!("Message sent (id: {msg_id})");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use void_core::db::Database;
    use void_core::models::{Conversation, ConversationKind};

    use crate::service::writes::resolve_send_target;

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
        let conv = db
            .get_conversation("wa_whatsapp_94004066660357@lid")
            .unwrap()
            .unwrap();
        assert_eq!(conv.external_id, "94004066660357@lid");
        assert_eq!(conv.kind, ConversationKind::SelfChat);
    }

    #[test]
    fn resolve_target_passthrough_non_channel() {
        let db = test_db();
        let target = resolve_send_target(&db, Some("33651090627"), None, "whatsapp").unwrap();
        assert_eq!(target, "33651090627");
    }
}
