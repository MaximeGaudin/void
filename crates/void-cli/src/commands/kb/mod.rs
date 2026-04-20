//! Knowledge base CLI (`void kb`): add, search, sync folders, list documents.

mod args;
mod daemon;
mod handlers;
mod runtime;

use args::KbCommand;

pub use args::*;
pub use daemon::spawn_kb_sync_loop;

pub fn run(args: &KbArgs) -> anyhow::Result<()> {
    match &args.command {
        KbCommand::Add(a) => handlers::run_add(a),
        KbCommand::Search(a) => handlers::run_search(a),
        KbCommand::Sync(a) => handlers::run_sync(a),
        KbCommand::Unsync(a) => handlers::run_unsync(a),
        KbCommand::List(a) => handlers::run_list(a),
        KbCommand::Remove(a) => handlers::run_remove(a),
        KbCommand::Status => handlers::run_status(),
    }
}

#[cfg(test)]
mod tests {
    use super::runtime::{parse_metadata, validate_expiration};

    #[test]
    fn parse_metadata_valid() {
        let raw = vec!["author:Alice".into(), "tag:test".into()];
        let result = parse_metadata(&raw).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].key, "author");
        assert_eq!(result[0].value, "Alice");
    }

    #[test]
    fn parse_metadata_with_colons_in_value() {
        let raw = vec!["url:https://example.com".into()];
        let result = parse_metadata(&raw).unwrap();
        assert_eq!(result[0].key, "url");
        assert_eq!(result[0].value, "https://example.com");
    }

    #[test]
    fn parse_metadata_empty_key_rejected() {
        let raw = vec![":value".into()];
        assert!(parse_metadata(&raw).is_err());
    }

    #[test]
    fn parse_metadata_empty_value_rejected() {
        let raw = vec!["key:".into()];
        assert!(parse_metadata(&raw).is_err());
    }

    #[test]
    fn parse_metadata_no_colon_rejected() {
        let raw = vec!["novalue".into()];
        assert!(parse_metadata(&raw).is_err());
    }

    #[test]
    fn validate_expiration_valid() {
        let result = validate_expiration(Some("2025-12-31T23:59:59Z")).unwrap();
        assert_eq!(result, Some("2025-12-31T23:59:59Z".to_string()));
    }

    #[test]
    fn validate_expiration_invalid() {
        assert!(validate_expiration(Some("not-a-date")).is_err());
    }

    #[test]
    fn validate_expiration_none() {
        assert_eq!(validate_expiration(None).unwrap(), None);
    }

    #[test]
    fn validate_expiration_with_offset() {
        let result = validate_expiration(Some("2025-06-15T10:00:00+02:00")).unwrap();
        assert!(result.is_some());
    }
}
