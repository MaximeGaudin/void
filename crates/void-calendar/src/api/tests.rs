use super::*;
use crate::error::CalendarError;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// -- Happy-path parsing --

#[tokio::test]
async fn list_events_parses_attendees_and_page_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/primary/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [
                {
                    "id": "ev1",
                    "summary": "Standup",
                    "location": "Room A",
                    "status": "confirmed",
                    "start": {"dateTime": "2026-06-11T09:00:00Z"},
                    "end": {"dateTime": "2026-06-11T09:30:00Z"},
                    "attendees": [
                        {"email": "a@example.com", "responseStatus": "accepted"},
                        {"email": "b@example.com", "responseStatus": "needsAction"}
                    ],
                    "htmlLink": "https://cal/ev1"
                },
                {
                    "id": "ev2",
                    "summary": "All day",
                    "start": {"date": "2026-06-12"},
                    "end": {"date": "2026-06-13"}
                }
            ],
            "nextPageToken": "page2"
        })))
        .mount(&server)
        .await;

    let api = CalendarApiClient::with_base_url("test-token", &server.uri());
    let resp = api
        .list_events("primary", Some("2026-06-11T00:00:00Z"), None, None, None)
        .await
        .unwrap();
    let items = resp.items.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id.as_deref(), Some("ev1"));
    assert_eq!(items[0].summary.as_deref(), Some("Standup"));
    assert_eq!(
        items[0].start.as_ref().unwrap().date_time.as_deref(),
        Some("2026-06-11T09:00:00Z")
    );
    let attendees = items[0].attendees.as_ref().unwrap();
    assert_eq!(attendees.len(), 2);
    assert_eq!(attendees[0].email.as_deref(), Some("a@example.com"));
    assert_eq!(attendees[0].response_status.as_deref(), Some("accepted"));
    // All-day event uses `date` not `dateTime`.
    assert_eq!(
        items[1].start.as_ref().unwrap().date.as_deref(),
        Some("2026-06-12")
    );
    assert_eq!(resp.next_page_token.as_deref(), Some("page2"));
}

#[tokio::test]
async fn list_events_second_page_consumed_via_page_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/primary/events"))
        .and(query_param("pageToken", "page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [{"id": "ev3", "summary": "Followup"}],
            "nextSyncToken": "sync-final"
        })))
        .mount(&server)
        .await;

    let api = CalendarApiClient::with_base_url("test-token", &server.uri());
    let resp = api
        .list_events("primary", None, None, None, Some("page2"))
        .await
        .unwrap();
    let items = resp.items.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id.as_deref(), Some("ev3"));
    assert_eq!(resp.next_sync_token.as_deref(), Some("sync-final"));
    assert!(resp.next_page_token.is_none());
}

#[tokio::test]
async fn list_calendars_parses_two_entries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [
                {"id": "primary", "summary": "Me", "primary": true},
                {"id": "team@example.com", "summary": "Team"}
            ]
        })))
        .mount(&server)
        .await;

    let api = CalendarApiClient::with_base_url("test-token", &server.uri());
    let resp = api.list_calendars().await.unwrap();
    let items = resp.items.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "primary");
    assert_eq!(items[0].primary, Some(true));
    assert_eq!(items[1].id, "team@example.com");
}

#[tokio::test]
async fn get_event_parses_conference_data() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/primary/events/ev1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "ev1",
            "summary": "Sync",
            "conferenceData": {
                "entryPoints": [
                    {"entryPointType": "video", "uri": "https://meet.example/ev1"}
                ]
            }
        })))
        .mount(&server)
        .await;

    let api = CalendarApiClient::with_base_url("test-token", &server.uri());
    let event = api.get_event("primary", "ev1").await.unwrap();
    assert_eq!(event.id.as_deref(), Some("ev1"));
    let ep = event.conference_data.unwrap().entry_points.unwrap();
    assert_eq!(ep[0].uri.as_deref(), Some("https://meet.example/ev1"));
}

// -- Error paths --

/// `list_events` calls `.error_for_status()`, so a 401 preserves the status.
#[tokio::test]
async fn list_events_401_preserves_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/primary/events"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let api = CalendarApiClient::with_base_url("test-token", &server.uri());
    let err = api
        .list_events("primary", None, None, None, None)
        .await
        .expect_err("expected error");
    match err {
        CalendarError::Http(e) => {
            assert_eq!(e.status(), Some(reqwest::StatusCode::UNAUTHORIZED))
        }
        other => panic!("expected Http error, got {other:?}"),
    }
}

/// `get_event` 429 is preserved via `.error_for_status()`.
#[tokio::test]
async fn get_event_429_preserves_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/primary/events/ev1"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .mount(&server)
        .await;

    let api = CalendarApiClient::with_base_url("test-token", &server.uri());
    let err = api
        .get_event("primary", "ev1")
        .await
        .expect_err("expected error");
    match err {
        CalendarError::Http(e) => {
            assert_eq!(e.status(), Some(reqwest::StatusCode::TOO_MANY_REQUESTS))
        }
        other => panic!("expected Http error, got {other:?}"),
    }
}

/// `search_events` 5xx is preserved via `.error_for_status()`.
#[tokio::test]
async fn search_events_500_preserves_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/primary/events"))
        .respond_with(ResponseTemplate::new(500).set_body_string("server error"))
        .mount(&server)
        .await;

    let api = CalendarApiClient::with_base_url("test-token", &server.uri());
    let err = api
        .search_events("primary", "lunch", None, None)
        .await
        .expect_err("expected error");
    match err {
        CalendarError::Http(e) => {
            assert_eq!(e.status(), Some(reqwest::StatusCode::INTERNAL_SERVER_ERROR))
        }
        other => panic!("expected Http error, got {other:?}"),
    }
}

/// `list_calendars` decodes directly (no `error_for_status`); a non-JSON 500
/// body surfaces as an Http decode error rather than a panic.
#[tokio::test]
async fn list_calendars_5xx_surfaces_http_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let api = CalendarApiClient::with_base_url("test-token", &server.uri());
    let err = api.list_calendars().await.expect_err("expected error");
    assert!(matches!(err, CalendarError::Http(_)), "got {err:?}");
}

/// Malformed JSON: a CalendarListEntry missing required `id` -> clean Err.
#[tokio::test]
async fn list_calendars_malformed_json_is_clean_err() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [{"summary": "no id here"}]
        })))
        .mount(&server)
        .await;

    let api = CalendarApiClient::with_base_url("test-token", &server.uri());
    let err = api
        .list_calendars()
        .await
        .expect_err("expected decode error");
    assert!(matches!(err, CalendarError::Http(_)), "got {err:?}");
}
