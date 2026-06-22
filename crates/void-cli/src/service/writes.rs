//! Write-path service functions returning data values for CLI/MCP formatting.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::{json, Value};
use void_core::config::VoidConfig;
use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::MessageContent;
use void_core::sync::is_daemon_running;

use crate::commands::connector_factory;
use crate::commands::resolve::resolve_message;
use crate::connectors;
use crate::output::{parse_connector_type, resolve_connector_filter};

pub struct SendParams<'a> {
    pub to: Option<&'a str>,
    pub conversation: Option<&'a str>,
    pub via: &'a str,
    pub connection: Option<&'a str>,
    pub message: &'a str,
    pub subject: Option<&'a str>,
    pub file: Option<&'a str>,
    pub at: Option<&'a str>,
}

pub struct ReplyParams<'a> {
    pub message_id: &'a str,
    pub message: &'a str,
    pub file: Option<&'a str>,
    pub in_thread: bool,
    pub at: Option<&'a str>,
}

pub struct ForwardParams<'a> {
    pub message_id: &'a str,
    pub to: &'a str,
    pub comment: Option<&'a str>,
}

pub struct ArchiveParams<'a> {
    pub message_ids: &'a [String],
    pub before: Option<&'a str>,
    pub connector: Option<&'a str>,
}

pub struct MuteParams<'a> {
    pub targets: &'a [String],
    pub unmute: bool,
    pub connection: Option<&'a str>,
    pub connector: Option<&'a str>,
}

pub async fn send(
    db: &Database,
    cfg: &VoidConfig,
    store_path: &Path,
    params: SendParams<'_>,
) -> anyhow::Result<String> {
    let connector_type = parse_connector_type(params.via)
        .ok_or_else(|| anyhow::anyhow!("Unknown connector type: {}", params.via))?;

    let target_type = connector_type.to_string();
    let connection = cfg
        .connections
        .iter()
        .find(|a| {
            let type_matches = a.connector_type.to_string() == target_type;
            let name_matches = params.connection.is_none_or(|n| a.id == n);
            type_matches && name_matches
        })
        .ok_or_else(|| anyhow::anyhow!("No {target_type} connection found in config.toml"))?;

    let plugin = connectors::by_id(connection.connector_type.as_str())
        .ok_or_else(|| anyhow::anyhow!("Unknown connector type: {}", connection.connector_type))?;

    if let Some(at_str) = params.at {
        if !plugin.supports_scheduling {
            anyhow::bail!("Scheduled sending (--at) is only supported for Slack.");
        }
        let to = params
            .to
            .ok_or_else(|| anyhow::anyhow!("--to is required for scheduled Slack sends"))?;
        return run_slack_scheduled_send(connection, to, params.message, at_str).await;
    }

    let to = resolve_send_target(db, params.to, params.conversation, &target_type)?;

    let content = if let Some(path) = params.file {
        MessageContent::File {
            path: path.into(),
            caption: Some(params.message.to_string()),
            mime_type: None,
            subject: params.subject.map(str::to_string),
        }
    } else {
        MessageContent::Text {
            body: params.message.to_string(),
            subject: params.subject.map(str::to_string),
        }
    };

    let msg_id = if plugin.uses_daemon_rpc && is_daemon_running(store_path) {
        void_whatsapp::rpc::send_message(store_path, &connection.id, &to, content).await?
    } else {
        let conn = connector_factory::build_connector(connection, store_path)?;
        conn.send_message(&to, content).await?
    };
    Ok(msg_id)
}

