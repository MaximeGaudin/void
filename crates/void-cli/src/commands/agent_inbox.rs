use clap::{Args, Subcommand};

use void_core::config::{self, VoidConfig};
use void_core::db::agent_inbox::{AgentInboxInsert, AgentInboxItem};
use void_core::db::Database;

#[derive(Debug, Args)]
pub struct AgentInboxArgs {
    #[command(subcommand)]
    pub command: AgentInboxCommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentInboxCommand {
    /// Submit a new item to the agent inbox
    Submit {
        /// Item type: fyi, approval, input, action
        #[arg(long = "type")]
        item_type: String,

        /// Unique callback ID (auto-generated UUID if omitted)
        #[arg(long)]
        callback_id: Option<String>,

        /// Source agent name
        #[arg(long)]
        source: String,

        /// Title / subject line
        #[arg(long)]
        title: String,

        /// Markdown body
        #[arg(long)]
        body: String,

        /// Priority: normal or high
        #[arg(long, default_value = "normal")]
        priority: String,

        /// Action JSON (inline)
        #[arg(long, conflicts_with = "action_file")]
        action: Option<String>,

        /// Read action JSON from a file (use - for stdin)
        #[arg(long, conflicts_with = "action")]
        action_file: Option<String>,

        /// Label for the input field (input type only)
        #[arg(long)]
        input_label: Option<String>,
    },
    /// List inbox items
    List {
        /// Filter by status: unread, read, done
        #[arg(long)]
        status: Option<String>,

        /// Filter by type: fyi, approval, input, action
        #[arg(long = "type")]
        item_type: Option<String>,

        /// Max number of items to return
        #[arg(long, default_value = "50")]
        size: i64,
    },
    /// Get a single item by callback ID
    Get {
        /// Callback ID
        callback_id: String,
    },
    /// Record a response on an item
    Respond {
        /// Callback ID
        callback_id: String,

        /// Response text
        #[arg(long)]
        response: String,

        /// Optional comment
        #[arg(long)]
        comment: Option<String>,
    },
    /// Mark an item as read
    MarkRead {
        /// Callback ID
        callback_id: String,
    },
    /// Archive one or more items (sets status to done)
    Archive {
        /// Callback IDs to archive
        callback_ids: Vec<String>,
    },
}

pub fn run(args: &AgentInboxArgs) -> anyhow::Result<()> {
    match &args.command {
        AgentInboxCommand::Submit {
            item_type,
            callback_id,
            source,
            title,
            body,
            priority,
            action,
            action_file,
            input_label,
        } => run_submit(
            item_type,
            callback_id.as_deref(),
            source,
            title,
            body,
            priority,
            action.as_deref(),
            action_file.as_deref(),
            input_label.as_deref(),
        ),
        AgentInboxCommand::List {
            status,
            item_type,
            size,
        } => run_list(status.as_deref(), item_type.as_deref(), *size),
        AgentInboxCommand::Get { callback_id } => run_get(callback_id),
        AgentInboxCommand::Respond {
            callback_id,
            response,
            comment,
        } => run_respond(callback_id, response, comment.as_deref()),
        AgentInboxCommand::MarkRead { callback_id } => run_mark_read(callback_id),
        AgentInboxCommand::Archive { callback_ids } => run_archive(callback_ids),
    }
}

fn open_db() -> anyhow::Result<Database> {
    let cfg = crate::context::config();
    crate::context::open_db()
}

fn print_item(item: &AgentInboxItem) -> anyhow::Result<()> {
    let output = serde_json::json!({ "data": item, "error": null });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_items(items: &[AgentInboxItem]) -> anyhow::Result<()> {
    let output = serde_json::json!({ "data": items, "error": null });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

const VALID_TYPES: &[&str] = &["fyi", "approval", "input", "action"];
const VALID_STATUSES: &[&str] = &["unread", "read", "done"];
const VALID_PRIORITIES: &[&str] = &["normal", "high"];

#[allow(clippy::too_many_arguments)]
fn run_submit(
    item_type: &str,
    callback_id: Option<&str>,
    source: &str,
    title: &str,
    body: &str,
    priority: &str,
    action: Option<&str>,
    action_file: Option<&str>,
    input_label: Option<&str>,
) -> anyhow::Result<()> {
    if !VALID_TYPES.contains(&item_type) {
        anyhow::bail!(
            "invalid type \"{item_type}\". Must be one of: {}",
            VALID_TYPES.join(", ")
        );
    }
    if !VALID_PRIORITIES.contains(&priority) {
        anyhow::bail!(
            "invalid priority \"{priority}\". Must be one of: {}",
            VALID_PRIORITIES.join(", ")
        );
    }

    let action_json = resolve_action_json(action, action_file)?;

    if item_type == "action" && action_json.is_none() {
        anyhow::bail!("action type requires --action or --action-file");
    }
    if let Some(ref json_str) = action_json {
        validate_action_json(json_str)?;
    }

    let generated_id;
    let callback_id = match callback_id {
        Some(id) => id,
        None => {
            generated_id = uuid::Uuid::new_v4().to_string();
            &generated_id
        }
    };

    let now = chrono::Utc::now().to_rfc3339();
    let db = open_db()?;
    let insert = AgentInboxInsert {
        callback_id,
        item_type,
        source,
        title,
        body,
        priority,
        action_json: action_json.as_deref(),
        input_label,
        created_at: &now,
    };
    let item = db.agent_inbox_insert(&insert)?;
    print_item(&item)
}

fn resolve_action_json(
    inline: Option<&str>,
    file_path: Option<&str>,
) -> anyhow::Result<Option<String>> {
    match (inline, file_path) {
        (Some(json), _) => Ok(Some(json.to_string())),
        (_, Some("-")) => {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
            Ok(Some(buf.trim().to_string()))
        }
        (_, Some(path)) => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("failed to read action file \"{path}\": {e}"))?;
            Ok(Some(content.trim().to_string()))
        }
        (None, None) => Ok(None),
    }
}

fn validate_action_json(json_str: &str) -> anyhow::Result<()> {
    let val: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| anyhow::anyhow!("invalid action JSON: {e}"))?;
    let obj = val
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("action JSON must be an object"))?;
    if !obj.contains_key("command") {
        anyhow::bail!("action JSON must contain a \"command\" field");
    }
    Ok(())
}

