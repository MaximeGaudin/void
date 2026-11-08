//! Cross-process lock around the token refresh.
//!
//! Withings invalidates the old refresh token the moment a new one is issued,
//! so two processes refreshing at the same time leave one of them holding a
//! dead grant. The sync daemon and a CLI command routinely run side by side, so
//! the refresh is serialized on a lock file next to the token.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tracing::debug;

use crate::error::WithingsError;

/// How long to wait for another process to finish its refresh.
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);
const LOCK_POLL: Duration = Duration::from_millis(200);

pub fn lock_path(token_path: &Path) -> PathBuf {
    let mut name = token_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "withings-token".to_string());
    name.push_str(".lock");
    token_path.with_file_name(name)
}

/// Held for the duration of a refresh; released on drop.
pub struct RefreshLock {
    // Unix unlocks through this handle on drop; Windows only needs to hold it
    // open, since closing it releases the exclusive share.
    #[cfg_attr(not(unix), allow(dead_code))]
    file: File,
}

impl RefreshLock {
    /// Take the lock, polling rather than blocking so the async runtime keeps
    /// working. Returns `None` if another process held it for the whole budget.
    pub async fn acquire(token_path: &Path) -> Result<Option<Self>, WithingsError> {
        let path = lock_path(token_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            if let Some(file) = try_acquire(&path)? {
                debug!(path = %path.display(), "took the Withings refresh lock");
                return Ok(Some(Self { file }));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            tokio::time::sleep(LOCK_POLL).await;
        }
    }
}

/// `Ok(None)` means another process holds the lock; an error is a real
/// filesystem failure.
#[cfg(unix)]
fn try_acquire(path: &Path) -> Result<Option<File>, WithingsError> {
    use std::os::unix::io::AsRawFd;

    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    // SAFETY: `file` owns the descriptor for the whole call.
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 };
    Ok(locked.then_some(file))
}

/// Windows has no `flock`, but an open with no sharing is exclusive by itself,
/// and the kernel drops the handle even if the process dies — so, unlike a
/// create-and-delete lock file, a crash cannot leave the lock stuck.
#[cfg(windows)]
fn try_acquire(path: &Path) -> Result<Option<File>, WithingsError> {
    use std::os::windows::fs::OpenOptionsExt;

    match OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
    {
        Ok(file) => Ok(Some(file)),
        Err(e) if is_contention(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// `ERROR_SHARING_VIOLATION` (32) and `ERROR_LOCK_VIOLATION` (33) mean someone
/// else is refreshing, not that anything is broken.
#[cfg(windows)]
fn is_contention(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32) | Some(33))
        || error.kind() == std::io::ErrorKind::PermissionDenied
}

#[cfg(unix)]
impl Drop for RefreshLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        // SAFETY: same descriptor, still owned by `self.file`.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_path_sits_next_to_the_token() {
        let path = lock_path(Path::new("/store/health-withings-token.json"));
        assert_eq!(
            path,
            PathBuf::from("/store/health-withings-token.json.lock")
        );
    }

    #[tokio::test]
    async fn the_lock_is_released_when_it_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("t-withings-token.json");

        let first = RefreshLock::acquire(&token).await.unwrap();
        assert!(first.is_some());
        drop(first);

        let second = RefreshLock::acquire(&token).await.unwrap();
        assert!(second.is_some());
    }

    #[tokio::test]
    async fn a_second_holder_is_turned_away_while_the_first_holds_it() {
        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("t-withings-token.json");
        let path = lock_path(&token);

        let _held = RefreshLock::acquire(&token).await.unwrap().unwrap();

        // Same process, a fresh open: flock is per open file description and
        // the Windows share mode is per handle, so this genuinely contends.
        assert!(try_acquire(&path).unwrap().is_none());
    }
}
