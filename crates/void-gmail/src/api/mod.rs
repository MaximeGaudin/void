mod client;
mod message;
mod types;

#[cfg(test)]
mod tests;

pub use client::{build_http_client, GmailApiClient};
pub use message::decode_attachment_data;
pub use types::*;
