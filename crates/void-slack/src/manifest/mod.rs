//! Auto-repair Slack Event Subscriptions via the App Manifest API.
//!
//! Slack disables event subscriptions when the app fails to acknowledge
//! events for too long (i.e. the Socket Mode client was not running).
//! This module detects that situation at sync startup and patches the
//! manifest back to the expected state.

use std::path::Path;

use serde::Deserialize;
use tracing::{debug, info, warn};

const SLACK_API_BASE: &str = "https://slack.com/api";

const EXPECTED_USER_EVENTS: &[&str] = &[
    "message.channels",
    "message.groups",
    "message.im",
    "message.mpim",
];

// ---------------------------------------------------------------------------
// Token persistence
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, Deserialize)]
struct TokenFile {
    refresh_token: String,
}

pub fn load_refresh_token(path: &Path) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(path)?;
    let file: TokenFile = serde_json::from_str(&data)?;
    Ok(Some(file.refresh_token))
}

pub fn save_refresh_token(path: &Path, token: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = TokenFile {
        refresh_token: token.to_string(),
    };
    let data = serde_json::to_string_pretty(&file)?;
    std::fs::write(path, data)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Low-level Slack API calls
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ConfigTokenRotation {
    pub token: String,
    pub refresh_token: String,
}

pub(crate) async fn rotate_config_token(
    http: &reqwest::Client,
    refresh_token: &str,
) -> anyhow::Result<ConfigTokenRotation> {
    rotate_config_token_with_url(http, SLACK_API_BASE, refresh_token).await
}

pub(crate) async fn rotate_config_token_with_url(
    http: &reqwest::Client,
    base_url: &str,
    refresh_token: &str,
) -> anyhow::Result<ConfigTokenRotation> {
    #[derive(Deserialize)]
    struct Resp {
        ok: bool,
        error: Option<String>,
        token: Option<String>,
        refresh_token: Option<String>,
    }

    let resp: Resp = http
        .post(format!("{base_url}/tooling.tokens.rotate"))
        .form(&[("refresh_token", refresh_token)])
        .send()
        .await?
        .json()
        .await?;

    if !resp.ok {
        anyhow::bail!(
            "tooling.tokens.rotate failed: {}",
            resp.error.as_deref().unwrap_or("unknown error")
        );
    }

    Ok(ConfigTokenRotation {
        token: resp
            .token
            .ok_or_else(|| anyhow::anyhow!("tooling.tokens.rotate: missing token in response"))?,
        refresh_token: resp.refresh_token.ok_or_else(|| {
            anyhow::anyhow!("tooling.tokens.rotate: missing refresh_token in response")
        })?,
    })
}

pub(crate) async fn export_manifest(
    http: &reqwest::Client,
    config_token: &str,
    app_id: &str,
) -> anyhow::Result<serde_json::Value> {
    export_manifest_with_url(http, SLACK_API_BASE, config_token, app_id).await
}

pub(crate) async fn export_manifest_with_url(
    http: &reqwest::Client,
    base_url: &str,
    config_token: &str,
    app_id: &str,
) -> anyhow::Result<serde_json::Value> {
    let resp: serde_json::Value = http
        .post(format!("{base_url}/apps.manifest.export"))
        .form(&[("token", config_token), ("app_id", app_id)])
        .send()
        .await?
        .json()
        .await?;

    if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = resp
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        anyhow::bail!("apps.manifest.export failed: {err}");
    }

    resp.get("manifest")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("apps.manifest.export: missing manifest in response"))
}

pub(crate) async fn update_manifest(
    http: &reqwest::Client,
    config_token: &str,
    app_id: &str,
    manifest: &serde_json::Value,
) -> anyhow::Result<()> {
    update_manifest_with_url(http, SLACK_API_BASE, config_token, app_id, manifest).await
}

pub(crate) async fn update_manifest_with_url(
    http: &reqwest::Client,
    base_url: &str,
    config_token: &str,
    app_id: &str,
    manifest: &serde_json::Value,
) -> anyhow::Result<()> {
    let manifest_str = serde_json::to_string(manifest)?;

    let resp: serde_json::Value = http
        .post(format!("{base_url}/apps.manifest.update"))
        .form(&[
            ("token", config_token),
            ("app_id", app_id),
            ("manifest", &manifest_str),
        ])
        .send()
        .await?
        .json()
        .await?;

    if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = resp
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        anyhow::bail!("apps.manifest.update failed: {err}");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Manifest patching
// ---------------------------------------------------------------------------

/// Returns `true` if the manifest already has the expected user events.
pub(crate) fn has_expected_events(manifest: &serde_json::Value) -> bool {
    let events = manifest
        .pointer("/settings/event_subscriptions/user_events")
        .and_then(|v| v.as_array());

    let Some(events) = events else {
        return false;
    };

    EXPECTED_USER_EVENTS
        .iter()
        .all(|expected| events.iter().any(|e| e.as_str() == Some(expected)))
}

/// Patch the manifest in-place to include the expected user events.
pub(crate) fn patch_event_subscriptions(manifest: &mut serde_json::Value) {
    let settings = manifest
        .as_object_mut()
        .and_then(|m| m.entry("settings").or_insert_with(|| serde_json::json!({})).as_object_mut());

    let Some(settings) = settings else { return };

    let event_subs = settings
        .entry("event_subscriptions")
        .or_insert_with(|| serde_json::json!({}));

    if let Some(obj) = event_subs.as_object_mut() {
        let events: Vec<serde_json::Value> = EXPECTED_USER_EVENTS
            .iter()
            .map(|e| serde_json::Value::String(e.to_string()))
            .collect();
        obj.insert("user_events".to_string(), serde_json::Value::Array(events));
    }
}

// ---------------------------------------------------------------------------
// High-level orchestrator
// ---------------------------------------------------------------------------

/// Check and repair event subscriptions if needed.
///
/// Rotates the config token, exports the current manifest, and patches
/// event subscriptions back to the expected state if Slack disabled them.
///
/// This is designed to be **non-fatal**: callers should log errors and
/// continue with sync even if this fails.
pub async fn ensure_event_subscriptions(
    token_path: &Path,
    app_id: &str,
    connection_id: &str,
) -> anyhow::Result<()> {
    let refresh_token = load_refresh_token(token_path)?
        .ok_or_else(|| anyhow::anyhow!("no config refresh token found; run `void setup` to configure auto-repair"))?;

    let http = reqwest::Client::new();

    debug!(connection_id, "rotating Slack config token");
    let rotated = rotate_config_token(&http, &refresh_token).await?;

    save_refresh_token(token_path, &rotated.refresh_token)?;
    debug!(connection_id, "saved rotated refresh token");

    debug!(connection_id, app_id, "exporting Slack app manifest");
    let mut manifest = export_manifest(&http, &rotated.token, app_id).await?;

    if has_expected_events(&manifest) {
        eprintln!("[slack:{connection_id}] Event subscriptions OK");
        info!(connection_id, "event subscriptions are correctly configured");
        return Ok(());
    }

    warn!(connection_id, "event subscriptions are missing or incomplete — restoring");
    eprintln!("[slack:{connection_id}] Event subscriptions disabled by Slack — restoring...");

    patch_event_subscriptions(&mut manifest);
    update_manifest(&http, &rotated.token, app_id, &manifest).await?;

    eprintln!("[slack:{connection_id}] Event subscriptions restored successfully");
    info!(connection_id, "event subscriptions restored via manifest update");

    Ok(())
}

#[cfg(test)]
mod tests;
