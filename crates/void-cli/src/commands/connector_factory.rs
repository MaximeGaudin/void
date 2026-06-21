use std::path::Path;
use std::sync::Arc;
use tracing::debug;

use void_calendar::connector::CalendarConnector;
use void_core::config::{expand_tilde, ConnectionConfig, VoidConfig};
use void_core::connector::Connector;
use void_core::models::ConnectorType;
use void_gmail::connector::GmailConnector;
use void_slack::connector::SlackConnector;
use void_telegram::connector::TelegramConnector;
use void_whatsapp::connector::WhatsAppConnector;

use crate::connectors;

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

pub fn build_gmail_connector(connection_filter: Option<&str>) -> anyhow::Result<GmailConnector> {
    let cfg = crate::context::void_config();
    let connection = find_connection(
        cfg,
        ConnectorType::from_static(void_gmail::CONNECTOR_ID),
        connection_filter,
        "No Gmail connection found in config. Run `void setup` to add one.",
    )?;

    let cred_path = void_core::config::settings_string(&connection.settings, "credentials_file")
        .map(|f| expand_tilde(&f));
    let store_path = crate::context::store_path();
    debug!(connection_id = %connection.id, "building Gmail connector for CLI");
    Ok(GmailConnector::new(
        &connection.id,
        cred_path.as_deref().and_then(|p| p.to_str()),
        &store_path,
    ))
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

    let credentials_file =
        void_core::config::settings_string(&connection.settings, "credentials_file");
    let calendar_ids =
        void_core::config::settings_string_list(&connection.settings, "calendar_ids");

    let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
    let store_path = crate::context::store_path();
    let connector = CalendarConnector::new(
        &connection.id,
        cred_path.as_deref().and_then(|p| p.to_str()),
        calendar_ids,
        &store_path,
    );

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

    let user_token = void_core::config::settings_string(&connection.settings, "user_token")
        .ok_or_else(|| anyhow::anyhow!("missing user_token"))?;
    let app_token = void_core::config::settings_string(&connection.settings, "app_token")
        .ok_or_else(|| anyhow::anyhow!("missing app_token"))?;
    let app_id = void_core::config::settings_string(&connection.settings, "app_id");
    let config_refresh_token =
        void_core::config::settings_string(&connection.settings, "config_refresh_token");

    let store_path = crate::context::store_path();
    debug!(connection_id = %connection.id, "building Slack connector for CLI");
    Ok(SlackConnector::new(
        &connection.id,
        &user_token,
        &app_token,
        app_id.as_deref(),
        config_refresh_token.as_deref(),
        &store_path,
        Some(&crate::context::client_config_path()),
    )?)
}

pub fn build_whatsapp_connector(
    connection: &ConnectionConfig,
    store_path: &Path,
) -> Arc<WhatsAppConnector> {
    let session_db = store_path.join(format!("whatsapp-{}.db", connection.id));
    Arc::new(WhatsAppConnector::new(
        &connection.id,
        session_db.to_str().unwrap_or(""),
    ))
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
    debug!(connection_id = %connection.id, "building WhatsApp connector for CLI");
    let session_db = store_path.join(format!("whatsapp-{}.db", connection.id));
    Ok(WhatsAppConnector::new(
        &connection.id,
        session_db.to_str().unwrap_or(""),
    ))
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

    let api_id = void_core::config::settings_i64(&connection.settings, "api_id").map(|v| v as i32);
    let api_hash = void_core::config::settings_string(&connection.settings, "api_hash");

    let store_path = crate::context::store_path();
    let session_path = store_path.join(format!("telegram-{}.json", connection.id));
    debug!(connection_id = %connection.id, "building Telegram connector for CLI");
    Ok(TelegramConnector::new(
        &connection.id,
        session_path.to_str().unwrap_or(""),
        api_id,
        api_hash.as_deref(),
    ))
}

pub fn build_connector(
    connection: &ConnectionConfig,
    store_path: &Path,
) -> anyhow::Result<Arc<dyn Connector>> {
    debug!(connection_id = %connection.id, type = %connection.connector_type, "building connector");
    let plugin = connectors::by_id(connection.connector_type.as_str())
        .ok_or_else(|| anyhow::anyhow!("unknown connector type: {}", connection.connector_type))?;
    let sync_cfg = &crate::context::config().sync;
    (plugin.build)(connection, store_path, sync_cfg)
}
