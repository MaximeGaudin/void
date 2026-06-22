use std::sync::Arc;

use tokio::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use void_core::connector::Connector;
use void_core::db::Database;
use void_core::hooks::{self, HookRunner};
use void_core::sync::SyncEngine;
use void_whatsapp::rpc::Server as WhatsAppRpcServer;

use crate::commands::connector_factory;
use crate::connectors;
use crate::output::{resolve_connector_filter, resolve_connector_list};

use super::SyncArgs;

const MAX_CONSECUTIVE_FAILURES: u32 = 10;
const STABLE_THRESHOLD: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

pub async fn run(args: &SyncArgs) -> anyhow::Result<()> {
    let cfg = crate::context::config();

    if cfg.connections.is_empty() {
        anyhow::bail!("No connections configured. Add connections to your config.toml first.");
    }

    connectors::validate_all_connections(cfg)?;

    let connector_filter = resolve_connector_list(args.connectors.as_deref())?;

    let store_path = crate::context::store_path();
    std::fs::create_dir_all(&store_path)?;

    if args.restart {
        let lock_path = store_path.join("LOCK");
        if lock_path.exists() {
            super::daemon::stop_daemon().ok();
        }
    }

    if args.clear {
        let db_path = crate::context::store_path().join("void.db");
        if db_path.exists() {
            std::fs::remove_file(&db_path)?;
            eprintln!("Database cleared: {}", db_path.display());
            info!(path = %db_path.display(), "database cleared");
        }
    }

    let db = Arc::new(crate::context::open_db_writable()?);

    if let Some(ref connector_type) = args.clear_connector {
        let ct = resolve_connector_filter(Some(connector_type))?.ok_or_else(|| {
            anyhow::anyhow!("internal error: connector type missing after successful parse")
        })?;
        let (msgs, convs, evts, sync_st) = db.clear_connector_data(&ct)?;
        eprintln!(
            "Cleared {ct} data: {msgs} messages, {convs} conversations, {evts} events, {sync_st} sync states"
        );
        info!(
            connector = %ct, msgs, convs, evts, sync_st,
            "connector data cleared"
        );

        for connection in &cfg.connections {
            if connection.connector_type.as_str() != ct {
                continue;
            }
            let plugin = connectors::by_id(&ct);
            if let Some(plugin) = plugin {
                for session_file in (plugin.session_files)(&store_path, &connection.id) {
                    if session_file.exists() {
                        std::fs::remove_file(&session_file)?;
                        eprintln!(
                            "Removed {} session: {} (will require re-pairing)",
                            plugin.menu_label,
                            session_file.display()
                        );
                    }
                }
            }
        }
    }

    let mut connectors: Vec<Arc<dyn void_core::connector::Connector>> = Vec::new();
    let mut broken: Vec<String> = Vec::new();
    let wa_rpc = WhatsAppRpcServer::new(&store_path);

    for connection in &cfg.connections {
        if let Some(ref filter) = connector_filter {
            let type_str = connection.connector_type.to_string();
            if !filter.iter().any(|f| type_str.contains(f)) {
                continue;
            }
        }

        let plugin = connectors::by_id(connection.connector_type.as_str());

        if plugin.is_some_and(|p| p.uses_daemon_rpc) {
            let wa = connector_factory::build_whatsapp_connector(connection, &store_path);
            wa_rpc.register(&connection.id, Arc::clone(&wa)).await;
            match wa.health_check().await {
                Ok(status) if status.ok => {
                    connectors.push(wa as Arc<dyn void_core::connector::Connector>)
                }
                Ok(status) => {
                    broken.push(format!(
                        "Connection '{}' ({}) is broken: {}. Run `void setup` to fix.",
                        connection.id, connection.connector_type, status.message
                    ));
                }
                Err(e) => {
                    broken.push(format!(
                        "Connection '{}' ({}) is broken: {e}. Run `void setup` to fix.",
                        connection.id, connection.connector_type
                    ));
                }
            }
            continue;
        }

        match connector_factory::build_connector(connection, &store_path) {
            Ok(conn) => match conn.health_check().await {
                Ok(status) if status.ok => connectors.push(conn),
                Ok(status) => {
                    let msg = format!(
                        "Connection '{}' ({}) is broken: {}. Run `void setup` to fix.",
                        connection.id, connection.connector_type, status.message
                    );
                    broken.push(msg);
                }
                Err(e) => {
                    let msg = format!(
                        "Connection '{}' ({}) is broken: {e}. Run `void setup` to fix.",
                        connection.id, connection.connector_type
                    );
                    broken.push(msg);
                }
            },
            Err(e) => {
                let msg = format!(
                    "Connection '{}' ({}) failed to build: {e}",
                    connection.id, connection.connector_type
                );
                broken.push(msg);
            }
        }
    }

    if !broken.is_empty() {
        for msg in &broken {
            eprintln!("[error] {msg}");
        }
        if !args.allow_broken {
            anyhow::bail!(
                "{} connector(s) failed health checks. Fix them with `void setup`, or pass --allow-broken to skip.",
                broken.len()
            );
        }
        eprintln!(
            "[warn] --allow-broken: skipping {} broken connector(s)",
            broken.len()
        );
    }

    if connectors.is_empty() {
        anyhow::bail!("No connectors to sync (check your config and --connectors filter).");
    }

    eprintln!(
        "Starting sync for {} connector(s)... (Ctrl+C to stop)",
        connectors.len()
    );

    let hooks_dir = hooks::hooks_dir();
    let loaded_hooks = hooks::load_hooks(&hooks_dir);
    let hook_runner = if loaded_hooks.is_empty() {
        None
    } else {
        let enabled = loaded_hooks.iter().filter(|h| h.enabled).count();
        eprintln!("Loaded {enabled} hook(s) from {}", hooks_dir.display());
        Some(Arc::new(
            HookRunner::new(loaded_hooks).with_db(Arc::clone(&db)),
        ))
    };

    let cancel = CancellationToken::new();

    if wa_rpc.has_handlers().await {
        let cancel_rpc = cancel.clone();
        tokio::spawn(async move {
            supervise_rpc(&wa_rpc, cancel_rpc).await;
        });
    }

    let ignore_rules: Vec<(String, Vec<String>)> = cfg
        .connections
        .iter()
        .map(|c| (c.id.clone(), c.ignore_conversations.clone()))
        .collect();

    apply_ignore_rules(&db, &ignore_rules);

    let db_bg = Arc::clone(&db);
    let cancel_bg = cancel.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(120));
        interval.tick().await;
        loop {
            tokio::select! {
                _ = cancel_bg.cancelled() => break,
                _ = interval.tick() => apply_ignore_rules(&db_bg, &ignore_rules),
            }
        }
    });

    let engine = SyncEngine::new(connectors, db, &store_path, hook_runner);
    engine.run_supervised(cancel).await
}

