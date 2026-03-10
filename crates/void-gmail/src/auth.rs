use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

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
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read token cache at {}", path.display()))?;
        serde_json::from_str(&content).context("failed to parse token cache")
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
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
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read credentials file at {}", path.display()))?;
    let creds: ClientCredentials =
        serde_json::from_str(&content).context("failed to parse credentials file")?;
    creds
        .installed
        .ok_or_else(|| anyhow::anyhow!("credentials file missing 'installed' key"))
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
