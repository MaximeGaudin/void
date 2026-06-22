use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{ConnectionConfig, SyncConfig};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

inventory::submit! {
    ConnectorPlugin {
        id: void_whatsapp::CONNECTOR_ID,
        aliases: &["whatsapp", "wa"],
        menu_label: "WhatsApp",
        badge: "WA",
        default_poll_interval_secs: None,
        reply_id_style: ReplyIdStyle::ConvMsg,
        supports_scheduling: false,
        uses_daemon_rpc: true,
        prompt_token_reauth: false,
        session_files,
        build,
        setup,
        parse_settings,
        show_config,
    }
}

fn session_files(store: &Path, connection_id: &str) -> Vec<PathBuf> {
    vec![store.join(format!("whatsapp-{connection_id}.db"))]
}

fn build(
    connection: &ConnectionConfig,
    store_path: &Path,
    _sync: &SyncConfig,
) -> anyhow::Result<Arc<dyn Connector>> {
    Ok(build_whatsapp(connection, store_path))
}

pub(crate) fn build_whatsapp_owned(
    connection: &ConnectionConfig,
    store_path: &Path,
) -> void_whatsapp::connector::WhatsAppConnector {
    let session_db = store_path.join(format!("whatsapp-{}.db", connection.id));
    void_whatsapp::connector::WhatsAppConnector::new(
        &connection.id,
        session_db.to_str().unwrap_or(""),
    )
}

pub(crate) fn build_whatsapp(
    connection: &ConnectionConfig,
    store_path: &Path,
) -> Arc<void_whatsapp::connector::WhatsAppConnector> {
    Arc::new(build_whatsapp_owned(connection, store_path))
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(crate::commands::setup::whatsapp::setup_whatsapp(
        ctx.cfg,
        ctx.store_path,
        ctx.add_only,
    ))
}

fn parse_settings(_table: &toml::Table) -> anyhow::Result<()> {
    Ok(())
}

fn show_config(_table: &toml::Table, _out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    Ok(())
}
