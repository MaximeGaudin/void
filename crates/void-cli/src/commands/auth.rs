use clap::Args;

#[derive(Debug, Args)]
pub struct AuthArgs {
    /// Channel type: whatsapp, slack, gmail, calendar
    pub channel_type: String,
    /// Account name (required for slack, gmail, calendar)
    pub account_name: Option<String>,
}

pub fn run(args: &AuthArgs) -> anyhow::Result<()> {
    eprintln!(
        "void auth {}{}: not yet implemented",
        args.channel_type,
        args.account_name
            .as_deref()
            .map(|n| format!(" {n}"))
            .unwrap_or_default()
    );
    Ok(())
}
