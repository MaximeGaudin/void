//! Circleback connector: meeting notes, action items and transcripts.
//!
//! Read-only. Each meeting becomes a conversation; its AI notes, action items
//! and transcript turns become messages sharing one context group, so the
//! inbox shows a single row per meeting while `void messages` and `void search`
//! reach every spoken turn.

pub mod api;
pub mod connector;

pub const CONNECTOR_ID: &str = "circleback";

/// How far back the first sync reaches when the user keeps the default.
pub const DEFAULT_BACKFILL_DAYS: u32 = 365;
