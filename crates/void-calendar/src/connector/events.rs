use tracing::{debug, info};

use void_core::db::Database;
use void_core::models::CalendarEvent;

use super::attendees::{build_attendee_list, build_response_attendees, find_self_attendee};
use super::mapping::{external_event_id, map_event};
use super::types::{CalendarConnector, CreateEventParams, UpdateEventParams};
use crate::api::{
    CalendarApiClient, ConferenceDataRequest, ConferenceSolutionKey, CreateConferenceRequest,
    EventDateTimeRequest, GoogleCalendarEvent, InsertEventRequest, UpdateEventRequest,
};

impl CalendarConnector {
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
        info!(connection_id = %self.connection_id, title = %params.title, calendar_id = %cal_id, "creating Calendar event");

        let timezone = "UTC".to_string();
        let attendee_list = build_attendee_list(&self.connection_id, params.attendees);

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
        debug!(connection_id = %self.connection_id, event_id = %event_id, "Calendar event created");

        let cal_event =
            map_event(&resp, &self.connection_id, cal_id).unwrap_or_else(|| CalendarEvent {
                id: format!(
                    "{}-{}",
                    self.connection_id,
                    resp.id.as_deref().unwrap_or("new")
                ),
                connection_id: self.connection_id.clone(),
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
        let event_id = external_event_id(&self.connection_id, params.event_id);
        info!(connection_id = %self.connection_id, event_id, "updating Calendar event");

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
            .update_event(cal_id, event_id, &update, params.send_updates)
            .await?;
        let cal_event =
            map_event(&resp, &self.connection_id, cal_id).unwrap_or_else(|| CalendarEvent {
                id: format!("{}-{}", self.connection_id, event_id),
                connection_id: self.connection_id.clone(),
                connector: "calendar".into(),
                external_id: event_id.to_string(),
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
        let event_id = external_event_id(&self.connection_id, event_id);
        info!(connection_id = %self.connection_id, event_id, "deleting Calendar event");
        api.delete_event(cal_id, event_id, send_updates)
            .await
            .map_err(Into::into)
    }

    /// RSVP to an event as the calendar owner.
    ///
    /// `event_id` may be the void id or the Google id. The owner is found on
    /// the guest list (`email` override, else `self: true`, else the primary
    /// calendar id); the call fails rather than adding a new attendee.
    pub async fn respond_to_event(
        &self,
        event_id: &str,
        email: Option<&str>,
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
        let event_id = external_event_id(&self.connection_id, event_id);
        info!(connection_id = %self.connection_id, event_id, status, "responding to Calendar event");

        let (event, resp) =
            respond_with_client(&api, cal_id, event_id, email, status, comment).await?;
        let cal_event =
            map_event(&resp, &self.connection_id, cal_id).unwrap_or_else(|| CalendarEvent {
                id: format!("{}-{}", self.connection_id, event_id),
                connection_id: self.connection_id.clone(),
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
                    if let Some(cal_event) = map_event(event, &self.connection_id, cal_id) {
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

/// Fetch `event_id`, set the owner's response and PATCH the attendee list.
/// Returns the event as fetched and the updated event.
pub(crate) async fn respond_with_client(
    api: &CalendarApiClient,
    cal_id: &str,
    event_id: &str,
    email: Option<&str>,
    status: &str,
    comment: Option<&str>,
) -> anyhow::Result<(GoogleCalendarEvent, GoogleCalendarEvent)> {
    let event = api.get_event(cal_id, event_id).await?;
    let attendees = event.attendees.as_deref().unwrap_or_default();

    let mut me = find_self_attendee(attendees, email, None);
    if me.is_none() && email.is_none() {
        // No `self` flag on the guest list: fall back to the account email,
        // which is the primary calendar's id.
        let account = api
            .list_calendars()
            .await?
            .items
            .unwrap_or_default()
            .into_iter()
            .find(|c| c.primary == Some(true))
            .map(|c| c.id);
        me = find_self_attendee(attendees, None, account.as_deref());
    }
    let Some(me) = me else {
        anyhow::bail!(
            "not an attendee of event \"{}\"{}: nothing to respond to",
            event.summary.as_deref().unwrap_or(event_id),
            email.map(|e| format!(" as {e}")).unwrap_or_default(),
        );
    };
    debug!(event_id, attendee = %me, status, "calendar: responding as attendee");

    let update = UpdateEventRequest {
        attendees: Some(build_response_attendees(attendees, &me, status, comment)),
        ..Default::default()
    };
    let resp = api
        .update_event(cal_id, event_id, &update, Some("all"))
        .await?;
    Ok((event, resp))
}
