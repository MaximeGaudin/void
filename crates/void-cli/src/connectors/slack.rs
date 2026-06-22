use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{
    redact_token, settings_str, settings_string, ConnectionConfig, SyncConfig,
};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

inventory::submit! {
    ConnectorPlugin {
        id: void_slack::CONNECTOR_ID,
        aliases: &["slack", "sl"],
        menu_label: "Slack",
        badge: "SL",
        default_poll_interval_secs: None,
        reply_id_style: ReplyIdStyle::ConvMsg,
        supports_scheduling: true,
        uses_daemon_rpc: false,
        prompt_token_reauth: true,
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
    store_path: &Path,
    sync: &SyncConfig,
) -> anyhow::Result<Arc<dyn Connector>> {
    Ok(Arc::new(build_slack(connection, store_path, sync)?))
}

pub(crate) fn build_slack(
    connection: &ConnectionConfig,
    store_path: &Path,
    _sync: &SyncConfig,
) -> anyhow::Result<void_slack::connector::SlackConnector> {
    let user_token = settings_string(&connection.settings, "user_token").ok_or_else(|| {
        anyhow::anyhow!(
            "missing user_token for Slack connection '{}'",
            connection.id
        )
    })?;
    let app_token = settings_string(&connection.settings, "app_token").ok_or_else(|| {
        anyhow::anyhow!("missing app_token for Slack connection '{}'", connection.id)
    })?;
    let app_id = settings_string(&connection.settings, "app_id");
    let config_refresh_token = settings_string(&connection.settings, "config_refresh_token");
    Ok(void_slack::connector::SlackConnector::new(
        &connection.id,
        &user_token,
        &app_token,
        app_id.as_deref(),
        config_refresh_token.as_deref(),
        store_path,
        Some(&crate::context::client_config_path()),
    )?)
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(crate::commands::setup::slack::setup_slack(
        ctx.cfg,
        ctx.store_path,
        ctx.add_only,
    ))
}

fn parse_settings(table: &toml::Table) -> anyhow::Result<()> {
    if settings_str(table, "app_token").is_none() {
        anyhow::bail!("missing app_token");
    }
    if settings_str(table, "user_token").is_none() {
        anyhow::bail!("missing user_token");
    }
    Ok(())
}

fn show_config(table: &toml::Table, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    if let Some(token) = settings_str(table, "app_token") {
        writeln!(out, "    app_token:  {}", redact_token(token))?;
    }
    if let Some(token) = settings_str(table, "user_token") {
        writeln!(out, "    user_token: {}", redact_token(token))?;
    }
    if let Some(id) = settings_str(table, "app_id") {
        writeln!(out, "    app_id:     {id}")?;
    }
    if let Some(token) = settings_str(table, "config_refresh_token") {
        writeln!(out, "    config_refresh_token: {}", redact_token(token))?;
    }
    Ok(())
}
