use clap::Args;

#[derive(Debug, Args)]
pub struct InboxArgs {
    /// Filter by channel type: whatsapp, slack, gmail
    #[arg(long)]
    pub channel: Option<String>,
    /// Maximum number of messages to show
    #[arg(long, default_value = "50")]
    pub limit: i64,
}

pub fn run(args: &InboxArgs) -> anyhow::Result<()> {
    eprintln!(
        "void inbox{}: not yet implemented",
        args.channel
            .as_deref()
            .map(|c| format!(" --channel {c}"))
            .unwrap_or_default()
    );
    Ok(())
}
