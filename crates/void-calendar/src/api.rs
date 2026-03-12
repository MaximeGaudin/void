use std::collections::HashMap;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tracing::debug;

const DEFAULT_BASE_URL: &str = "https://www.googleapis.com";

/// Google Calendar API client.
pub struct CalendarApiClient {
    http: reqwest::Client,
    access_token: String,
    base_url: String,
}

impl CalendarApiClient {
    pub fn new(access_token: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            access_token: access_token.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(access_token: &str, base_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            access_token: access_token.to_string(),
            base_url: base_url.to_string(),
        }
    }

    pub async fn list_calendars(&self) -> anyhow::Result<CalendarListResponse> {
        debug!("calendar: list_calendars");
        let resp: CalendarListResponse = self
            .http
            .get(format!(
                "{}/calendar/v3/users/me/calendarList",
                self.base_url
            ))
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
        let mut params: Vec<(&str, String)> = vec![("maxResults", "2500".into())];

        if sync_token.is_some() {
            // syncToken is incompatible with singleEvents, orderBy, timeMin, timeMax
            if let Some(st) = sync_token {
                params.push(("syncToken", st.into()));
            }
            // showDeleted must be true (default) to receive cancelled events
        } else {
            params.push(("singleEvents", "true".into()));
            params.push(("orderBy", "startTime".into()));
            if let Some(t) = time_min {
                params.push(("timeMin", t.into()));
            }
            if let Some(t) = time_max {
                params.push(("timeMax", t.into()));
            }
        }
        if let Some(pt) = page_token {
            params.push(("pageToken", pt.into()));
        }

        let url = format!(
            "{}/calendar/v3/calendars/{}/events",
            self.base_url,
            urlencoded(calendar_id)
        );
        let resp: EventListResponse = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&params)
            .send()
            .await?
            .error_for_status()
            .map_err(anyhow::Error::from)?
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
            "{}/calendar/v3/calendars/{}/events",
            self.base_url,
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

    pub async fn get_event(
        &self,
        calendar_id: &str,
        event_id: &str,
    ) -> anyhow::Result<GoogleCalendarEvent> {
        debug!(calendar_id, event_id, "calendar: get_event");
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}",
            self.base_url,
            urlencoded(calendar_id),
            urlencoded(event_id)
        );
        let resp: GoogleCalendarEvent = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()
            .map_err(anyhow::Error::from)?
            .json()
            .await
            .context("calendar: failed to get event")?;
        debug!(event_id, "calendar: get_event ok");
        Ok(resp)
    }

    pub async fn search_events(
        &self,
        calendar_id: &str,
        query: &str,
        time_min: Option<&str>,
        time_max: Option<&str>,
    ) -> anyhow::Result<EventListResponse> {
        debug!(calendar_id, query, "calendar: search_events");
        let mut params: Vec<(&str, String)> = vec![
            ("singleEvents", "true".into()),
            ("orderBy", "startTime".into()),
            ("maxResults", "2500".into()),
            ("q", query.into()),
        ];
        if let Some(t) = time_min {
            params.push(("timeMin", t.into()));
        }
        if let Some(t) = time_max {
            params.push(("timeMax", t.into()));
        }
        let url = format!(
            "{}/calendar/v3/calendars/{}/events",
            self.base_url,
            urlencoded(calendar_id)
        );
        let resp: EventListResponse = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&params)
            .send()
            .await?
            .error_for_status()
            .map_err(anyhow::Error::from)?
            .json()
            .await
            .context("calendar: failed to search events")?;
        let count = resp.items.as_ref().map(|i| i.len()).unwrap_or(0);
        debug!(count, "calendar: search_events ok");
        Ok(resp)
    }

    pub async fn update_event(
        &self,
        calendar_id: &str,
        event_id: &str,
        update: &UpdateEventRequest,
        send_updates: Option<&str>,
    ) -> anyhow::Result<GoogleCalendarEvent> {
        debug!(calendar_id, event_id, "calendar: update_event");
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}",
            self.base_url,
            urlencoded(calendar_id),
            urlencoded(event_id)
        );
        let mut req = self
            .http
            .patch(&url)
            .bearer_auth(&self.access_token)
            .json(update);
        if let Some(su) = send_updates {
            req = req.query(&[("sendUpdates", su)]);
        }
        let resp: GoogleCalendarEvent = req
            .send()
            .await?
            .error_for_status()
            .map_err(anyhow::Error::from)?
            .json()
            .await
            .context("calendar: failed to update event")?;
        debug!(event_id, "calendar: update_event ok");
        Ok(resp)
    }

    pub async fn freebusy(
        &self,
        time_min: &str,
        time_max: &str,
        emails: &[String],
    ) -> anyhow::Result<FreeBusyResponse> {
        debug!(
            time_min,
            time_max,
            attendees = emails.len(),
            "calendar: freebusy"
        );
        let items: Vec<serde_json::Value> = emails
            .iter()
            .map(|e| serde_json::json!({ "id": e }))
            .collect();
        let body = serde_json::json!({
            "timeMin": time_min,
            "timeMax": time_max,
            "items": items,
        });
        let url = format!("{}/calendar/v3/freeBusy", self.base_url);
        let resp: FreeBusyResponse = self
            .http
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?
            .error_for_status()
            .map_err(anyhow::Error::from)?
            .json()
            .await
            .context("calendar: failed to query freebusy")?;
        debug!(calendars = resp.calendars.len(), "calendar: freebusy ok");
        Ok(resp)
    }

    pub async fn delete_event(
        &self,
        calendar_id: &str,
        event_id: &str,
        send_updates: Option<&str>,
    ) -> anyhow::Result<()> {
        debug!(calendar_id, event_id, "calendar: delete_event");
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}",
            self.base_url,
            urlencoded(calendar_id),
            urlencoded(event_id)
        );
        let mut req = self.http.delete(&url).bearer_auth(&self.access_token);
        if let Some(su) = send_updates {
            req = req.query(&[("sendUpdates", su)]);
        }
        req.send()
            .await?
            .error_for_status()
            .map_err(anyhow::Error::from)?;
        debug!(event_id, "calendar: delete_event ok");
        Ok(())
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

// -- FreeBusy types --

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeBusyResponse {
    pub time_min: Option<String>,
    pub time_max: Option<String>,
    #[serde(default)]
    pub calendars: HashMap<String, FreeBusyCalendar>,
}

#[derive(Debug, Deserialize)]
pub struct FreeBusyCalendar {
    #[serde(default)]
    pub busy: Vec<FreeBusySlot>,
    #[serde(default)]
    pub errors: Vec<FreeBusyError>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FreeBusySlot {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Deserialize)]
pub struct FreeBusyError {
    pub domain: Option<String>,
    pub reason: Option<String>,
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

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEventRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<EventDateTimeRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<EventDateTimeRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attendees: Option<Vec<AttendeeResponseRequest>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttendeeResponseRequest {
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
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
