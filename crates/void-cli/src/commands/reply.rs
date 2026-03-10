use clap::Args;

#[derive(Debug, Args)]
pub struct ReplyArgs {
    /// Message ID to reply to
    pub message_id: String,
    /// Message text
    #[arg(long)]
    pub message: String,
    /// Reply in thread (Slack) or as quote (WhatsApp)
    #[arg(long)]
    pub in_thread: bool,
}

pub fn run(args: &ReplyArgs) -> anyhow::Result<()> {
    eprintln!(
        "void reply {}{}: not yet implemented",
        args.message_id,
        if args.in_thread { " --in-thread" } else { "" }
    );
    Ok(())
}
