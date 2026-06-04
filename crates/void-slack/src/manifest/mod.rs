//! Auto-repair Slack Event Subscriptions via the App Manifest API.
//!
//! Slack disables event subscriptions when the app fails to acknowledge
//! events for too long (i.e. the Socket Mode client was not running).
//! This module detects that situation at sync startup and patches the
//! manifest back to the expected state.

use serde::Deserialize;
use tracing::{debug, info};

const SLACK_API_BASE: &str = "https://slack.com/api";

const EXPECTED_USER_EVENTS: &[&str] = &[
    "message.channels",
    "message.groups",
    "message.im",
    "message.mpim",
];

// ---------------------------------------------------------------------------
// Low-level Slack API calls
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ConfigTokenRotation {
    pub token: String,
    pub refresh_token: String,
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
///
/// **Note:** This only checks the manifest JSON. Slack can report events in
/// the manifest while the "Enable Events" UI toggle is OFF. Use this for
/// diagnostics, not as the sole gate for skipping updates.
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
    let settings = manifest.as_object_mut().and_then(|m| {
        m.entry("settings")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
    });

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

/// Ensure event subscriptions are enabled by always writing the manifest.
///
/// Slack can keep `user_events` listed in the exported manifest while the
/// "Enable Events" toggle is actually OFF. Checking the manifest alone is
/// not reliable, so we always patch and push an update. The operation is
/// idempotent — if events are already enabled it's a no-op on Slack's side.
///
/// This is designed to be **non-fatal**: callers should log errors and
/// continue with sync even if this fails.
///
/// On success, `config_refresh_token` is updated when Slack rotates the token.
/// On `invalid_refresh_token`, callers should reload from config and retry —
/// the token is not cleared here because another process may have rotated it.
pub async fn ensure_event_subscriptions(
    config_refresh_token: &mut Option<String>,
    app_id: &str,
    connection_id: &str,
) -> anyhow::Result<()> {
    ensure_event_subscriptions_with_url(config_refresh_token, app_id, connection_id, SLACK_API_BASE)
        .await
}

pub(crate) async fn ensure_event_subscriptions_with_url(
    config_refresh_token: &mut Option<String>,
    app_id: &str,
    connection_id: &str,
    base_url: &str,
) -> anyhow::Result<()> {
    let Some(refresh_token) = config_refresh_token.clone() else {
        return Ok(());
    };

    let http = reqwest::Client::new();

    debug!(connection_id, "rotating Slack config token");
    let rotated = match rotate_config_token_with_url(&http, base_url, &refresh_token).await {
        Ok(r) => r,
        Err(e) => {
            if e.to_string().contains("invalid_refresh_token") {
                anyhow::bail!(
                    "Slack config token is invalid (invalid_refresh_token). \
                     Reload config_refresh_token from config.toml or run `void setup` \
                     if the token was revoked."
                );
            }
            return Err(e);
        }
    };

    *config_refresh_token = Some(rotated.refresh_token.clone());
    debug!(connection_id, "saved rotated refresh token");

    debug!(connection_id, app_id, "exporting Slack app manifest");
    let mut manifest = export_manifest_with_url(&http, base_url, &rotated.token, app_id).await?;

    let events_present = has_expected_events(&manifest);
    patch_event_subscriptions(&mut manifest);
    update_manifest_with_url(&http, base_url, &rotated.token, app_id, &manifest).await?;

    if events_present {
        void_core::status!(
            "[slack:{connection_id}] Event subscriptions enforced (were present in manifest)"
        );
        info!(
            connection_id,
            "event subscriptions enforced via manifest update"
        );
    } else {
        void_core::status!(
            "[slack:{connection_id}] Event subscriptions restored (were missing from manifest)"
        );
        info!(
            connection_id,
            "event subscriptions restored via manifest update"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests;
