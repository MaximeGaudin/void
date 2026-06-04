use sysinfo::{Pid, System};
use void_core::config::VoidConfig;

use super::lock::{parse_lock_pid, refresh_process_exists};

/// Output sync daemon status and per-connector sync info as JSON to stdout.
pub fn show_status() -> anyhow::Result<()> {
    let cfg = crate::context::config();

    if crate::context::is_remote() {
        return show_remote_status(cfg);
    }

    show_local_status(cfg)
}

fn show_remote_status(cfg: &VoidConfig) -> anyhow::Result<()> {
    let remote = crate::context::get().remote_status()?;
    let db = crate::context::open_db().ok();

    let connections = build_connection_status(cfg, db.as_ref());
    let daemon_running = remote
        .get("remote_daemon_running")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let output = serde_json::json!({
        "daemon": {
            "running": daemon_running,
            "location": "remote",
        },
        "connections": connections,
        "remote": remote,
        "store_mode": "remote",
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn show_local_status(cfg: &VoidConfig) -> anyhow::Result<()> {
    let store_path = crate::context::store_path();
    let lock_path = store_path.join("LOCK");
    let log_path = store_path.join("void-sync.log");

    let mut daemon = serde_json::json!({ "running": false, "location": "local" });
    if lock_path.exists() {
        let content = std::fs::read_to_string(&lock_path).unwrap_or_default();
        if let Ok(pid) = parse_lock_pid(&content) {
            let sys_pid = Pid::from_u32(pid);
            let mut system = System::new_all();
            let alive = refresh_process_exists(&mut system, sys_pid);
            daemon = serde_json::json!({ "running": alive, "pid": pid, "location": "local" });
        }
    }

    let db = crate::context::open_db().ok();
    let connections = build_connection_status(cfg, db.as_ref());

    let mut output = serde_json::json!({
        "daemon": daemon,
        "connections": connections,
        "store_mode": "local",
    });

    if log_path.exists() {
        output["log_file"] = serde_json::json!(log_path.to_string_lossy());
        if let Ok(meta) = std::fs::metadata(&log_path) {
            output["log_file_bytes"] = serde_json::json!(meta.len());
        }
    }

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn build_connection_status(
    cfg: &VoidConfig,
    db: Option<&void_core::db::Database>,
) -> Vec<serde_json::Value> {
    let state_map = db
        .and_then(|db| db.list_sync_states().ok())
        .unwrap_or_default()
        .into_iter()
        .fold(
            std::collections::HashMap::<String, serde_json::Map<String, serde_json::Value>>::new(),
            |mut map, (conn_id, key, value)| {
                map.entry(conn_id)
                    .or_default()
                    .insert(key, serde_json::Value::String(value));
                map
            },
        );

    let mut connections = Vec::new();
    for connection in &cfg.connections {
        let conn_id = &connection.id;
        let connector_type = connection.connector_type.to_string();

        let last_message_at = db
            .and_then(|db| db.latest_message_timestamp(conn_id, &connector_type).ok())
            .flatten();

        let sync_state = state_map.get(conn_id).cloned().unwrap_or_default();

        let mut entry = serde_json::json!({
            "id": conn_id,
            "connector": connector_type,
        });
        if let Some(ts) = last_message_at {
            entry["last_message_at"] = serde_json::json!(ts);
        }
        if !sync_state.is_empty() {
            entry["sync_state"] = serde_json::Value::Object(sync_state);
        }
        connections.push(entry);
    }
    connections
}
