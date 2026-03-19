use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

use crate::api::{
    AttendeeRequest, AttendeeResponseRequest, CalendarApiClient, ConferenceDataRequest,
    ConferenceSolutionKey, CreateConferenceRequest, EventDateTimeRequest, GoogleCalendarEvent,
    InsertEventRequest, UpdateEventRequest,
};

/// Parameters for creating a new calendar event.
#[derive(Debug, Clone)]
pub struct CreateEventParams<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub start: &'a str,
    pub end: &'a str,
    pub meet: bool,
    pub attendees: Option<&'a str>,
}

/// Parameters for updating an existing calendar event.
#[derive(Debug, Clone)]
pub struct UpdateEventParams<'a> {
    pub event_id: &'a str,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub start: Option<&'a str>,
    pub end: Option<&'a str>,
    pub send_updates: Option<&'a str>,
}

pub struct CalendarConnector {
    account_id: String,
    credentials_file: Option<String>,
    calendar_ids: Vec<String>,
    store_path: std::path::PathBuf,
}

impl CalendarConnector {
    pub fn new(
        account_id: &str,
        credentials_file: Option<&str>,
        calendar_ids: Vec<String>,
        store_path: &std::path::Path,
    ) -> Self {
        Self {
            account_id: account_id.to_string(),
            credentials_file: credentials_file.map(|s| s.to_string()),
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

    async fn initial_sync(&self, db: &Database) -> anyhow::Result<()> {
        let has_sync_tokens = self.calendar_ids.iter().any(|cal_id| {
            db.get_sync_state(&self.account_id, &format!("sync_token:{cal_id}"))
                .ok()
                .flatten()
                .is_some()
        });

        if has_sync_tokens {
            debug!(account_id = %self.account_id, "skipping initial sync — sync tokens exist, incremental will catch up");
            return Ok(());
        }

        let api = self.get_client().await?;
        info!(account_id = %self.account_id, "starting Calendar initial sync");

        let now = chrono::Utc::now();
        let time_min = (now - chrono::Duration::days(30)).to_rfc3339();
        let time_max = (now + chrono::Duration::days(90)).to_rfc3339();

        let mut progress = void_core::progress::BackfillProgress::new(
            &format!("calendar:{}", self.account_id),
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
                        if let Some(cal_event) = map_event(event, &self.account_id, cal_id) {
                            db.upsert_event(&cal_event)?;
                            progress.inc(1);
                        }
                    }
                }

                if let Some(token) = &resp.next_sync_token {
                    db.set_sync_state(&self.account_id, &format!("sync_token:{cal_id}"), token)?;
                }

                page_token = resp.next_page_token;
                if page_token.is_none() {
                    break;
                }
            }
            progress.inc_page();
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

            match api
                .list_events(cal_id, None, None, Some(&sync_token), None)
                .await
            {
                Ok(resp) => {
                    if let Some(events) = &resp.items {
                        for event in events {
                            let event_id = event.id.as_deref().unwrap_or("");
                            if event.status.as_deref() == Some("cancelled") {
                                if db.delete_event(&self.account_id, event_id)? {
                                    eprintln!(
                                        "[calendar:{}] deleted: {}",
                                        self.account_id, event_id
                                    );
                                }
                                continue;
                            }
                            if let Some(cal_event) = map_event(event, &self.account_id, cal_id) {
                                eprintln!(
                                    "[calendar:{}] new: {}",
                                    self.account_id, cal_event.title
                                );
                                db.upsert_event(&cal_event)?;
                            }
                        }
                    }
                    if let Some(token) = &resp.next_sync_token {
                        db.set_sync_state(&self.account_id, &key, token)?;
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

    pub async fn create_event(
        &self,
        params: &CreateEventParams<'_>,
        db: &Database,
    ) -> anyhow::Result<CalendarEvent> {
        let api = self.get_client().await?;

        let cal_id = self
            .calendar_ids
            .first()
            .map(|s| s.as_str())
            .unwrap_or("primary");
        info!(account_id = %self.account_id, title = %params.title, calendar_id = %cal_id, "creating Calendar event");

        let timezone = "UTC".to_string();
        let attendee_list = params.attendees.map(|a| {
            a.split(',')
                .map(|email| AttendeeRequest {
                    email: email.trim().to_string(),
                })
                .collect::<Vec<_>>()
        });

        let conference_data = if params.meet {
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
            summary: params.title.to_string(),
            description: params.description.map(|d| d.to_string()),
            start: EventDateTimeRequest {
                date_time: params.start.to_string(),
                time_zone: timezone.clone(),
            },
            end: EventDateTimeRequest {
                date_time: params.end.to_string(),
                time_zone: timezone,
            },
            attendees: attendee_list,
            conference_data,
        };

        let conference_version = if params.meet { Some(1) } else { None };
        let send_notif = if request.attendees.is_some() {
            Some("all")
        } else {
            None
        };
        let resp = api
            .insert_event(cal_id, &request, conference_version, send_notif)
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
                title: params.title.to_string(),
                description: params.description.map(|d| d.to_string()),
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

    pub async fn update_event(
        &self,
        params: &UpdateEventParams<'_>,
        db: &Database,
    ) -> anyhow::Result<CalendarEvent> {
        let api = self.get_client().await?;
        let cal_id = self
            .calendar_ids
            .first()
            .map(|s| s.as_str())
            .unwrap_or("primary");
        info!(account_id = %self.account_id, event_id = %params.event_id, "updating Calendar event");

        let timezone = "UTC".to_string();
        let update = UpdateEventRequest {
            summary: params.title.map(|s| s.to_string()),
            description: params.description.map(|s| s.to_string()),
            location: None,
            start: params.start.map(|s| EventDateTimeRequest {
                date_time: s.to_string(),
                time_zone: timezone.clone(),
            }),
            end: params.end.map(|s| EventDateTimeRequest {
                date_time: s.to_string(),
                time_zone: timezone,
            }),
            attendees: None,
        };

        let resp = api
            .update_event(cal_id, params.event_id, &update, params.send_updates)
            .await?;
        let cal_event =
            map_event(&resp, &self.account_id, cal_id).unwrap_or_else(|| CalendarEvent {
                id: format!("{}-{}", self.account_id, params.event_id),
                account_id: self.account_id.clone(),
                connector: "calendar".into(),
                external_id: params.event_id.to_string(),
                title: params.title.unwrap_or("(updated)").to_string(),
                description: params.description.map(|s| s.to_string()),
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

    pub async fn delete_event(
        &self,
        event_id: &str,
        send_updates: Option<&str>,
    ) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        let cal_id = self
            .calendar_ids
            .first()
            .map(|s| s.as_str())
            .unwrap_or("primary");
        info!(account_id = %self.account_id, event_id, "deleting Calendar event");
        api.delete_event(cal_id, event_id, send_updates)
            .await
            .map_err(Into::into)
    }

    pub async fn respond_to_event(
        &self,
        event_id: &str,
        email: &str,
        status: &str,
        comment: Option<&str>,
        db: &Database,
    ) -> anyhow::Result<CalendarEvent> {
        let api = self.get_client().await?;
        let cal_id = self
            .calendar_ids
            .first()
            .map(|s| s.as_str())
            .unwrap_or("primary");
        info!(account_id = %self.account_id, event_id, status, "responding to Calendar event");

        let event = api.get_event(cal_id, event_id).await?;
        let mut attendees_req: Vec<AttendeeResponseRequest> = event
            .attendees
            .as_ref()
            .map(|atts| {
                atts.iter()
                    .map(|a| {
                        let is_me = a.email.as_deref() == Some(email);
                        AttendeeResponseRequest {
                            email: a.email.clone().unwrap_or_default(),
                            response_status: if is_me {
                                Some(status.to_string())
                            } else {
                                a.response_status.clone()
                            },
                            comment: if is_me {
                                comment.map(|c| c.to_string())
                            } else {
                                None
                            },
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        if !attendees_req.iter().any(|a| a.email == email) {
            attendees_req.push(AttendeeResponseRequest {
                email: email.to_string(),
                response_status: Some(status.to_string()),
                comment: comment.map(|c| c.to_string()),
            });
        }

        let update = UpdateEventRequest {
            attendees: Some(attendees_req),
            ..Default::default()
        };

        let resp = api
            .update_event(cal_id, event_id, &update, Some("all"))
            .await?;
        let cal_event =
            map_event(&resp, &self.account_id, cal_id).unwrap_or_else(|| CalendarEvent {
                id: format!("{}-{}", self.account_id, event_id),
                account_id: self.account_id.clone(),
                connector: "calendar".into(),
                external_id: event_id.to_string(),
                title: event.summary.unwrap_or_default(),
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

    pub async fn search_events(
        &self,
        query: &str,
        time_min: Option<&str>,
        time_max: Option<&str>,
        db: &Database,
    ) -> anyhow::Result<Vec<CalendarEvent>> {
        let api = self.get_client().await?;
        let mut results = Vec::new();

        for cal_id in &self.calendar_ids {
            let resp = api.search_events(cal_id, query, time_min, time_max).await?;
            if let Some(events) = &resp.items {
                for event in events {
                    if let Some(cal_event) = map_event(event, &self.account_id, cal_id) {
                        db.upsert_event(&cal_event)?;
                        results.push(cal_event);
                    }
                }
            }
        }

        Ok(results)
    }

    pub async fn list_calendars(&self) -> anyhow::Result<Vec<crate::api::CalendarListEntry>> {
        let api = self.get_client().await?;
        let resp = api.list_calendars().await?;
        Ok(resp.items.unwrap_or_default())
    }

    pub async fn check_availability(
        &self,
        time_min: &str,
        time_max: &str,
        emails: &[String],
    ) -> anyhow::Result<crate::api::FreeBusyResponse> {
        let api = self.get_client().await?;
        api.freebusy(time_min, time_max, emails)
            .await
            .map_err(Into::into)
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
        let creds = void_gmail::auth::load_client_credentials(self.credentials_file.as_deref())?;
        let token_path = void_gmail::auth::token_cache_path(&self.store_path, &self.account_id);

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
    use void_core::db::Database;
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Runs the initial sync pagination loop using a pre-built API client (for testing without tokens).
    async fn run_initial_sync_with_client(
        api: &CalendarApiClient,
        db: &Database,
        account_id: &str,
        calendar_ids: &[String],
        time_min: &str,
        time_max: &str,
    ) -> anyhow::Result<()> {
        for cal_id in calendar_ids {
            let mut page_token: Option<String> = None;
            loop {
                let resp = api
                    .list_events(
                        cal_id,
                        Some(time_min),
                        Some(time_max),
                        None,
                        page_token.as_deref(),
                    )
                    .await?;

                if let Some(events) = &resp.items {
                    for event in events {
                        if let Some(cal_event) = map_event(event, account_id, cal_id) {
                            db.upsert_event(&cal_event)?;
                        }
                    }
                }

                if let Some(token) = &resp.next_sync_token {
                    db.set_sync_state(account_id, &format!("sync_token:{cal_id}"), token)?;
                }

                page_token = resp.next_page_token;
                if page_token.is_none() {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Runs the incremental sync loop using a pre-built API client (for testing without tokens).
    async fn run_incremental_sync_with_client(
        api: &CalendarApiClient,
        db: &Database,
        account_id: &str,
        calendar_ids: &[String],
        time_min: &str,
        time_max: &str,
    ) -> anyhow::Result<()> {
        for cal_id in calendar_ids {
            let key = format!("sync_token:{cal_id}");
            let Some(sync_token) = db.get_sync_state(account_id, &key)? else {
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
                                db.delete_event(account_id, event_id)?;
                                continue;
                            }
                            if let Some(cal_event) = map_event(event, account_id, cal_id) {
                                db.upsert_event(&cal_event)?;
                            }
                        }
                    }
                    if let Some(token) = &resp.next_sync_token {
                        db.set_sync_state(account_id, &key, token)?;
                    }
                }
                Err(e) => {
                    if e.to_string().contains("410") {
                        run_initial_sync_with_client(
                            api,
                            db,
                            account_id,
                            calendar_ids,
                            time_min,
                            time_max,
                        )
                        .await?;
                    } else {
                        return Err(e.into());
                    }
                }
            }
        }
        Ok(())
    }

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

    #[tokio::test]
    async fn api_list_events_paginates() {
        let mock_server = MockServer::start().await;

        let page1_body = r#"{
            "items": [
                {"id": "ev1", "summary": "Event 1", "start": {"dateTime": "2026-03-11T10:00:00Z"}, "end": {"dateTime": "2026-03-11T11:00:00Z"}, "status": "confirmed"},
                {"id": "ev2", "summary": "Event 2", "start": {"dateTime": "2026-03-11T12:00:00Z"}, "end": {"dateTime": "2026-03-11T13:00:00Z"}, "status": "confirmed"}
            ],
            "nextPageToken": "page2"
        }"#;

        let page2_body = r#"{
            "items": [
                {"id": "ev3", "summary": "Event 3", "start": {"dateTime": "2026-03-11T14:00:00Z"}, "end": {"dateTime": "2026-03-11T15:00:00Z"}, "status": "confirmed"}
            ],
            "nextSyncToken": "sync123"
        }"#;

        Mock::given(method("GET"))
            .and(path("/calendar/v3/calendars/primary/events"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page1_body))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/calendar/v3/calendars/primary/events"))
            .and(query_param("pageToken", "page2"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page2_body))
            .mount(&mock_server)
            .await;

        let api = CalendarApiClient::with_base_url("test-token", &mock_server.uri());
        let now = chrono::Utc::now();
        let time_min = (now - chrono::Duration::days(30)).to_rfc3339();
        let time_max = (now + chrono::Duration::days(90)).to_rfc3339();

        let mut total_events = 0;
        let mut sync_token = None;
        let mut page_token: Option<String> = None;
        loop {
            let resp = api
                .list_events(
                    "primary",
                    Some(&time_min),
                    Some(&time_max),
                    None,
                    page_token.as_deref(),
                )
                .await
                .unwrap();

            total_events += resp.items.as_ref().map(|i| i.len()).unwrap_or(0);
            if let Some(t) = resp.next_sync_token {
                sync_token = Some(t);
            }
            page_token = resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        assert_eq!(total_events, 3);
        assert_eq!(sync_token.as_deref(), Some("sync123"));
    }

    #[tokio::test]
    async fn initial_sync_stores_all_pages_in_db() {
        let mock_server = MockServer::start().await;

        let page1_body = r#"{
            "items": [
                {"id": "ev1", "summary": "Event 1", "start": {"dateTime": "2026-03-11T10:00:00Z"}, "end": {"dateTime": "2026-03-11T11:00:00Z"}, "status": "confirmed"},
                {"id": "ev2", "summary": "Event 2", "start": {"dateTime": "2026-03-11T12:00:00Z"}, "end": {"dateTime": "2026-03-11T13:00:00Z"}, "status": "confirmed"}
            ],
            "nextPageToken": "page2"
        }"#;

        let page2_body = r#"{
            "items": [
                {"id": "ev3", "summary": "Event 3", "start": {"dateTime": "2026-03-11T14:00:00Z"}, "end": {"dateTime": "2026-03-11T15:00:00Z"}, "status": "confirmed"}
            ],
            "nextSyncToken": "sync123"
        }"#;

        Mock::given(method("GET"))
            .and(path("/calendar/v3/calendars/primary/events"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page1_body))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/calendar/v3/calendars/primary/events"))
            .and(query_param("pageToken", "page2"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page2_body))
            .mount(&mock_server)
            .await;

        let api = CalendarApiClient::with_base_url("test-token", &mock_server.uri());
        let db = Database::open_in_memory().unwrap();
        let now = chrono::Utc::now();
        let time_min = (now - chrono::Duration::days(30)).to_rfc3339();
        let time_max = (now + chrono::Duration::days(90)).to_rfc3339();
        let calendar_ids = vec!["primary".to_string()];

        run_initial_sync_with_client(&api, &db, "test-cal", &calendar_ids, &time_min, &time_max)
            .await
            .unwrap();

        let events = db.list_events(None, None, None, None, 100).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].title, "Event 1");
        assert_eq!(events[1].title, "Event 2");
        assert_eq!(events[2].title, "Event 3");

        let stored_token = db.get_sync_state("test-cal", "sync_token:primary").unwrap();
        assert_eq!(stored_token.as_deref(), Some("sync123"));
    }

    #[tokio::test]
    async fn incremental_sync_uses_sync_token() {
        let mock_server = MockServer::start().await;

        let incremental_body = r#"{
            "items": [
                {"id": "ev4", "summary": "New Event", "start": {"dateTime": "2026-03-12T10:00:00Z"}, "end": {"dateTime": "2026-03-12T11:00:00Z"}, "status": "confirmed"}
            ],
            "nextSyncToken": "new-sync-token"
        }"#;

        Mock::given(method("GET"))
            .and(path("/calendar/v3/calendars/primary/events"))
            .and(query_param("syncToken", "old-token"))
            .respond_with(ResponseTemplate::new(200).set_body_string(incremental_body))
            .mount(&mock_server)
            .await;

        let api = CalendarApiClient::with_base_url("test-token", &mock_server.uri());
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("test-cal", "sync_token:primary", "old-token")
            .unwrap();

        let calendar_ids = vec!["primary".to_string()];
        let now = chrono::Utc::now();
        let time_min = (now - chrono::Duration::days(30)).to_rfc3339();
        let time_max = (now + chrono::Duration::days(90)).to_rfc3339();

        run_incremental_sync_with_client(
            &api,
            &db,
            "test-cal",
            &calendar_ids,
            &time_min,
            &time_max,
        )
        .await
        .unwrap();

        let events = db.list_events(None, None, None, None, 100).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "New Event");
        assert_eq!(events[0].external_id, "ev4");

        let stored_token = db.get_sync_state("test-cal", "sync_token:primary").unwrap();
        assert_eq!(stored_token.as_deref(), Some("new-sync-token"));
    }

    #[tokio::test]
    async fn incremental_sync_410_triggers_resync() {
        let mock_server = MockServer::start().await;

        // 410 when syncToken is provided
        Mock::given(method("GET"))
            .and(path("/calendar/v3/calendars/primary/events"))
            .and(query_param("syncToken", "invalid-old-token"))
            .respond_with(ResponseTemplate::new(410))
            .mount(&mock_server)
            .await;

        // Full resync (timeMin/timeMax, no syncToken)
        let full_sync_body = r#"{
            "items": [
                {"id": "ev1", "summary": "Resynced Event", "start": {"dateTime": "2026-03-11T10:00:00Z"}, "end": {"dateTime": "2026-03-11T11:00:00Z"}, "status": "confirmed"}
            ],
            "nextSyncToken": "fresh-sync-token"
        }"#;

        Mock::given(method("GET"))
            .and(path("/calendar/v3/calendars/primary/events"))
            .and(query_param_is_missing("syncToken"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_string(full_sync_body))
            .mount(&mock_server)
            .await;

        let api = CalendarApiClient::with_base_url("test-token", &mock_server.uri());
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("test-cal", "sync_token:primary", "invalid-old-token")
            .unwrap();

        let calendar_ids = vec!["primary".to_string()];
        let now = chrono::Utc::now();
        let time_min = (now - chrono::Duration::days(30)).to_rfc3339();
        let time_max = (now + chrono::Duration::days(90)).to_rfc3339();

        run_incremental_sync_with_client(
            &api,
            &db,
            "test-cal",
            &calendar_ids,
            &time_min,
            &time_max,
        )
        .await
        .unwrap();

        let events = db.list_events(None, None, None, None, 100).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Resynced Event");

        let stored_token = db.get_sync_state("test-cal", "sync_token:primary").unwrap();
        assert_eq!(stored_token.as_deref(), Some("fresh-sync-token"));
    }

    #[tokio::test]
    async fn incremental_sync_deletes_cancelled_events() {
        let mock_server = MockServer::start().await;

        let incremental_body = r#"{
            "items": [
                {"id": "ev1", "status": "cancelled"},
                {"id": "ev5", "summary": "New Event", "start": {"dateTime": "2026-03-12T10:00:00Z"}, "end": {"dateTime": "2026-03-12T11:00:00Z"}, "status": "confirmed"}
            ],
            "nextSyncToken": "after-delete-token"
        }"#;

        Mock::given(method("GET"))
            .and(path("/calendar/v3/calendars/primary/events"))
            .and(query_param("syncToken", "pre-delete-token"))
            .respond_with(ResponseTemplate::new(200).set_body_string(incremental_body))
            .mount(&mock_server)
            .await;

        let api = CalendarApiClient::with_base_url("test-token", &mock_server.uri());
        let db = Database::open_in_memory().unwrap();

        let existing = CalendarEvent {
            id: "test-cal-ev1".into(),
            account_id: "test-cal".into(),
            connector: "calendar".into(),
            external_id: "ev1".into(),
            title: "To Be Deleted".into(),
            description: None,
            location: None,
            start_at: 1_710_000_000,
            end_at: 1_710_003_600,
            all_day: false,
            attendees: None,
            status: Some("confirmed".into()),
            calendar_name: Some("primary".into()),
            meet_link: None,
            metadata: None,
        };
        db.upsert_event(&existing).unwrap();
        assert_eq!(
            db.list_events(None, None, None, None, 100).unwrap().len(),
            1
        );

        db.set_sync_state("test-cal", "sync_token:primary", "pre-delete-token")
            .unwrap();

        let calendar_ids = vec!["primary".to_string()];
        let now = chrono::Utc::now();
        let time_min = (now - chrono::Duration::days(30)).to_rfc3339();
        let time_max = (now + chrono::Duration::days(90)).to_rfc3339();

        run_incremental_sync_with_client(
            &api,
            &db,
            "test-cal",
            &calendar_ids,
            &time_min,
            &time_max,
        )
        .await
        .unwrap();

        let events = db.list_events(None, None, None, None, 100).unwrap();
        assert_eq!(
            events.len(),
            1,
            "cancelled event should be deleted, new one added"
        );
        assert_eq!(events[0].external_id, "ev5");
        assert_eq!(events[0].title, "New Event");

        let stored_token = db.get_sync_state("test-cal", "sync_token:primary").unwrap();
        assert_eq!(stored_token.as_deref(), Some("after-delete-token"));
    }
}