async fn supervise_rpc(server: &WhatsAppRpcServer, cancel: CancellationToken) {
    let mut failures = 0u32;
    loop {
        if cancel.is_cancelled() {
            break;
        }
        let started = Instant::now();
        match server.run(cancel.clone()).await {
            Ok(()) => break,
            Err(e) => {
                if cancel.is_cancelled() {
                    break;
                }
                if started.elapsed() > STABLE_THRESHOLD {
                    failures = 0;
                }
                failures += 1;
                if failures >= MAX_CONSECUTIVE_FAILURES {
                    error!("WhatsApp RPC server failed {failures} times, giving up: {e}");
                    break;
                }
                let delay = backoff_delay(failures);
                error!(
                    attempt = failures,
                    "WhatsApp RPC server crashed: {e} — restarting in {delay:?}"
                );
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(delay) => {},
                }
            }
        }
    }
}

fn backoff_delay(attempt: u32) -> Duration {
    Duration::from_secs(5u64.saturating_mul(2u64.saturating_pow(attempt.saturating_sub(1).min(6))))
        .min(MAX_BACKOFF)
}

fn apply_ignore_rules(db: &Database, rules: &[(String, Vec<String>)]) {
    for (connection_id, patterns) in rules {
        match db.sync_ignore_conversations(connection_id, patterns) {
            Ok((muted, unmuted)) if muted > 0 || unmuted > 0 => {
                if muted > 0 && unmuted > 0 {
                    void_core::status!(
                        "[{connection_id}] synced mute list: {muted} muted, {unmuted} unmuted"
                    );
                } else if muted > 0 {
                    void_core::status!(
                        "[{connection_id}] synced mute list: {muted} conversation(s) muted"
                    );
                } else {
                    void_core::status!(
                        "[{connection_id}] synced mute list: {unmuted} conversation(s) unmuted"
                    );
                }
                info!(
                    connection_id,
                    muted, unmuted, "synced conversation mute flags from config"
                );
            }
            Err(e) => {
                void_core::status!("[{connection_id}] failed to apply ignore patterns: {e}");
            }
            _ => {}
        }
    }
}
