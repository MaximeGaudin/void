use clap::Args;

#[derive(Debug, Args)]
pub struct MessagesArgs {
    /// Conversation ID
    pub conversation_id: String,
    /// Show messages since this date (YYYY-MM-DD)
    #[arg(long)]
    pub since: Option<String>,
    /// Show messages until this date (YYYY-MM-DD)
    #[arg(long)]
    pub until: Option<String>,
    /// Maximum number of messages
    #[arg(long, default_value = "100")]
    pub limit: i64,
}

pub fn run(args: &MessagesArgs) -> anyhow::Result<()> {
    eprintln!(
        "void messages {}: not yet implemented",
        args.conversation_id
    );
    Ok(())
}
