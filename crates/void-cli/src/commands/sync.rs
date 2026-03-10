use clap::Args;

#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Sync only specific channels (comma-separated: whatsapp,slack,gmail,calendar)
    #[arg(long)]
    pub channels: Option<String>,
    /// Run as background daemon
    #[arg(long)]
    pub daemon: bool,
}

pub fn run(args: &SyncArgs) -> anyhow::Result<()> {
    eprintln!(
        "void sync{}: not yet implemented",
        args.channels
            .as_deref()
            .map(|c| format!(" --channels {c}"))
            .unwrap_or_default()
    );
    Ok(())
}
