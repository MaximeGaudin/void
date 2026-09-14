use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{settings_string, ConnectionConfig, SyncConfig};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

/// The Messages store is local, so polling is cheap. A minute keeps the inbox
/// close to live without spinning on a file the Messages app is writing to.
const DEFAULT_POLL_INTERVAL_SECS: u64 = 60;

inventory::submit! {
    ConnectorPlugin {
        id: void_imessage::CONNECTOR_ID,
        aliases: &["imessage", "imsg"],
        menu_label: "iMessage (macOS)",
        badge: "iM",
        default_poll_interval_secs: Some(DEFAULT_POLL_INTERVAL_SECS),
        reply_id_style: ReplyIdStyle::MsgOnly,
        supports_scheduling: false,
        uses_daemon_rpc: false,
        prompt_token_reauth: false,
        session_files,
        build,
        setup,
        parse_settings,
        show_config,
    }
}

fn session_files(_store: &Path, _connection_id: &str) -> Vec<PathBuf> {
    // The store belongs to macOS; void never copies or owns any part of it.
    vec![]
}

fn build(
    connection: &ConnectionConfig,
    _store_path: &Path,
    sync: &SyncConfig,
) -> anyhow::Result<Arc<dyn Connector>> {
    if !cfg!(target_os = "macos") {
        anyhow::bail!(
            "the iMessage connector only works on macOS: it reads the local \
             Messages database, which exists nowhere else"
        );
    }

    let db_path = match settings_string(&connection.settings, "db_path") {
        Some(p) if !p.is_empty() => PathBuf::from(shellexpand_home(&p)),
        _ => void_imessage::connector_default_db_path()?,
    };

    let poll_secs =
        sync.poll_interval_secs(void_imessage::CONNECTOR_ID, DEFAULT_POLL_INTERVAL_SECS);
    Ok(Arc::new(void_imessage::connector::ImessageConnector::new(
        &connection.id,
        db_path,
        poll_secs,
    )))
}

/// Expand a leading `~` so a config written by hand still resolves.
///
/// The daemon, not the shell, opens this path, so a literal `~` would be taken
/// as a directory name and the store would look missing while it plainly exists.
fn shellexpand_home(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home)
                .join(rest)
                .to_string_lossy()
                .into_owned(),
            None => path.to_string(),
        },
        None => path.to_string(),
    }
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(async move {
        crate::commands::setup::imessage::setup_imessage(ctx.cfg, ctx.add_only)?;
        Ok(())
    })
}

fn parse_settings(_table: &toml::Table) -> anyhow::Result<()> {
    Ok(())
}

fn show_config(table: &toml::Table, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    match settings_string(table, "db_path") {
        Some(p) if !p.is_empty() => writeln!(out, "    db_path:   {p}"),
        _ => writeln!(out, "    db_path:   (default: ~/Library/Messages/chat.db)"),
    }
}

#[cfg(test)]
mod tests {
    use super::shellexpand_home;

    #[test]
    fn expands_leading_tilde_so_the_daemon_finds_the_store() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            shellexpand_home("~/Library/Messages/chat.db"),
            format!("{home}/Library/Messages/chat.db")
        );
    }

    #[test]
    fn leaves_absolute_paths_alone() {
        assert_eq!(shellexpand_home("/tmp/chat.db"), "/tmp/chat.db");
    }

    #[test]
    fn does_not_expand_a_tilde_inside_the_path() {
        // Only a leading "~/" is a home reference; anywhere else it is a
        // perfectly ordinary character in a filename.
        assert_eq!(shellexpand_home("/tmp/~/chat.db"), "/tmp/~/chat.db");
    }
}