pub async fn reply(
    db: &Database,
    cfg: &VoidConfig,
    store_path: &Path,
    params: ReplyParams<'_>,
) -> anyhow::Result<String> {
    let msg = resolve_message(db, params.message_id)?;

    let conv = db
        .get_conversation(&msg.conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", msg.conversation_id))?;

    let connection = cfg
        .find_connection_by_connector(&msg.connector)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No {} connection found in config.toml for message {}",
                msg.connector,
                msg.id
            )
        })?;

    let plugin = connectors::by_id(connection.connector_type.as_str())
        .ok_or_else(|| anyhow::anyhow!("Unknown connector type: {}", connection.connector_type))?;

    if let Some(at_str) = params.at {
        if !plugin.supports_scheduling {
            anyhow::bail!("Scheduled sending (--at) is only supported for Slack.");
        }
        return run_slack_scheduled_reply(
            connection,
            &conv.external_id,
            &msg.external_id,
            params.message,
            at_str,
        )
        .await;
    }

    let connector_type = parse_connector_type(&connection.connector_type.to_string())
        .ok_or_else(|| anyhow::anyhow!("Unknown connector type: {}", connection.connector_type))?;

    let reply_id = connectors::build_reply_id(connector_type, &conv.external_id, &msg.external_id);

    let content = if let Some(path) = params.file {
        MessageContent::File {
            path: path.into(),
            caption: Some(params.message.to_string()),
            mime_type: None,
            subject: None,
        }
    } else {
        MessageContent::from_text(params.message.to_string())
    };

    let sent_id = if plugin.uses_daemon_rpc && is_daemon_running(store_path) {
        void_whatsapp::rpc::reply_message(
            store_path,
            &connection.id,
            &reply_id,
            content,
            params.in_thread,
        )
        .await?
    } else {
        let conn = connector_factory::build_connector(connection, store_path)?;
        conn.reply(&reply_id, content, params.in_thread).await?
    };
    Ok(sent_id)
}

pub async fn forward(
    db: &Database,
    cfg: &VoidConfig,
    store_path: &Path,
    params: ForwardParams<'_>,
) -> anyhow::Result<String> {
    let msg = resolve_message(db, params.message_id)?;

    let conv = db
        .get_conversation(&msg.conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", msg.conversation_id))?;

    let connection = cfg
        .find_connection(&msg.connection_id)
        .or_else(|| cfg.find_connection_by_connector(&msg.connector))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No {} connection found in config for message {}",
                msg.connector,
                msg.id
            )
        })?;

    let conn = connector_factory::build_connector(connection, store_path)?;

    let fwd_id = conn
        .forward(
            &msg.external_id,
            &conv.external_id,
            params.to,
            params.comment,
        )
        .await?;
    Ok(fwd_id)
}

pub async fn archive(
    db: &Database,
    cfg: &VoidConfig,
    store_path: &Path,
    params: ArchiveParams<'_>,
) -> anyhow::Result<Value> {
    if let Some(before) = params.before {
        return archive_bulk_before(db, before, params.connector).await;
    }

    if params.message_ids.is_empty() {
        anyhow::bail!("at least one message ID is required (or use --before DATE)");
    }

    archive_by_ids(db, cfg, store_path, params.message_ids).await
}

pub fn mute(
    db: &Database,
    cfg: &mut VoidConfig,
    config_path: &Path,
    params: MuteParams<'_>,
) -> anyhow::Result<Value> {
    use crate::commands::mute::resolve;

    let connector = resolve_connector_filter(params.connector)?;
    let mute = !params.unmute;
    let action = if mute { "muted" } else { "unmuted" };
    let mut results = Vec::new();
    let mut affected_connections = HashSet::new();
    let mut config_changed = false;

    for target in params.targets {
        let matches =
            resolve::resolve_targets(db, target, params.connection, connector.as_deref())?;

        if matches.is_empty() {
            eprintln!("no conversation matching \"{target}\" found");
            results.push(json!({
                "target": target,
                "error": "not found",
            }));
            continue;
        }

        for conv in matches {
            let changed = if mute {
                cfg.add_ignore_conversation(&conv.connection_id, conv.external_id.clone())
            } else {
                cfg.remove_ignore_conversation(
                    &conv.connection_id,
                    &conv.external_id,
                    conv.name.as_deref(),
                )
            };
            config_changed |= changed;
            affected_connections.insert(conv.connection_id.clone());

            let name = conv.name.as_deref().unwrap_or(&conv.id);
            eprintln!("{action}: {name} [{}] ({})", conv.connector, conv.id);
            results.push(json!({
                "id": conv.id,
                "name": name,
                "connector": conv.connector,
                "connection_id": conv.connection_id,
                "external_id": conv.external_id,
                "is_muted": mute,
            }));
        }
    }

    if config_changed {
        cfg.save(config_path)?;
        for connection_id in &affected_connections {
            if let Some(conn) = cfg.connections.iter().find(|c| c.id == *connection_id) {
                db.sync_ignore_conversations(&conn.id, &conn.ignore_conversations)?;
            }
        }
    }

    Ok(json!({ "data": results, "error": null }))
}

