use void_core::config::{self, VoidConfig};
use void_core::db::Database;

pub fn run() {
    eprintln!("void v{}\n", env!("CARGO_PKG_VERSION"));

    let config_path = config::default_config_path();
    let cfg = match VoidConfig::load(&config_path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("No config found. Run `void setup` to get started.");
            eprintln!("Then add connections and run `void sync` to start syncing.\n");
            eprintln!("Usage: void <command> [options]");
            eprintln!("       void --help for all commands");
            return;
        }
    };

    let connections = cfg.connections.len();
    let store_path = cfg.store_path();
    let lock_running = store_path.join("LOCK").exists();

    if connections == 0 {
        eprintln!(
            "No connections configured. Edit {} to add connections.",
            config_path.display()
        );
        return;
    }

    eprintln!(
        "{} connection(s) | sync {}",
        connections,
        if lock_running { "running" } else { "stopped" }
    );

    if let Ok(db) = Database::open(&cfg.db_path()) {
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

    if !lock_running {
        eprintln!("\nRun `void sync` to start syncing messages.");
    }
}
