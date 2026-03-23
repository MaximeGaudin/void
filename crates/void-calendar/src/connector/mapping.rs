use void_core::models::CalendarEvent;

use crate::api::GoogleCalendarEvent;

pub(crate) fn map_event(
    event: &GoogleCalendarEvent,
    connection_id: &str,
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
        id: format!("{connection_id}-{id}"),
        connection_id: connection_id.to_string(),
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

pub(crate) fn parse_rfc3339(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.timestamp())
        .unwrap_or(0)
}

pub(crate) fn parse_date(s: &str) -> i64 {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0)
}
