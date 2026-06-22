//! Read-path service functions returning CLI-identical JSON envelopes.

use chrono::{Datelike, Local};
use serde_json::Value;
use void_core::db::Database;

use crate::commands::calendar::parsing::{parse_date_to_ts, parse_day_spec};
use crate::commands::pagination::{build_meta, parse_page};
use crate::commands::resolve::{resolve_messages_target, MessagesTarget};
use crate::output::{json_wrap, json_wrap_paginated, resolve_connector_filter};

pub struct InboxQuery<'a> {
    pub connection: Option<&'a str>,
    pub connector: Option<&'a str>,
    pub size: i64,
    pub page: i64,
    pub all: bool,
    pub include_muted: bool,
}

pub struct SearchQuery<'a> {
    pub query: &'a str,
    pub connection: Option<&'a str>,
    pub connector: Option<&'a str>,
    pub size: i64,
    pub page: i64,
    pub include_muted: bool,
}

pub struct ContactsQuery<'a> {
    pub search: Option<&'a str>,
    pub connection: Option<&'a str>,
    pub connector: Option<&'a str>,
    pub size: i64,
    pub page: i64,
}

pub struct ChannelsQuery<'a> {
    pub search: Option<&'a str>,
    pub connection: Option<&'a str>,
    pub connector: Option<&'a str>,
    pub size: i64,
    pub page: i64,
    pub include_muted: bool,
}

pub struct MessagesQuery<'a> {
    pub target: &'a str,
    pub since: Option<&'a str>,
    pub until: Option<&'a str>,
    pub size: i64,
    pub page: i64,
}

pub struct SlackSavedQuery<'a> {
    pub connection: Option<&'a str>,
    pub size: i64,
    pub page: i64,
}

pub struct CalendarQuery<'a> {
    pub day: Option<&'a str>,
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
    pub connection: Option<&'a str>,
    pub connector: Option<&'a str>,
}

pub fn inbox(db: &Database, query: &InboxQuery<'_>, enrich_context: bool) -> anyhow::Result<Value> {
    let connector = resolve_connector_filter(query.connector)?;
    let offset = parse_page(query.size, query.page)?;

    let include_muted = query.include_muted || query.all;
    let (mut messages, total_elements) = db.recent_messages_paginated(
        query.connection,
        connector.as_deref(),
        query.size,
        offset,
        query.all,
        include_muted,
        enrich_context,
    )?;
    messages.reverse();
    if enrich_context {
        db.enrich_with_context(&mut messages)?;
    }
    let meta = build_meta(query.page, query.size, total_elements);
    Ok(json_wrap_paginated(&messages, meta))
}

pub fn conversations(db: &Database, query: &InboxQuery<'_>) -> anyhow::Result<Value> {
    let connector = resolve_connector_filter(query.connector)?;
    let offset = parse_page(query.size, query.page)?;

    let (conversations, total_elements) = db.list_conversations_paginated(
        query.connection,
        connector.as_deref(),
        query.size,
        offset,
        query.include_muted,
    )?;
    let meta = build_meta(query.page, query.size, total_elements);
    Ok(json_wrap_paginated(&conversations, meta))
}

pub fn messages(
    db: &Database,
    query: &MessagesQuery<'_>,
    enrich_context: bool,
) -> anyhow::Result<Value> {
    match resolve_messages_target(db, query.target)? {
        MessagesTarget::Link {
            message_id,
            conversation_id: _,
        } => {
            let msg = db
                .get_message(&message_id)?
                .ok_or_else(|| anyhow::anyhow!("Message vanished after lookup: {message_id}"))?;
            let mut messages = vec![msg];
            if enrich_context {
                db.enrich_with_context(&mut messages)?;
            }
            Ok(json_wrap(&messages))
        }
        MessagesTarget::UnresolvedSlackLink {
            channel_id,
            message_ts,
            workspace,
        } => anyhow::bail!(
            "Slack message not found locally for link (workspace: {workspace}, channel: {channel_id}, ts: {message_ts}). \
            The channel may not be synced yet, or the specific message hasn't been fetched — try `void sync` first."
        ),
        MessagesTarget::ConversationId(conv_id) => {
            let since = query.since.and_then(parse_date_to_ts);
            let until = query.until.and_then(parse_date_to_ts);
            let offset = parse_page(query.size, query.page)?;

            let (mut messages, total_elements) = db.list_messages_paginated(
                &conv_id,
                query.size,
                offset,
                since,
                until,
                enrich_context,
            )?;
            if enrich_context {
                db.enrich_with_context(&mut messages)?;
            }
            let meta = build_meta(query.page, query.size, total_elements);
            Ok(json_wrap_paginated(&messages, meta))
        }
        MessagesTarget::Connector { connector } => {
            let offset = parse_page(query.size, query.page)?;
            let (mut messages, total_elements) = db.recent_messages_paginated(
                None,
                Some(&connector),
                query.size,
                offset,
                true,
                true,
                enrich_context,
            )?;
            messages.reverse();
            if enrich_context {
                db.enrich_with_context(&mut messages)?;
            }
            let meta = build_meta(query.page, query.size, total_elements);
            Ok(json_wrap_paginated(&messages, meta))
        }
    }
}

pub fn search(
    db: &Database,
    query: &SearchQuery<'_>,
    enrich_context: bool,
) -> anyhow::Result<Value> {
    let connector = resolve_connector_filter(query.connector)?;
    let offset = parse_page(query.size, query.page)?;

    let (mut messages, total_elements) = db.search_messages_paginated(
        query.query,
        query.connection,
        connector.as_deref(),
        query.size,
        offset,
        query.include_muted,
        false,
    )?;
    if enrich_context {
        db.enrich_with_context(&mut messages)?;
    }
    let meta = build_meta(query.page, query.size, total_elements);
    Ok(json_wrap_paginated(&messages, meta))
}