/// Resolve `#channel-name` to a channel ID using the local database, or map a
/// void conversation id to its connector external id when `--conversation` is used.
pub fn resolve_send_target(
    db: &Database,
    to: Option<&str>,
    conversation: Option<&str>,
    connector_type: &str,
) -> anyhow::Result<String> {
    if let Some(conv_id) = conversation {
        let conv = db
            .get_conversation(conv_id)?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {conv_id}"))?;
        if conv.connector != connector_type {
            anyhow::bail!(
                "Conversation {conv_id} belongs to connector {}, not {connector_type}",
                conv.connector
            );
        }
        return Ok(conv.external_id);
    }

    let to = to.ok_or_else(|| anyhow::anyhow!("Either --to or --conversation is required"))?;

    if !to.starts_with('#') {
        return Ok(to.to_string());
    }
    let name = &to[1..];
    if let Some(conv) = db.find_conversation_by_name(name, connector_type)? {
        Ok(conv.external_id)
    } else {
        Ok(to.to_string())
    }
}

async fn archive_bulk_before(
    db: &Database,
    date_str: &str,
    connector: Option<&str>,
) -> anyhow::Result<Value> {
    let before_ts = parse_date_to_ts(date_str)
        .ok_or_else(|| anyhow::anyhow!("invalid date \"{date_str}\", expected YYYY-MM-DD"))?;

    let connector_filter = resolve_connector_filter(connector)?;

    let messages = db.bulk_archive_before(before_ts, connector_filter.as_deref())?;
    for msg in &messages {
        cleanup_cached_files(msg);
    }

    Ok(json!({ "data": { "archived_count": messages.len() }, "error": null }))
}

async fn archive_by_ids(
    db: &Database,
    cfg: &VoidConfig,
    store_path: &Path,
    message_ids: &[String],
) -> anyhow::Result<Value> {
    let mut connectors: HashMap<String, std::sync::Arc<dyn Connector>> = HashMap::new();
    let mut results = Vec::new();

    for message_id in message_ids {
        let msg = match resolve_message(db, message_id) {
            Ok(m) => m,
            Err(_) => {
                results.push(json!({
                    "message_id": message_id,
                    "is_archived": false,
                    "error": "message not found",
                }));
                continue;
            }
        };

        let conv = match db.get_conversation(&msg.conversation_id)? {
            Some(c) => c,
            None => {
                results.push(json!({
                    "message_id": message_id,
                    "is_archived": false,
                    "error": "conversation not found",
                }));
                continue;
            }
        };

        let connector_key = format!("{}:{}", msg.connector, msg.connection_id);
        if !connectors.contains_key(&connector_key) {
            if let Some(connection) = cfg
                .find_connection(&msg.connection_id)
                .or_else(|| cfg.find_connection_by_connector(&msg.connector))
            {
                if let Ok(c) = connector_factory::build_connector(connection, store_path) {
                    connectors.insert(connector_key.clone(), c);
                }
            }
        }

        let remote_synced = if let Some(conn) = connectors.get(&connector_key) {
            conn.archive(&msg.external_id, &conv.external_id)
                .await
                .is_ok()
        } else {
            false
        };

        db.mark_message_archived(message_id)?;
        cleanup_cached_files(&msg);

        results.push(json!({
            "message_id": message_id,
            "is_archived": true,
            "remote_synced": remote_synced,
        }));
    }

    Ok(json!({ "data": results, "error": null }))
}

