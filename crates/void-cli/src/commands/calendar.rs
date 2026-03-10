use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct CalendarArgs {
    #[command(subcommand)]
    pub command: Option<CalendarCommand>,
    /// Start date filter (YYYY-MM-DD)
    #[arg(long)]
    pub from: Option<String>,
    /// End date filter (YYYY-MM-DD)
    #[arg(long)]
    pub to: Option<String>,
    /// Filter by calendar account
    #[arg(long)]
    pub account: Option<String>,
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

pub fn run(args: &CalendarArgs) -> anyhow::Result<()> {
    match &args.command {
        Some(CalendarCommand::Week) => {
            eprintln!("void calendar week: not yet implemented");
        }
        Some(CalendarCommand::Create(create_args)) => {
            eprintln!(
                "void calendar create --title \"{}\": not yet implemented",
                create_args.title
            );
        }
        None => {
            eprintln!("void calendar: not yet implemented");
        }
    }
    Ok(())
}
