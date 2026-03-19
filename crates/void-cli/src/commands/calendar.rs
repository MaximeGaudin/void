use chrono::{Datelike, Local};
use clap::{Args, Subcommand};
use tracing::debug;
use void_core::config::{self, expand_tilde, AccountType, VoidConfig};
use void_core::connector::Connector;
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
    /// Search events by keyword (queries Google Calendar API directly)
    Search(SearchEventArgs),
    /// List available calendars
    Calendars,
    /// Update an existing event
    Update(UpdateEventArgs),
    /// Respond to an event invitation (accept/decline/tentative)
    Respond(RespondEventArgs),
    /// Delete an event
    Delete(DeleteEventArgs),
    /// Check attendees' availability (free/busy)
    Availability(AvailabilityArgs),
}

#[derive(Debug, Args)]
pub struct CreateEventArgs {
    /// Event title
    #[arg(long)]
    pub title: String,
    /// Event description / notes
    #[arg(long)]
    pub description: Option<String>,
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

#[derive(Debug, Args)]
pub struct SearchEventArgs {
    /// Search query
    pub query: String,
    /// Start date filter (YYYY-MM-DD)
    #[arg(long)]
    pub from: Option<String>,
    /// End date filter (YYYY-MM-DD)
    #[arg(long)]
    pub to: Option<String>,
    /// Calendar account to use
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Debug, Args)]
pub struct UpdateEventArgs {
    /// Event ID to update (use `void calendar` to find IDs)
    pub event_id: String,
    /// New title
    #[arg(long)]
    pub title: Option<String>,
    /// New description
    #[arg(long)]
    pub description: Option<String>,
    /// New start time (RFC 3339 or "YYYY-MM-DD HH:MM")
    #[arg(long)]
    pub start: Option<String>,
    /// New end time (RFC 3339 or "YYYY-MM-DD HH:MM")
    #[arg(long)]
    pub end: Option<String>,
    /// Calendar account to use
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Debug, Args)]
pub struct RespondEventArgs {
    /// Event ID to respond to
    pub event_id: String,
    /// Response: accepted, declined, tentative
    #[arg(long)]
    pub status: String,
    /// Optional note/comment with your response
    #[arg(long)]
    pub comment: Option<String>,
    /// Your email address (defaults to account ID)
    #[arg(long)]
    pub email: Option<String>,
    /// Calendar account to use
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Debug, Args)]
pub struct AvailabilityArgs {
    /// Comma-separated email addresses to check
    #[arg(long)]
    pub attendees: String,
    /// Start of time window (YYYY-MM-DD or RFC 3339)
    #[arg(long)]
    pub from: String,
    /// End of time window (YYYY-MM-DD or RFC 3339)
    #[arg(long)]
    pub to: String,
    /// Calendar account to use
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Debug, Args)]
pub struct DeleteEventArgs {
    /// Event ID to delete
    pub event_id: String,
    /// Calendar account to use
    #[arg(long)]
    pub account: Option<String>,
}

pub async fn run(args: &CalendarArgs) -> anyhow::Result<()> {
    let subcommand = match &args.command {
        None => "list",
        Some(CalendarCommand::Week) => "week",
        Some(CalendarCommand::Create(_)) => "create",
        Some(CalendarCommand::Search(_)) => "search",
        Some(CalendarCommand::Calendars) => "calendars",
        Some(CalendarCommand::Update(_)) => "update",
        Some(CalendarCommand::Respond(_)) => "respond",
        Some(CalendarCommand::Delete(_)) => "delete",
        Some(CalendarCommand::Availability(_)) => "availability",
    };
    debug!(subcommand, "calendar");
    match &args.command {
        Some(CalendarCommand::Week) => run_week(),
        Some(CalendarCommand::Create(create_args)) => run_create(create_args).await,
        Some(CalendarCommand::Search(search_args)) => run_search(search_args).await,
        Some(CalendarCommand::Calendars) => run_calendars().await,
        Some(CalendarCommand::Update(update_args)) => run_update(update_args).await,
        Some(CalendarCommand::Respond(respond_args)) => run_respond(respond_args).await,
        Some(CalendarCommand::Delete(delete_args)) => run_delete(delete_args).await,
        Some(CalendarCommand::Availability(avail_args)) => run_availability(avail_args).await,
        None => run_list(args),
    }
}

