use std::path::Path;
use std::sync::Arc;

use void_calendar::connector::CalendarConnector;
use void_core::config::{ConnectionConfig, VoidConfig};
use void_core::connector::Connector;
use void_core::models::ConnectorType;
use void_gmail::connector::GmailConnector;
use void_slack::connector::SlackConnector;
use void_telegram::connector::TelegramConnector;
use void_whatsapp::connector::WhatsAppConnector;

use crate::connectors::{
    self, build_calendar, build_gmail, build_slack, build_telegram, build_whatsapp,
    build_whatsapp_owned,
};

fn find_connection<'a>(
    cfg: &'a VoidConfig,
    connector_type: ConnectorType,
    filter: Option<&str>,
    not_found_msg: &str,
) -> anyhow::Result<&'a ConnectionConfig> {
    cfg.connections
        .iter()
        .find(|a| a.connector_type == connector_type && filter.is_none_or(|n| a.id == n))
        .ok_or_else(|| anyhow::anyhow!("{}", not_found_msg))
}

fn sync_config() -> void_core::config::SyncConfig {
    crate::context::config().sync.clone()
}

pub fn build_gmail_connector(connection_filter: Option<&str>) -> anyhow::Result<GmailConnector> {
    let cfg = crate::context::void_config();
    let connection = find_connection(
        cfg,
        ConnectorType::from_static(void_gmail::CONNECTOR_ID),
        connection_filter,
        "No Gmail connection found in config. Run `void setup` to add one.",
    )?;
    let store_path = crate::context::store_path();
    build_gmail(connection, &store_path, &sync_config())
}

pub fn build_calendar_connector(
    connection_filter: Option<&str>,
) -> anyhow::Result<(CalendarConnector, VoidConfig)> {
    let cfg = crate::context::void_config();
    let connection = find_connection(
        cfg,
        ConnectorType::from_static(void_calendar::CONNECTOR_ID),
        connection_filter,
        "No calendar connection found in config. Run `void setup` to add one.",
    )?;
    let store_path = crate::context::store_path();
    let connector = build_calendar(connection, &store_path, &sync_config())?;
    Ok((connector, cfg.clone()))
}

pub fn build_slack_connector(
    connection_filter: Option<&str>,
    cfg: &VoidConfig,
) -> anyhow::Result<SlackConnector> {
    let connection = find_connection(
        cfg,
        ConnectorType::from_static(void_slack::CONNECTOR_ID),
        connection_filter,
        "No Slack connection found in config. Run `void setup` to add one.",
    )?;
    let store_path = crate::context::store_path();
    build_slack(connection, &store_path, &cfg.sync)
}

pub fn build_whatsapp_connector(
    connection: &ConnectionConfig,
    store_path: &Path,
) -> Arc<WhatsAppConnector> {
    build_whatsapp(connection, store_path)
}

pub fn build_whatsapp_connector_for_cli(
    connection_filter: Option<&str>,
    cfg: &VoidConfig,
) -> anyhow::Result<WhatsAppConnector> {
    let connection = find_connection(
        cfg,
        ConnectorType::from_static(void_whatsapp::CONNECTOR_ID),
        connection_filter,
        "No WhatsApp connection found in config. Run `void setup` to add one.",
    )?;
    let store_path = crate::context::store_path();
    Ok(build_whatsapp_owned(connection, &store_path))
}

pub fn build_telegram_connector(
    connection_filter: Option<&str>,
    cfg: &VoidConfig,
) -> anyhow::Result<TelegramConnector> {
    let connection = find_connection(
        cfg,
        ConnectorType::from_static(void_telegram::CONNECTOR_ID),
        connection_filter,
        "No Telegram connection found in config. Run `void setup` to add one.",
    )?;
    let store_path = crate::context::store_path();
    build_telegram(connection, &store_path)
}

pub fn build_connector(
    connection: &ConnectionConfig,
    store_path: &Path,
) -> anyhow::Result<Arc<dyn Connector>> {
    let plugin = connectors::by_id(connection.connector_type.as_str())
        .ok_or_else(|| anyhow::anyhow!("unknown connector type: {}", connection.connector_type))?;
    let sync_cfg = &crate::context::config().sync;
    (plugin.build)(connection, store_path, sync_cfg)
}
