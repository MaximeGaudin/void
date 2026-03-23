use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

use super::types::CalendarConnector;
use crate::api::CalendarApiClient;

#[async_trait]
impl Connector for CalendarConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::Calendar
    }

    fn connection_id(&self) -> &str {
        &self.connection_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let creds = void_gmail::auth::load_client_credentials(self.credentials_file.as_deref())?;
        let token_path = void_gmail::auth::token_cache_path(&self.store_path, &self.connection_id);

        let scopes = "https://www.googleapis.com/auth/calendar.readonly \
                      https://www.googleapis.com/auth/calendar.events";
        let cache = void_gmail::auth::authorize_interactive(&creds, Some(scopes)).await?;
        cache.save(&token_path)?;

        let api = CalendarApiClient::new(&cache.access_token);
        let cals = api.list_calendars().await?;
        let count = cals.items.as_ref().map(|i| i.len()).unwrap_or(0);
        let calendar_list: Vec<&str> = cals
            .items
            .as_ref()
            .map(|items| items.iter().filter_map(|c| c.summary.as_deref()).collect())
            .unwrap_or_default();
        debug!(connection_id = %self.connection_id, calendars = count, calendar_list = ?calendar_list, "Calendar authenticated");
        info!(calendars = count, "Calendar authenticated");
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        self.initial_sync(&db).await?;

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!(connection_id = %self.connection_id, "Calendar sync cancelled");
                    break;
                }
                _ = interval.tick() => {
                    if let Err(e) = self.incremental_sync(&db).await {
                        error!(connection_id = %self.connection_id, "incremental sync error: {e}");
                    }
                }
            }
        }
        Ok(())
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        match self.get_client().await {
            Ok(api) => match api.list_calendars().await {
                Ok(cals) => {
                    let count = cals.items.as_ref().map(|i| i.len()).unwrap_or(0);
                    Ok(HealthStatus {
                        connection_id: self.connection_id.clone(),
                        connector_type: ConnectorType::Calendar,
                        ok: true,
                        message: format!("{count} calendar(s) accessible"),
                        last_sync: None,
                        message_count: None,
                    })
                }
                Err(e) => {
                    warn!(connection_id = %self.connection_id, error = %e, "Calendar health check API error");
                    Ok(HealthStatus {
                        connection_id: self.connection_id.clone(),
                        connector_type: ConnectorType::Calendar,
                        ok: false,
                        message: format!("API error: {e}"),
                        last_sync: None,
                        message_count: None,
                    })
                }
            },
            Err(e) => {
                warn!(connection_id = %self.connection_id, error = %e, "Calendar health check auth error");
                Ok(HealthStatus {
                    connection_id: self.connection_id.clone(),
                    connector_type: ConnectorType::Calendar,
                    ok: false,
                    message: format!("Auth error: {e}"),
                    last_sync: None,
                    message_count: None,
                })
            }
        }
    }

    async fn send_message(&self, _to: &str, _content: MessageContent) -> anyhow::Result<String> {
        anyhow::bail!("Calendar does not support send_message; use create_event instead")
    }

    async fn reply(
        &self,
        _message_id: &str,
        _content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
        anyhow::bail!("Calendar does not support reply")
    }
}
