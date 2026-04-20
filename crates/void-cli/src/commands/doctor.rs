use crate::commands::connector_factory;
use crate::commands::setup::prompt::confirm_default_yes;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

pub async fn run() -> anyhow::Result<()> {
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
    if cfg.connections.is_empty() {
        eprintln!("[!!] No connections configured");
    } else {
        eprintln!("[OK] {} connection(s) configured:", cfg.connections.len());

        let mut failed_connections = Vec::new();

        for conn_config in &cfg.connections {
            eprint!(
                "     - {} ({}): checking... ",
                conn_config.id, conn_config.connector_type
            );

            // Flush stderr to ensure checking message appears immediately
            std::io::Write::flush(&mut std::io::stderr()).ok();

            match connector_factory::build_connector(conn_config, &store_path) {
                Ok(connector) => match connector.health_check().await {
                    Ok(status) => {
                        if status.ok {
                            eprintln!("OK ({})", status.message);
                        } else {
                            eprintln!("FAILED ({})", status.message);
                            failed_connections.push(conn_config.clone());
                        }
                    }
                    Err(e) => {
                        eprintln!("ERROR ({})", e);
                        failed_connections.push(conn_config.clone());
                    }
                },
                Err(e) => {
                    eprintln!("ERROR BUILDING ({})", e);
                    failed_connections.push(conn_config.clone());
                }
            }
        }

        if !failed_connections.is_empty() {
            eprintln!("\n[!!] Some connections failed health checks.");
            let mut cfg_mut = cfg.clone();
            for conn_config in failed_connections {
                if confirm_default_yes(&format!(
                    "Would you like to re-authenticate connection '{}'?",
                    conn_config.id
                )) {
                    eprintln!("Re-authenticating {}...", conn_config.id);
                    if let Some(idx) = cfg_mut
                        .connections
                        .iter()
                        .position(|c| c.id == conn_config.id)
                    {
                        if let Err(e) = crate::commands::setup::connection_menu::reauthenticate_specific_connection(
                            &mut cfg_mut,
                            &config_path,
                            &store_path,
                            idx,
                        )
                        .await
                        {
                            eprintln!("  ✗ Error during re-authentication: {}", e);
                        }
                    }
                }
            }
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
