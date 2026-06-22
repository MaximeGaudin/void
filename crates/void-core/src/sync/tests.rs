use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::connector::Connector;
use crate::db::Database;
use crate::models::{ConnectorType, HealthStatus, MessageContent};

use super::lock::FileLock;
use super::SyncEngine;

/// Test-only connector that records calls and offers configurable behaviour
/// for `start_sync` (immediate success, immediate error, or block-until-cancelled).
enum SyncBehavior {
    /// Return `Ok(())` immediately.
    SucceedImmediately,
    /// Return `Err(..)` immediately.
    FailImmediately,
    /// Block until the cancellation token fires, then return `Ok(())`.
    BlockUntilCancelled,
}

struct MockConnector {
    connection_id: String,
    behavior: SyncBehavior,
    start_sync_calls: Arc<AtomicUsize>,
    /// Records the connection_ids that actually ran start_sync, in completion order.
    ran: Arc<StdMutex<Vec<String>>>,
    /// Set to true once start_sync observed the cancellation token.
    observed_cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl MockConnector {
    fn new(id: &str, behavior: SyncBehavior) -> Self {
        Self {
            connection_id: id.to_string(),
            behavior,
            start_sync_calls: Arc::new(AtomicUsize::new(0)),
            ran: Arc::new(StdMutex::new(Vec::new())),
            observed_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

#[async_trait]
impl Connector for MockConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::from_static("slack")
    }

    fn connection_id(&self) -> &str {
        &self.connection_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start_sync(
        &self,
        _db: Arc<Database>,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        self.start_sync_calls.fetch_add(1, Ordering::SeqCst);
        let result = match self.behavior {
            SyncBehavior::SucceedImmediately => Ok(()),
            SyncBehavior::FailImmediately => Err(anyhow::anyhow!("boom")),
            SyncBehavior::BlockUntilCancelled => {
                cancel.cancelled().await;
                self.observed_cancel.store(true, Ordering::SeqCst);
                Ok(())
            }
        };
        self.ran.lock().unwrap().push(self.connection_id.clone());
        result
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        Ok(HealthStatus {
            connection_id: self.connection_id.clone(),
            connector_type: ConnectorType::from_static("slack"),
            ok: true,
            message: "ok".into(),
            last_sync: None,
            message_count: None,
        })
    }

    async fn send_message(&self, _to: &str, _content: MessageContent) -> anyhow::Result<String> {
        Ok("ok".into())
    }

    async fn reply(
        &self,
        _message_id: &str,
        _content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
        Ok("ok".into())
    }
}

fn temp_store_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("void-sync-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn run_calls_start_sync_on_every_connector() {
    let dir = temp_store_dir("multi");
    let db = Arc::new(Database::open_in_memory().unwrap());

    let a = Arc::new(MockConnector::new("a", SyncBehavior::SucceedImmediately));
    let b = Arc::new(MockConnector::new("b", SyncBehavior::SucceedImmediately));
    let a_calls = Arc::clone(&a.start_sync_calls);
    let b_calls = Arc::clone(&b.start_sync_calls);

    let engine = SyncEngine::new(
        vec![a as Arc<dyn Connector>, b as Arc<dyn Connector>],
        db,
        &dir,
        None,
    );

    let cancel = CancellationToken::new();
    // Both connectors return immediately, so run() completes on its own.
    engine.run(cancel).await.unwrap();

    assert_eq!(a_calls.load(Ordering::SeqCst), 1, "connector a synced once");
    assert_eq!(b_calls.load(Ordering::SeqCst), 1, "connector b synced once");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_continues_when_one_connector_errors() {
    let dir = temp_store_dir("broken");
    let db = Arc::new(Database::open_in_memory().unwrap());

    let bad = Arc::new(MockConnector::new("bad", SyncBehavior::FailImmediately));
    let good = Arc::new(MockConnector::new("good", SyncBehavior::SucceedImmediately));
    let bad_calls = Arc::clone(&bad.start_sync_calls);
    let good_calls = Arc::clone(&good.start_sync_calls);
    let good_ran = Arc::clone(&good.ran);

    let engine = SyncEngine::new(
        vec![bad as Arc<dyn Connector>, good as Arc<dyn Connector>],
        db,
        &dir,
        None,
    );

    // The erroring connector must not abort run(); the good one still runs to
    // completion and run() returns Ok overall (the --allow-broken contract).
    engine.run(CancellationToken::new()).await.unwrap();

    assert_eq!(bad_calls.load(Ordering::SeqCst), 1);
    assert_eq!(good_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        good_ran.lock().unwrap().as_slice(),
        ["good"],
        "good connector ran despite sibling error"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_cancellation_stops_loops_and_releases_lock() {
    let dir = temp_store_dir("cancel");
    let lock_path = dir.join("LOCK");
    let db = Arc::new(Database::open_in_memory().unwrap());

    let conn = Arc::new(MockConnector::new(
        "blocker",
        SyncBehavior::BlockUntilCancelled,
    ));
    let observed = Arc::clone(&conn.observed_cancel);

    let engine = SyncEngine::new(vec![conn as Arc<dyn Connector>], db, &dir, None);

    let cancel = CancellationToken::new();
    let cancel_for_task = cancel.clone();
    let handle = tokio::spawn(async move { engine.run(cancel_for_task).await });

    // Give the spawned sync loop a moment to acquire the lock and start blocking.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(lock_path.exists(), "lock should be held while syncing");

    cancel.cancel();
    // run() should now drain the blocked connector and finish.
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("run() did not finish after cancellation")
        .expect("join")
        .expect("run() returned Ok");

    assert!(
        observed.load(Ordering::SeqCst),
        "connector observed the cancellation token"
    );
    assert!(
        !lock_path.exists(),
        "lock file must be released after run() returns"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_with_no_connectors_is_ok_and_releases_lock() {
    let dir = temp_store_dir("empty");
    let lock_path = dir.join("LOCK");
    let db = Arc::new(Database::open_in_memory().unwrap());

    let engine = SyncEngine::new(vec![], db, &dir, None);
    engine.run(CancellationToken::new()).await.unwrap();

    assert!(!lock_path.exists(), "lock released on early return");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn file_lock_acquire_and_release() {
    let dir = std::env::temp_dir().join(format!("void-lock-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let lock_path = dir.join("LOCK");

    {
        let _lock = FileLock::acquire(&lock_path).unwrap();
        assert!(lock_path.exists());

        let result = FileLock::acquire(&lock_path);
        assert!(result.is_err());
    }

    assert!(!lock_path.exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn file_lock_stale_lock_auto_removed() {
    let dir = std::env::temp_dir().join(format!("void-lock-stale-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let lock_path = dir.join("LOCK");

    // Write a lock with a PID that definitely doesn't exist
    std::fs::write(&lock_path, "pid=999999999").unwrap();
    assert!(lock_path.exists());

    // Should auto-remove the stale lock and acquire successfully
    let _lock = FileLock::acquire(&lock_path).unwrap();
    assert!(lock_path.exists());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn file_lock_malformed_lock_file_overwritten() {
    let dir = std::env::temp_dir().join(format!("void-lock-garbage-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let lock_path = dir.join("LOCK");

    std::fs::write(&lock_path, "not-a-pid-line\n").unwrap();

    let _lock = FileLock::acquire(&lock_path).unwrap();
    let content = std::fs::read_to_string(&lock_path).unwrap();
    assert!(
        content.starts_with("pid="),
        "expected lock to be overwritten with pid=..., got {content:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn is_daemon_running_true_for_current_pid() {
    let dir = std::env::temp_dir().join(format!("void-daemon-detect-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let lock_path = dir.join("LOCK");
    std::fs::write(&lock_path, format!("pid={}", std::process::id())).unwrap();
    assert!(super::is_daemon_running(&dir));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn is_daemon_running_false_when_pid_is_stale() {
    let dir = std::env::temp_dir().join(format!("void-daemon-stale-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let lock_path = dir.join("LOCK");
    std::fs::write(&lock_path, "pid=999999999").unwrap();
    assert!(!super::is_daemon_running(&dir));
    std::fs::remove_dir_all(&dir).ok();
}
