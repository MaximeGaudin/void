use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{
    redact_token, settings_str, settings_string, settings_string_list, settings_u32,
    ConnectionConfig, SyncConfig,
};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 3600;

inventory::submit! {
    ConnectorPlugin {
        id: void_reddit::CONNECTOR_ID,
        aliases: &["reddit", "rd"],
        menu_label: "Reddit",
        badge: "RD",
        default_poll_interval_secs: Some(DEFAULT_POLL_INTERVAL_SECS),
        reply_id_style: ReplyIdStyle::ConvMsg,
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
    let client_id = settings_string(&connection.settings, "client_id").ok_or_else(|| {
        anyhow::anyhow!(
            "missing client_id for Reddit connection '{}'",
            connection.id
        )
    })?;
    let client_secret =
        settings_string(&connection.settings, "client_secret").ok_or_else(|| {
            anyhow::anyhow!(
                "missing client_secret for Reddit connection '{}'",
                connection.id
            )
        })?;
    let refresh_token = settings_string(&connection.settings, "refresh_token");
    let subreddits = settings_string_list(&connection.settings, "subreddits");
    let keywords = settings_string_list(&connection.settings, "keywords");
    let min_score = settings_u32(&connection.settings, "min_score").unwrap_or(0);
    let poll_secs = sync.poll_interval_secs(void_reddit::CONNECTOR_ID, DEFAULT_POLL_INTERVAL_SECS);

    Ok(Arc::new(void_reddit::connector::RedditConnector::new(
        &connection.id,
        client_id,
        client_secret,
        refresh_token,
        subreddits,
        keywords,
        min_score,
        poll_secs,
    )))
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(crate::commands::setup::reddit::setup_reddit(
        ctx.cfg,
        ctx.add_only,
    ))
}

fn parse_settings(table: &toml::Table) -> anyhow::Result<()> {
    if settings_str(table, "client_id").is_none() {
        anyhow::bail!("missing client_id");
    }
    if settings_str(table, "client_secret").is_none() {
        anyhow::bail!("missing client_secret");
    }
    Ok(())
}

fn show_config(table: &toml::Table, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    if let Some(client_id) = settings_str(table, "client_id") {
        writeln!(out, "    client_id:     {}", redact_token(client_id))?;
    }
    if let Some(client_secret) = settings_str(table, "client_secret") {
        writeln!(out, "    client_secret: {}", redact_token(client_secret))?;
    }
    if settings_str(table, "refresh_token").is_some() {
        writeln!(out, "    refresh_token: (set — commenting enabled)")?;
    } else {
        writeln!(out, "    refresh_token: (not set — read-only)")?;
    }
    let subreddits = settings_string_list(table, "subreddits");
    if subreddits.is_empty() {
        writeln!(out, "    subreddits:    (none)")?;
    } else {
        writeln!(out, "    subreddits:    {}", subreddits.join(", "))?;
    }
    let keywords = settings_string_list(table, "keywords");
    if keywords.is_empty() {
        writeln!(out, "    keywords:      (none — all posts)")?;
    } else {
        writeln!(out, "    keywords:      {}", keywords.join(", "))?;
    }
    let min_score = settings_u32(table, "min_score").unwrap_or(0);
    writeln!(out, "    min_score:     {min_score}")?;
    Ok(())
}
