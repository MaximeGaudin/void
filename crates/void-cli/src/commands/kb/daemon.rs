use std::path::Path;

use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use void_kb::db::KbDatabase;
use void_kb::sync::{diff_and_apply, SyncEvent};

use super::runtime::build_embedder;

/// Spawn the KB sync background loop. Called from the `void sync` daemon.
/// Periodically scans all registered KB folders and indexes changes.
pub async fn spawn_kb_sync_loop(store_path: &Path, cancel: CancellationToken) {
    let kb_path = store_path.join("kb.db");
    let store = store_path.to_path_buf();

    tokio::spawn(async move {
        info!("KB sync loop started");

        loop {
            run_kb_sync_cycle(&kb_path, &store);

            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("KB sync loop shutting down");
                    return;
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {}
            }
        }
    });
}

fn run_kb_sync_cycle(kb_path: &Path, _store_path: &Path) {
    let db = match KbDatabase::open(kb_path) {
        Ok(db) => db,
        Err(e) => {
            // No KB database yet — nothing to do
            tracing::debug!(error = %e, "KB database not available, skipping cycle");
            return;
        }
    };

    let folders = match db.list_sync_folders() {
        Ok(f) => f,
        Err(e) => {
            error!(error = %e, "failed to list KB sync folders");
            return;
        }
    };

    if folders.is_empty() {
        return;
    }

    let embedder = match build_embedder() {
        Ok(e) => e,
        Err(e) => {
            error!(error = %e, "failed to initialize embedder for KB sync");
            return;
        }
    };

    if let Ok(n) = db.cleanup_expired() {
        if n > 0 {
            info!(count = n, "cleaned up expired KB documents");
        }
    }

    for folder in &folders {
        if !Path::new(&folder.folder_path).is_dir() {
            tracing::warn!(path = %folder.folder_path, "KB sync folder no longer exists, skipping");
            continue;
        }

        let progress = |event: SyncEvent| match &event {
            SyncEvent::DiffComputed {
                to_add,
                to_update,
                to_delete,
                ..
            } => {
                if *to_add + *to_update + *to_delete > 0 {
                    info!(
                        folder = %folder.folder_path,
                        to_add, to_update, to_delete,
                        "KB sync found changes"
                    );
                }
            }
            SyncEvent::FileDone { path, ok, .. } => {
                if !ok {
                    tracing::warn!(path, "KB sync failed to index file");
                }
            }
            _ => {}
        };

        match diff_and_apply(&db, embedder.as_ref(), &folder.folder_path, &progress) {
            Ok(report) => {
                let total = report.added + report.updated + report.deleted;
                if total > 0 || report.errors > 0 {
                    info!(
                        folder = %folder.folder_path,
                        added = report.added,
                        updated = report.updated,
                        deleted = report.deleted,
                        errors = report.errors,
                        "KB sync cycle complete"
                    );
                }
            }
            Err(e) => {
                error!(
                    folder = %folder.folder_path,
                    error = %e,
                    "KB sync cycle failed"
                );
            }
        }

        let now = chrono::Utc::now().to_rfc3339();
        db.update_sync_folder_scan_time(&folder.folder_path, &now)
            .ok();
    }
}
