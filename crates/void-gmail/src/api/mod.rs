mod client;
mod message;
mod rate_limit;
mod retry;
mod types;

#[cfg(test)]
mod tests;

pub use client::{build_http_client, GmailApiClient};
pub use message::decode_attachment_data;
pub use retry::RetryPolicy;
pub use types::*;
