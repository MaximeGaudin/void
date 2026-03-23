use tracing::{debug, info};

use void_core::db::Database;

use super::mapping::map_event;
use super::types::CalendarConnector;
use crate::api::CalendarApiClient;

impl CalendarConnector {
    pub(crate) async fn get_client(&self) -> anyhow::Result<CalendarApiClient> {
        let token_path = void_gmail::auth::token_cache_path(&self.store_path, &self.connection_id);
        let mut cache = void_gmail::auth::TokenCache::load(&token_path)?;

        let is_expired = cache
            .expires_at
            .map(|exp| chrono::Utc::now().timestamp() >= exp - 60)
            .unwrap_or(true);

        if is_expired {
            debug!(connection_id = %self.connection_id, "refreshing Calendar access token");
            if let Some(ref refresh_token) = cache.refresh_token {
                let creds =
                    void_gmail::auth::load_client_credentials(self.credentials_file.as_deref())?;
                let http = void_gmail::api::build_http_client();
                cache =
                    void_gmail::auth::refresh_access_token(&http, &creds, refresh_token).await?;
                cache.save(&token_path)?;
            } else {
                anyhow::bail!("token expired and no refresh token. Run `void setup`");
            }
        }

        Ok(CalendarApiClient::new(&cache.access_token))
    }

    pub(crate) async fn initial_sync(&self, db: &Database) -> anyhow::Result<()> {
        let has_sync_tokens = self.calendar_ids.iter().any(|cal_id| {
            db.get_sync_state(&self.connection_id, &format!("sync_token:{cal_id}"))
                .ok()
                .flatten()
                .is_some()
        });

        if has_sync_tokens {
            debug!(connection_id = %self.connection_id, "skipping initial sync — sync tokens exist, incremental will catch up");
            return Ok(());
        }

        let api = self.get_client().await?;
        info!(connection_id = %self.connection_id, "starting Calendar initial sync");

        let now = chrono::Utc::now();
        let time_min = (now - chrono::Duration::days(30)).to_rfc3339();
        let time_max = (now + chrono::Duration::days(90)).to_rfc3339();

        let mut progress = void_core::progress::BackfillProgress::new(
            &format!("calendar:{}", self.connection_id),
            "events",
        );
        progress.set_pages(self.calendar_ids.len() as u64);

        for cal_id in &self.calendar_ids {
            let mut page_token: Option<String> = None;
            loop {
                let resp = api
                    .list_events(
                        cal_id,
                        Some(&time_min),
                        Some(&time_max),
                        None,
                        page_token.as_deref(),
                    )
                    .await?;

                if let Some(events) = &resp.items {
                    for event in events {
                        if let Some(cal_event) = map_event(event, &self.connection_id, cal_id) {
                            db.upsert_event(&cal_event)?;
                            progress.inc(1);
                        }
                    }
                }

                if let Some(token) = &resp.next_sync_token {
                    db.set_sync_state(&self.connection_id, &format!("sync_token:{cal_id}"), token)?;
                }

                page_token = resp.next_page_token;
                if page_token.is_none() {
                    break;
                }
            }
            progress.inc_page();
        }

        progress.finish();
        info!(connection_id = %self.connection_id, events = progress.items, "Calendar initial sync complete");
        Ok(())
    }

    pub(crate) async fn incremental_sync(&self, db: &Database) -> anyhow::Result<()> {
        let api = self.get_client().await?;

        for cal_id in &self.calendar_ids {
            let key = format!("sync_token:{cal_id}");
            let Some(sync_token) = db.get_sync_state(&self.connection_id, &key)? else {
                debug!(calendar = %cal_id, "no sync_token, skipping incremental");
                continue;
            };

            match api
                .list_events(cal_id, None, None, Some(&sync_token), None)
                .await
            {
                Ok(resp) => {
                    if let Some(events) = &resp.items {
                        for event in events {
                            let event_id = event.id.as_deref().unwrap_or("");
                            if event.status.as_deref() == Some("cancelled") {
                                if db.delete_event(&self.connection_id, event_id)? {
                                    eprintln!(
                                        "[calendar:{}] deleted: {}",
                                        self.connection_id, event_id
                                    );
                                }
                                continue;
                            }
                            if let Some(cal_event) = map_event(event, &self.connection_id, cal_id) {
                                eprintln!(
                                    "[calendar:{}] new: {}",
                                    self.connection_id, cal_event.title
                                );
                                db.upsert_event(&cal_event)?;
                            }
                        }
                    }
                    if let Some(token) = &resp.next_sync_token {
                        db.set_sync_state(&self.connection_id, &key, token)?;
                    }
                }
                Err(e) => {
                    if e.to_string().contains("410") {
                        info!(calendar = %cal_id, "syncToken invalidated, re-syncing");
                        self.initial_sync(db).await?;
                    } else {
                        return Err(e.into());
                    }
                }
            }
        }
        Ok(())
    }
}
