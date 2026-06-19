//! Slack CLI helpers (react, edit, schedule, open DM/group).

mod args;
mod saved;

pub use args::*;

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use void_core::config::VoidConfig;
use void_core::connector::Connector;

use crate::commands::connector_factory::build_slack_connector;

pub async fn run(args: &SlackArgs) -> anyhow::Result<()> {
    match &args.command {
        SlackCommand::React(a) => run_react(a).await,
        SlackCommand::Edit(a) => run_edit(a).await,
        SlackCommand::Schedule(a) => run_schedule(a).await,
        SlackCommand::Open(a) => run_open(a).await,
        SlackCommand::Forward(a) => run_forward(a).await,
        SlackCommand::Saved(a) => saved::run(a),
    }
}

async fn run_react(args: &ReactArgs) -> anyhow::Result<()> {
    let cfg = load_config();
    let db = crate::context::open_db()?;

    let msg = super::resolve::resolve_message(&db, &args.message_id)?;

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

    let connector = build_slack_connector(args.connection.as_deref(), cfg)?;
    connector
        .react(&conv.external_id, &msg.external_id, &args.emoji)
        .await?;

    eprintln!("Reacted with :{}: to message.", args.emoji);
    Ok(())
}

async fn run_edit(args: &EditArgs) -> anyhow::Result<()> {
    let cfg = load_config();
    let db = crate::context::open_db()?;

    let msg = super::resolve::resolve_message(&db, &args.message_id)?;

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

    let connector = build_slack_connector(args.connection.as_deref(), cfg)?;
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

    let cfg = load_config();
    let connector = build_slack_connector(args.connection.as_deref(), cfg)?;

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

async fn run_open(args: &OpenArgs) -> anyhow::Result<()> {
    let cfg = load_config();
    let connector = build_slack_connector(args.connection.as_deref(), cfg)?;

    let user_ids: Vec<&str> = args.users.split(',').map(|s| s.trim()).collect();
    if user_ids.is_empty() {
        anyhow::bail!("Provide at least one user ID with --users");
    }

    let channel_id = connector.open_conversation(&user_ids).await?;

    if user_ids.len() == 1 {
        eprintln!("DM opened: {channel_id}");
    } else {
        eprintln!("Group conversation opened: {channel_id}");
    }
    println!("{channel_id}");
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
        let naive = date
            .and_hms_opt(9, 0, 0)
            .ok_or_else(|| anyhow::anyhow!("Invalid date for schedule time '{s}'"))?;
        let local = Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| anyhow::anyhow!("Ambiguous local date: {s}"))?;
        return Ok(local.timestamp());
    }

    anyhow::bail!("Cannot parse time '{s}'. Use HH:MM, YYYY-MM-DD HH:MM, or a Unix timestamp.")
}

async fn run_forward(args: &ForwardArgs) -> anyhow::Result<()> {
    let cfg = load_config();
    let db = crate::context::open_db()?;

    let msg = super::resolve::resolve_message(&db, &args.message_id)?;

    super::resolve::check_forward_connector(&args.message_id, &msg.connector, "slack")?;

    let conv = db
        .get_conversation(&msg.conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", msg.conversation_id))?;

    let conn_id =
        super::resolve::resolve_forward_connection(args.connection.as_deref(), &msg.connection_id);
    let connector = build_slack_connector(Some(conn_id), cfg)?;

    let fwd_id = connector
        .forward(
            &msg.external_id,
            &conv.external_id,
            &args.to,
            args.comment.as_deref(),
        )
        .await?;

    eprintln!("Message forwarded (id: {fwd_id})");
    Ok(())
}

fn load_config() -> &'static VoidConfig {
    crate::context::void_config()
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
