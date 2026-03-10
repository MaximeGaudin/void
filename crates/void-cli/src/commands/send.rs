use clap::Args;

#[derive(Debug, Args)]
pub struct SendArgs {
    /// Recipient (phone number, channel name, email)
    #[arg(long)]
    pub to: String,
    /// Channel to send via: whatsapp, slack, gmail
    #[arg(long)]
    pub via: String,
    /// Account to use (for multi-account channels)
    #[arg(long)]
    pub account: Option<String>,
    /// Message text
    #[arg(long)]
    pub message: String,
    /// Email subject (gmail only)
    #[arg(long)]
    pub subject: Option<String>,
    /// File to attach
    #[arg(long)]
    pub file: Option<String>,
}

pub fn run(args: &SendArgs) -> anyhow::Result<()> {
    eprintln!(
        "void send --to {} --via {}: not yet implemented",
        args.to, args.via
    );
    Ok(())
}
