use tracing::{debug, info};

use void_core::db::Database;
use void_core::models::CalendarEvent;

use super::mapping::map_event;
use super::types::{CalendarConnector, CreateEventParams, UpdateEventParams};
use crate::api::{
    AttendeeRequest, AttendeeResponseRequest, ConferenceDataRequest, ConferenceSolutionKey,
    CreateConferenceRequest, EventDateTimeRequest, InsertEventRequest, UpdateEventRequest,
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
        info!(connection_id = %self.connection_id, event_id = %params.event_id, "updating Calendar event");

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
            map_event(&resp, &self.connection_id, cal_id).unwrap_or_else(|| CalendarEvent {
                id: format!("{}-{}", self.connection_id, params.event_id),
                connection_id: self.connection_id.clone(),
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
        info!(connection_id = %self.connection_id, event_id, "deleting Calendar event");
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
        info!(connection_id = %self.connection_id, event_id, status, "responding to Calendar event");

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
