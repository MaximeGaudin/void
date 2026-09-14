//! HTTP transport: GET/POST helpers with automatic retry on transient failures.

use std::time::Duration;

use reqwest::Response;
use serde::de::DeserializeOwned;
use tracing::warn;

use super::types::SlackResponse;
use super::SlackApiClient;
use crate::error::SlackError;

pub(crate) const MAX_RETRIES: u32 = 5;
const DEFAULT_RETRY_SECS: u64 = 5;

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn is_retryable_json(err: &str) -> bool {
    matches!(err, "ratelimited" | "internal_error" | "fatal_error")
}

impl SlackApiClient {
    /// Extract `Retry-After` header (seconds) from a response, default to `DEFAULT_RETRY_SECS`.
    fn retry_after(resp: &Response) -> u64 {
        resp.headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_RETRY_SECS)
    }

    /// Send with automatic retry on 429, 5xx, and retryable Slack JSON errors.
    ///
    /// POST calls (including `chat.postMessage`) are retried too: a 5xx that
    /// already applied the write can produce a duplicate message. This is an
    /// accepted trade-off — the same policy as the Gmail connector.
    async fn request_with_retry<T, F, Fut>(&self, label: &str, mut send: F) -> Result<T, SlackError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<Response, reqwest::Error>>,
        T: DeserializeOwned,
    {
        for attempt in 0..=MAX_RETRIES {
            let resp = send().await?;
            let status = resp.status();
            let wait = Self::retry_after(&resp);

            if is_retryable_status(status) {
                if attempt < MAX_RETRIES {
                    warn!(
                        %status,
                        wait_secs = wait,
                        attempt,
                        label,
                        "slack: transient HTTP failure, backing off"
                    );
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    continue;
                }
                if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    return Err(SlackError::RateLimited(MAX_RETRIES, label.to_string()));
                }
                return Err(resp.error_for_status().expect_err("retryable 5xx").into());
            }

            let slack_resp: SlackResponse<T> = resp.json().await?;
            if let Some(ref err) = slack_resp.error {
                if is_retryable_json(err) && attempt < MAX_RETRIES {
                    warn!(
                        error = %err,
                        wait_secs = wait,
                        attempt,
                        label,
                        "slack: transient API error, backing off"
                    );
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    continue;
                }
            }
            return slack_resp.into_result();
        }
        unreachable!()
    }

    /// GET with retry on 429, 5xx, and retryable Slack JSON errors.
    pub(crate) async fn get_with_retry<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, String)],
        label: &str,
    ) -> Result<T, SlackError> {
        self.request_with_retry(label, || {
            self.http
                .get(url)
                .bearer_auth(&self.user_token)
                .query(params)
                .send()
        })
        .await
    }

    /// POST (JSON body) with the same retry policy as GET, including sends.
    pub(crate) async fn post_with_retry<T: DeserializeOwned>(
        &self,
        url: &str,
        body: &serde_json::Value,
        label: &str,
    ) -> Result<T, SlackError> {
        self.request_with_retry(label, || {
            self.http
                .post(url)
                .bearer_auth(&self.user_token)
                .json(body)
                .send()
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_status_covers_quota_and_backend() {
        assert!(is_retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(is_retryable_status(reqwest::StatusCode::BAD_GATEWAY));
        assert!(is_retryable_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE
        ));
        assert!(!is_retryable_status(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!is_retryable_status(reqwest::StatusCode::NOT_FOUND));
        assert!(!is_retryable_status(reqwest::StatusCode::OK));
    }

    #[test]
    fn retryable_json_covers_slack_transients() {
        assert!(is_retryable_json("ratelimited"));
        assert!(is_retryable_json("internal_error"));
        assert!(is_retryable_json("fatal_error"));
        assert!(!is_retryable_json("invalid_auth"));
        assert!(!is_retryable_json("channel_not_found"));
    }
}
