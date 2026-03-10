use std::io::{BufRead, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

const GMAIL_SCOPES: &str = "https://www.googleapis.com/auth/gmail.readonly \
                            https://www.googleapis.com/auth/gmail.send \
                            https://www.googleapis.com/auth/gmail.modify";

/// Google OAuth2 token state, cached to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCache {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
}

/// Google OAuth2 client credentials (from downloaded JSON).
#[derive(Debug, Clone, Deserialize)]
pub struct ClientCredentials {
    pub installed: Option<InstalledCredentials>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstalledCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub auth_uri: String,
    pub token_uri: String,
}

impl TokenCache {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        debug!(path = %path.display(), "loading token cache");
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read token cache at {}", path.display()))?;
        serde_json::from_str(&content).context("failed to parse token cache")
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        debug!(path = %path.display(), "saving token cache");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

pub fn token_cache_path(store_path: &Path, account_id: &str) -> PathBuf {
    store_path.join(format!("{account_id}-token.json"))
}

pub fn load_client_credentials(credentials_file: &str) -> anyhow::Result<InstalledCredentials> {
    let path = void_core::config::expand_tilde(credentials_file);
    debug!(path = %path.display(), "loading client credentials");
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read credentials file at {}", path.display()))?;
    let creds: ClientCredentials =
        serde_json::from_str(&content).context("failed to parse credentials file")?;
    creds
        .installed
        .ok_or_else(|| anyhow::anyhow!("credentials file missing 'installed' key"))
}

pub fn scopes() -> &'static str {
    GMAIL_SCOPES
}

/// Run the full OAuth2 installed-app flow: open browser, listen on localhost
/// for the redirect, exchange code for tokens, and return the token cache.
pub async fn authorize_interactive(
    creds: &InstalledCredentials,
    custom_scopes: Option<&str>,
) -> anyhow::Result<TokenCache> {
    let listener = TcpListener::bind("127.0.0.1:0").context("failed to bind loopback port")?;
    let port = listener.local_addr()?.port();
    info!(port, "starting OAuth flow");
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let scopes = custom_scopes.unwrap_or(GMAIL_SCOPES);

    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent",
        creds.auth_uri,
        urlencoding::encode(&creds.client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(scopes),
    );

    eprintln!("\nOpening browser for Google authorization...");
    eprintln!("If it doesn't open, visit this URL manually:\n{auth_url}\n");
    open::that(&auth_url).ok();

    let code = wait_for_auth_code(&listener)?;
    debug!(code_len = code.len(), "authorization code received");

    let tokens = exchange_code_for_tokens(creds, &code, &redirect_uri).await?;
    info!("token exchange successful");
    Ok(tokens)
}

/// Block until the OAuth redirect hits our local server, extract the `code` param.
fn wait_for_auth_code(listener: &TcpListener) -> anyhow::Result<String> {
    let (mut stream, _) = listener.accept().context("failed to accept connection")?;
    let mut reader = std::io::BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("malformed HTTP request from redirect"))?;

    let code = url::Url::parse(&format!("http://localhost{path}"))
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "code")
                .map(|(_, v)| v.to_string())
        })
        .ok_or_else(|| {
            anyhow::anyhow!("no authorization code found in redirect (did you deny access?)")
        })?;
    debug!(code_len = code.len(), "authorization code extracted");

    let body = "<!DOCTYPE html><html><body><h2>Authorization successful!</h2>\
                <p>You can close this tab and return to your terminal.</p></body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).ok();

    Ok(code)
}

/// Exchange the authorization code for access + refresh tokens.
async fn exchange_code_for_tokens(
    creds: &InstalledCredentials,
    code: &str,
    redirect_uri: &str,
) -> anyhow::Result<TokenCache> {
    let http = reqwest::Client::new();
    let resp = http
        .post(&creds.token_uri)
        .form(&[
            ("code", code),
            ("client_id", &creds.client_id),
            ("client_secret", &creds.client_secret),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await?;

    let status = resp.status();
    let body: serde_json::Value = resp.json().await?;

    if !status.is_success() {
        anyhow::bail!(
            "token exchange failed ({}): {}",
            status,
            body.get("error_description")
                .or(body.get("error"))
                .map(|v| v.to_string())
                .unwrap_or_else(|| body.to_string())
        );
    }

    let access_token = body["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no access_token in token response"))?
        .to_string();
    let refresh_token = body["refresh_token"].as_str().map(|s| s.to_string());
    let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    Ok(TokenCache {
        access_token,
        refresh_token,
        expires_at: Some(expires_at),
    })
}

/// Refresh the access token using the refresh token.
pub async fn refresh_access_token(
    http: &reqwest::Client,
    creds: &InstalledCredentials,
    refresh_token: &str,
) -> anyhow::Result<TokenCache> {
    let resp = http
        .post(&creds.token_uri)
        .form(&[
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await?;

    let body: serde_json::Value = resp.json().await?;
    let access_token = body["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no access_token in refresh response"))?
        .to_string();

    let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    Ok(TokenCache {
        access_token,
        refresh_token: Some(refresh_token.to_string()),
        expires_at: Some(expires_at),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_cache_save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("void-gmail-auth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("token.json");

        let cache = TokenCache {
            access_token: "ya29.test".into(),
            refresh_token: Some("1//refresh".into()),
            expires_at: Some(1_700_000_000),
        };
        cache.save(&path).unwrap();

        let loaded = TokenCache::load(&path).unwrap();
        assert_eq!(loaded.access_token, "ya29.test");
        assert_eq!(loaded.refresh_token.as_deref(), Some("1//refresh"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