async fn run_slack_scheduled_send(
    connection: &void_core::config::ConnectionConfig,
    channel: &str,
    message: &str,
    at_str: &str,
) -> anyhow::Result<String> {
    use crate::commands::slack::parse_schedule_time;

    let post_at = parse_schedule_time(at_str)?;
    let now = chrono::Utc::now().timestamp();
    if post_at <= now {
        anyhow::bail!("Scheduled time must be in the future.");
    }

    let user_token = void_core::config::settings_string(&connection.settings, "user_token")
        .ok_or_else(|| anyhow::anyhow!("missing user_token"))?;
    let app_token = void_core::config::settings_string(&connection.settings, "app_token")
        .ok_or_else(|| anyhow::anyhow!("missing app_token"))?;

    let connector = void_slack::connector::SlackConnector::new(
        &connection.id,
        &user_token,
        &app_token,
        None,
        None,
        std::env::temp_dir().as_path(),
        None,
    )?;

    connector
        .schedule_message(channel, message, post_at, None)
        .await
}

async fn run_slack_scheduled_reply(
    connection: &void_core::config::ConnectionConfig,
    channel_id: &str,
    thread_ts: &str,
    message: &str,
    at_str: &str,
) -> anyhow::Result<String> {
    use crate::commands::slack::parse_schedule_time;

    let post_at = parse_schedule_time(at_str)?;
    let now = chrono::Utc::now().timestamp();
    if post_at <= now {
        anyhow::bail!("Scheduled time must be in the future.");
    }

    let user_token = void_core::config::settings_string(&connection.settings, "user_token")
        .ok_or_else(|| anyhow::anyhow!("missing user_token"))?;
    let app_token = void_core::config::settings_string(&connection.settings, "app_token")
        .ok_or_else(|| anyhow::anyhow!("missing app_token"))?;

    let connector = void_slack::connector::SlackConnector::new(
        &connection.id,
        &user_token,
        &app_token,
        None,
        None,
        std::env::temp_dir().as_path(),
        None,
    )?;

    connector
        .schedule_message(channel_id, message, post_at, Some(thread_ts))
        .await
}

fn parse_date_to_ts(date: &str) -> Option<i64> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
}

fn cleanup_cached_files(msg: &void_core::models::Message) {
    let files = match msg
        .metadata
        .as_ref()
        .and_then(|m| m.get("files"))
        .and_then(|f| f.as_array())
    {
        Some(f) => f,
        None => return,
    };
    for file in files {
        if let Some(path) = file.get("local_path").and_then(|v| v.as_str()) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use void_core::config::VoidConfig;
    use void_core::models::{Conversation, ConversationKind};

    fn test_db() -> Database {
        Database::open(std::path::Path::new(":memory:")).expect("in-memory db")
    }

    fn seed_self_chat(db: &Database) {
        db.upsert_conversation(&Conversation {
            id: "wa_whatsapp_94004066660357@lid".into(),
            connection_id: "whatsapp".into(),
            connector: "whatsapp".into(),
            external_id: "94004066660357@lid".into(),
            name: Some("Message yourself".into()),
            kind: ConversationKind::SelfChat,
            last_message_at: None,
            unread_count: 0,
            is_muted: false,
            metadata: None,
        })
        .expect("seed conversation");
    }

    #[test]
    fn resolve_send_target_conversation_returns_external_id() {
        let db = test_db();
        seed_self_chat(&db);
        let target = resolve_send_target(
            &db,
            None,
            Some("wa_whatsapp_94004066660357@lid"),
            "whatsapp",
        )
        .unwrap();
        assert_eq!(target, "94004066660357@lid");
    }

    #[test]
    fn resolve_send_target_passthrough_non_channel() {
        let db = test_db();
        let target = resolve_send_target(&db, Some("33651090627"), None, "whatsapp").unwrap();
        assert_eq!(target, "33651090627");
    }

    #[test]
    fn archive_requires_ids_or_before() {
        let db = test_db();
        let cfg = VoidConfig::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(archive(
                &db,
                &cfg,
                std::path::Path::new("/tmp"),
                ArchiveParams {
                    message_ids: &[],
                    before: None,
                    connector: None,
                },
            ))
            .unwrap_err();
        assert!(err.to_string().contains("message ID is required"));
    }
}
