//! Minimal Circleback REST client (<https://circleback.ai/docs/api>).
//!
//! Endpoints used:
//! - `GET /meetings?cursor=…` — newest first, 20 per page, RFC 8288 `Link: <…>; rel="next"`
//! - `GET /meeting/{id}` — one meeting (notes, attendees, action items)
//! - `GET /meeting/{id}/transcript` — `[{speaker, text, timestamp}]`
//!
//! Rate limits are per account (free plan: 3 req/s, 20 req/min); the client
//! paces every call and honours `Retry-After` on `429`.

use std::time::Duration;

use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;

pub const DEFAULT_BASE_URL: &str = "https://circleback.ai/api";

/// Minimum spacing between two requests.
const REQUEST_PACE: Duration = Duration::from_millis(350);
/// How many times a `429` is retried before giving up.
const MAX_RATE_LIMIT_RETRIES: u32 = 3;
/// Fallback wait when `429` comes without a usable `Retry-After`.
const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Seconds.
    #[serde(default)]
    pub duration: Option<f64>,
    /// Markdown notes, `null` until Circleback has processed the recording.
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub private_notes: Option<String>,
    #[serde(default)]
    pub ical_uid: Option<String>,
    #[serde(default)]
    pub recording_url: Option<String>,
    #[serde(default)]
    pub tags: Vec<serde_json::Value>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    #[serde(default)]
    pub action_items: Vec<ActionItem>,
    #[serde(default)]
    pub calendar_event: Option<serde_json::Value>,
}

impl Meeting {
    /// Notes or a duration mean Circleback finished processing the recording.
    pub fn is_processed(&self) -> bool {
        self.notes.as_deref().is_some_and(|n| !n.trim().is_empty()) || self.duration.is_some()
    }

