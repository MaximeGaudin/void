use chrono::{Datelike, Local};
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use super::args::CalendarArgs;
use super::parsing::{parse_date_to_ts, parse_day_spec};
use crate::output::{resolve_connector_filter, OutputFormatter};

pub(super) fn run_list(args: &CalendarArgs) -> anyhow::Result<()> {
    let connector = resolve_connector_filter(args.connector.as_deref())?;
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
        args.connection.as_deref(),
        connector.as_deref(),
        200,
    )?;
    formatter.print_events(&events)
}

pub(super) fn run_week() -> anyhow::Result<()> {
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
