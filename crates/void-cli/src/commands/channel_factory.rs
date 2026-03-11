use std::path::Path;
use std::sync::Arc;
use tracing::debug;

use void_core::channel::Channel;
use void_core::config::{expand_tilde, AccountConfig, AccountSettings, AccountType};

pub fn build_channel(
    account: &AccountConfig,
    store_path: &Path,
) -> anyhow::Result<Arc<dyn Channel>> {
    debug!(account_id = %account.id, type = %account.account_type, "building channel");
    match (&account.account_type, &account.settings) {
        (
            AccountType::Slack,
            AccountSettings::Slack {
                user_token,
                app_token,
                exclude_channels,
            },
        ) => Ok(Arc::new(void_slack::channel::SlackChannel::new(
            &account.id,
            user_token,
            app_token,
            exclude_channels.clone(),
        ))),
        (AccountType::Gmail, AccountSettings::Gmail { credentials_file }) => {
            let cred_path = expand_tilde(credentials_file);
            Ok(Arc::new(void_gmail::channel::GmailChannel::new(
                &account.id,
                cred_path.to_str().unwrap_or(""),
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
            let cred_path = expand_tilde(credentials_file);
            Ok(Arc::new(void_calendar::channel::CalendarChannel::new(
                &account.id,
                cred_path.to_str().unwrap_or(""),
                calendar_ids.clone(),
                store_path,
            )))
        }
        (AccountType::WhatsApp, AccountSettings::WhatsApp {}) => {
            let session_db = store_path.join(format!("whatsapp-{}.db", account.id));
            Ok(Arc::new(void_whatsapp::channel::WhatsAppChannel::new(
                &account.id,
                session_db.to_str().unwrap_or(""),
            )))
        }
        _ => anyhow::bail!(
            "Mismatched account type and settings for '{}': type={}, settings don't match",
            account.id,
            account.account_type
        ),
    }
}
