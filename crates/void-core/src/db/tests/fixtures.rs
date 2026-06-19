use super::*;

pub use crate::test_fixtures::{make_conversation, make_message};

pub fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

pub fn seed_search_db() -> Database {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut conv2 = make_conversation("c2", "me@gmail.com", "G456");
    conv2.connector = "gmail".into();
    db.upsert_conversation(&conv2).unwrap();

    db.upsert_message(&make_message(
        "m1",
        "c1",
        "test-slack",
        "hello @MadMax how are you?",
        1_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m2",
        "c1",
        "test-slack",
        "meeting with @alice tomorrow",
        2_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m3",
        "c1",
        "test-slack",
        "the C++ compiler is broken",
        3_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m4",
        "c1",
        "test-slack",
        "file: budget-report-2024.xlsx",
        4_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m5",
        "c1",
        "test-slack",
        "say \"hello\" to everyone",
        5_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m6",
        "c1",
        "test-slack",
        "NOT a problem AND it works OR fails",
        6_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m7",
        "c1",
        "test-slack",
        "user:admin password:secret",
        7_000,
    ))
    .unwrap();

    let mut gmail_msg = make_message("m8", "c2", "me@gmail.com", "invoice from @accounts", 8_000);
    gmail_msg.connector = "gmail".into();
    db.upsert_message(&gmail_msg).unwrap();

    db
}

pub fn make_conversation_with_connector(
    id: &str,
    connection_id: &str,
    ext_id: &str,
    connector: &str,
) -> crate::models::Conversation {
    let mut conv = make_conversation(id, connection_id, ext_id);
    conv.connector = connector.into();
    conv
}

pub fn make_message_with_connector(
    id: &str,
    conv_id: &str,
    connection_id: &str,
    body: &str,
    ts: i64,
    connector: &str,
) -> crate::models::Message {
    let mut msg = make_message(id, conv_id, connection_id, body, ts);
    msg.connector = connector.into();
    msg
}

pub fn make_message_with_context(
    id: &str,
    conv_id: &str,
    connection_id: &str,
    body: &str,
    ts: i64,
    context_id: Option<&str>,
) -> crate::models::Message {
    let mut msg = make_message(id, conv_id, connection_id, body, ts);
    msg.context_id = context_id.map(|s| s.into());
    msg
}
