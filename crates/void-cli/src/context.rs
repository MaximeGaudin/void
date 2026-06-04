use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use void_core::config::{resolve_config_path, StoreMode, VoidConfig};
use void_core::db::Database;
use void_core::store::{RefreshPolicy, ResolvedContext};

static CONTEXT: OnceLock<ResolvedContext> = OnceLock::new();

pub fn init(
    config: Option<&str>,
    store: Option<&str>,
    refresh: RefreshPolicy,
    force_local_store: bool,
) -> anyhow::Result<()> {
    let config_path = config.map(|s| resolve_config_path(Some(Path::new(s))));
    let ctx = ResolvedContext::load(config_path.as_deref(), store, refresh, force_local_store)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    CONTEXT
        .set(ctx)
        .map_err(|_| anyhow::anyhow!("application context already initialized"))?;
    Ok(())
}

pub fn get() -> &'static ResolvedContext {
    CONTEXT
        .get()
        .expect("void context not initialized — this is an internal error")
}

/// Effective config: remote connections and store paths when `store.mode = remote`.
pub fn void_config() -> &'static VoidConfig {
    config()
}

pub fn config() -> &'static VoidConfig {
    get().config()
}

pub fn open_db() -> anyhow::Result<Database> {
    get().open_database().map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn open_db_writable() -> anyhow::Result<Database> {
    get()
        .open_database_writable()
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn store_path() -> PathBuf {
    get().store_path()
}

pub fn is_remote() -> bool {
    get().is_remote()
}

pub fn client_config_path() -> PathBuf {
    get().client_config_path().to_path_buf()
}

pub fn refresh_cache() -> anyhow::Result<()> {
    // OnceLock doesn't allow mutation; reload by replacing via new init isn't possible.
    // Force refresh by loading fresh context into a temp and copying...
    // For remote refresh command, we'll call ResolvedContext::load directly.
    Ok(())
}

pub fn load_fresh(config: Option<&str>, store: Option<&str>) -> anyhow::Result<ResolvedContext> {
    let config_path = config.map(|s| resolve_config_path(Some(Path::new(s))));
    ResolvedContext::load(config_path.as_deref(), store, RefreshPolicy::Force, false)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn ensure_local_sync_allowed() -> anyhow::Result<()> {
    get()
        .ensure_local_sync_allowed()
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn ensure_local_setup_allowed() -> anyhow::Result<()> {
    get()
        .ensure_local_setup_allowed()
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn proxy_current_command() -> anyhow::Result<()> {
    let raw_args = collect_proxy_args();
    let code = get()
        .proxy_command(&raw_args)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    std::process::exit(code);
}

/// Collect CLI args to forward to the remote host, excluding client-only globals.
fn collect_proxy_args() -> Vec<String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut filtered = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--local-store" => {
                i += 1;
            }
            "--store" | "--config" => {
                i += 2;
            }
            "-v" | "--verbose" | "--no-context" => {
                i += 1;
            }
            _ => {
                filtered.push(args[i].clone());
                i += 1;
            }
        }
    }
    filtered
}

/// Commands that only read the local snapshot of the remote store (no SSH proxy).
pub(crate) fn runs_with_local_cache(command: &crate::Command) -> bool {
    use crate::Command;

    match command {
        Command::Inbox(_)
        | Command::Conversations(_)
        | Command::Messages(_)
        | Command::Contacts(_)
        | Command::Channels(_)
        | Command::Search(_)
        | Command::Doctor(_)
        | Command::Remote(_) => true,
        Command::Calendar(args) => calendar_reads_local_cache(args),
        Command::Hn(args) => hackernews_reads_local_cache(args),
        Command::Sync(args) => args.status,
        Command::Setup => false,
        _ => false,
    }
}

/// List/week calendar commands only read the synced DB snapshot.
fn calendar_reads_local_cache(args: &crate::commands::calendar::CalendarArgs) -> bool {
    use crate::commands::calendar::CalendarCommand;

    matches!(args.command, None | Some(CalendarCommand::Week))
}

fn hackernews_reads_local_cache(args: &crate::commands::hackernews::HackerNewsArgs) -> bool {
    use crate::commands::hackernews::{HnCommand, KeywordsAction};

    match &args.command {
        HnCommand::Config => true,
        HnCommand::Keywords(kw) => matches!(kw.action, KeywordsAction::List),
        HnCommand::MinScore(_) => false,
    }
}

pub fn mode_label() -> &'static str {
    match get().mode() {
        StoreMode::Local => "local",
        StoreMode::Remote => "remote",
    }
}
