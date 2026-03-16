use std::path::Path;
use std::sync::Arc;
use tracing::debug;

use void_core::config::{expand_tilde, AccountConfig, AccountSettings, AccountType};
use void_core::connector::Connector;

pub fn build_connector(
    account: &AccountConfig,
    store_path: &Path,
) -> anyhow::Result<Arc<dyn Connector>> {
    debug!(account_id = %account.id, type = %account.account_type, "building connector");
    match (&account.account_type, &account.settings) {
        (
            AccountType::Slack,
            AccountSettings::Slack {
                user_token,
                app_token,
                exclude_channels,
            },
        ) => Ok(Arc::new(void_slack::connector::SlackConnector::new(
            &account.id,
            user_token,
            app_token,
            exclude_channels.clone(),
        ))),
        (AccountType::Gmail, AccountSettings::Gmail { credentials_file }) => {
            let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
            Ok(Arc::new(void_gmail::connector::GmailConnector::new(
                &account.id,
                cred_path.as_deref().and_then(|p| p.to_str()),
                store_path,
            )))
        }
        (
            AccountType::Calendar,
            AccountSettings::Calendar {
                credentials_file,
                calendar_ids,
            },
        ) => {
            let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
            Ok(Arc::new(void_calendar::connector::CalendarConnector::new(
                &account.id,
                cred_path.as_deref().and_then(|p| p.to_str()),
                calendar_ids.clone(),
                store_path,
            )))
        }
        (AccountType::WhatsApp, AccountSettings::WhatsApp {}) => {
            let session_db = store_path.join(format!("whatsapp-{}.db", account.id));
            Ok(Arc::new(void_whatsapp::connector::WhatsAppConnector::new(
                &account.id,
                session_db.to_str().unwrap_or(""),
            )))
        }
        (
            AccountType::Telegram,
            AccountSettings::Telegram {
                api_id, api_hash, ..
            },
        ) => {
            let session_path = store_path.join(format!("telegram-{}.json", account.id));
            Ok(Arc::new(void_telegram::connector::TelegramConnector::new(
                &account.id,
                session_path.to_str().unwrap_or(""),
                *api_id,
                api_hash.as_deref(),
            )))
        }
        _ => anyhow::bail!(
            "Mismatched account type and settings for '{}': type={}, settings don't match",
            account.id,
            account.account_type
        ),
    }
}
