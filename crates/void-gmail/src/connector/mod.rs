mod api_methods;
mod compose;
mod connector_trait;
mod sync;

#[cfg(test)]
mod tests;

use crate::auth;

pub use compose::{
    compose_rfc2822, compose_rfc2822_with_attachment, encode_rfc2047, html_to_markdown,
    looks_like_html, parse_email_address, parse_email_name,
};

pub struct GmailConnector {
    config_id: String,
    credentials_file: Option<String>,
    store_path: std::path::PathBuf,
    my_email: std::sync::Mutex<Option<String>>,
}

impl GmailConnector {
    pub fn new(
        connection_id: &str,
        credentials_file: Option<&str>,
        store_path: &std::path::Path,
    ) -> Self {
        Self {
            config_id: connection_id.to_string(),
            credentials_file: credentials_file.map(|s| s.to_string()),
            store_path: store_path.to_path_buf(),
            my_email: std::sync::Mutex::new(None),
        }
    }

    fn token_path(&self) -> std::path::PathBuf {
        auth::token_cache_path(&self.store_path, &self.config_id)
    }

    fn display_connection_id(&self) -> String {
        self.my_email
            .lock()
            .expect("mutex")
            .clone()
            .unwrap_or_else(|| self.config_id.clone())
    }

    pub fn gmail_url(thread_id: &str) -> String {
        format!("https://mail.google.com/mail/u/0/#inbox/{thread_id}")
    }
}
