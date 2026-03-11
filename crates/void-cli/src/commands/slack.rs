use clap::{Args, Subcommand};
use tracing::debug;
use void_core::config::{self, AccountType, VoidConfig};
use void_core::db::Database;

#[derive(Debug, Args)]
pub struct SlackArgs {
    #[command(subcommand)]
    pub command: SlackCommand,
}

#[derive(Debug, Subcommand)]
pub enum SlackCommand {
    /// Add an emoji reaction to a message
    React(ReactArgs),
    /// Edit an existing message
    Edit(EditArgs),
}

#[derive(Debug, Args)]
pub struct ReactArgs {
    /// Message ID (void internal ID)
    pub message_id: String,
    /// Emoji name (without colons, e.g. "thumbsup", "eyes", "white_check_mark")
    #[arg(long)]
    pub emoji: String,
    /// Slack account to use
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Debug, Args)]
pub struct EditArgs {
    /// Message ID (void internal ID)
    pub message_id: String,
    /// New message text
    #[arg(long)]
    pub message: String,
    /// Slack account to use
    #[arg(long)]
    pub account: Option<String>,
}

pub async fn run(args: &SlackArgs, _json: bool) -> anyhow::Result<()> {
    match &args.command {
        SlackCommand::React(a) => run_react(a).await,
        SlackCommand::Edit(a) => run_edit(a).await,
    }
}

async fn run_react(args: &ReactArgs) -> anyhow::Result<()> {
    let cfg = load_config()?;
    let db = Database::open(&cfg.db_path())?;

    let msg = db
        .get_message(&args.message_id)?
        .ok_or_else(|| anyhow::anyhow!("Message not found: {}", args.message_id))?;

    if msg.connector != "slack" {
        anyhow::bail!(
            "Message {} is from connector '{}', not slack.",
            args.message_id,
            msg.connector
        );
    }

    let conv = db
        .get_conversation(&msg.conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", msg.conversation_id))?;

    let connector = build_slack_connector(args.account.as_deref(), &cfg)?;
    connector
        .react(&conv.external_id, &msg.external_id, &args.emoji)
        .await?;

    eprintln!("Reacted with :{}: to message.", args.emoji);
    Ok(())
}

async fn run_edit(args: &EditArgs) -> anyhow::Result<()> {
    let cfg = load_config()?;
    let db = Database::open(&cfg.db_path())?;

    let msg = db
        .get_message(&args.message_id)?
        .ok_or_else(|| anyhow::anyhow!("Message not found: {}", args.message_id))?;

    if msg.connector != "slack" {
        anyhow::bail!(
            "Message {} is from connector '{}', not slack.",
            args.message_id,
            msg.connector
        );
    }

    let conv = db
        .get_conversation(&msg.conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", msg.conversation_id))?;

    let connector = build_slack_connector(args.account.as_deref(), &cfg)?;
    connector
        .edit_message(&conv.external_id, &msg.external_id, &args.message)
        .await?;

    eprintln!("Message updated.");
    Ok(())
}

fn load_config() -> anyhow::Result<VoidConfig> {
    let config_path = config::default_config_path();
    VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))
}

fn build_slack_connector(
    account_filter: Option<&str>,
    cfg: &VoidConfig,
) -> anyhow::Result<void_slack::connector::SlackConnector> {
    let account = cfg
        .accounts
        .iter()
        .find(|a| {
            let is_slack = a.account_type == AccountType::Slack;
            let name_matches = account_filter.map_or(true, |n| a.id == n);
            is_slack && name_matches
        })
        .ok_or_else(|| {
            anyhow::anyhow!("No Slack account found in config. Run `void setup` to add one.")
        })?;

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
        _ => anyhow::bail!(
            "Mismatched account settings for Slack account '{}'",
            account.id
        ),
    };

    debug!(account_id = %account.id, "building Slack connector for CLI");
    Ok(void_slack::connector::SlackConnector::new(
        &account.id,
        &user_token,
        &app_token,
        exclude_channels,
    ))
}
