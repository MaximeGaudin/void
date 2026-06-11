use void_core::db::agent_inbox::{AgentInboxInsert, AgentInboxItem};
use void_core::db::Database;

pub(crate) const VALID_TYPES: &[&str] = &["fyi", "approval", "input", "action"];
pub(crate) const VALID_STATUSES: &[&str] = &["unread", "read", "done"];
pub(crate) const VALID_PRIORITIES: &[&str] = &["normal", "high"];

fn open_db() -> anyhow::Result<Database> {
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_submit(
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

pub(crate) fn validate_action_json(json_str: &str) -> anyhow::Result<()> {
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

pub(crate) fn run_list(status: Option<&str>, item_type: Option<&str>, size: i64) -> anyhow::Result<()> {
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

pub(crate) fn run_get(callback_id: &str) -> anyhow::Result<()> {
    let db = open_db()?;
    match db.agent_inbox_get(callback_id)? {
        Some(item) => print_item(&item),
        None => anyhow::bail!("item not found: {callback_id}"),
    }
}

pub(crate) fn run_respond(
    callback_id: &str,
    response: &str,
    comment: Option<&str>,
) -> anyhow::Result<()> {
    let db = open_db()?;
    let updated = db.agent_inbox_respond(callback_id, response, comment)?;
    if !updated {
        anyhow::bail!("item not found: {callback_id}");
    }
    match db.agent_inbox_get(callback_id)? {
        Some(item) => print_item(&item),
        None => anyhow::bail!("item not found after respond: {callback_id}"),
    }
}

pub(crate) fn run_mark_read(callback_id: &str) -> anyhow::Result<()> {
    let db = open_db()?;
    db.agent_inbox_mark_read(callback_id)?;
    match db.agent_inbox_get(callback_id)? {
        Some(item) => print_item(&item),
        None => anyhow::bail!("item not found: {callback_id}"),
    }
}

pub(crate) fn run_archive(callback_ids: &[String]) -> anyhow::Result<()> {
    if callback_ids.is_empty() {
        anyhow::bail!("at least one callback ID is required");
    }
    let db = open_db()?;
    let count = db.agent_inbox_archive(callback_ids)?;
    let output = serde_json::json!({ "data": { "archived_count": count }, "error": null });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
