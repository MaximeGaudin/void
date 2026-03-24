use std::path::Path;
use std::sync::Arc;
use tracing::debug;

use void_core::config::{expand_tilde, ConnectionConfig, ConnectionSettings};
use void_core::connector::Connector;
use void_core::models::ConnectorType;

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
                exclude_channels,
                app_id,
            },
        ) => Ok(Arc::new(void_slack::connector::SlackConnector::new(
            &connection.id,
            user_token,
            app_token,
            exclude_channels.clone(),
            app_id.as_deref(),
            store_path,
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
            let session_db = store_path.join(format!("whatsapp-{}.db", connection.id));
            Ok(Arc::new(void_whatsapp::connector::WhatsAppConnector::new(
                &connection.id,
                session_db.to_str().unwrap_or(""),
            )))
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
            let poll_secs = void_core::config::VoidConfig::load_or_default(
                &void_core::config::default_config_path(),
            )
            .sync
            .hackernews_poll_interval_secs;
            Ok(Arc::new(
                void_hackernews::connector::HackerNewsConnector::new(
                    &connection.id,
                    keywords.clone(),
                    *min_score,
                    poll_secs,
                ),
            ))
        }
        _ => anyhow::bail!(
            "Mismatched connector type and settings for '{}': type={}, settings don't match",
            connection.id,
            connection.connector_type
        ),
    }
}
