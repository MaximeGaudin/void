use super::*;

#[test]
fn connector_type_display() {
    assert_eq!(
        ConnectorType::from_static("whatsapp").to_string(),
        "whatsapp"
    );
    assert_eq!(ConnectorType::from_static("slack").to_string(), "slack");
    assert_eq!(ConnectorType::from_static("gmail").to_string(), "gmail");
    assert_eq!(
        ConnectorType::from_static("calendar").to_string(),
        "calendar"
    );
    assert_eq!(
        ConnectorType::from_static("telegram").to_string(),
        "telegram"
    );
    assert_eq!(
        ConnectorType::from_static("hackernews").to_string(),
        "hackernews"
    );
    assert_eq!(
        ConnectorType::from_static("linkedin").to_string(),
        "linkedin"
    );
    assert_eq!(ConnectorType::from_static("github").to_string(), "github");
}

#[test]
fn conversation_kind_display() {
    assert_eq!(ConversationKind::Dm.to_string(), "dm");
    assert_eq!(ConversationKind::Group.to_string(), "group");
    assert_eq!(ConversationKind::Channel.to_string(), "channel");
    assert_eq!(ConversationKind::Thread.to_string(), "thread");
    assert_eq!(ConversationKind::SelfChat.to_string(), "self");
}

#[test]
fn message_serialization_roundtrip() {
    let msg = Message {
        id: "m1".into(),
        conversation_id: "c1".into(),
        connection_id: "a1".into(),
        connector: "slack".into(),
        external_id: "ext1".into(),
        sender: "user@example.com".into(),
        sender_name: Some("Alice".into()),
        sender_avatar_url: None,
        body: Some("Hello world".into()),
        timestamp: 1_700_000_000,
        synced_at: Some(1_700_000_010),
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        context_id: None,
        context: None,
    };
    let json = serde_json::to_string(&msg).unwrap();

    assert!(
        json.contains("2023-11-14T22:13:20Z"),
        "timestamp should be ISO 8601, got: {json}"
    );
    assert!(
        json.contains("2023-11-14T22:13:30Z"),
        "synced_at should be ISO 8601, got: {json}"
    );
    assert!(
        !json.contains("1700000000"),
        "should not contain raw unix timestamps"
    );

    let deserialized: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, "m1");
    assert_eq!(deserialized.body.as_deref(), Some("Hello world"));
    assert_eq!(deserialized.timestamp, 1_700_000_000);
    assert_eq!(deserialized.synced_at, Some(1_700_000_010));
}

#[test]
fn message_deserializes_legacy_integer_timestamp() {
    let json = r#"{
        "id": "m1",
        "conversation_id": "c1",
        "connection_id": "a1",
        "connector": "slack",
        "external_id": "ext1",
        "sender": "u@x",
        "sender_name": null,
        "body": "hi",
        "timestamp": 1700000000,
        "synced_at": null,
        "is_archived": false,
        "reply_to_id": null,
        "media_type": null,
        "metadata": null,
        "context_id": null
    }"#;
    let msg: Message = serde_json::from_str(json).unwrap();
    assert_eq!(msg.timestamp, 1_700_000_000);
    assert_eq!(msg.synced_at, None);
}

#[test]
fn message_deserializes_synced_at_as_integer() {
    let json = r#"{
        "id": "m1",
        "conversation_id": "c1",
        "connection_id": "a1",
        "connector": "slack",
        "external_id": "ext1",
        "sender": "u@x",
        "sender_name": null,
        "body": null,
        "timestamp": 1700000000,
        "synced_at": 1700000010,
        "is_archived": false,
        "reply_to_id": null,
        "media_type": null,
        "metadata": null,
        "context_id": null
    }"#;
    let msg: Message = serde_json::from_str(json).unwrap();
    assert_eq!(msg.synced_at, Some(1_700_000_010));
}

#[test]
fn calendar_event_serialization() {
    let event = CalendarEvent {
        id: "e1".into(),
        connection_id: "cal1".into(),
        connector: "calendar".into(),
        external_id: "goog123".into(),
        title: "Standup".into(),
        description: None,
        location: None,
        start_at: 1_700_000_000,
        end_at: 1_700_001_800,
        all_day: false,
        attendees: Some(serde_json::json!(["alice@co.com"])),
        status: Some("confirmed".into()),
        calendar_name: Some("primary".into()),
        meet_link: Some("https://meet.google.com/abc-defg-hij".into()),
        metadata: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("meet.google.com"));

    assert!(
        json.contains("2023-11-14T22:13:20Z"),
        "start_at should be ISO 8601, got: {json}"
    );
    assert!(
        json.contains("2023-11-14T22:43:20Z"),
        "end_at should be ISO 8601, got: {json}"
    );
    assert!(
        !json.contains("1700000000"),
        "should not contain raw unix timestamp"
    );

    let roundtrip: CalendarEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.start_at, 1_700_000_000);
    assert_eq!(roundtrip.end_at, 1_700_001_800);
}

#[test]
fn parse_reply_id_valid() {
    let (conv, msg) = parse_reply_id("conv123:msg456").unwrap();
    assert_eq!(conv, "conv123");
    assert_eq!(msg, "msg456");
}

#[test]
fn parse_reply_id_splits_on_first_colon_only() {
    let (conv, msg) = parse_reply_id("left:mid:right").unwrap();
    assert_eq!(conv, "left");
    assert_eq!(msg, "mid:right");
}

#[test]
fn parse_reply_id_invalid_no_colon() {
    assert!(parse_reply_id("no-separator-here").is_err());
}

