use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{
    redact_token, settings_str, settings_string, ConnectionConfig, SyncConfig,
};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 30 * 60;

inventory::submit! {
    ConnectorPlugin {
        id: void_linkedin::CONNECTOR_ID,
        aliases: &["linkedin", "li"],
        menu_label: "LinkedIn",
        badge: "LI",
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
    let api_key = settings_string(&connection.settings, "api_key").ok_or_else(|| {
        anyhow::anyhow!(
            "missing api_key for LinkedIn connection '{}'",
            connection.id
        )
    })?;
    let dsn = settings_string(&connection.settings, "dsn").ok_or_else(|| {
        anyhow::anyhow!("missing dsn for LinkedIn connection '{}'", connection.id)
    })?;
    let account_id = settings_string(&connection.settings, "account_id").ok_or_else(|| {
        anyhow::anyhow!(
            "missing account_id for LinkedIn connection '{}'",
            connection.id
        )
    })?;
    Ok(Arc::new(void_linkedin::connector::LinkedInConnector::new(
        &connection.id,
        &api_key,
        &dsn,
        &account_id,
        sync.poll_interval_secs(void_linkedin::CONNECTOR_ID, DEFAULT_POLL_INTERVAL_SECS),
        sync.linkedin_backfill_days(),
    )))
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(crate::commands::setup::linkedin::setup_linkedin(
        ctx.cfg,
        ctx.store_path,
        ctx.add_only,
    ))
}

fn parse_settings(table: &toml::Table) -> anyhow::Result<()> {
    if settings_str(table, "api_key").is_none() {
        anyhow::bail!("missing api_key");
    }
    if settings_str(table, "dsn").is_none() {
        anyhow::bail!("missing dsn");
    }
    if settings_str(table, "account_id").is_none() {
        anyhow::bail!("missing account_id");
    }
    Ok(())
}

fn show_config(table: &toml::Table, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    if let Some(key) = settings_str(table, "api_key") {
        writeln!(out, "    api_key:    {}", redact_token(key))?;
    }
    if let Some(dsn) = settings_str(table, "dsn") {
        writeln!(out, "    dsn:        {dsn}")?;
    }
    if let Some(id) = settings_str(table, "account_id") {
        writeln!(out, "    account_id: {id}")?;
    }
    Ok(())
}
