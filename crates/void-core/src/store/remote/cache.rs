use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::expand_tilde;
use crate::error::ConfigError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMeta {
    pub config_fetched_at: u64,
    pub database_fetched_at: u64,
}

impl CacheMeta {
    pub fn load(cache_dir: &Path) -> Option<Self> {
        let path = cache_dir.join(".meta.json");
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn save(&self, cache_dir: &Path) -> Result<(), ConfigError> {
        std::fs::create_dir_all(cache_dir)?;
        let path = cache_dir.join(".meta.json");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// Mark the cached database snapshot stale so the next local-cache read
/// re-fetches it from the remote host instead of waiting out the TTL.
///
/// Called after every proxied command: the remote run may have written to
/// the store (archive, mute, send…), and serving the pre-write snapshot would
/// break read-after-write (`archive` then `inbox` still listing the items).
/// The snapshot file is kept so a failed refresh can still fall back to it.
pub fn invalidate_database_snapshot(cache_dir: &Path) -> Result<(), ConfigError> {
    let Some(mut meta) = CacheMeta::load(cache_dir) else {
        // No metadata yet: the next read already treats the snapshot as stale.
        return Ok(());
    };
    if meta.database_fetched_at == 0 {
        return Ok(());
    }
    meta.database_fetched_at = 0;
    meta.save(cache_dir)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

pub fn cache_is_fresh(fetched_at: u64, ttl_secs: u64) -> bool {
    if ttl_secs == 0 {
        return false;
    }
    now_secs().saturating_sub(fetched_at) < ttl_secs
}

pub fn default_cache_dir(host: &str) -> PathBuf {
    expand_tilde(&format!("~/.cache/void/remote/{host}"))
}
