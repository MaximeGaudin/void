//! Google Calendar CLI: list events, create/update/delete, availability.

mod api;
mod args;
mod connector;
mod list;
mod parsing;

pub use args::{CalendarArgs, CalendarCommand};

use tracing::debug;

/// Dispatch `void calendar` subcommands.
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
        Some(CalendarCommand::Week) => list::run_week(),
        Some(CalendarCommand::Create(create_args)) => api::run_create(create_args).await,
        Some(CalendarCommand::Search(search_args)) => api::run_search(search_args).await,
        Some(CalendarCommand::Calendars) => api::run_calendars().await,
        Some(CalendarCommand::Update(update_args)) => api::run_update(update_args).await,
        Some(CalendarCommand::Respond(respond_args)) => api::run_respond(respond_args).await,
        Some(CalendarCommand::Delete(delete_args)) => api::run_delete(delete_args).await,
        Some(CalendarCommand::Availability(avail_args)) => api::run_availability(avail_args).await,
        None => list::run_list(args),
    }
}
