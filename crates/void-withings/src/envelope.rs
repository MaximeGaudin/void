//! Every Withings response is `{"status": <code>, "body": {...}}` returned with
//! HTTP 200 — errors included. Unwrapping it in one place keeps every call site
//! honest about failures.

use serde::de::DeserializeOwned;

use crate::error::WithingsError;

/// Return the `body` of a successful response, or the API status as an error.
pub fn unwrap_body(raw: &str) -> Result<serde_json::Value, WithingsError> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| WithingsError::Decode(format!("{e}: {raw}")))?;

    let status = value
        .get("status")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| WithingsError::Decode(format!("response has no status field: {raw}")))?;

    if status != 0 {
        let message = value
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| status_hint(status))
            .to_string();
        return Err(WithingsError::Api { status, message });
    }

    Ok(value
        .get("body")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

/// Unwrap the envelope and decode the body into `T`.
pub fn decode_body<T: DeserializeOwned>(raw: &str) -> Result<T, WithingsError> {
    let body = unwrap_body(raw)?;
    serde_json::from_value(body).map_err(|e| WithingsError::Decode(format!("{e}: {raw}")))
}

/// Plain-English hint for the status codes users actually hit.
fn status_hint(status: i64) -> &'static str {
    match status {
        100..=102 => "the request was rejected as malformed or unauthorized",
        401 => "the access token is invalid or expired",
        601 => "too many requests — Withings is rate limiting this app",
        2554..=2556 => "Withings service is temporarily unavailable",
        _ => "unexpected Withings status",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_a_successful_body() {
        let body = unwrap_body(r#"{"status":0,"body":{"measuregrps":[]}}"#).unwrap();
        assert!(body.get("measuregrps").is_some());
    }

    #[test]
    fn surfaces_the_api_error_message() {
        let err = unwrap_body(r#"{"status":401,"body":{},"error":"invalid_token"}"#).unwrap_err();
        assert!(err.is_invalid_token());
        assert!(err.to_string().contains("invalid_token"));
    }

    #[test]
    fn falls_back_to_a_hint_when_error_is_absent() {
        let err = unwrap_body(r#"{"status":601,"body":{}}"#).unwrap_err();
        assert!(err.to_string().contains("rate limiting"));
    }

    #[test]
    fn rejects_a_response_without_status() {
        let err = unwrap_body(r#"{"body":{}}"#).unwrap_err();
        assert!(err.to_string().contains("no status field"));
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(unwrap_body("{not json").is_err());
    }

    #[test]
    fn decode_body_maps_into_a_type() {
        #[derive(serde::Deserialize)]
        struct Body {
            more: bool,
        }
        let decoded: Body = decode_body(r#"{"status":0,"body":{"more":true}}"#).unwrap();
        assert!(decoded.more);
    }
}
