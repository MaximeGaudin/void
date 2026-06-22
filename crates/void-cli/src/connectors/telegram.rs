use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{
    redact_token, settings_i64, settings_string, ConnectionConfig, SyncConfig,
};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

inventory::submit! {
    ConnectorPlugin {
        id: void_telegram::CONNECTOR_ID,
        aliases: &["telegram", "tg"],
        menu_label: "Telegram",
        badge: "TG",
        default_poll_interval_secs: None,
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

fn session_files(store: &Path, connection_id: &str) -> Vec<PathBuf> {
    vec![store.join(format!("telegram-{connection_id}.json"))]
}

fn build(
    connection: &ConnectionConfig,
    store_path: &Path,
    _sync: &SyncConfig,
) -> anyhow::Result<Arc<dyn Connector>> {
    Ok(Arc::new(build_telegram(connection, store_path)?))
}

pub(crate) fn build_telegram(
    connection: &ConnectionConfig,
    store_path: &Path,
) -> anyhow::Result<void_telegram::connector::TelegramConnector> {
    let api_id = settings_i64(&connection.settings, "api_id").map(|v| v as i32);
    let api_hash = settings_string(&connection.settings, "api_hash");
    let session_path = store_path.join(format!("telegram-{}.json", connection.id));
    Ok(void_telegram::connector::TelegramConnector::new(
        &connection.id,
        session_path.to_str().unwrap_or(""),
        api_id,
        api_hash.as_deref(),
    ))
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(crate::commands::setup::telegram::setup_telegram(
        ctx.cfg,
        ctx.store_path,
        ctx.add_only,
    ))
}

fn parse_settings(_table: &toml::Table) -> anyhow::Result<()> {
    Ok(())
}

fn show_config(table: &toml::Table, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    let api_id = settings_i64(table, "api_id");
    let api_hash = settings_string(table, "api_hash");
    if let Some(id) = api_id {
        writeln!(out, "    api_id:   {id}")?;
    }
    if let Some(ref hash) = api_hash {
        writeln!(out, "    api_hash: {}", redact_token(hash))?;
    }
    if api_id.is_none() && settings_string(table, "api_hash").is_none() {
        writeln!(out, "    (using built-in API credentials)")?;
    }
    Ok(())
}
