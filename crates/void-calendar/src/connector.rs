use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

use crate::api::{
    AttendeeRequest, CalendarApiClient, ConferenceDataRequest, ConferenceSolutionKey,
    CreateConferenceRequest, EventDateTimeRequest, GoogleCalendarEvent, InsertEventRequest,
};

pub struct CalendarConnector {
    account_id: String,
    credentials_file: String,
    calendar_ids: Vec<String>,
    store_path: std::path::PathBuf,
}

impl CalendarConnector {
    pub fn new(
        account_id: &str,
        credentials_file: &str,
        calendar_ids: Vec<String>,
        store_path: &std::path::Path,
    ) -> Self {
        Self {
            account_id: account_id.to_string(),
            credentials_file: credentials_file.to_string(),
            calendar_ids: if calendar_ids.is_empty() {
                vec!["primary".to_string()]
            } else {
                calendar_ids
            },
            store_path: store_path.to_path_buf(),
        }
    }

    async fn get_client(&self) -> anyhow::Result<CalendarApiClient> {
        let token_path = void_gmail::auth::token_cache_path(&self.store_path, &self.account_id);
        let mut cache = void_gmail::auth::TokenCache::load(&token_path)?;

        let is_expired = cache
            .expires_at
            .map(|exp| chrono::Utc::now().timestamp() >= exp - 60)
            .unwrap_or(true);

        if is_expired {
            debug!(account_id = %self.account_id, "refreshing Calendar access token");
            if let Some(ref refresh_token) = cache.refresh_token {
                let creds = void_gmail::auth::load_client_credentials(&self.credentials_file)?;
                let http = reqwest::Client::new();
                cache =
                    void_gmail::auth::refresh_access_token(&http, &creds, refresh_token).await?;
                cache.save(&token_path)?;
            } else {
                anyhow::bail!("token expired and no refresh token. Run `void setup`");
            }
        }

        Ok(CalendarApiClient::new(&cache.access_token))
    }

    async fn initial_sync(&self, db: &Database) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        info!(account_id = %self.account_id, "starting Calendar initial sync");

        let now = chrono::Utc::now();
        let time_min = (now - chrono::Duration::days(30)).to_rfc3339();
        let time_max = (now + chrono::Duration::days(90)).to_rfc3339();

        let mut progress = void_core::progress::BackfillProgress::new("calendar", "events");
        progress.set_pages(self.calendar_ids.len() as u64);

        for cal_id in &self.calendar_ids {
            let resp = api
                .list_events(cal_id, Some(&time_min), Some(&time_max), None)
                .await?;
            progress.inc_page();

            if let Some(events) = &resp.items {
                for event in events {
                    if let Some(cal_event) = map_event(event, &self.account_id, cal_id) {
                        db.upsert_event(&cal_event)?;
                        progress.inc(1);
                    }
                }
            }

            if let Some(token) = &resp.next_sync_token {
                db.set_sync_state(&self.account_id, &format!("sync_token:{cal_id}"), token)?;
            }
        }

