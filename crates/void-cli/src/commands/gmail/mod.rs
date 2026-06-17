//! Gmail CLI subcommands (search, thread, labels, drafts, attachments).

mod args;
mod handlers;

pub use args::*;

pub async fn run(args: &GmailArgs) -> anyhow::Result<()> {
    handlers::dispatch(args).await
}

/// Strip the void internal ID prefix from a Gmail message or thread ID.
///
/// Void stores IDs as `{connection_id}-{external_id}`, e.g.
/// `mgaudin@gladia.io-19c9ae5982d4b217`. Gmail IDs are pure hex and
/// never contain `@`, so the presence of `@` is an unambiguous indicator
/// that the void prefix must be stripped before passing the ID to the API.
pub(super) fn strip_void_id_prefix(id: &str) -> &str {
    if let Some(at_pos) = id.find('@') {
        if let Some(dash_offset) = id[at_pos..].find('-') {
            return &id[at_pos + dash_offset + 1..];
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use super::strip_void_id_prefix;

    #[test]
    fn strip_void_prefix_removes_connection_prefix() {
        assert_eq!(
            strip_void_id_prefix("mgaudin@gladia.io-19c9ae5982d4b217"),
            "19c9ae5982d4b217"
        );
    }

    #[test]
    fn strip_void_prefix_handles_personal_email() {
        assert_eq!(
            strip_void_id_prefix("me@maxime.ly-abcdef1234567890"),
            "abcdef1234567890"
        );
    }

    #[test]
    fn strip_void_prefix_passthrough_raw_gmail_id() {
        assert_eq!(strip_void_id_prefix("19c9ae5982d4b217"), "19c9ae5982d4b217");
    }

    #[test]
    fn strip_void_prefix_passthrough_when_no_dash_after_at() {
        // Malformed input with @ but no dash — return as-is rather than panic.
        assert_eq!(strip_void_id_prefix("weird@nodash"), "weird@nodash");
    }

    #[test]
    fn strip_void_prefix_empty_string() {
        assert_eq!(strip_void_id_prefix(""), "");
    }
}
