use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
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
    /// Schedule a message to be sent later
    Schedule(ScheduleArgs),
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

#[derive(Debug, Args)]
pub struct ScheduleArgs {
    /// Channel name or ID to send to
    #[arg(long)]
    pub channel: String,
    /// Message text
    #[arg(long)]
    pub message: String,
    /// When to send — accepts "HH:MM" (today), "YYYY-MM-DD HH:MM", or a Unix timestamp
    #[arg(long)]
    pub at: String,
    /// Thread timestamp to reply in a thread
    #[arg(long)]
    pub thread: Option<String>,
    /// Slack account to use
    #[arg(long)]
    pub account: Option<String>,
}

pub async fn run(args: &SlackArgs, _json: bool) -> anyhow::Result<()> {
    match &args.command {
        SlackCommand::React(a) => run_react(a).await,
        SlackCommand::Edit(a) => run_edit(a).await,
        SlackCommand::Schedule(a) => run_schedule(a).await,
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

async fn run_schedule(args: &ScheduleArgs) -> anyhow::Result<()> {
    let post_at = parse_schedule_time(&args.at)?;
    let now = chrono::Utc::now().timestamp();
    if post_at <= now {
        anyhow::bail!("Scheduled time must be in the future (parsed as Unix ts {post_at})");
    }

    let cfg = load_config()?;
    let connector = build_slack_connector(args.account.as_deref(), &cfg)?;

    let scheduled_id = connector
        .schedule_message(
            &args.channel,
            &args.message,
            post_at,
            args.thread.as_deref(),
        )
        .await?;

    let dt = chrono::DateTime::from_timestamp(post_at, 0)
        .map(|utc| utc.with_timezone(&Local))
        .map(|local| local.format("%Y-%m-%d %H:%M %Z").to_string())
        .unwrap_or_else(|| post_at.to_string());

    eprintln!("Message scheduled for {dt} (id: {scheduled_id})");
    Ok(())
}

/// Parse a human-friendly time string into a Unix timestamp.
///
/// Accepted formats:
///   - `HH:MM`              — today at this local time
///   - `YYYY-MM-DD HH:MM`   — specific date and time in local timezone
///   - Plain integer         — Unix timestamp
pub fn parse_schedule_time(input: &str) -> anyhow::Result<i64> {
    let s = input.trim();

    if let Ok(ts) = s.parse::<i64>() {
        return Ok(ts);
    }

    if let Ok(time) = NaiveTime::parse_from_str(s, "%H:%M") {
        let today = Local::now().date_naive();
        let naive = NaiveDateTime::new(today, time);
        let local = Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| anyhow::anyhow!("Ambiguous local time: {s}"))?;
        return Ok(local.timestamp());
    }

    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
        let local = Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| anyhow::anyhow!("Ambiguous local time: {s}"))?;
        return Ok(local.timestamp());
    }

    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let naive = date.and_hms_opt(9, 0, 0).unwrap();
        let local = Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| anyhow::anyhow!("Ambiguous local date: {s}"))?;
        return Ok(local.timestamp());
    }

    anyhow::bail!("Cannot parse time '{s}'. Use HH:MM, YYYY-MM-DD HH:MM, or a Unix timestamp.")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_schedule_time_unix_timestamp() {
        let ts = parse_schedule_time("1234567890").unwrap();
        assert_eq!(ts, 1_234_567_890);
    }

    #[test]
    fn parse_schedule_time_invalid_returns_error() {
        let result = parse_schedule_time("not-a-time");
        assert!(result.is_err());
    }

    #[test]
    fn parse_schedule_time_date_time_format() {
        // 2025-01-15 14:30 in local timezone
        let ts = parse_schedule_time("2025-01-15 14:30").unwrap();
        let dt = Local.timestamp_opt(ts, 0).single().unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2025-01-15");
        assert_eq!(dt.format("%H:%M").to_string(), "14:30");
    }

    #[test]
    fn parse_schedule_time_date_only_defaults_to_9am() {
        let ts = parse_schedule_time("2025-06-10").unwrap();
        let dt = Local.timestamp_opt(ts, 0).single().unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2025-06-10");
        assert_eq!(dt.format("%H").to_string(), "09");
    }
}
