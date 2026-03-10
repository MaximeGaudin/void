use clap::Args;

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
    /// Filter by channel type
    #[arg(long)]
    pub channel: Option<String>,
    /// Maximum results
    #[arg(long, default_value = "50")]
    pub limit: i64,
}

pub fn run(args: &SearchArgs) -> anyhow::Result<()> {
    eprintln!("void search \"{}\": not yet implemented", args.query);
    Ok(())
}
