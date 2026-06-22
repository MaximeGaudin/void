use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{
    expand_tilde, settings_str, settings_string, settings_string_list, ConnectionConfig, SyncConfig,
};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 60;

inventory::submit! {
    ConnectorPlugin {
        id: void_calendar::CONNECTOR_ID,
        aliases: &["calendar", "cal", "ca"],
        menu_label: "Google Calendar",
        badge: "CA",
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

fn session_files(store: &Path, connection_id: &str) -> Vec<PathBuf> {
    vec![store.join(format!("{connection_id}-token.json"))]
}

fn build(
    connection: &ConnectionConfig,
    store_path: &Path,
    sync: &SyncConfig,
) -> anyhow::Result<Arc<dyn Connector>> {
    Ok(Arc::new(build_calendar(connection, store_path, sync)?))
}

pub(crate) fn build_calendar(
    connection: &ConnectionConfig,
    store_path: &Path,
    sync: &SyncConfig,
) -> anyhow::Result<void_calendar::connector::CalendarConnector> {
    let credentials_file = settings_string(&connection.settings, "credentials_file");
    let calendar_ids = settings_string_list(&connection.settings, "calendar_ids");
    let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
    let poll_secs =
        sync.poll_interval_secs(void_calendar::CONNECTOR_ID, DEFAULT_POLL_INTERVAL_SECS);
    Ok(void_calendar::connector::CalendarConnector::new(
        &connection.id,
        cred_path.as_deref().and_then(|p| p.to_str()),
        calendar_ids,
        store_path,
        poll_secs,
    ))
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(crate::commands::setup::calendar::setup_calendar(
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
    writeln!(out, "    credentials:  {label}")?;
    let calendar_ids = settings_string_list(table, "calendar_ids");
    writeln!(out, "    calendar_ids: {calendar_ids:?}")?;
    Ok(())
}
