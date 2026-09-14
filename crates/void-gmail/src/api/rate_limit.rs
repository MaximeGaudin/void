//! Process-shared Gmail quota bucket (CLI + sync daemon, same store).

use std::time::Duration;

use tracing::{debug, warn};
use void_core::db::Database;

/// Gmail per-user quota: ~250 quota units / minute, where reads cost ~5 units
/// and list calls ~2. ~90 requests/min is a safe operational ceiling that leaves
/// headroom for spikes. See https://developers.google.com/gmail/api/reference/quota
const CAPACITY: f64 = 90.0;
const REFILL_PER_SEC: f64 = 90.0 / 60.0;
const KEY: &str = "gmail_rate_limit";
const MAX_WAIT: Duration = Duration::from_secs(60);

/// Token bucket persisted in SQLite `sync_state`.
pub struct StoreRateLimiter {
    db: Database,
    connection_id: String,
}

impl StoreRateLimiter {
    pub fn new(db: Database, connection_id: String) -> Self {
        Self { db, connection_id }
    }

    /// Wait until a token is available. Errors (and a missing store) fail open.
    pub async fn acquire(&self) {
        loop {
            match self
                .db
                .take_rate_token(&self.connection_id, KEY, CAPACITY, REFILL_PER_SEC)
            {
                Ok(wait) if wait.is_zero() => return,
                Ok(wait) => {
                    let sleep = wait.min(MAX_WAIT);
                    debug!(
                        wait_ms = sleep.as_millis() as u64,
                        connection_id = %self.connection_id,
                        "gmail: waiting for rate-limit token"
                    );
                    tokio::time::sleep(sleep).await;
                }
                Err(e) => {
                    warn!(error = %e, "gmail: rate limiter failed open");
                    return;
                }
            }
        }
    }
}
