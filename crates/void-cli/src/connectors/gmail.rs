use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{
    expand_tilde, settings_str, settings_string, ConnectionConfig, SyncConfig,
};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

inventory::submit! {
    ConnectorPlugin {
        id: void_gmail::CONNECTOR_ID,
        aliases: &["gmail", "gm", "email"],
        menu_label: "Gmail",
        badge: "GM",
        default_poll_interval_secs: Some(30),
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

fn session_files(store: &Path, connection_id: &str) -> Vec<PathBuf> {
    vec![store.join(format!("{connection_id}-token.json"))]
}

fn build(
    connection: &ConnectionConfig,
    store_path: &Path,
    _sync: &SyncConfig,
) -> anyhow::Result<Arc<dyn Connector>> {
    let credentials_file = settings_string(&connection.settings, "credentials_file");
    let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
    Ok(Arc::new(void_gmail::connector::GmailConnector::new(
        &connection.id,
        cred_path.as_deref().and_then(|p| p.to_str()),
        store_path,
    )))
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(crate::commands::setup::gmail::setup_gmail(
        ctx.cfg,
        ctx.store_path,
        ctx.add_only,
    ))
}

fn parse_settings(_table: &toml::Table) -> anyhow::Result<()> {
    Ok(())
}

fn show_config(table: &toml::Table, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    let label = settings_str(table, "credentials_file").unwrap_or("(built-in)");
    writeln!(out, "    credentials: {label}")?;
    Ok(())
}
