use super::types::flexible;
use super::{normalize_api_base, ListResponse, UnipileChat, UnipileChatAttendee, UnipileMessage};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct BoolFixture {
    #[serde(default, deserialize_with = "flexible::option_bool")]
    is_sender: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    hidden: Option<bool>,
}

/// Captured from GET /chats/{id}/messages (LinkedIn, May 2026).
const LIVE_MESSAGE_JSON: &str = r#"{
    "object": "MessageList",
    "items": [{
        "object": "Message",
        "seen": 0,
        "text": "Gladia's latency benchmarks for real-time audio transcription are genuinely impressive - sub-300ms in production is rare to pull off at scale.\n\nCurious what the hardest infrastructure tradeoff was to get there.",
        "edited": 0,
        "hidden": 0,
        "chat_id": "Efc-rFoUVMy4MRBsN6BWSw",
        "deleted": 0,
        "seen_by": {},
        "subject": null,
        "behavior": null,
        "is_event": 0,
        "original": "",
        "delivered": 1,
        "is_sender": 0,
        "reactions": [],
        "sender_id": "ACoAAA8Br58BDseKnXYW51e4TU617k-ohrisrcs",
        "timestamp": "2026-05-19T11:41:45.871Z",
        "account_id": "nKz6AVaoTcSef6grRHqsYA",
        "attachments": [],
        "provider_id": "2-MTc3OTE5MDkwNTg3MWI5NTU0My0xMDAmOWYyMjMyNTItYzkzZC00ZjdjLWJjMjYtMTBlNTJmNDJlNTkyXzEwMA==",
        "message_type": "MESSAGE",
        "attendee_type": "MEMBER",
        "chat_provider_id": "2-OWYyMjMyNTItYzkzZC00ZjdjLWJjMjYtMTBlNTJmNDJlNTkyXzEwMA==",
        "attendee_distance": 1,
        "sender_attendee_id": "kZ86fPIEVVmgQbhVgb7auw",
        "id": "lD0rb4Q5W4KdoUICf_MgDQ"
    }]
}"#;

/// Captured from GET /chats?account_type=LINKEDIN (May 2026).
const LIVE_CHAT_JSON: &str = r#"{
    "object": "ChatList",
    "items": [{
        "object": "Chat",
        "name": null,
        "type": 0,
        "folder": ["INBOX", "INBOX_LINKEDIN_CLASSIC"],
        "pinned": 0,
        "unread": 1,
        "archived": 0,
        "read_only": 0,
        "timestamp": "2026-05-19T11:41:46.000Z",
        "account_id": "nKz6AVaoTcSef6grRHqsYA",
        "provider_id": "2-OWYyMjMyNTItYzkzZC00ZjdjLWJjMjYtMTBlNTJmNDJlNTkyXzEwMA==",
        "account_type": "LINKEDIN",
        "unread_count": 1,
        "disabledFeatures": [],
        "attendee_provider_id": "ACoAAA8Br58BDseKnXYW51e4TU617k-ohrisrcs",
        "id": "Efc-rFoUVMy4MRBsN6BWSw",
        "muted_until": null
    }]
}"#;

#[test]
fn normalize_api_base_adds_https_when_scheme_missing() {
    assert_eq!(
        normalize_api_base("api45.unipile.com:17560"),
        "https://api45.unipile.com:17560/api/v1"
    );
}

#[test]
fn deserialize_live_linkedin_message() {
    let list: ListResponse<UnipileMessage> = serde_json::from_str(LIVE_MESSAGE_JSON).unwrap();
    let msg = &list.items[0];
    assert_eq!(msg.id, "lD0rb4Q5W4KdoUICf_MgDQ");
    assert_eq!(msg.is_sender, Some(false));
    assert_eq!(msg.delivered, Some(true));
    assert_eq!(msg.is_event, Some(false));
    assert!(msg.is_syncable());
    assert!(msg.text.as_ref().unwrap().contains("Gladia"));
}

#[test]
fn deserialize_live_linkedin_chat() {
    let list: ListResponse<UnipileChat> = serde_json::from_str(LIVE_CHAT_JSON).unwrap();
    let chat = &list.items[0];
    assert_eq!(chat.id, "Efc-rFoUVMy4MRBsN6BWSw");
    assert_eq!(chat.pinned, Some(false));
    assert_eq!(chat.archived, Some(false));
    assert_eq!(chat.unread_count, Some(1));
}

#[test]
fn skips_hidden_event_and_deleted_messages() {
    let json = r#"{
        "object": "MessageList",
        "items": [
            {"object": "Message", "id": "a", "hidden": 1, "is_event": 0},
            {"object": "Message", "id": "b", "hidden": 0, "is_event": 1, "event_type": 1},
            {"object": "Message", "id": "c", "hidden": 0, "is_event": 0, "deleted": 1},
            {"object": "Message", "id": "d", "hidden": 0, "is_event": 0, "text": "ok"}
        ]
    }"#;
    let list: ListResponse<UnipileMessage> = serde_json::from_str(json).unwrap();
    assert!(!list.items[0].is_syncable());
    assert!(!list.items[1].is_syncable());
    assert!(!list.items[2].is_syncable());
    assert!(list.items[3].is_syncable());
}

#[test]
fn deserialize_legacy_message_id_alias() {
    let json = r#"{"object":"MessageList","items":[{"object":"Message","message_id":"legacy1","is_sender":0}]}"#;
    let list: ListResponse<UnipileMessage> = serde_json::from_str(json).unwrap();
    assert_eq!(list.items[0].id, "legacy1");
}

#[test]
fn flexible_option_bool_deserializes_integers() {
    let v: BoolFixture = serde_json::from_str(r#"{"is_sender":0,"hidden":1}"#).unwrap();
    assert_eq!(v.is_sender, Some(false));
    assert_eq!(v.hidden, Some(true));
}

#[test]
fn flexible_option_bool_deserializes_json_bools() {
    let v: BoolFixture = serde_json::from_str(r#"{"is_sender":true,"hidden":false}"#).unwrap();
    assert_eq!(v.is_sender, Some(true));
    assert_eq!(v.hidden, Some(false));
}

#[test]
fn deserialize_chat_attendee_profile() {
    let json = r#"{
        "object": "ChatAttendee",
        "id": "kZ86fPIEVVmgQbhVgb7auw",
        "name": "Zhirayr Gumruyan",
        "provider_id": "ACoAAA8Br58BDseKnXYW51e4TU617k-ohrisrcs",
        "profile_url": "https://www.linkedin.com/in/gumruyan",
        "picture_url": "https://media.licdn.com/dms/image/example.jpg"
    }"#;
    let attendee: UnipileChatAttendee = serde_json::from_str(json).unwrap();
    assert_eq!(attendee.id, "kZ86fPIEVVmgQbhVgb7auw");
    assert_eq!(attendee.name.as_deref(), Some("Zhirayr Gumruyan"));
    assert_eq!(
        attendee.provider_id,
        "ACoAAA8Br58BDseKnXYW51e4TU617k-ohrisrcs"
    );
}
