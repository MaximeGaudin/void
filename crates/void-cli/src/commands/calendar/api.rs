use void_core::connector::Connector;
use void_core::db::Database;

use super::args::{
    AvailabilityArgs, CreateEventArgs, DeleteEventArgs, RespondEventArgs, SearchEventArgs,
    UpdateEventArgs,
};
use super::connector::build_calendar_connector;
use super::parsing::{normalize_datetime, parse_datetime_or_date};
use crate::output::OutputFormatter;

pub(super) async fn run_create(args: &CreateEventArgs) -> anyhow::Result<()> {
    let (connector, cfg) = build_calendar_connector(args.connection.as_deref())?;
    let db = Database::open(&cfg.db_path())?;

    let start = normalize_datetime(&args.start)?;
    let end = match &args.end {
        Some(e) => normalize_datetime(e)?,
        None => {
            let dt = chrono::DateTime::parse_from_rfc3339(&start).map_err(|e| {
                anyhow::anyhow!("internal error: normalized start time is not valid RFC 3339: {e}")
            })?;
            (dt + chrono::Duration::minutes(30)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        }
    };

    let params = void_calendar::connector::CreateEventParams {
        title: &args.title,
        description: args.description.as_deref(),
        start: &start,
        end: &end,
        meet: args.meet,
        attendees: args.attendees.as_deref(),
    };
    let event = connector.create_event(&params, &db).await?;

    let formatter = OutputFormatter::new();
    formatter.print_events(&[event])
}

pub(super) async fn run_search(args: &SearchEventArgs) -> anyhow::Result<()> {
    let (connector, cfg) = build_calendar_connector(args.connection.as_deref())?;
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

pub(super) async fn run_calendars() -> anyhow::Result<()> {
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

pub(super) async fn run_update(args: &UpdateEventArgs) -> anyhow::Result<()> {
    let (connector, cfg) = build_calendar_connector(args.connection.as_deref())?;
    let db = Database::open(&cfg.db_path())?;

    let start = args.start.as_deref().map(normalize_datetime).transpose()?;
    let end = args.end.as_deref().map(normalize_datetime).transpose()?;

    let params = void_calendar::connector::UpdateEventParams {
        event_id: &args.event_id,
        title: args.title.as_deref(),
        description: args.description.as_deref(),
        start: start.as_deref(),
        end: end.as_deref(),
        send_updates: Some("all"),
    };
    let event = connector.update_event(&params, &db).await?;

    eprintln!("Event updated.");
    let formatter = OutputFormatter::new();
    formatter.print_events(&[event])
}

pub(super) async fn run_respond(args: &RespondEventArgs) -> anyhow::Result<()> {
    let valid = ["accepted", "declined", "tentative"];
    if !valid.contains(&args.status.as_str()) {
        anyhow::bail!(
            "Invalid status \"{}\". Must be one of: accepted, declined, tentative.",
            args.status
        );
    }

    let (connector, cfg) = build_calendar_connector(args.connection.as_deref())?;
    let db = Database::open(&cfg.db_path())?;

    let email = args
        .email
        .clone()
        .unwrap_or_else(|| connector.connection_id().to_string());

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

pub(super) async fn run_delete(args: &DeleteEventArgs) -> anyhow::Result<()> {
    let (connector, _cfg) = build_calendar_connector(args.connection.as_deref())?;

    connector.delete_event(&args.event_id, Some("all")).await?;

    eprintln!("Event {} deleted.", args.event_id);
    Ok(())
}

pub(super) async fn run_availability(args: &AvailabilityArgs) -> anyhow::Result<()> {
    let (connector, _cfg) = build_calendar_connector(args.connection.as_deref())?;

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