#[test]
fn parse_reply_id_empty_string_is_error() {
    assert!(parse_reply_id("").is_err());
}

#[test]
fn parse_reply_id_leading_colon_yields_empty_conv() {
    let (conv, msg) = parse_reply_id(":42").unwrap();
    assert_eq!(conv, "");
    assert_eq!(msg, "42");
}

#[test]
fn parse_reply_id_trailing_colon_yields_empty_msg() {
    let (conv, msg) = parse_reply_id("chat:").unwrap();
    assert_eq!(conv, "chat");
    assert_eq!(msg, "");
}

#[test]
fn parse_reply_id_error_message_includes_input() {
    let err = parse_reply_id("xyz").unwrap_err().to_string();
    assert!(err.contains("xyz"), "error should include the input: {err}");
}

#[test]
fn message_content_subject_returns_email_subject() {
    let with_subject = MessageContent::Text {
        body: "body".into(),
        subject: Some("Re: test".into()),
    };
    assert_eq!(with_subject.subject(), Some("Re: test"));

    let without = MessageContent::from_text("body");
    assert_eq!(without.subject(), None);
}

#[test]
fn message_content_text_returns_body() {
    assert_eq!(MessageContent::from_text("hello").text(), "hello");
    assert_eq!(MessageContent::from_text(String::new()).text(), "");
}

#[test]
fn message_content_text_returns_caption_for_file() {
    let with_caption = MessageContent::File {
        path: "/tmp/x.png".into(),
        caption: Some("a photo".into()),
        mime_type: Some("image/png".into()),
        subject: None,
    };
    assert_eq!(with_caption.text(), "a photo");

    let no_caption = MessageContent::File {
        path: "/tmp/x.png".into(),
        caption: None,
        mime_type: None,
        subject: None,
    };
    assert_eq!(no_caption.text(), "");
}

fn make_msg_ts(id: &str, ts: i64, ctx_id: Option<&str>) -> Message {
    Message {
        id: id.into(),
        conversation_id: "c1".into(),
        connection_id: "a1".into(),
        connector: "slack".into(),
        external_id: format!("ext-{id}"),
        sender: "user@test".into(),
        sender_name: None,
        sender_avatar_url: None,
        body: Some(format!("body of {id}")),
        timestamp: ts,
        synced_at: None,
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        context_id: ctx_id.map(|s| s.to_string()),
        context: None,
    }
}

fn make_msg(id: &str) -> Message {
    make_msg_ts(id, 1_000, None)
}

#[test]
fn dedup_no_context_returns_all() {
    let messages = vec![make_msg("m1"), make_msg("m2"), make_msg("m3")];
    let result = message::dedup_context_messages(messages);
    assert_eq!(result.len(), 3);
}

#[test]
fn dedup_removes_messages_shown_in_other_context() {
    let m1 = make_msg_ts("m1", 100, Some("ctx1"));
    let m2 = make_msg_ts("m2", 200, Some("ctx1"));
    let mut m3 = make_msg_ts("m3", 300, Some("ctx1"));
    m3.context = Some(vec![m1.clone(), m2.clone(), m3.clone()]);

    let mut m1_with_ctx = m1.clone();
    m1_with_ctx.context = Some(vec![m1.clone(), m2.clone(), m3.clone()]);
    let mut m2_with_ctx = m2.clone();
    m2_with_ctx.context = Some(vec![m1.clone(), m2.clone(), m3.clone()]);

    let messages = vec![m1_with_ctx, m2_with_ctx, m3];
    let result = message::dedup_context_messages(messages);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, "m3");
}

#[test]
fn dedup_keeps_anchor_even_if_in_own_context() {
    let m1 = make_msg_ts("m1", 100, Some("ctx1"));
    let mut m2 = make_msg_ts("m2", 200, Some("ctx1"));
    m2.context = Some(vec![m1.clone(), m2.clone()]);

    let mut m1_with_ctx = m1.clone();
    m1_with_ctx.context = Some(vec![m1.clone(), m2.clone()]);

    let messages = vec![m1_with_ctx, m2];
    let result = message::dedup_context_messages(messages);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, "m2");
    assert!(result[0].context.is_some());
}

#[test]
fn dedup_preserves_messages_without_context_overlap() {
    let m1 = make_msg_ts("m1", 100, Some("ctx1"));
    let mut m2 = make_msg_ts("m2", 200, Some("ctx1"));
    m2.context = Some(vec![m1.clone(), m2.clone()]);

    let mut m1_with_ctx = m1.clone();
    m1_with_ctx.context = Some(vec![m1.clone(), m2.clone()]);

    let standalone = make_msg_ts("m3", 300, None);

    let messages = vec![m1_with_ctx, m2, standalone];
    let result = message::dedup_context_messages(messages);

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].id, "m2");
    assert_eq!(result[1].id, "m3");
}

#[test]
fn dedup_all_same_context_keeps_most_recent() {
    let m1 = make_msg_ts("m1", 100, Some("ctx1"));
    let m2 = make_msg_ts("m2", 200, Some("ctx1"));
    let m3 = make_msg_ts("m3", 300, Some("ctx1"));
    let ctx = vec![m1.clone(), m2.clone(), m3.clone()];

    let mut m1e = m1;
    m1e.context = Some(ctx.clone());
    let mut m2e = m2;
    m2e.context = Some(ctx.clone());
    let mut m3e = m3;
    m3e.context = Some(ctx);

    let messages = vec![m1e, m2e, m3e];
    let result = message::dedup_context_messages(messages);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, "m3");
}
