use void_core::config::{self, VoidConfig};
use void_core::db::Database;

pub fn run() -> anyhow::Result<()> {
    eprintln!("void doctor: checking system health...\n");

    let config_path = config::default_config_path();
    if config_path.exists() {
        eprintln!("[OK] Config file: {}", config_path.display());
    } else {
        eprintln!("[!!] No config file found at {}", config_path.display());
        eprintln!("     Run `void config init` to create one.");
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
    match Database::open(&db_path) {
        Ok(_db) => {
            eprintln!("[OK] Database: {}", db_path.display());
        }
        Err(e) => {
            eprintln!("[!!] Database error: {e}");
        }
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

    eprintln!("\nDoctor check complete.");
    Ok(())
}