fn run_list(args: &CalendarArgs) -> anyhow::Result<()> {
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new();

    let (from, to) = if let Some(day) = &args.day {
        let date = parse_day_spec(day)?;
        let start = date
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| dt.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp());
        let end = (date + chrono::Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| dt.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp());
        (start, end)
    } else {
        let today = Local::now().date_naive();
        let from = args.from.as_deref().and_then(parse_date_to_ts).or_else(|| {
            today
                .and_hms_opt(0, 0, 0)
                .and_then(|dt| dt.and_local_timezone(Local).single())
                .map(|dt| dt.timestamp())
        });

        let to = args.to.as_deref().and_then(parse_date_to_ts).or_else(|| {
            (today + chrono::Duration::days(1))
                .and_hms_opt(0, 0, 0)
                .and_then(|dt| dt.and_local_timezone(Local).single())
                .map(|dt| dt.timestamp())
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

fn run_week() -> anyhow::Result<()> {
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new();

    let today = Local::now().date_naive();
    let weekday = today.weekday().num_days_from_monday();
    let monday = today - chrono::Duration::days(weekday as i64);
    let sunday = monday + chrono::Duration::days(7);

    let from = monday
        .and_hms_opt(0, 0, 0)
        .and_then(|dt| dt.and_local_timezone(Local).single())
        .map(|dt| dt.timestamp());
    let to = sunday
        .and_hms_opt(0, 0, 0)
        .and_then(|dt| dt.and_local_timezone(Local).single())
        .map(|dt| dt.timestamp());

    let events = db.list_events(from, to, None, None, 200)?;
    formatter.print_events(&events)
}

async fn run_create(args: &CreateEventArgs) -> anyhow::Result<()> {
    let (connector, cfg) = build_calendar_connector(args.account.as_deref())?;
    let db = Database::open(&cfg.db_path())?;

    let end = args.end.clone().unwrap_or_else(|| {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&args.start) {
            (dt + chrono::Duration::minutes(30)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        } else {
            args.start.clone()
        }
    });

    let params = void_calendar::connector::CreateEventParams {
        title: &args.title,
        description: args.description.as_deref(),
        start: &args.start,
        end: &end,
        meet: args.meet,
        attendees: args.attendees.as_deref(),
    };
    let event = connector.create_event(&params, &db).await?;

    let formatter = OutputFormatter::new();
    formatter.print_events(&[event])
}

async fn run_search(args: &SearchEventArgs) -> anyhow::Result<()> {
    let (connector, cfg) = build_calendar_connector(args.account.as_deref())?;
    let db = Database::open(&cfg.db_path())?;

    let time_min = args.from.as_deref().and_then(|d| {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .ok()
            .and_then(|nd| nd.and_hms_opt(0, 0, 0))
            .map(|ndt| ndt.and_utc().to_rfc3339())
    });
    let time_max = args.to.as_deref().and_then(|d| {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .ok()
            .and_then(|nd| nd.and_hms_opt(23, 59, 59))
            .map(|ndt| ndt.and_utc().to_rfc3339())
    });

    let events = connector
        .search_events(&args.query, time_min.as_deref(), time_max.as_deref(), &db)
        .await?;

    let formatter = OutputFormatter::new();
    formatter.print_events(&events)
}

async fn run_calendars() -> anyhow::Result<()> {
    let (connector, _cfg) = build_calendar_connector(None)?;
    let calendars = connector.list_calendars().await?;

    let items: Vec<serde_json::Value> = calendars
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "summary": c.summary,
                "primary": c.primary.unwrap_or(false),
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({ "data": items, "error": null }))?
    );
    Ok(())
}

async fn run_update(args: &UpdateEventArgs) -> anyhow::Result<()> {
    let (connector, cfg) = build_calendar_connector(args.account.as_deref())?;
    let db = Database::open(&cfg.db_path())?;

    let params = void_calendar::connector::UpdateEventParams {
        event_id: &args.event_id,
        title: args.title.as_deref(),
        description: args.description.as_deref(),
        start: args.start.as_deref(),
        end: args.end.as_deref(),
        send_updates: Some("all"),
    };
    let event = connector.update_event(&params, &db).await?;

    eprintln!("Event updated.");
    let formatter = OutputFormatter::new();
    formatter.print_events(&[event])
}

async fn run_respond(args: &RespondEventArgs) -> anyhow::Result<()> {
    let valid = ["accepted", "declined", "tentative"];
    if !valid.contains(&args.status.as_str()) {
        anyhow::bail!(
            "Invalid status \"{}\". Must be one of: accepted, declined, tentative.",
            args.status
        );
    }

    let (connector, cfg) = build_calendar_connector(args.account.as_deref())?;
    let db = Database::open(&cfg.db_path())?;

    let email = args
        .email
        .clone()
        .unwrap_or_else(|| connector.account_id().to_string());

    let event = connector
        .respond_to_event(
            &args.event_id,
            &email,
            &args.status,
            args.comment.as_deref(),
            &db,
        )
        .await?;

    eprintln!(
        "Responded \"{}\" to event \"{}\".",
        args.status, event.title
    );
    let formatter = OutputFormatter::new();
    formatter.print_events(&[event])
}

async fn run_delete(args: &DeleteEventArgs) -> anyhow::Result<()> {
    let (connector, _cfg) = build_calendar_connector(args.account.as_deref())?;

    connector.delete_event(&args.event_id, Some("all")).await?;

    eprintln!("Event {} deleted.", args.event_id);
    Ok(())
}

async fn run_availability(args: &AvailabilityArgs) -> anyhow::Result<()> {
    let (connector, _cfg) = build_calendar_connector(args.account.as_deref())?;

    let emails: Vec<String> = args
        .attendees
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if emails.is_empty() {
        anyhow::bail!("At least one attendee email is required.");
    }

    let time_min = parse_datetime_or_date(&args.from)?;
    let time_max = parse_datetime_or_date(&args.to)?;

    let resp = connector
        .check_availability(&time_min, &time_max, &emails)
        .await?;

    let mut data = serde_json::Map::new();
    for (email, cal) in &resp.calendars {
        if !cal.errors.is_empty() {
            let reasons: Vec<&str> = cal
                .errors
                .iter()
                .filter_map(|e| e.reason.as_deref())
                .collect();
            data.insert(
                email.clone(),
                serde_json::json!({ "error": reasons.join(", ") }),
            );
        } else {
            data.insert(email.clone(), serde_json::json!({ "busy": cal.busy }));
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({ "data": data, "error": null }))?
    );
    Ok(())
}

fn parse_datetime_or_date(s: &str) -> anyhow::Result<String> {
    if chrono::DateTime::parse_from_rfc3339(s).is_ok() {
        return Ok(s.to_string());
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = date
            .and_hms_opt(0, 0, 0)
            .and_then(|ndt| ndt.and_local_timezone(Local).single())
            .ok_or_else(|| anyhow::anyhow!("Failed to convert date to local timezone"))?;
        return Ok(dt.to_rfc3339());
    }
    anyhow::bail!("Invalid date/time: \"{s}\". Use YYYY-MM-DD or RFC 3339 format.")
}

fn build_calendar_connector(
    account_filter: Option<&str>,
) -> anyhow::Result<(void_calendar::connector::CalendarConnector, VoidConfig)> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let account = cfg
        .accounts
        .iter()
        .find(|a| {
            let is_calendar = a.account_type == AccountType::Calendar;
            let name_matches = account_filter.map_or(true, |n| a.id == n);
            is_calendar && name_matches
        })
        .ok_or_else(|| {
            anyhow::anyhow!("No calendar account found in config. Run `void setup` to add one.")
        })?;

    let (credentials_file, calendar_ids) = match &account.settings {
        void_core::config::AccountSettings::Calendar {
            credentials_file,
            calendar_ids,
        } => (credentials_file.clone(), calendar_ids.clone()),
        _ => anyhow::bail!(
            "Mismatched account settings for calendar account '{}'",
            account.id
        ),
    };

    let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
    let store_path = cfg.store_path();
    let connector = void_calendar::connector::CalendarConnector::new(
        &account.id,
        cred_path.as_deref().and_then(|p| p.to_str()),
        calendar_ids,
        &store_path,
    );

    Ok((connector, cfg))
}

