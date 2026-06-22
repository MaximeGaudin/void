use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{
    redact_token, settings_str, settings_string, ConnectionConfig, SyncConfig,
};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 120;

inventory::submit! {
    ConnectorPlugin {
        id: void_github::CONNECTOR_ID,
        aliases: &["github", "gh"],
        menu_label: "GitHub",
        badge: "GH",
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
    let token = settings_string(&connection.settings, "token").ok_or_else(|| {
        anyhow::anyhow!("missing token for GitHub connection '{}'", connection.id)
    })?;
    let username = settings_string(&connection.settings, "username").ok_or_else(|| {
        anyhow::anyhow!("missing username for GitHub connection '{}'", connection.id)
    })?;
    let poll_secs = sync.poll_interval_secs(void_github::CONNECTOR_ID, DEFAULT_POLL_INTERVAL_SECS);
    Ok(Arc::new(void_github::connector::GitHubConnector::new(
        &connection.id,
        token,
        username,
        poll_secs,
    )))
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(crate::commands::setup::github::setup_github(
        ctx.cfg,
        ctx.add_only,
    ))
}

fn parse_settings(table: &toml::Table) -> anyhow::Result<()> {
    if settings_str(table, "token").is_none() {
        anyhow::bail!("missing token");
    }
    if settings_str(table, "username").is_none() {
        anyhow::bail!("missing username");
    }
    Ok(())
}

fn show_config(table: &toml::Table, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    if let Some(token) = settings_str(table, "token") {
        writeln!(out, "    token:    {}", redact_token(token))?;
    }
    if let Some(username) = settings_str(table, "username") {
        writeln!(out, "    username: {username}")?;
    }
    Ok(())
}
