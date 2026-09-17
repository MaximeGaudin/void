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
    /// Update the remote host's void binary to the latest release over SSH
    Update,
}

pub fn run(args: &RemoteArgs, config: Option<&str>, store: Option<&str>) -> anyhow::Result<()> {
    match args.command {
        RemoteCommand::Status => run_status(),
        RemoteCommand::Refresh => run_refresh(config, store),
        RemoteCommand::Update => run_update(),
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

fn run_update() -> anyhow::Result<()> {
    if !crate::context::is_remote() {
        anyhow::bail!("store.mode is not \"remote\" — nothing to update");
    }
    let code = crate::context::run_remote_update()?;
    if code != 0 {
        std::process::exit(code);
    }
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