fn parse_day_spec(spec: &str) -> anyhow::Result<chrono::NaiveDate> {
    let today = Local::now().date_naive();
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
        .and_then(|dt| dt.and_local_timezone(Local).single())
        .map(|dt| dt.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Local, NaiveDate};

    #[test]
    fn parse_day_spec_today() {
        let result = parse_day_spec("today").unwrap();
        assert_eq!(result, Local::now().date_naive());
    }

    #[test]
    fn parse_day_spec_tomorrow() {
        let result = parse_day_spec("tomorrow").unwrap();
        assert_eq!(result, Local::now().date_naive() + Duration::days(1));
    }

    #[test]
    fn parse_day_spec_yesterday() {
        let result = parse_day_spec("yesterday").unwrap();
        assert_eq!(result, Local::now().date_naive() - Duration::days(1));
    }

    #[test]
    fn parse_day_spec_iso_date() {
        let result = parse_day_spec("2026-06-15").unwrap();
        assert_eq!(result, NaiveDate::from_ymd_opt(2026, 6, 15).unwrap());
    }

    #[test]
    fn parse_day_spec_case_insensitive() {
        assert!(parse_day_spec("Today").is_ok());
        assert!(parse_day_spec("TOMORROW").is_ok());
        assert!(parse_day_spec("Yesterday").is_ok());
    }

    #[test]
    fn parse_day_spec_invalid() {
        assert!(parse_day_spec("not-a-date").is_err());
        assert!(parse_day_spec("2026-13-01").is_err());
    }

    #[test]
    fn parse_date_to_ts_valid() {
        let ts = parse_date_to_ts("2026-06-15").unwrap();
        assert!(ts > 0);
        let local_midnight = NaiveDate::from_ymd_opt(2026, 6, 15)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        assert_eq!(ts, local_midnight);
    }

    #[test]
    fn parse_date_to_ts_invalid() {
        assert!(parse_date_to_ts("invalid").is_none());
        assert!(parse_date_to_ts("2026-13-45").is_none());
    }

    #[test]
    fn default_date_range_uses_local_timezone() {
        let today = Local::now().date_naive();
        let expected_from = today
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        let expected_to = (today + Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();

        let _utc_from = today.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();

        assert!(expected_from > 0);
        assert!(expected_to > expected_from);
    }

    #[test]
    fn parse_date_to_ts_uses_local_not_utc() {
        let ts = parse_date_to_ts("2026-06-15").unwrap();
        let _utc_midnight = NaiveDate::from_ymd_opt(2026, 6, 15)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        let local_midnight = NaiveDate::from_ymd_opt(2026, 6, 15)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        assert_eq!(ts, local_midnight);
    }
}
