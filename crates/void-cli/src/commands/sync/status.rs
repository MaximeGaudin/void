use sysinfo::{Pid, System};
use void_core::config::{self, VoidConfig};

use super::lock::{parse_lock_pid, refresh_process_exists};

/// Output sync daemon status and per-connector sync info as JSON to stdout.
pub fn show_status() -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path).map_err(|e| {
        anyhow::anyhow!(
            "Cannot load config from {}: {e}\nRun `void setup` first.",
            config_path.display()
        )
    })?;

    let store_path = cfg.store_path();
    let lock_path = store_path.join("LOCK");
    let log_path = store_path.join("void-sync.log");

    let mut daemon = serde_json::json!({ "running": false });
    if lock_path.exists() {
        let content = std::fs::read_to_string(&lock_path).unwrap_or_default();
        if let Ok(pid) = parse_lock_pid(&content) {
            let sys_pid = Pid::from_u32(pid);
            let mut system = System::new_all();
            let alive = refresh_process_exists(&mut system, sys_pid);
            daemon = serde_json::json!({ "running": alive, "pid": pid });
        }
    }

    let mut connections = Vec::new();

    let db = void_core::db::Database::open(&cfg.db_path()).ok();

    let state_map = db
        .as_ref()
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

    for connection in &cfg.connections {
        let conn_id = &connection.id;
        let connector_type = connection.connector_type.to_string();

        let last_message_at = db
            .as_ref()
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

    let mut output = serde_json::json!({
        "daemon": daemon,
        "connections": connections,
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
