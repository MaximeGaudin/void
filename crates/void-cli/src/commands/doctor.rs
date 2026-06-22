use clap::Args;

use crate::commands::connector_factory;
use crate::commands::mute::run_one_time_legacy_mute_migration;
use crate::commands::setup::prompt::confirm_default_yes;
use crate::connectors;
use void_core::config::VoidConfig;
use void_core::db::Database;

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Report issues and exit with status 1 (skip interactive re-auth prompts)
    #[arg(long)]
    pub non_interactive: bool,
}

pub async fn run(args: &DoctorArgs) -> anyhow::Result<()> {
    eprintln!("void doctor: checking system health...\n");

    let mut issues = 0usize;
    let config_path = crate::context::client_config_path();

    if config_path.exists() {
        eprintln!("[OK] Config file: {}", config_path.display());
    } else {
        eprintln!("[!!] No config file found at {}", config_path.display());
        eprintln!("     Run `void setup` to create one.");
        issues += 1;
        return finish(args.non_interactive, issues);
    }

    if crate::context::is_remote() {
        return run_remote_doctor(args, &mut issues);
    }

    let cfg = match VoidConfig::load(&config_path) {
        Ok(c) => {
            eprintln!("[OK] Config file parses correctly");
            c
        }
        Err(e) => {
            eprintln!("[!!] Config parse error: {e}");
            issues += 1;
            return finish(args.non_interactive, issues);
        }
    };

    let mut cfg = cfg;

    if let Err(e) = connectors::validate_all_connections(&cfg) {
        eprintln!("[!!] Connection settings invalid: {e}");
        issues += 1;
    }

    let db_path = cfg.db_path();
    let db = match Database::open(&db_path) {
        Ok(db) => {
            eprintln!("[OK] Database: {}", db_path.display());
            match run_one_time_legacy_mute_migration(&mut cfg, &db, &config_path) {
                Ok(0) => {}
                Ok(count) => {
                    eprintln!(
                        "[OK] Migrated {count} muted conversation(s) from database into config.toml"
                    );
                }
                Err(e) => {
                    eprintln!("[!!] Failed to migrate legacy mutes to config: {e}");
                    issues += 1;
                }
            }
            Some(db)
        }
        Err(e) => {
            eprintln!("[!!] Database error: {e}");
            issues += 1;
            None
        }
    };

    let store_path = crate::context::store_path();
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
        issues += 1;
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
            issues += failed_connections.len();
            if !args.non_interactive {
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

    finish(args.non_interactive, issues)
}

fn run_remote_doctor(args: &DoctorArgs, issues: &mut usize) -> anyhow::Result<()> {
    eprintln!("[OK] Store mode: remote");
    eprintln!(
        "[OK] Local client profile: {} (no [[connections]] here is expected)",
        crate::context::client_config_path().display()
    );

    let mut remote_host = "remote host".to_string();
    match crate::context::get().remote_status() {
        Ok(status) => {
            if let Some(host) = status.get("host").and_then(|v| v.as_str()) {
                remote_host = host.to_string();
            }
            let ssh_ok = status
                .get("ssh_reachable")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if ssh_ok {
                eprintln!("[OK] SSH connection to remote host");
            } else {
                eprintln!("[!!] Cannot reach remote host via SSH");
                *issues += 1;
            }

            let daemon = status
                .get("remote_daemon_running")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if daemon {
                eprintln!("[OK] Remote sync daemon appears running");
            } else {
                eprintln!("[!!] Remote sync daemon not running");
                *issues += 1;
            }

            if let Some(age) = status.get("database_age_secs").and_then(|v| v.as_u64()) {
                eprintln!("[OK] Local database snapshot age: {age}s");
            }
        }
        Err(e) => {
            eprintln!("[!!] Remote status error: {e}");
            *issues += 1;
        }
    }

    let mut conv_count = 0usize;
    match crate::context::open_db() {
        Ok(db) => {
            eprintln!(
                "[OK] Local database snapshot: {}",
                crate::context::get().db_path().display()
            );
            conv_count = db
                .list_conversations(None, None, 10000, true)
                .map(|c| c.len())
                .unwrap_or(0);
            eprintln!("  Conversations: {conv_count}");
        }
        Err(e) => {
            eprintln!("[!!] Database snapshot error: {e}");
            *issues += 1;
        }
    }

    let cache_config = crate::context::store_path().join("config.toml");
    eprintln!("[--] Cached server config: {}", cache_config.display());

    // Read the raw cached config to detect if the server file is actually a Mac remote stub.
    // The context normalizes store.mode to Local after loading, so we must check the raw file.
    let raw_is_client_stub = cache_config
        .exists()
        .then(|| std::fs::read_to_string(&cache_config).ok())
        .flatten()
        .and_then(|content| VoidConfig::parse(&content).ok())
        .map(|raw| raw.is_remote_client_profile())
        .unwrap_or(false);

    let cfg = crate::context::config();

    if raw_is_client_stub {
        eprintln!(
            "[WARN] Server config on {remote_host} is the Mac remote profile, not a full void config"
        );
        eprintln!(
            "       Put [[connections]] on the server at ~/.config/void/config.toml (tokens, accounts)."
        );
        eprintln!(
            "       Keep only [store.remote] on this Mac at {}.",
            crate::context::client_config_path().display()
        );
        if conv_count > 0 {
            eprintln!(
                "       Snapshot has {conv_count} conversations (daemon is using older in-memory config)."
            );
        } else {
            *issues += 1;
        }
    } else if cfg.connections.is_empty() {
        eprintln!("[!!] Cached server config has no [[connections]]");
        *issues += 1;
    } else {
        eprintln!(
            "[OK] {} connection(s) in cached server config (connector checks run on server)",
            cfg.connections.len()
        );
    }

    finish(args.non_interactive, *issues)
}

fn finish(non_interactive: bool, issues: usize) -> anyhow::Result<()> {
    eprintln!("\nDoctor check complete.");
    if non_interactive && issues > 0 {
        anyhow::bail!("doctor found {issues} issue(s)");
    }
    Ok(())
}
