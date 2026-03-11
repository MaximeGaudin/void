use chrono::Datelike;
use clap::{Args, Subcommand};
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use crate::output::OutputFormatter;

#[derive(Debug, Args)]
pub struct CalendarArgs {
    #[command(subcommand)]
    pub command: Option<CalendarCommand>,
    /// Show events for a specific day (YYYY-MM-DD, "today", "tomorrow", "yesterday")
    #[arg(long, short)]
    pub day: Option<String>,
    /// Start date filter (YYYY-MM-DD)
    #[arg(long)]
    pub from: Option<String>,
    /// End date filter (YYYY-MM-DD)
    #[arg(long)]
    pub to: Option<String>,
    /// Filter by calendar account
    #[arg(long)]
    pub account: Option<String>,
    /// Filter by connector (slack, gmail, whatsapp, calendar)
    #[arg(long)]
    pub connector: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum CalendarCommand {
    /// Show this week's events
    Week,
    /// Create a new calendar event
    Create(CreateEventArgs),
}

#[derive(Debug, Args)]
pub struct CreateEventArgs {
    /// Event title
    #[arg(long)]
    pub title: String,
    /// Start time (RFC 3339 or "YYYY-MM-DD HH:MM")
    #[arg(long)]
    pub start: String,
    /// End time (default: start + 30min)
    #[arg(long)]
    pub end: Option<String>,
    /// Auto-attach Google Meet link
    #[arg(long)]
    pub meet: bool,
    /// Comma-separated attendee emails
    #[arg(long)]
    pub attendees: Option<String>,
    /// Calendar account to use
    #[arg(long)]
    pub account: Option<String>,
}

pub fn run(args: &CalendarArgs, json: bool) -> anyhow::Result<()> {
    let subcommand = match &args.command {
        None => "list",
        Some(CalendarCommand::Week) => "week",
        Some(CalendarCommand::Create(_)) => "create",
    };
    debug!(subcommand, "calendar");
    match &args.command {
        Some(CalendarCommand::Week) => run_week(json),
        Some(CalendarCommand::Create(create_args)) => {
            eprintln!(
                "void calendar create --title \"{}\": not yet implemented (requires Calendar adapter)",
                create_args.title
            );
            Ok(())
        }
        None => run_list(args, json),
    }
}

fn run_list(args: &CalendarArgs, json: bool) -> anyhow::Result<()> {
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let (from, to) = if let Some(day) = &args.day {
        let date = parse_day_spec(day)?;
        let start = date.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc().timestamp());
        let end = (date + chrono::Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_utc().timestamp());
        (start, end)
    } else {
        let from = args.from.as_deref().and_then(parse_date_to_ts).or_else(|| {
            Some(
                chrono::Utc::now()
                    .date_naive()
                    .and_hms_opt(0, 0, 0)?
                    .and_utc()
                    .timestamp(),
            )
        });

        let to = args.to.as_deref().and_then(parse_date_to_ts).or_else(|| {
            Some(
                (chrono::Utc::now().date_naive() + chrono::Duration::days(1))
                    .and_hms_opt(0, 0, 0)?
                    .and_utc()
                    .timestamp(),
            )
        });
        (from, to)
    };

    let events = db.list_events(
        from,
        to,
        args.account.as_deref(),
        args.connector.as_deref(),
        200,
    )?;
    formatter.print_events(&events)
}

fn run_week(json: bool) -> anyhow::Result<()> {
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let today = chrono::Utc::now().date_naive();
    let weekday = today.weekday().num_days_from_monday();
    let monday = today - chrono::Duration::days(weekday as i64);
    let sunday = monday + chrono::Duration::days(7);

    let from = monday
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp());
    let to = sunday
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp());

    let events = db.list_events(from, to, None, None, 200)?;
    formatter.print_events(&events)
}

fn parse_day_spec(spec: &str) -> anyhow::Result<chrono::NaiveDate> {
    let today = chrono::Utc::now().date_naive();
    match spec.to_lowercase().as_str() {
        "today" => Ok(today),
        "tomorrow" => Ok(today + chrono::Duration::days(1)),
        "yesterday" => Ok(today - chrono::Duration::days(1)),
        other => chrono::NaiveDate::parse_from_str(other, "%Y-%m-%d").map_err(|_| {
            anyhow::anyhow!(
                "Invalid day: \"{other}\". Use YYYY-MM-DD, today, tomorrow, or yesterday."
            )
        }),
    }
}

fn parse_date_to_ts(date: &str) -> Option<i64> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
}
