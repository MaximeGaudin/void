use clap::{Args, Subcommand};

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
    /// Open (or get) a direct message or group conversation with one or more users
    Open(OpenArgs),
    /// Forward a message to another channel or user
    Forward(ForwardArgs),
}

#[derive(Debug, Args)]
pub struct ReactArgs {
    /// Message ID (void internal ID)
    pub message_id: String,
    /// Emoji name (without colons, e.g. "thumbsup", "eyes", "white_check_mark")
    #[arg(long)]
    pub emoji: String,
    /// Slack connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct EditArgs {
    /// Message ID (void internal ID)
    pub message_id: String,
    /// New message text
    #[arg(long)]
    pub message: String,
    /// Slack connection to use
    #[arg(long)]
    pub connection: Option<String>,
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
    /// Slack connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct ForwardArgs {
    /// Message ID (void internal ID)
    pub message_id: String,
    /// Channel or user ID to forward to
    #[arg(long)]
    pub to: String,
    /// Optional comment to include above the forwarded message
    #[arg(long)]
    pub comment: Option<String>,
    /// Slack connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct OpenArgs {
    /// Comma-separated list of Slack user IDs to open a conversation with
    #[arg(long)]
    pub users: String,
    /// Slack connection to use
    #[arg(long)]
    pub connection: Option<String>,
}
