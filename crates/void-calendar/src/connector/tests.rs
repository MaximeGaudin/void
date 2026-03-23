use crate::api::*;
use void_core::db::Database;
use void_core::models::CalendarEvent;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::mapping::{map_event, parse_date, parse_rfc3339};

/// Runs the initial sync pagination loop using a pre-built API client (for testing without tokens).
async fn run_initial_sync_with_client(
    api: &CalendarApiClient,
    db: &Database,
    connection_id: &str,
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
                    if let Some(cal_event) = map_event(event, connection_id, cal_id) {
                        db.upsert_event(&cal_event)?;
                    }
                }
            }

            if let Some(token) = &resp.next_sync_token {
                db.set_sync_state(connection_id, &format!("sync_token:{cal_id}"), token)?;
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
    connection_id: &str,
    calendar_ids: &[String],
    time_min: &str,
    time_max: &str,
) -> anyhow::Result<()> {
    for cal_id in calendar_ids {
        let key = format!("sync_token:{cal_id}");
        let Some(sync_token) = db.get_sync_state(connection_id, &key)? else {
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
                            db.delete_event(connection_id, event_id)?;
                            continue;
                        }
                        if let Some(cal_event) = map_event(event, connection_id, cal_id) {
                            db.upsert_event(&cal_event)?;
                        }
                    }
                }
                if let Some(token) = &resp.next_sync_token {
                    db.set_sync_state(connection_id, &key, token)?;
                }
            }
            Err(e) => {
                if e.to_string().contains("410") {
                    run_initial_sync_with_client(
                        api,
                        db,
                        connection_id,
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

    run_incremental_sync_with_client(&api, &db, "test-cal", &calendar_ids, &time_min, &time_max)
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

    run_incremental_sync_with_client(&api, &db, "test-cal", &calendar_ids, &time_min, &time_max)
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
        connection_id: "test-cal".into(),
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

    run_incremental_sync_with_client(&api, &db, "test-cal", &calendar_ids, &time_min, &time_max)
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
