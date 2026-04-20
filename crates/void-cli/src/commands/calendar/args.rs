use clap::{Args, Subcommand};

use crate::output::CONNECTOR_FILTER_HELP;

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
    /// Filter by calendar connection
    #[arg(long)]
    pub connection: Option<String>,
    #[arg(long, help = CONNECTOR_FILTER_HELP)]
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
    /// Start time in ISO 8601 format (e.g. 2026-03-31T17:00:00)
    #[arg(long)]
    pub start: String,
    /// End time in ISO 8601 format (default: start + 30min)
    #[arg(long)]
    pub end: Option<String>,
    /// Auto-attach Google Meet link
    #[arg(long)]
    pub meet: bool,
    /// Comma-separated attendee emails
    #[arg(long)]
    pub attendees: Option<String>,
    /// Calendar connection to use
    #[arg(long)]
    pub connection: Option<String>,
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
    /// Calendar connection to use
    #[arg(long)]
    pub connection: Option<String>,
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
    /// New start time in ISO 8601 format (e.g. 2026-03-31T17:00:00)
    #[arg(long)]
    pub start: Option<String>,
    /// New end time in ISO 8601 format (e.g. 2026-03-31T17:00:00)
    #[arg(long)]
    pub end: Option<String>,
    /// Calendar connection to use
    #[arg(long)]
    pub connection: Option<String>,
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
    /// Your email address (defaults to connection ID)
    #[arg(long)]
    pub email: Option<String>,
    /// Calendar connection to use
    #[arg(long)]
    pub connection: Option<String>,
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
    /// Calendar connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct DeleteEventArgs {
    /// Event ID to delete
    pub event_id: String,
    /// Calendar connection to use
    #[arg(long)]
    pub connection: Option<String>,
}
