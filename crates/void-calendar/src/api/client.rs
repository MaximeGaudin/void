use crate::api::types::{
    CalendarListResponse, EventListResponse, FreeBusyResponse, GoogleCalendarEvent,
    InsertEventRequest, UpdateEventRequest,
};
use crate::error::CalendarError;
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
            http: void_gmail::api::build_http_client(),
            access_token: access_token.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(access_token: &str, base_url: &str) -> Self {
        Self {
            http: void_gmail::api::build_http_client(),
            access_token: access_token.to_string(),
            base_url: base_url.to_string(),
        }
    }

    pub async fn list_calendars(&self) -> Result<CalendarListResponse, CalendarError> {
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
            .map_err(CalendarError::from)?;
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
    ) -> Result<EventListResponse, CalendarError> {
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
            // singleEvents expands recurring instances for display; orderBy must not be
            // set — Google Calendar API does not return nextSyncToken when orderBy is used.
            params.push(("singleEvents", "true".into()));
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
            .error_for_status()?
            .json()
            .await
            .map_err(CalendarError::from)?;
        let count = resp.items.as_ref().map(|i| i.len()).unwrap_or(0);
        let has_sync_token = resp.next_sync_token.is_some();
        let has_page_token = resp.next_page_token.is_some();
        debug!(
            count,
            has_sync_token, has_page_token, "calendar: list_events ok"
        );
        Ok(resp)
    }

    /// Unfiltered events.list for sync-token bootstrap (no timeMin/timeMax/singleEvents/orderBy).
    pub async fn list_events_sync_bootstrap(
        &self,
        calendar_id: &str,
        page_token: Option<&str>,
    ) -> Result<EventListResponse, CalendarError> {
        debug!(calendar_id, "calendar: list_events_sync_bootstrap");
        let mut params: Vec<(&str, String)> = vec![("maxResults", "2500".into())];
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
            .error_for_status()?
            .json()
            .await
            .map_err(CalendarError::from)?;
        debug!(
            count = resp.items.as_ref().map(|i| i.len()).unwrap_or(0),
            has_sync_token = resp.next_sync_token.is_some(),
            has_page_token = resp.next_page_token.is_some(),
            "calendar: list_events_sync_bootstrap ok"
        );
        Ok(resp)
    }

    pub async fn insert_event(
        &self,
        calendar_id: &str,
        event: &InsertEventRequest,
        conference_data_version: Option<u32>,
        send_updates: Option<&str>,
    ) -> Result<GoogleCalendarEvent, CalendarError> {
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
        if let Some(su) = send_updates {
            req = req.query(&[("sendUpdates", su)]);
        }

        let resp: GoogleCalendarEvent = req
            .send()
            .await?
            .json()
            .await
            .map_err(CalendarError::from)?;
        let event_id = resp.id.as_deref().unwrap_or("(none)");
        debug!(event_id, "calendar: insert_event ok");
        Ok(resp)
    }

    pub async fn get_event(
        &self,
        calendar_id: &str,
        event_id: &str,
    ) -> Result<GoogleCalendarEvent, CalendarError> {
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
            .error_for_status()?
            .json()
            .await
            .map_err(CalendarError::from)?;
        debug!(event_id, "calendar: get_event ok");
        Ok(resp)
    }

    pub async fn search_events(
        &self,
        calendar_id: &str,
        query: &str,
        time_min: Option<&str>,
        time_max: Option<&str>,
    ) -> Result<EventListResponse, CalendarError> {
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
            .error_for_status()?
            .json()
            .await
            .map_err(CalendarError::from)?;
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
    ) -> Result<GoogleCalendarEvent, CalendarError> {
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
        let resp: GoogleCalendarEvent = req.send().await?.error_for_status()?.json().await?;
        debug!(event_id, "calendar: update_event ok");
        Ok(resp)
    }

    pub async fn freebusy(
        &self,
        time_min: &str,
        time_max: &str,
        emails: &[String],
    ) -> Result<FreeBusyResponse, CalendarError> {
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
            .error_for_status()?
            .json()
            .await?;
        debug!(calendars = resp.calendars.len(), "calendar: freebusy ok");
        Ok(resp)
    }

    pub async fn delete_event(
        &self,
        calendar_id: &str,
        event_id: &str,
        send_updates: Option<&str>,
    ) -> Result<(), CalendarError> {
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
        req.send().await?.error_for_status()?;
        debug!(event_id, "calendar: delete_event ok");
        Ok(())
    }
}

fn urlencoded(s: &str) -> String {
    s.replace('#', "%23").replace(' ', "%20")
}
