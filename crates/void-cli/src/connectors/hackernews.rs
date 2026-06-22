use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{settings_string_list, settings_u32, ConnectionConfig, SyncConfig};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 3600;

inventory::submit! {
    ConnectorPlugin {
        id: void_hackernews::CONNECTOR_ID,
        aliases: &["hackernews", "hn"],
        menu_label: "Hacker News",
        badge: "HN",
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
    vec![]
}

fn build(
    connection: &ConnectionConfig,
    _store_path: &Path,
    sync: &SyncConfig,
) -> anyhow::Result<Arc<dyn Connector>> {
    let keywords = settings_string_list(&connection.settings, "keywords");
    let min_score = settings_u32(&connection.settings, "min_score").unwrap_or(0);
    let poll_secs =
        sync.poll_interval_secs(void_hackernews::CONNECTOR_ID, DEFAULT_POLL_INTERVAL_SECS);
    Ok(Arc::new(
        void_hackernews::connector::HackerNewsConnector::new(
            &connection.id,
            keywords,
            min_score,
            poll_secs,
        ),
    ))
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(async move {
        crate::commands::setup::hackernews::setup_hackernews(ctx.cfg, ctx.add_only)?;
        Ok(())
    })
}

fn parse_settings(_table: &toml::Table) -> anyhow::Result<()> {
    Ok(())
}

fn show_config(table: &toml::Table, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    let keywords = settings_string_list(table, "keywords");
    if keywords.is_empty() {
        writeln!(out, "    keywords:  (none — all stories)")?;
    } else {
        writeln!(out, "    keywords:  {}", keywords.join(", "))?;
    }
    let min_score = settings_u32(table, "min_score").unwrap_or(0);
    writeln!(out, "    min_score: {min_score}")?;
    Ok(())
}