fn run_list(status: Option<&str>, item_type: Option<&str>, size: i64) -> anyhow::Result<()> {
    if let Some(s) = status {
        if !VALID_STATUSES.contains(&s) {
            anyhow::bail!(
                "invalid status \"{s}\". Must be one of: {}",
                VALID_STATUSES.join(", ")
            );
        }
    }
    if let Some(t) = item_type {
        if !VALID_TYPES.contains(&t) {
            anyhow::bail!(
                "invalid type \"{t}\". Must be one of: {}",
                VALID_TYPES.join(", ")
            );
        }
    }

    let db = open_db()?;
    let items = db.agent_inbox_list(status, item_type, size)?;
    print_items(&items)
}

fn run_get(callback_id: &str) -> anyhow::Result<()> {
    let db = open_db()?;
    match db.agent_inbox_get(callback_id)? {
        Some(item) => print_item(&item),
        None => anyhow::bail!("item not found: {callback_id}"),
    }
}

fn run_respond(callback_id: &str, response: &str, comment: Option<&str>) -> anyhow::Result<()> {
    let db = open_db()?;
    let updated = db.agent_inbox_respond(callback_id, response, comment)?;
    if !updated {
        anyhow::bail!("item not found: {callback_id}");
    }
    let item = db.agent_inbox_get(callback_id)?.unwrap();
    print_item(&item)
}

fn run_mark_read(callback_id: &str) -> anyhow::Result<()> {
    let db = open_db()?;
    db.agent_inbox_mark_read(callback_id)?;
    match db.agent_inbox_get(callback_id)? {
        Some(item) => print_item(&item),
        None => anyhow::bail!("item not found: {callback_id}"),
    }
}

