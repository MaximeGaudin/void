use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct RemoteArgs {
    #[command(subcommand)]
    pub command: RemoteCommand,
}

#[derive(Debug, Subcommand)]
pub enum RemoteCommand {
    /// Show remote store connection and cache status
    Status,
    /// Force-refresh cached remote config and database snapshot
    Refresh,
}

pub fn run(args: &RemoteArgs, config: Option<&str>, store: Option<&str>) -> anyhow::Result<()> {
    match args.command {
        RemoteCommand::Status => run_status(),
        RemoteCommand::Refresh => run_refresh(config, store),
    }
}

fn run_status() -> anyhow::Result<()> {
    if !crate::context::is_remote() {
        anyhow::bail!("store.mode is not \"remote\" — nothing to report");
    }
    let status = crate::context::get().remote_status()?;
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

fn run_refresh(config: Option<&str>, store: Option<&str>) -> anyhow::Result<()> {
    let ctx = crate::context::load_fresh(config, store)?;
    if !ctx.is_remote() {
        anyhow::bail!("store.mode is not \"remote\" — nothing to refresh");
    }
    let status = ctx.remote_status()?;
    eprintln!("Remote cache refreshed.");
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}