pub fn contacts(db: &Database, query: &ContactsQuery<'_>) -> anyhow::Result<Value> {
    let connector = resolve_connector_filter(query.connector)?;
    let offset = parse_page(query.size, query.page)?;

    let (contacts, total_elements) = db.list_contacts_paginated(
        query.connection,
        connector.as_deref(),
        query.search,
        query.size,
        offset,
    )?;
    let meta = build_meta(query.page, query.size, total_elements);
    Ok(json_wrap_paginated(&contacts, meta))
}

pub fn channels(db: &Database, query: &ChannelsQuery<'_>) -> anyhow::Result<Value> {
    let connector = resolve_connector_filter(query.connector)?;
    let offset = parse_page(query.size, query.page)?;

    let (channels, total_elements) = db.list_channels_paginated(
        query.connection,
        connector.as_deref(),
        query.search,
        query.size,
        offset,
        query.include_muted,
    )?;
    let meta = build_meta(query.page, query.size, total_elements);
    Ok(json_wrap_paginated(&channels, meta))
}

pub fn slack_saved(db: &Database, query: &SlackSavedQuery<'_>) -> anyhow::Result<Value> {
    let offset = parse_page(query.size, query.page)?;

    let (mut messages, total_elements) =
        db.list_saved_messages(query.connection, Some("slack"), query.size, offset)?;
    messages.reverse();
    let meta = build_meta(query.page, query.size, total_elements);
    Ok(json_wrap_paginated(&messages, meta))
}

pub fn calendar_list(db: &Database, query: &CalendarQuery<'_>) -> anyhow::Result<Value> {
    let connector = resolve_connector_filter(query.connector)?;

    let (from, to) = if let Some(day) = query.day {
        let date = parse_day_spec(day)?;
        let start = date
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| dt.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp());
        let end = (date + chrono::Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| dt.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp());
        (start, end)
    } else {
        let today = Local::now().date_naive();
        let from = query.from.and_then(parse_date_to_ts).or_else(|| {
            today
                .and_hms_opt(0, 0, 0)
                .and_then(|dt| dt.and_local_timezone(Local).single())
                .map(|dt| dt.timestamp())
        });

        let to = query.to.and_then(parse_date_to_ts).or_else(|| {
            (today + chrono::Duration::days(1))
                .and_hms_opt(0, 0, 0)
                .and_then(|dt| dt.and_local_timezone(Local).single())
                .map(|dt| dt.timestamp())
        });
        (from, to)
    };

    let events = db.list_events(from, to, query.connection, connector.as_deref(), 200)?;
    Ok(json_wrap(&events))
}

pub fn calendar_week(db: &Database) -> anyhow::Result<Value> {
    let today = Local::now().date_naive();
    let weekday = today.weekday().num_days_from_monday();
    let monday = today - chrono::Duration::days(weekday as i64);
    let sunday = monday + chrono::Duration::days(7);

    let from = monday
        .and_hms_opt(0, 0, 0)
        .and_then(|dt| dt.and_local_timezone(Local).single())
        .map(|dt| dt.timestamp());
    let to = sunday
        .and_hms_opt(0, 0, 0)
        .and_then(|dt| dt.and_local_timezone(Local).single())
        .map(|dt| dt.timestamp());

    let events = db.list_events(from, to, None, None, 200)?;
    Ok(json_wrap(&events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use void_core::models::ConversationKind;
    use void_core::test_fixtures::{make_conversation_named, make_message_with_sender};

    fn test_db() -> Database {
        Database::open(std::path::Path::new(":memory:")).expect("in-memory db")
    }

    fn seed_basic(db: &Database) {
        let channel =
            make_conversation_named("c-chan", "C-CHAN-EXT", "general", ConversationKind::Channel);
        db.upsert_conversation(&channel).expect("upsert channel");
        let mut msg = make_message_with_sender(
            "m1",
            "c-chan",
            "alice@example.com",
            "hello saved",
            1_700_000_100,
        );
        msg.is_saved = true;
        db.upsert_message(&msg).expect("upsert saved message");
        let mut unsaved = make_message_with_sender(
            "m2",
            "c-chan",
            "bob@example.com",
            "not saved",
            1_700_000_200,
        );
        unsaved.is_saved = false;
        db.upsert_message(&unsaved).expect("upsert unsaved message");
    }

    #[test]
    fn inbox_envelope_has_data_and_pagination() {
        let db = test_db();
        seed_basic(&db);
        let query = InboxQuery {
            connection: None,
            connector: None,
            size: 50,
            page: 1,
            all: true,
            include_muted: false,
        };
        let value = inbox(&db, &query, false).unwrap();
        assert!(value.get("data").is_some());
        assert!(value.get("pagination").is_some());
        assert!(value.get("error").unwrap().is_null());
    }

    #[test]
    fn slack_saved_filters_to_saved_messages_only() {
        let db = test_db();
        seed_basic(&db);
        let query = SlackSavedQuery {
            connection: None,
            size: 50,
            page: 1,
        };
        let value = slack_saved(&db, &query).unwrap();
        let data = value.get("data").unwrap().as_array().unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].get("id").unwrap(), "m1");
    }

    #[test]
    fn parse_day_spec_today_via_calendar_parsing() {
        let today = Local::now().date_naive();
        assert_eq!(parse_day_spec("today").unwrap(), today);
    }
}
