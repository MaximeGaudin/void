//! Cross-process token bucket stored in `sync_state`.
//!
//! Two processes opening the same store (CLI + sync daemon) serialize on
//! `BEGIN IMMEDIATE` and share one bucket. The function never sleeps: it
//! returns how long the caller should wait so the write lock is not held
//! across a sleep.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::error::DbError;

#[derive(Debug, Serialize, Deserialize)]
struct Bucket {
    tokens: f64,
    /// Unix seconds, fractional.
    updated: f64,
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub(super) fn take_token(
    conn: &mut Connection,
    connection_id: &str,
    key: &str,
    capacity: f64,
    refill_per_sec: f64,
) -> Result<Duration, DbError> {
    if capacity <= 0.0 || refill_per_sec <= 0.0 {
        return Ok(Duration::ZERO);
    }

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let raw: Option<String> = tx
        .query_row(
            "SELECT value FROM sync_state WHERE connection_id = ?1 AND key = ?2",
            params![connection_id, key],
            |row| row.get(0),
        )
        .optional()?;

    let now = now_secs();
    let mut bucket = raw
        .as_deref()
        .and_then(|s| serde_json::from_str::<Bucket>(s).ok())
        .unwrap_or(Bucket {
            tokens: capacity,
            updated: now,
        });

    let elapsed = (now - bucket.updated).max(0.0);
    bucket.tokens = (bucket.tokens + elapsed * refill_per_sec).min(capacity);
    bucket.updated = now;

    let wait = if bucket.tokens >= 1.0 {
        bucket.tokens -= 1.0;
        Duration::ZERO
    } else {
        let needed = 1.0 - bucket.tokens;
        Duration::from_secs_f64((needed / refill_per_sec).clamp(0.0, 3600.0))
    };

    let value = serde_json::to_string(&bucket).expect("bucket serializes");
    tx.execute(
        "INSERT INTO sync_state (connection_id, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(connection_id, key) DO UPDATE SET value = excluded.value",
        params![connection_id, key, value],
    )?;
    tx.commit()?;
    Ok(wait)
}
