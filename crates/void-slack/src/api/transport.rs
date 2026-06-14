//! HTTP transport: GET/POST helpers with automatic rate-limit retry.

use reqwest::Response;
use serde::de::DeserializeOwned;
use tracing::warn;

use super::types::SlackResponse;
use super::SlackApiClient;
use crate::error::SlackError;

pub(crate) const MAX_RETRIES: u32 = 5;
const DEFAULT_RETRY_SECS: u64 = 5;

impl SlackApiClient {
    /// Extract `Retry-After` header (seconds) from a response, default to `DEFAULT_RETRY_SECS`.
    fn retry_after(resp: &Response) -> u64 {
        resp.headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_RETRY_SECS)
    }

    /// GET with automatic retry on 429 / `ratelimited`.
    pub(crate) async fn get_with_retry<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, String)],
        label: &str,
    ) -> Result<T, SlackError> {
        for attempt in 0..=MAX_RETRIES {
            let resp = self
                .http
                .get(url)
                .bearer_auth(&self.user_token)
                .query(params)
                .send()
                .await?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let wait = Self::retry_after(&resp);
                if attempt < MAX_RETRIES {
                    warn!(
                        wait_secs = wait,
                        attempt, label, "rate limited, backing off"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                return Err(SlackError::RateLimited(MAX_RETRIES, label.to_string()));
            }

            let slack_resp: SlackResponse<T> = resp.json().await?;
            if let Some(ref err) = slack_resp.error {
                if err == "ratelimited" && attempt < MAX_RETRIES {
                    warn!(attempt, label, "rate limited (json), backing off");
                    tokio::time::sleep(std::time::Duration::from_secs(DEFAULT_RETRY_SECS)).await;
                    continue;
                }
            }
            return slack_resp.into_result();
        }
        unreachable!()
    }

    /// POST (JSON body) with automatic retry on 429 / `ratelimited`.
    pub(crate) async fn post_with_retry<T: DeserializeOwned>(
        &self,
        url: &str,
        body: &serde_json::Value,
        label: &str,
    ) -> Result<T, SlackError> {
        for attempt in 0..=MAX_RETRIES {
            let resp = self
                .http
                .post(url)
                .bearer_auth(&self.user_token)
                .json(body)
                .send()
                .await?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let wait = Self::retry_after(&resp);
                if attempt < MAX_RETRIES {
                    warn!(
                        wait_secs = wait,
                        attempt, label, "rate limited, backing off"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                return Err(SlackError::RateLimited(MAX_RETRIES, label.to_string()));
            }

            let slack_resp: SlackResponse<T> = resp.json().await?;
            if let Some(ref err) = slack_resp.error {
                if err == "ratelimited" && attempt < MAX_RETRIES {
                    warn!(attempt, label, "rate limited (json), backing off");
                    tokio::time::sleep(std::time::Duration::from_secs(DEFAULT_RETRY_SECS)).await;
                    continue;
                }
            }
            return slack_resp.into_result();
        }
        unreachable!()
    }
}
