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
    /// The stored `historyId` is too old: Gmail purges history after a limited
    /// window, so incremental sync must fall back to a full INBOX refresh.
    #[error("gmail history expired; full inbox refresh required")]
    HistoryExpired,
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

/// The `reason` values carried by a Google API error body, lowercased.
///
/// Google uses two shapes for the same field: the classic
/// `error.errors[].reason` and the newer `error.details[].reason`. Both are read
/// here so every caller matches against one list instead of learning the shapes
/// again. A body that is not JSON yields nothing, and callers fall back to
/// matching the reason token in the raw text.
fn google_error_reasons(body: &str) -> Vec<String> {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(error) = parsed.get("error") else {
        return Vec::new();
    };
    ["errors", "details"]
        .iter()
        .filter_map(|key| error.get(key)?.as_array())
        .flatten()
        .filter_map(|item| item.get("reason")?.as_str())
        .map(|reason| reason.to_ascii_lowercase())
        .collect()
}

/// Google reasons that mean "the request was fine, come back later".
///
/// `rateLimitExceeded` and `userRateLimitExceeded` are the per-user quota,
/// `quotaExceeded` the project quota, `backendError` a transient Gmail fault.
const RETRYABLE_REASONS: [&str; 4] = [
    "ratelimitexceeded",
    "userratelimitexceeded",
    "quotaexceeded",
    "backenderror",
];

/// Whether a Gmail API error body indicates missing OAuth scopes (vs other 403s).
///
/// Matches Google's `ACCESS_TOKEN_SCOPE_INSUFFICIENT` reason and the common
/// "insufficient authentication scopes" message. Avoids broad phrases like
/// "insufficient permissions", which appear on unrelated 403s.
pub fn is_insufficient_scope_body(body: &str) -> bool {
    let reasons = google_error_reasons(body);
    if reasons
        .iter()
        .any(|r| r == "access_token_scope_insufficient" || r == "insufficientpermissions")
    {
        return true;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("access_token_scope_insufficient")
        || lower.contains("insufficientpermissions")
        || lower.contains("insufficient authentication scopes")
}

/// Whether a Gmail API error body says the quota was hit, so the call is worth
/// retrying after a backoff.
///
/// Needed because Gmail does not answer 429 for the per-user quota: it answers
/// **403 with `reason: rateLimitExceeded`**. The status alone therefore cannot
/// decide, and every other 403 (missing scope, denied permission, domain policy)
/// is a real answer that must keep failing fast. Retrying those burns quota and
/// hides an auth problem.
pub fn is_retryable_quota_body(body: &str) -> bool {
    // A scope error wins: it is a 403 that will never clear on its own, and some
    // bodies mention both a permission reason and quota-looking prose.
    if is_insufficient_scope_body(body) {
        return false;
    }
    if google_error_reasons(body)
        .iter()
        .any(|r| RETRYABLE_REASONS.contains(&r.as_str()))
    {
        return true;
    }
    // Non-JSON or an unparsed shape: match the reason token itself, never loose
    // prose like "quota", which shows up in unrelated messages.
    let lower = body.to_ascii_lowercase();
    RETRYABLE_REASONS.iter().any(|r| lower.contains(r))
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

    #[test]
    fn google_error_reasons_reads_both_shapes() {
        assert_eq!(
            google_error_reasons(r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#),
            vec!["ratelimitexceeded"]
        );
        assert_eq!(
            google_error_reasons(r#"{"error":{"details":[{"reason":"backendError"}]}}"#),
            vec!["backenderror"]
        );
        assert!(google_error_reasons("not json at all").is_empty());
        assert!(google_error_reasons(r#"{"something":"else"}"#).is_empty());
    }

    #[test]
    fn retryable_quota_body_detects_the_403_gmail_actually_sends() {
        // The real payload, trimmed: this is what the per-user quota looks like.
        assert!(is_retryable_quota_body(
            r#"{"error":{"code":403,"message":"User-rate limit exceeded.  Retry after 2026-09-11T20:00:00.000Z","errors":[{"message":"User-rate limit exceeded.","domain":"usageLimits","reason":"rateLimitExceeded"}],"status":"PERMISSION_DENIED"}}"#
        ));
        assert!(is_retryable_quota_body(
            r#"{"error":{"errors":[{"reason":"userRateLimitExceeded"}]}}"#
        ));
        assert!(is_retryable_quota_body(
            r#"{"error":{"errors":[{"reason":"quotaExceeded"}]}}"#
        ));
        assert!(is_retryable_quota_body(
            r#"{"error":{"details":[{"reason":"backendError"}]}}"#
        ));
    }

    #[test]
    fn retryable_quota_body_rejects_real_answers() {
        // A scope error must keep failing fast: retrying hides the auth problem.
        assert!(!is_retryable_quota_body(
            r#"{"error":{"message":"Request had insufficient authentication scopes.","status":"PERMISSION_DENIED","details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#
        ));
        assert!(!is_retryable_quota_body(
            r#"{"error":{"errors":[{"reason":"insufficientPermissions"}]}}"#
        ));
        assert!(!is_retryable_quota_body(
            r#"{"error":{"errors":[{"reason":"forbidden"}]}}"#
        ));
        assert!(!is_retryable_quota_body(
            "Admin has disabled this API for the domain."
        ));
        // Prose about quotas is not a reason. Only the reason token counts.
        assert!(!is_retryable_quota_body(
            "You have exceeded your daily quota of patience."
        ));
        assert!(!is_retryable_quota_body(""));
    }
}
