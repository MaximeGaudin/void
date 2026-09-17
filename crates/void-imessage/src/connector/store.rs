//! Read side of the Messages store.
//!
//! The store is a plain SQLite file at `~/Library/Messages/chat.db`, protected
//! by Full Disk Access. Void opens it read-only and never writes to it: the
//! Messages app owns that file and is usually holding it open.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use super::typedstream;

/// Apple's epoch (2001-01-01) offset from the Unix epoch, in seconds.
const APPLE_EPOCH_OFFSET: i64 = 978_307_200;

/// Default location of the Messages store for the current user.
///
/// Uses `dirs::home_dir()` rather than reading `HOME` directly, to match the
/// rest of the workspace (`void_core::config::paths`) and to keep working when
/// the variable is absent from the daemon's environment.
pub fn default_db_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join("Library").join("Messages").join("chat.db"))
}

/// Why the store could not be read.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("the Messages database does not exist at {0}")]
    Missing(PathBuf),
    /// macOS denied the read. This is the common case and it needs an
    /// actionable message: the file is there, the bytes are refused.
    #[error(
        "macOS denied access to the Messages database at {0}.\n\
         Grant Full Disk Access to the binary running void:\n\
         System Settings > Privacy & Security > Full Disk Access.\n\
         Note that the file's size is readable without the grant, so an\n\
         existence check is not enough to tell the two cases apart."
    )]
    AccessDenied(PathBuf),
    #[error("could not open the Messages database at {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
}

/// Convert an Apple nanosecond timestamp to Unix seconds.
///
/// Messages has used two encodings: seconds since 2001 in older databases, and
/// nanoseconds since 2001 since macOS 10.13. Both appear in the same column of
/// a long-lived store, so the magnitude decides. Applying the nanosecond rule
/// to a seconds value yields a date in 1970; applying the seconds rule to a
/// nanosecond value yields a year far in the future.
pub fn apple_time_to_unix(raw: i64) -> i64 {
    if raw == 0 {
        return 0;
    }
    // Anything past this magnitude cannot be a seconds-since-2001 value: it
    // would sit tens of thousands of years in the future.
    const NANOSECOND_THRESHOLD: i64 = 1_000_000_000_000;
    if raw.abs() > NANOSECOND_THRESHOLD {
        raw / 1_000_000_000 + APPLE_EPOCH_OFFSET
    } else {
        raw + APPLE_EPOCH_OFFSET
    }
}

/// Resolve the body of a message row, preferring `text` and falling back to
/// decoding `attributedBody`.
///
/// Recent macOS leaves `text` NULL and stores the body only in
/// `attributedBody`, so a connector that reads `text` alone silently indexes
/// empty messages.
pub fn resolve_body(text: Option<&str>, attributed_body: Option<&[u8]>) -> Option<String> {
    if let Some(text) = text {
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    match attributed_body {
        Some(blob) => match typedstream::decode_text(blob) {
            Ok(decoded) => decoded,
            Err(err) => {
                tracing::warn!("could not decode attributedBody: {err}");
                text.map(str::to_string)
            }
        },
        None => text.map(str::to_string),
    }
}

/// Open the store read-only, mapping a TCC denial to an actionable error.
pub fn open_read_only(path: &Path) -> Result<rusqlite::Connection, StoreError> {
    if !path.exists() {
        return Err(StoreError::Missing(path.to_path_buf()));
    }

    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
        | rusqlite::OpenFlags::SQLITE_OPEN_URI
        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;

    let conn = rusqlite::Connection::open_with_flags(path, flags).map_err(|source| {
        if is_access_denied(&source) {
            StoreError::AccessDenied(path.to_path_buf())
        } else {
            StoreError::Open {
                path: path.to_path_buf(),
                source,
            }
        }
    })?;

    // Opening can succeed lazily; the first real read is what actually trips
    // the sandbox. Force it here so health_check reports the truth.
    match conn.query_row("SELECT 1 FROM message LIMIT 1", [], |_| Ok(())) {
        Ok(()) => Ok(conn),
        // An empty table is a perfectly healthy store.
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(conn),
        Err(err) if is_access_denied(&err) => Err(StoreError::AccessDenied(path.to_path_buf())),
        Err(source) => Err(StoreError::Open {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Whether a SQLite error is macOS refusing the read rather than a real fault.
///
/// TCC surfaces as `SQLITE_AUTH` ("authorization denied") or as `SQLITE_CANTOPEN`
/// on a file that demonstrably exists, which is why the caller checks existence
/// first.
fn is_access_denied(err: &rusqlite::Error) -> bool {
    use rusqlite::ffi::ErrorCode;
    match err {
        rusqlite::Error::SqliteFailure(ffi, msg) => {
            matches!(
                ffi.code,
                ErrorCode::AuthorizationForStatementDenied | ErrorCode::CannotOpen
            ) || msg
                .as_deref()
                .is_some_and(|m| m.contains("authorization denied"))
        }
        _ => false,
    }
}

/// One message as Void sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImessageRow {
    pub rowid: i64,
    pub guid: String,
    pub body: Option<String>,
    pub handle: Option<String>,
    pub chat_guid: Option<String>,
    pub service: Option<String>,
    pub is_from_me: bool,
    pub date_unix: i64,
    pub date_delivered_unix: i64,
    pub date_read_unix: i64,
}

/// Fetch messages with `ROWID` greater than `after_rowid`, oldest first.
pub fn fetch_since(
    conn: &rusqlite::Connection,
    after_rowid: i64,
    limit: usize,
) -> Result<Vec<ImessageRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT m.ROWID, m.guid, m.text, m.attributedBody, h.id, c.guid,
                    m.service, m.is_from_me, m.date, m.date_delivered, m.date_read
             FROM message m
             LEFT JOIN handle h ON m.handle_id = h.ROWID
             LEFT JOIN chat_message_join cmj ON cmj.message_id = m.ROWID
             LEFT JOIN chat c ON c.ROWID = cmj.chat_id
             WHERE m.ROWID > ?1
             ORDER BY m.ROWID ASC
             LIMIT ?2",
        )
        .context("preparing the message query")?;

    let rows = stmt
        .query_map(rusqlite::params![after_rowid, limit as i64], |row| {
            let text: Option<String> = row.get(2)?;
            let blob: Option<Vec<u8>> = row.get(3)?;
            Ok(ImessageRow {
                rowid: row.get(0)?,
                guid: row.get(1)?,
                body: resolve_body(text.as_deref(), blob.as_deref()),
                handle: row.get(4)?,
                chat_guid: row.get(5)?,
                service: row.get(6)?,
                is_from_me: row.get::<_, i64>(7)? != 0,
                date_unix: apple_time_to_unix(row.get(8)?),
                date_delivered_unix: apple_time_to_unix(row.get::<_, Option<i64>>(9)?.unwrap_or(0)),
                date_read_unix: apple_time_to_unix(row.get::<_, Option<i64>>(10)?.unwrap_or(0)),
            })
        })
        .context("running the message query")?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| anyhow!("reading message rows: {err}"))
}
