use void_core::config::{conversation_matches_ignore, VoidConfig};
use void_core::db::Database;

pub(super) fn list_muted(
    cfg: &VoidConfig,
    db: &Database,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
) -> anyhow::Result<()> {
    let mut items = Vec::new();

    for conn in &cfg.connections {
        if connection_filter.is_some_and(|filter| !conn.id.contains(filter)) {
            continue;
        }
        if connector_filter.is_some_and(|filter| conn.connector_type.to_string() != filter) {
            continue;
        }
        if conn.ignore_conversations.is_empty() {
            continue;
        }

        let conversations = db.list_conversations(Some(&conn.id), None, 10_000, true)?;

        for pattern in &conn.ignore_conversations {
            let matches: Vec<_> = conversations
                .iter()
                .filter(|c| {
                    conversation_matches_ignore(
                        c.name.as_deref(),
                        &c.external_id,
                        std::slice::from_ref(pattern),
                    )
                })
                .collect();

            if matches.is_empty() {
                items.push(serde_json::json!({
                    "connection_id": conn.id,
                    "connector": conn.connector_type.to_string(),
                    "pattern": pattern,
                }));
                continue;
            }

            for conv in matches {
                items.push(serde_json::json!({
                    "id": conv.id,
                    "name": conv.name,
                    "connector": conv.connector,
                    "connection_id": conv.connection_id,
                    "pattern": pattern,
                }));
            }
        }
    }

    println!("{}", serde_json::json!({ "data": items, "error": null }));
    Ok(())
}
