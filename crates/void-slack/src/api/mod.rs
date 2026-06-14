//! Low-level Slack Web API client.
//!
//! The endpoint wrappers are split by Slack API namespace across submodules
//! (`conversations`, `chat`), with shared transport/retry logic in `transport`
//! and response DTOs in `types`.

mod chat;
mod conversations;
mod transport;
mod types;

#[cfg(test)]
mod tests;

pub use types::*;

use crate::error::SlackError;

const DEFAULT_BASE_URL: &str = "https://slack.com/api";

/// Low-level Slack Web API client using user token.
pub struct SlackApiClient {
    http: reqwest::Client,
    user_token: String,
    base_url: String,
}

impl SlackApiClient {
    fn build_http_client() -> Result<reqwest::Client, SlackError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| SlackError::Other(format!("failed to build HTTP client: {e}")))
    }

    pub fn new(user_token: &str) -> Result<Self, SlackError> {
        Ok(Self {
            http: Self::build_http_client()?,
            user_token: user_token.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
        })
    }

    #[cfg(test)]
    pub fn with_base_url(user_token: &str, base_url: &str) -> Result<Self, SlackError> {
        Ok(Self {
            http: Self::build_http_client()?,
            user_token: user_token.to_string(),
            base_url: base_url.to_string(),
        })
    }
}
