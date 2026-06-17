use std::path::Path;
use std::sync::Arc;
use tracing::debug;

use void_core::config::{expand_tilde, ConnectionConfig, ConnectionSettings};
use void_core::connector::Connector;
use void_core::models::ConnectorType;
use void_whatsapp::connector::WhatsAppConnector;

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