    /// `updatedAt` when present, else `createdAt`: changes whenever notes or
    /// action items are (re)generated.
    pub fn version(&self) -> &str {
        self.updated_at.as_deref().unwrap_or(&self.created_at)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attendee {
    #[serde(default)]
    pub profile_id: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub company_name: Option<String>,
    #[serde(default)]
    pub is_calendar_invitee: Option<bool>,
    #[serde(default)]
    pub is_calendar_event_organizer: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItem {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// `PENDING`, `COMPLETED`, …
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub assignee: Option<Assignee>,
}

impl ActionItem {
    pub fn is_done(&self) -> bool {
        self.completed_at.is_some()
            || self.status.as_deref().is_some_and(|s| {
                s.eq_ignore_ascii_case("completed") || s.eq_ignore_ascii_case("done")
            })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignee {
    #[serde(default)]
    pub profile_id: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranscriptTurn {
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub text: String,
    /// Seconds since the start of the recording.
    #[serde(default)]
    pub timestamp: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct MeetingsPage {
    pub meetings: Vec<Meeting>,
    /// Opaque cursor for the next page, `None` on the last page.
    pub next_cursor: Option<String>,
}

pub struct CirclebackClient {
    http: Client,
    base_url: String,
    api_key: String,
}

impl CirclebackClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_base_url(api_key, DEFAULT_BASE_URL)
    }

    /// Override the API base URL (tests point it at a mock server).
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    async fn get(&self, path: &str) -> anyhow::Result<Response> {
        let url = format!("{}/{}", self.base_url, path.trim_start_matches('/'));
        let mut attempt = 0;
        loop {
            tokio::time::sleep(REQUEST_PACE).await;
            let resp = self
                .http
                .get(&url)
                .bearer_auth(&self.api_key)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await?;
            match resp.status() {
                StatusCode::TOO_MANY_REQUESTS if attempt < MAX_RATE_LIMIT_RETRIES => {
                    let wait = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.trim().parse::<u64>().ok())
                        .map(Duration::from_secs)
                        .unwrap_or(DEFAULT_RETRY_AFTER);
                    tracing::warn!(?wait, attempt, "circleback rate limited, waiting");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    anyhow::bail!("Circleback rejected the API key ({})", resp.status())
                }
                _ => return Ok(resp),
            }
        }
    }

    /// One page of meetings, newest first. Pass the previous page's
    /// `next_cursor` to continue, `None` for the first page.
    pub async fn list_meetings(&self, cursor: Option<&str>) -> anyhow::Result<MeetingsPage> {
        let path = match cursor {
            Some(c) => format!("meetings?cursor={}", urlencode(c)),
            None => "meetings".to_string(),
        };
        let resp = self.get(&path).await?;
        if !resp.status().is_success() {
            anyhow::bail!("GET /meetings failed: {}", resp.status());
        }
        let next_cursor = resp
            .headers()
            .get(reqwest::header::LINK)
            .and_then(|v| v.to_str().ok())
            .and_then(next_cursor_from_link);
        let meetings: Vec<Meeting> = resp.json().await?;
        Ok(MeetingsPage {
            meetings,
            next_cursor,
        })
    }

    /// A single meeting, `None` when it does not exist (or is not visible).
    pub async fn get_meeting(&self, id: &str) -> anyhow::Result<Option<Meeting>> {
        let resp = self.get(&format!("meeting/{}", urlencode(id))).await?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            anyhow::bail!("GET /meeting/{id} failed: {}", resp.status());
        }
        Ok(Some(resp.json().await?))
    }

    /// Speaker turns of a meeting; empty when no transcript exists.
    pub async fn transcript(&self, id: &str) -> anyhow::Result<Vec<TranscriptTurn>> {
        let resp = self
            .get(&format!("meeting/{}/transcript", urlencode(id)))
            .await?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        if !resp.status().is_success() {
            anyhow::bail!("GET /meeting/{id}/transcript failed: {}", resp.status());
        }
        Ok(resp.json().await?)
    }
}

/// Extract the `cursor` query parameter of the `rel="next"` link.
///
/// `</api/meetings?limit=2&cursor=eyJwYWdlIjoxfQ>; rel="next"` → `eyJwYWdlIjoxfQ`
pub(crate) fn next_cursor_from_link(link: &str) -> Option<String> {
    for part in link.split(',') {
        let part = part.trim();
        if !part.contains("rel=\"next\"") && !part.contains("rel=next") {
            continue;
        }
        let url = part
            .split(';')
            .next()?
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>');
        let query = url.split_once('?').map(|(_, q)| q).unwrap_or("");
        for kv in query.split('&') {
            if let Some(v) = kv.strip_prefix("cursor=") {
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn meeting_json(id: &str, notes: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "name": "Daily Model",
            "url": "https://meet.google.com/abc-defg-hij",
            "createdAt": "2026-09-10T14:01:02.106Z",
            "updatedAt": "2026-09-10T15:02:00.000Z",
            "duration": notes.map(|_| 2904.04),
            "notes": notes,
            "icalUid": "abc@google.com",
            "recordingUrl": null,
            "tags": [],
            "attendees": [{"profileId": 1, "name": "Ada", "email": "ada@example.com", "title": "CEO", "companyName": "Acme", "isCalendarInvitee": true, "isCalendarEventOrganizer": false}],
            "actionItems": [{"id": 7, "title": "Ship it", "description": "Soon", "status": "PENDING", "completedAt": null, "assignee": {"profileId": 2, "name": "Bob", "email": "bob@example.com"}}],
            "calendarEvent": {"id": 1, "platform": "GoogleCalendar"},
            "privateNotes": "",
            "insights": {}
        })
    }

    #[test]
    fn parses_next_cursor_from_link_header() {
        let link = "</api/meetings?limit=2&cursor=eyJwYWdlIjoxfQ>; rel=\"next\"";
        assert_eq!(
            next_cursor_from_link(link).as_deref(),
            Some("eyJwYWdlIjoxfQ")
        );
        assert_eq!(
            next_cursor_from_link("</api/meetings?cursor=abc>; rel=\"prev\""),
            None
        );
        assert_eq!(next_cursor_from_link(""), None);
    }

    #[test]
    fn deserializes_meeting_and_flags_processing_state() {
        let m: Meeting = serde_json::from_value(meeting_json("m1", Some("#### Overview"))).unwrap();
        assert!(m.is_processed());
        assert_eq!(m.version(), "2026-09-10T15:02:00.000Z");
        assert_eq!(m.attendees[0].name.as_deref(), Some("Ada"));
        assert_eq!(
            m.action_items[0].assignee.as_ref().unwrap().name.as_deref(),
            Some("Bob")
        );
        assert!(!m.action_items[0].is_done());

        let pending: Meeting = serde_json::from_value(meeting_json("m2", None)).unwrap();
        assert!(!pending.is_processed());
    }

    #[tokio::test]
    async fn list_meetings_follows_link_cursor() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/meetings"))
            .and(query_param_is_missing("cursor"))
            .and(header("authorization", "Bearer cb_test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(vec![meeting_json("m1", Some("notes"))])
                    .insert_header("link", "</api/meetings?cursor=page2>; rel=\"next\""),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/meetings"))
            .and(query_param("cursor", "page2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(vec![meeting_json("m2", None)]))
            .mount(&server)
            .await;

        let client = CirclebackClient::with_base_url("cb_test", server.uri());
        let first = client.list_meetings(None).await.unwrap();
        assert_eq!(first.meetings.len(), 1);
        assert_eq!(first.next_cursor.as_deref(), Some("page2"));
        let second = client
            .list_meetings(first.next_cursor.as_deref())
            .await
            .unwrap();
        assert_eq!(second.meetings[0].id, "m2");
        assert!(second.next_cursor.is_none());
    }

    #[tokio::test]
    async fn transcript_returns_turns_and_empty_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/meeting/m1/transcript"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"speaker": "Ada", "text": "Hello", "timestamp": 2.48},
                {"speaker": "Bob", "text": "Hi", "timestamp": 5.0}
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/meeting/m2/transcript"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = CirclebackClient::with_base_url("cb_test", server.uri());
        let turns = client.transcript("m1").await.unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].speaker.as_deref(), Some("Bob"));
        assert!(client.transcript("m2").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejected_key_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/meetings"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let client = CirclebackClient::with_base_url("bad", server.uri());
        let err = client.list_meetings(None).await.unwrap_err();
        assert!(err.to_string().contains("401"));
    }

    #[tokio::test]
    async fn get_meeting_returns_none_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/meeting/nope"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let client = CirclebackClient::with_base_url("cb_test", server.uri());
        assert!(client.get_meeting("nope").await.unwrap().is_none());
    }
}
