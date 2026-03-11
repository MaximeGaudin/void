use void_core::config::{self, VoidConfig};
use void_core::db::Database;

pub fn run() -> anyhow::Result<()> {
    eprintln!("void doctor: checking system health...\n");

    let config_path = config::default_config_path();
    if config_path.exists() {
        eprintln!("[OK] Config file: {}", config_path.display());
    } else {
        eprintln!("[!!] No config file found at {}", config_path.display());
        eprintln!("     Run `void setup` to create one.");
        return Ok(());
    }

    let cfg = match VoidConfig::load(&config_path) {
        Ok(c) => {
            eprintln!("[OK] Config file parses correctly");
            c
        }
        Err(e) => {
            eprintln!("[!!] Config parse error: {e}");
            return Ok(());
        }
    };

    let db_path = cfg.db_path();
    let db = match Database::open(&db_path) {
        Ok(db) => {
            eprintln!("[OK] Database: {}", db_path.display());
            Some(db)
        }
        Err(e) => {
            eprintln!("[!!] Database error: {e}");
            None
        }
    };

    let store_path = cfg.store_path();
    let lock_path = store_path.join("LOCK");
    if lock_path.exists() {
        let pid = std::fs::read_to_string(&lock_path).unwrap_or_default();
        eprintln!("[OK] Sync daemon appears running ({})", pid.trim());
    } else {
        eprintln!("[--] Sync daemon not running");
    }

    eprintln!();
    if cfg.accounts.is_empty() {
        eprintln!("[!!] No accounts configured");
    } else {
        eprintln!("[OK] {} account(s) configured:", cfg.accounts.len());
        for acc in &cfg.accounts {
            eprintln!("     - {} ({})", acc.id, acc.account_type);
        }
    }

    if let Some(ref db) = db {
        eprintln!();
        let conv_count = db
            .list_conversations(None, None, 10000, true)
            .map(|c| c.len())
            .unwrap_or(0);
        let msg_count = db
            .recent_messages(None, None, 1, true, true)
            .map(|m| m.len())
            .unwrap_or(0);
        let event_count = db
            .list_events(Some(0), Some(i64::MAX), None, None, 10000)
            .map(|e| e.len())
            .unwrap_or(0);

        eprintln!("Database stats:");
        eprintln!("  Conversations: {conv_count}");
        eprintln!(
            "  Messages:      {}",
            if msg_count > 0 { "yes" } else { "empty" }
        );
        eprintln!(
            "  Events:        {}",
            if event_count > 0 { "yes" } else { "empty" }
        );
    }

    eprintln!("\nDoctor check complete.");
    Ok(())
}
