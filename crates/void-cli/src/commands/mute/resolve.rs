use std::collections::HashSet;

use void_core::db::Database;
use void_core::models::Conversation;

pub(crate) fn resolve_targets(
    db: &Database,
    target: &str,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
) -> anyhow::Result<Vec<Conversation>> {
    if let Some(conv) = db.get_conversation(target)? {
        if connection_filter.is_some_and(|filter| !conv.connection_id.contains(filter)) {
            return Ok(vec![]);
        }
        if connector_filter.is_some_and(|filter| conv.connector != filter) {
            return Ok(vec![]);
        }
        return Ok(vec![conv]);
    }

    let matches = db.list_channels(connection_filter, connector_filter, Some(target), 100, true)?;
    let dm_matches = find_conversations_by_name(db, target, connection_filter, connector_filter)?;
    let mut seen = HashSet::new();
    Ok(matches
        .into_iter()
        .chain(dm_matches)
        .filter(|conv| seen.insert(conv.id.clone()))
        .collect())
}

pub(super) fn find_conversations_by_name(
    db: &Database,
    search: &str,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
) -> anyhow::Result<Vec<Conversation>> {
    let all = db.list_conversations(connection_filter, connector_filter, 500, true)?;
    let lower = search.to_lowercase();
    Ok(all
        .into_iter()
        .filter(|c| {
            c.name
                .as_ref()
                .is_some_and(|n| n.to_lowercase().contains(&lower))
        })
        .collect())
}
