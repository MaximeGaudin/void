//! Message row operations.

mod archive;
mod inbox;
mod lookup;
mod read;
mod saved;
mod upsert;

/// SQL clause that keeps only the most recent message per `context_id`,
/// letting NULL-context messages pass through unchanged.
const DEDUP_CONTEXT_CLAUSE: &str =
    " AND (context_id IS NULL OR id = (SELECT m2.id FROM messages m2 WHERE m2.context_id = messages.context_id ORDER BY m2.timestamp DESC, m2.id DESC LIMIT 1))";

/// Variant that picks the latest *unarchived* message per thread. Without this,
/// a thread whose newest message is archived becomes invisible in the inbox even
/// if older unarchived messages exist (the global-latest wins dedup, then gets
/// filtered out by `is_archived = 0`).
const DEDUP_CONTEXT_CLAUSE_UNARCHIVED: &str =
    " AND (context_id IS NULL OR id = (SELECT m2.id FROM messages m2 WHERE m2.context_id = messages.context_id AND m2.is_archived = 0 ORDER BY m2.timestamp DESC, m2.id DESC LIMIT 1))";

/// Same clause but using `m.` alias (for JOINed queries like FTS search).
pub(super) const DEDUP_CONTEXT_CLAUSE_ALIASED: &str =
    " AND (m.context_id IS NULL OR m.id = (SELECT m2.id FROM messages m2 WHERE m2.context_id = m.context_id ORDER BY m2.timestamp DESC, m2.id DESC LIMIT 1))";

pub use archive::{bulk_archive_before, mark_archived, update_metadata};
pub use inbox::{
    backfill_avatar_urls, enrich_with_context, messages_pending_file_download, reconcile_inbox,
    senders_missing_avatar,
};
pub use lookup::{
    find_by_external_id, find_by_slack_link, find_slack_conversation_by_external_id,
    last_in_conversation,
};
pub use read::{
    count_for_conversation, count_recent, get, latest_timestamp, list_for_conversation, list_recent,
};
pub use saved::{count_saved, list_saved, reconcile_saved};
pub use upsert::{message_exists, upsert_row};