        progress.finish();
        info!(account_id = %self.account_id, events = progress.items, "Calendar initial sync complete");
        Ok(())
    }

    async fn incremental_sync(&self, db: &Database) -> anyhow::Result<()> {
        let api = self.get_client().await?;

        for cal_id in &self.calendar_ids {
            let key = format!("sync_token:{cal_id}");
            let Some(sync_token) = db.get_sync_state(&self.account_id, &key)? else {
                debug!(calendar = %cal_id, "no sync_token, skipping incremental");
                continue;
            };

            match api.list_events(cal_id, None, None, Some(&sync_token)).await {
                Ok(resp) => {
                    if let Some(events) = &resp.items {
                        for event in events {
                            if let Some(cal_event) = map_event(event, &self.account_id, cal_id) {
                                db.upsert_event(&cal_event)?;
                            }
                        }
                    }
                    if let Some(token) = &resp.next_sync_token {
                        db.set_sync_state(&self.account_id, &key, token)?;
                    }
                }
                Err(e) => {
                    // 410 Gone means syncToken is invalid, need full re-sync
                    if e.to_string().contains("410") {
                        info!(calendar = %cal_id, "syncToken invalidated, re-syncing");
                        self.initial_sync(db).await?;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn create_event(
        &self,
        title: &str,
        start: &str,
        end: &str,
        meet: bool,
        attendees: Option<&str>,
        db: &Database,
    ) -> anyhow::Result<CalendarEvent> {
        let api = self.get_client().await?;

        let cal_id = self
            .calendar_ids
            .first()
            .map(|s| s.as_str())
            .unwrap_or("primary");
        info!(account_id = %self.account_id, title = %title, calendar_id = %cal_id, "creating Calendar event");

        let timezone = "UTC".to_string();
        let attendee_list = attendees.map(|a| {
            a.split(',')
                .map(|email| AttendeeRequest {
                    email: email.trim().to_string(),
                })
                .collect::<Vec<_>>()
        });

        let conference_data = if meet {
            Some(ConferenceDataRequest {
                create_request: CreateConferenceRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    conference_solution_key: ConferenceSolutionKey {
                        key_type: "hangoutsMeet".to_string(),
                    },
                },
            })
        } else {
            None
        };

        let request = InsertEventRequest {
            summary: title.to_string(),
            description: None,
            start: EventDateTimeRequest {
                date_time: start.to_string(),
                time_zone: timezone.clone(),
            },
            end: EventDateTimeRequest {
                date_time: end.to_string(),
                time_zone: timezone,
            },
            attendees: attendee_list,
            conference_data,
        };

        let conference_version = if meet { Some(1) } else { None };
        let resp = api
            .insert_event(cal_id, &request, conference_version)
            .await?;

        let event_id = resp.id.as_deref().unwrap_or("new");
        debug!(account_id = %self.account_id, event_id = %event_id, "Calendar event created");

        let cal_event =
            map_event(&resp, &self.account_id, cal_id).unwrap_or_else(|| CalendarEvent {
                id: format!(
                    "{}-{}",
                    self.account_id,
                    resp.id.as_deref().unwrap_or("new")
                ),
                account_id: self.account_id.clone(),
                connector: "calendar".into(),
                external_id: resp.id.clone().unwrap_or_default(),
                title: title.to_string(),
                description: None,
                location: None,
                start_at: 0,
                end_at: 0,
                all_day: false,
                attendees: None,
                status: Some("confirmed".into()),
                calendar_name: Some(cal_id.into()),
                meet_link: None,
                metadata: None,
            });

        db.upsert_event(&cal_event)?;
        Ok(cal_event)
    }
}

#[async_trait]
impl Connector for CalendarConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::Calendar
    }

    fn account_id(&self) -> &str {
        &self.account_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        let cals = api.list_calendars().await?;
        let count = cals.items.as_ref().map(|i| i.len()).unwrap_or(0);
        let calendar_list: Vec<&str> = cals
            .items
            .as_ref()
            .map(|items| items.iter().filter_map(|c| c.summary.as_deref()).collect())
            .unwrap_or_default();
        debug!(account_id = %self.account_id, calendars = count, calendar_list = ?calendar_list, "Calendar authenticated");
        info!(calendars = count, "Calendar authenticated");
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        self.initial_sync(&db).await?;

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!(account_id = %self.account_id, "Calendar sync cancelled");
                    break;
                }
                _ = interval.tick() => {
                    if let Err(e) = self.incremental_sync(&db).await {
                        error!(account_id = %self.account_id, "incremental sync error: {e}");
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
                        account_id: self.account_id.clone(),
                        connector_type: ConnectorType::Calendar,
                        ok: true,
                        message: format!("{count} calendar(s) accessible"),
                        last_sync: None,
                        message_count: None,
                    })
                }
                Err(e) => {
                    warn!(account_id = %self.account_id, error = %e, "Calendar health check API error");
                    Ok(HealthStatus {
                        account_id: self.account_id.clone(),
                        connector_type: ConnectorType::Calendar,
                        ok: false,
                        message: format!("API error: {e}"),
                        last_sync: None,
                        message_count: None,
                    })
                }
            },
            Err(e) => {
                warn!(account_id = %self.account_id, error = %e, "Calendar health check auth error");
                Ok(HealthStatus {
                    account_id: self.account_id.clone(),
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

fn map_event(
    event: &GoogleCalendarEvent,
    account_id: &str,
    calendar_name: &str,
) -> Option<CalendarEvent> {
    let id = event.id.as_ref()?;

    let (start_at, all_day) = if let Some(ref start) = event.start {
        if let Some(ref dt) = start.date_time {
            (parse_rfc3339(dt), false)
        } else if let Some(ref d) = start.date {
            (parse_date(d), true)
        } else {
            (0, false)
        }
    } else {
        (0, false)
    };

    let end_at = event
        .end
        .as_ref()
        .and_then(|e| {
            e.date_time
                .as_deref()
                .map(parse_rfc3339)
                .or_else(|| e.date.as_deref().map(parse_date))
        })
        .unwrap_or(start_at);

    let meet_link = event
        .conference_data
        .as_ref()
        .and_then(|cd| cd.entry_points.as_ref())
        .and_then(|eps| {
            eps.iter().find_map(|ep| {
                if ep.entry_point_type.as_deref() == Some("video") {
                    ep.uri.clone()
                } else {
                    None
                }
            })
        });

    let attendees = event.attendees.as_ref().map(|atts| {
        serde_json::json!(atts
            .iter()
            .filter_map(|a| a.email.clone())
            .collect::<Vec<_>>())
    });

    Some(CalendarEvent {
        id: format!("{account_id}-{id}"),
        account_id: account_id.to_string(),
        connector: "calendar".into(),
        external_id: id.clone(),
        title: event.summary.clone().unwrap_or_else(|| "(no title)".into()),
        description: event.description.clone(),
        location: event.location.clone(),
        start_at,
        end_at,
        all_day,
        attendees,
        status: event.status.clone(),
        calendar_name: Some(calendar_name.to_string()),
        meet_link,
        metadata: None,
    })
}

fn parse_rfc3339(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.timestamp())
        .unwrap_or(0)
}

fn parse_date(s: &str) -> i64 {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::*;

    #[test]
    fn map_event_basic() {
        let event = GoogleCalendarEvent {
            id: Some("event123".into()),
            summary: Some("Team Standup".into()),
            description: None,
            location: Some("Room A".into()),
            start: Some(EventDateTime {
                date_time: Some("2025-03-15T10:00:00Z".into()),
                date: None,
            }),
            end: Some(EventDateTime {
                date_time: Some("2025-03-15T10:30:00Z".into()),
                date: None,
            }),
            status: Some("confirmed".into()),
            attendees: None,
            conference_data: None,
            html_link: None,
        };

        let result = map_event(&event, "my-cal", "primary").unwrap();
        assert_eq!(result.title, "Team Standup");
        assert_eq!(result.location.as_deref(), Some("Room A"));
        assert!(!result.all_day);
    }

    #[test]
    fn map_event_all_day() {
        let event = GoogleCalendarEvent {
            id: Some("e2".into()),
            summary: Some("Holiday".into()),
            description: None,
            location: None,
            start: Some(EventDateTime {
                date_time: None,
                date: Some("2025-12-25".into()),
            }),
            end: Some(EventDateTime {
                date_time: None,
                date: Some("2025-12-26".into()),
            }),
            status: Some("confirmed".into()),
            attendees: None,
            conference_data: None,
            html_link: None,
        };

        let result = map_event(&event, "my-cal", "primary").unwrap();
        assert!(result.all_day);
    }

    #[test]
    fn map_event_with_meet() {
        let event = GoogleCalendarEvent {
            id: Some("e3".into()),
            summary: Some("1:1".into()),
            description: None,
            location: None,
            start: Some(EventDateTime {
                date_time: Some("2025-03-15T14:00:00Z".into()),
                date: None,
            }),
            end: Some(EventDateTime {
                date_time: Some("2025-03-15T14:30:00Z".into()),
                date: None,
            }),
            status: Some("confirmed".into()),
            attendees: Some(vec![EventAttendee {
                email: Some("alice@example.com".into()),
                response_status: Some("accepted".into()),
            }]),
            conference_data: Some(ConferenceData {
                entry_points: Some(vec![EntryPoint {
                    entry_point_type: Some("video".into()),
                    uri: Some("https://meet.google.com/abc-defg-hij".into()),
                }]),
            }),
            html_link: None,
        };

        let result = map_event(&event, "my-cal", "primary").unwrap();
        assert_eq!(
            result.meet_link.as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
        assert!(result.attendees.is_some());
    }

    #[test]
    fn parse_rfc3339_valid() {
        let ts = parse_rfc3339("2025-03-15T10:00:00Z");
        assert!(ts > 1_740_000_000);
        assert!(ts < 1_750_000_000);
    }

    #[test]
    fn parse_rfc3339_invalid_returns_zero() {
        assert_eq!(parse_rfc3339("not-a-date"), 0);
    }

    #[test]
    fn parse_date_valid() {
        let ts = parse_date("2025-12-25");
        assert!(ts > 1_765_000_000);
    }

    #[test]
    fn parse_date_invalid_returns_zero() {
        assert_eq!(parse_date("invalid"), 0);
    }
}