fn run_archive(callback_ids: &[String]) -> anyhow::Result<()> {
    if callback_ids.is_empty() {
        anyhow::bail!("at least one callback ID is required");
    }
    let db = open_db()?;
    let count = db.agent_inbox_archive(callback_ids)?;
    let output = serde_json::json!({ "data": { "archived_count": count }, "error": null });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCommand,
    }

    #[derive(Debug, clap::Subcommand)]
    enum TestCommand {
        AgentInbox(AgentInboxArgs),
    }

    fn parse(args: &[&str]) -> TestCli {
        TestCli::try_parse_from(args).expect("should parse")
    }

    fn parse_err(args: &[&str]) -> clap::Error {
        TestCli::try_parse_from(args).expect_err("should fail to parse")
    }

    // ---- Submit parsing ----

    #[test]
    fn parse_submit_minimal() {
        let cli = parse(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "fyi",
            "--source",
            "agent",
            "--title",
            "Title",
            "--body",
            "Body",
        ]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Submit {
                    item_type,
                    source,
                    title,
                    body,
                    priority,
                    ..
                } => {
                    assert_eq!(item_type, "fyi");
                    assert_eq!(source, "agent");
                    assert_eq!(title, "Title");
                    assert_eq!(body, "Body");
                    assert_eq!(priority, "normal");
                }
                other => panic!("expected Submit, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_submit_with_all_options() {
        let cli = parse(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "action",
            "--callback-id",
            "cb-123",
            "--source",
            "daily-routine",
            "--title",
            "Reply needed",
            "--body",
            "Full body",
            "--priority",
            "high",
            "--action",
            r#"{"command":"reply"}"#,
            "--input-label",
            "Your reply",
        ]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Submit {
                    item_type,
                    callback_id,
                    priority,
                    action,
                    input_label,
                    ..
                } => {
                    assert_eq!(item_type, "action");
                    assert_eq!(callback_id.as_deref(), Some("cb-123"));
                    assert_eq!(priority, "high");
                    assert!(action.as_ref().unwrap().contains("reply"));
                    assert_eq!(input_label.as_deref(), Some("Your reply"));
                }
                other => panic!("expected Submit, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_submit_action_and_action_file_conflict() {
        parse_err(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "action",
            "--source",
            "a",
            "--title",
            "t",
            "--body",
            "b",
            "--action",
            "{}",
            "--action-file",
            "path.json",
        ]);
    }

    #[test]
    fn parse_submit_requires_source() {
        parse_err(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "fyi",
            "--title",
            "t",
            "--body",
            "b",
        ]);
    }

    #[test]
    fn parse_submit_requires_title() {
        parse_err(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "fyi",
            "--source",
            "a",
            "--body",
            "b",
        ]);
    }

    #[test]
    fn parse_submit_requires_body() {
        parse_err(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "fyi",
            "--source",
            "a",
            "--title",
            "t",
        ]);
    }

    #[test]
    fn parse_submit_requires_type() {
        parse_err(&[
            "test",
            "agent-inbox",
            "submit",
            "--source",
            "a",
            "--title",
            "t",
            "--body",
            "b",
        ]);
    }

    // ---- List parsing ----

    #[test]
    fn parse_list_defaults() {
        let cli = parse(&["test", "agent-inbox", "list"]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::List {
                    status,
                    item_type,
                    size,
                } => {
                    assert!(status.is_none());
                    assert!(item_type.is_none());
                    assert_eq!(*size, 50);
                }
                other => panic!("expected List, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_list_with_filters() {
        let cli = parse(&[
            "test",
            "agent-inbox",
            "list",
            "--status",
            "unread",
            "--type",
            "approval",
            "--size",
            "10",
        ]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::List {
                    status,
                    item_type,
                    size,
                } => {
                    assert_eq!(status.as_deref(), Some("unread"));
                    assert_eq!(item_type.as_deref(), Some("approval"));
                    assert_eq!(*size, 10);
                }
                other => panic!("expected List, got {other:?}"),
            },
        }
    }

    // ---- Get parsing ----

    #[test]
    fn parse_get() {
        let cli = parse(&["test", "agent-inbox", "get", "cb-123"]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Get { callback_id } => {
                    assert_eq!(callback_id, "cb-123");
                }
                other => panic!("expected Get, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_get_requires_callback_id() {
        parse_err(&["test", "agent-inbox", "get"]);
    }

    // ---- Respond parsing ----

    #[test]
    fn parse_respond() {
        let cli = parse(&[
            "test",
            "agent-inbox",
            "respond",
            "cb-123",
            "--response",
            "approved",
            "--comment",
            "LGTM",
        ]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Respond {
                    callback_id,
                    response,
                    comment,
                } => {
                    assert_eq!(callback_id, "cb-123");
                    assert_eq!(response, "approved");
                    assert_eq!(comment.as_deref(), Some("LGTM"));
                }
                other => panic!("expected Respond, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_respond_requires_response_flag() {
        parse_err(&["test", "agent-inbox", "respond", "cb-123"]);
    }

    // ---- Mark-read parsing ----

    #[test]
    fn parse_mark_read() {
        let cli = parse(&["test", "agent-inbox", "mark-read", "cb-123"]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::MarkRead { callback_id } => {
                    assert_eq!(callback_id, "cb-123");
                }
                other => panic!("expected MarkRead, got {other:?}"),
            },
        }
    }

    // ---- Archive parsing ----

    #[test]
    fn parse_archive_single() {
        let cli = parse(&["test", "agent-inbox", "archive", "cb-1"]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Archive { callback_ids } => {
                    assert_eq!(callback_ids, &["cb-1"]);
                }
                other => panic!("expected Archive, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_archive_multiple() {
        let cli = parse(&["test", "agent-inbox", "archive", "cb-1", "cb-2", "cb-3"]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Archive { callback_ids } => {
                    assert_eq!(callback_ids, &["cb-1", "cb-2", "cb-3"]);
                }
                other => panic!("expected Archive, got {other:?}"),
            },
        }
    }

    // ---- Validation tests ----

    #[test]
    fn validate_action_json_valid() {
        assert!(validate_action_json(r#"{"command":"reply","void_message_id":"m1"}"#).is_ok());
    }

    #[test]
    fn validate_action_json_missing_command() {
        let err = validate_action_json(r#"{"void_message_id":"m1"}"#).unwrap_err();
        assert!(err.to_string().contains("command"));
    }

    #[test]
    fn validate_action_json_not_object() {
        let err = validate_action_json(r#"["array"]"#).unwrap_err();
        assert!(err.to_string().contains("object"));
    }

    #[test]
    fn validate_action_json_invalid_json() {
        let err = validate_action_json("not json").unwrap_err();
        assert!(err.to_string().contains("invalid"));
    }
}
