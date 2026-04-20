use clap::Args;

#[derive(Clone, Debug, Args)]
pub struct SyncArgs {
    /// Sync only specific connectors (comma-separated: whatsapp,telegram,slack,gmail,calendar,hackernews)
    #[arg(long)]
    pub connectors: Option<String>,
    /// Detach and run as a background daemon
    #[arg(long)]
    pub daemon: bool,
    /// Stop any existing sync before starting this one
    #[arg(long)]
    pub restart: bool,
    /// Clear the database before syncing (fresh start)
    #[arg(long)]
    pub clear: bool,
    /// Clear data for a specific connector before syncing (e.g. whatsapp, telegram, slack, gmail, calendar, hackernews)
    #[arg(long)]
    pub clear_connector: Option<String>,
    /// Stop the running sync daemon
    #[arg(long)]
    pub stop: bool,
    /// Show sync daemon status and per-connector sync info
    #[arg(long)]
    pub status: bool,
    /// Internal: run sync process as detached child.
    #[arg(long, hide = true)]
    pub daemon_inner: bool,
}
