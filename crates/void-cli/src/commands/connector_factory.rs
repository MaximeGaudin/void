use std::path::Path;
use std::sync::Arc;
use tracing::debug;

use void_calendar::connector::CalendarConnector;
use void_core::config::{expand_tilde, ConnectionConfig, ConnectionSettings, VoidConfig};
use void_core::connector::Connector;
use void_core::models::ConnectorType;
use void_gmail::connector::GmailConnector;
use void_slack::connector::SlackConnector;
use void_telegram::connector::TelegramConnector;
use void_whatsapp::connector::WhatsAppConnector;

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
        ConnectorType::Gmail,
        connection_filter,
        "No Gmail connection found in config. Run `void setup` to add one.",
    )?;

    let credentials_file = match &connection.settings {
        ConnectionSettings::Gmail { credentials_file } => credentials_file.clone(),
        _ => anyhow::bail!(
            "Mismatched connection settings for Gmail connection '{}'",
            connection.id
        ),
    };

    let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
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
        ConnectorType::Calendar,
        connection_filter,
        "No calendar connection found in config. Run `void setup` to add one.",
    )?;

    let (credentials_file, calendar_ids) = match &connection.settings {
        ConnectionSettings::Calendar {
            credentials_file,
            calendar_ids,
        } => (credentials_file.clone(), calendar_ids.clone()),
        _ => anyhow::bail!(
            "Mismatched connection settings for calendar connection '{}'",
            connection.id
        ),
    };

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
        ConnectorType::Slack,
        connection_filter,
        "No Slack connection found in config. Run `void setup` to add one.",
    )?;

    let (user_token, app_token, app_id, config_refresh_token) = match &connection.settings {
        ConnectionSettings::Slack {
            user_token,
            app_token,
            app_id,
            config_refresh_token,
        } => (
            user_token.clone(),
            app_token.clone(),
            app_id.clone(),
            config_refresh_token.clone(),
        ),
        _ => anyhow::bail!(
            "Mismatched connection settings for Slack connection '{}'",
            connection.id
        ),
    };

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
        ConnectorType::WhatsApp,
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
        ConnectorType::Telegram,
        connection_filter,
        "No Telegram connection found in config. Run `void setup` to add one.",
    )?;

    let (api_id, api_hash) = match &connection.settings {
        ConnectionSettings::Telegram {
            api_id, api_hash, ..
        } => (*api_id, api_hash.clone()),
        _ => anyhow::bail!("connection '{}' has mismatched settings", connection.id),
    };

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
    match (&connection.connector_type, &connection.settings) {
        (
            ConnectorType::Slack,
            ConnectionSettings::Slack {
                user_token,
                app_token,
                app_id,
                config_refresh_token,
            },
        ) => Ok(Arc::new(void_slack::connector::SlackConnector::new(
            &connection.id,
            user_token,
            app_token,
            app_id.as_deref(),
            config_refresh_token.as_deref(),
            store_path,
            Some(&crate::context::client_config_path()),
        )?)),
        (ConnectorType::Gmail, ConnectionSettings::Gmail { credentials_file }) => {
            let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
            Ok(Arc::new(void_gmail::connector::GmailConnector::new(
                &connection.id,
                cred_path.as_deref().and_then(|p| p.to_str()),
                store_path,
            )))
        }
        (
            ConnectorType::Calendar,
            ConnectionSettings::Calendar {
                credentials_file,
                calendar_ids,
            },
        ) => {
            let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
            Ok(Arc::new(void_calendar::connector::CalendarConnector::new(
                &connection.id,
                cred_path.as_deref().and_then(|p| p.to_str()),
                calendar_ids.clone(),
                store_path,
            )))
        }
        (ConnectorType::WhatsApp, ConnectionSettings::WhatsApp {}) => {
            Ok(build_whatsapp_connector(connection, store_path) as Arc<dyn Connector>)
        }
        (
            ConnectorType::Telegram,
            ConnectionSettings::Telegram {
                api_id, api_hash, ..
            },
        ) => {
            let session_path = store_path.join(format!("telegram-{}.json", connection.id));
            Ok(Arc::new(void_telegram::connector::TelegramConnector::new(
                &connection.id,
                session_path.to_str().unwrap_or(""),
                *api_id,
                api_hash.as_deref(),
            )))
        }
        (
            ConnectorType::HackerNews,
            ConnectionSettings::HackerNews {
                keywords,
                min_score,
            },
        ) => {
            let poll_secs = crate::context::config().sync.hackernews_poll_interval_secs;
            Ok(Arc::new(
                void_hackernews::connector::HackerNewsConnector::new(
                    &connection.id,
                    keywords.clone(),
                    *min_score,
                    poll_secs,
                ),
            ))
        }
        (
            ConnectorType::GoogleNews,
            ConnectionSettings::GoogleNews {
                keywords,
                when,
                language,
                country,
            },
        ) => {
            let poll_secs = crate::context::config().sync.googlenews_poll_interval_secs;
            Ok(Arc::new(
                void_googlenews::connector::GoogleNewsConnector::new(
                    &connection.id,
                    keywords.clone(),
                    when,
                    language,
                    country,
                    poll_secs,
                ),
            ))
        }
        (
            ConnectorType::LinkedIn,
            ConnectionSettings::LinkedIn {
                api_key,
                dsn,
                account_id,
            },
        ) => {
            let sync_cfg = &crate::context::config().sync;
            Ok(Arc::new(void_linkedin::connector::LinkedInConnector::new(
                &connection.id,
                api_key,
                dsn,
                account_id,
                sync_cfg.linkedin_poll_interval_secs,
                sync_cfg.linkedin_backfill_days,
            )))
        }
        _ => anyhow::bail!(
            "Mismatched connector type and settings for '{}': type={}, settings don't match",
            connection.id,
            connection.connector_type
        ),
    }
}
