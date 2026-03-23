use std::path::Path;
use std::sync::Arc;

use void_core::config::{ConnectionConfig, VoidConfig};

use super::prompt::{confirm_default_yes, select};
use crate::commands::connector_factory;

pub(crate) enum ConnectorAction {
    Skip,
    Keep,
    Replace(usize),
    Add,
}

pub(crate) fn pick_connector_action(
    name: &str,
    existing_indices: &[usize],
    cfg: &VoidConfig,
) -> ConnectorAction {
    if existing_indices.is_empty() {
        if confirm_default_yes(&format!("Set up {name}?")) {
            ConnectorAction::Add
        } else {
            ConnectorAction::Skip
        }
    } else if existing_indices.len() == 1 {
        let idx = existing_indices[0];
        let acc = &cfg.connections[idx];
        eprintln!();
        eprintln!("  Existing connection: {} ({})", acc.id, acc.connector_type);
        let choice = select(
            &format!("You already have a {name} connection configured:"),
            &[
                "Keep the current configuration",
                "Replace with new configuration",
                "Add another connection",
                "Skip",
            ],
        );
        match choice {
            0 => ConnectorAction::Keep,
            1 => ConnectorAction::Replace(idx),
            2 => ConnectorAction::Add,
            _ => ConnectorAction::Skip,
        }
    } else {
        eprintln!();
        eprintln!("  Existing connections:");
        for &idx in existing_indices {
            eprintln!(
                "    • {} ({})",
                cfg.connections[idx].id, cfg.connections[idx].connector_type
            );
        }
        let choice = select(
            &format!(
                "You have {} {name} connections configured:",
                existing_indices.len()
            ),
            &[
                "Keep all current connections",
                "Add another connection",
                "Skip",
            ],
        );
        match choice {
            0 => ConnectorAction::Keep,
            1 => ConnectorAction::Add,
            _ => ConnectorAction::Skip,
        }
    }
}

pub(crate) async fn authenticate_connection(
    connection: &ConnectionConfig,
    store_path: &Path,
) -> anyhow::Result<()> {
    let mut conn = connector_factory::build_connector(connection, store_path)?;
    let conn_mut = Arc::get_mut(&mut conn)
        .ok_or_else(|| anyhow::anyhow!("internal error: could not get mutable connector ref"))?;
    conn_mut.authenticate().await
}
