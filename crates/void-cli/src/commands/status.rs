pub fn run() {
    eprintln!("void v{}\n", env!("CARGO_PKG_VERSION"));

    let config_path = crate::context::client_config_path();
    if !config_path.exists() {
        eprintln!("No config found. Run `void setup` to get started.");
        eprintln!("Then add connections and run `void sync` to start syncing.\n");
        eprintln!("Usage: void <command> [options]");
        eprintln!("       void --help for all commands");
        return;
    }

    let cfg = crate::context::config();
    let connections = cfg.connections.len();
    let store_path = crate::context::store_path();
    let mode = crate::context::mode_label();

    let sync_label = if crate::context::is_remote() {
        match crate::context::get().remote_status() {
            Ok(status)
                if status
                    .get("remote_daemon_running")
                    .and_then(|v| v.as_bool())
                    == Some(true) =>
            {
                "running (remote)"
            }
            _ => "stopped (remote)",
        }
    } else if store_path.join("LOCK").exists() {
        "running"
    } else {
        "stopped"
    };

    if connections == 0 && !crate::context::is_remote() {
        eprintln!(
            "No connections configured. Edit {} to add connections.",
            config_path.display()
        );
        return;
    }

    eprintln!("store: {mode} | {connections} connection(s) | sync {sync_label}");

    if let Ok(db) = crate::context::open_db() {
        let convs = db
            .list_conversations(None, None, 10000, true)
            .map(|c| c.len())
            .unwrap_or(0);
        let recent = db
            .recent_messages(None, None, 5, true, true)
            .unwrap_or_default();

        if convs > 0 {
            eprintln!("{convs} conversations\n");
        }

        if !recent.is_empty() {
            eprintln!("Recent messages:");
            for msg in &recent {
                let sender = msg.sender_name.as_deref().unwrap_or(&msg.sender);
                let body = msg.body.as_deref().unwrap_or("(no text)");
                let preview = if body.len() > 50 {
                    format!("{}...", &body[..47])
                } else {
                    body.to_string()
                };
                eprintln!("  {sender}: {preview}");
            }
        }
    }

    if !crate::context::is_remote() && !store_path.join("LOCK").exists() {
        eprintln!("\nRun `void sync` to start syncing messages.");
    } else if crate::context::is_remote() {
        eprintln!("\nUse `void remote status` for cache and SSH details.");
    }
}
