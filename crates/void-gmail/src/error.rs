use thiserror::Error;

#[derive(Debug, Error)]
pub enum GmailError {
    #[error("API error: {0}")]
    Api(String),
    #[error("Auth error: {0}")]
    Auth(String),
    /// Token lacks `gmail.settings.basic` (or similar) for send-as / signature reads.
    #[error(
        "insufficient OAuth scope for Gmail settings (need gmail.settings.basic); re-authenticate"
    )]
    InsufficientScope,
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

/// Whether a Gmail API error body indicates missing OAuth scopes (vs other 403s).
///
/// Matches Google's `ACCESS_TOKEN_SCOPE_INSUFFICIENT` reason and the common
/// "insufficient authentication scopes" message. Avoids broad phrases like
/// "insufficient permissions", which appear on unrelated 403s.
pub fn is_insufficient_scope_body(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("access_token_scope_insufficient")
        || lower.contains("insufficientpermissions")
        || lower.contains("insufficient authentication scopes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insufficient_scope_body_detects_google_reason() {
        assert!(is_insufficient_scope_body(
            r#"{"error":{"details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#
        ));
        assert!(is_insufficient_scope_body(
            "Request had insufficient authentication scopes."
        ));
        assert!(is_insufficient_scope_body(
            r#"{"error":{"errors":[{"reason":"insufficientPermissions"}]}}"#
        ));
        assert!(!is_insufficient_scope_body(
            "Admin has disabled this API for the domain."
        ));
        assert!(!is_insufficient_scope_body(
            "You do not have permission to access this resource."
        ));
        assert!(!is_insufficient_scope_body("insufficient permissions"));
        assert!(!is_insufficient_scope_body(""));
    }
}
