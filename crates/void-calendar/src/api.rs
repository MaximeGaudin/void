use anyhow::Context;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Google Calendar API client.
pub struct CalendarApiClient {
    http: reqwest::Client,
    access_token: String,
}

impl CalendarApiClient {
    pub fn new(access_token: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            access_token: access_token.to_string(),
        }
    }

    pub async fn list_calendars(&self) -> anyhow::Result<CalendarListResponse> {
        debug!("calendar: list_calendars");
        let resp: CalendarListResponse = self
            .http
            .get("https://www.googleapis.com/calendar/v3/users/me/calendarList")
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .json()
            .await
            .context("calendar: failed to list calendars")?;
        let count = resp.items.as_ref().map(|i| i.len()).unwrap_or(0);
        debug!(count, "calendar: list_calendars ok");
        Ok(resp)
    }

    pub async fn list_events(
        &self,
        calendar_id: &str,
        time_min: Option<&str>,
        time_max: Option<&str>,
        sync_token: Option<&str>,
        page_token: Option<&str>,
    ) -> anyhow::Result<EventListResponse> {
        debug!(
            calendar_id,
            time_min = ?time_min,
            time_max = ?time_max,
            "calendar: list_events"
        );
        let mut params: Vec<(&str, String)> = vec![
            ("singleEvents", "true".into()),
            ("orderBy", "startTime".into()),
            ("maxResults", "2500".into()),
        ];
        if let Some(t) = time_min {
            params.push(("timeMin", t.into()));
        }
        if let Some(t) = time_max {
            params.push(("timeMax", t.into()));
        }
        if let Some(st) = sync_token {
            params.push(("syncToken", st.into()));
        }
        if let Some(pt) = page_token {
            params.push(("pageToken", pt.into()));
        }

        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{}/events",
            urlencoded(calendar_id)
        );
        let resp: EventListResponse = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&params)
            .send()
            .await?
            .json()
            .await
            .context("calendar: failed to list events")?;
        let count = resp.items.as_ref().map(|i| i.len()).unwrap_or(0);
        let has_sync_token = resp.next_sync_token.is_some();
        let has_page_token = resp.next_page_token.is_some();
        debug!(
            count,
            has_sync_token, has_page_token, "calendar: list_events ok"
        );
        Ok(resp)
    }

    pub async fn insert_event(
        &self,
        calendar_id: &str,
        event: &InsertEventRequest,
        conference_data_version: Option<u32>,
    ) -> anyhow::Result<GoogleCalendarEvent> {
        debug!(
            calendar_id,
            summary = event.summary.as_str(),
            "calendar: insert_event"
        );
        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{}/events",
            urlencoded(calendar_id)
        );
        let mut req = self
            .http
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(event);

        if let Some(v) = conference_data_version {
            req = req.query(&[("conferenceDataVersion", v.to_string())]);
        }

        let resp: GoogleCalendarEvent = req
            .send()
            .await?
            .json()
            .await
            .context("calendar: failed to insert event")?;
        let event_id = resp.id.as_deref().unwrap_or("(none)");
        debug!(event_id, "calendar: insert_event ok");
        Ok(resp)
    }
}

fn urlencoded(s: &str) -> String {
    s.replace('#', "%23").replace(' ', "%20")
}

// -- Calendar API types --

#[derive(Debug, Deserialize)]
pub struct CalendarListResponse {
    pub items: Option<Vec<CalendarListEntry>>,
}

#[derive(Debug, Deserialize)]
pub struct CalendarListEntry {
    pub id: String,
    pub summary: Option<String>,
    pub primary: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventListResponse {
    pub items: Option<Vec<GoogleCalendarEvent>>,
    pub next_sync_token: Option<String>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleCalendarEvent {
    pub id: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start: Option<EventDateTime>,
    pub end: Option<EventDateTime>,
    pub status: Option<String>,
    pub attendees: Option<Vec<EventAttendee>>,
    pub conference_data: Option<ConferenceData>,
    pub html_link: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDateTime {
    pub date_time: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EventAttendee {
    pub email: Option<String>,
    #[serde(rename = "responseStatus")]
    pub response_status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConferenceData {
    pub entry_points: Option<Vec<EntryPoint>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPoint {
    pub entry_point_type: Option<String>,
    pub uri: Option<String>,
}

// -- Request types --

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsertEventRequest {
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub start: EventDateTimeRequest,
    pub end: EventDateTimeRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attendees: Option<Vec<AttendeeRequest>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conference_data: Option<ConferenceDataRequest>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDateTimeRequest {
    pub date_time: String,
    pub time_zone: String,
}

#[derive(Debug, Serialize)]
pub struct AttendeeRequest {
    pub email: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConferenceDataRequest {
    pub create_request: CreateConferenceRequest,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateConferenceRequest {
    pub request_id: String,
    pub conference_solution_key: ConferenceSolutionKey,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConferenceSolutionKey {
    #[serde(rename = "type")]
    pub key_type: String,
}
