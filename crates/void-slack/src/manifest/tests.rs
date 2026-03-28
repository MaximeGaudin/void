use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

// ── Token persistence ─────────────────────────────────────────────────────

#[test]
fn save_and_load_refresh_token_roundtrip() {
    let dir = std::env::temp_dir().join(format!("void-test-manifest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("token.json");

    save_refresh_token(&path, "xoxe-test-token").unwrap();
    let loaded = load_refresh_token(&path).unwrap();
    assert_eq!(loaded, Some("xoxe-test-token".to_string()));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_refresh_token_missing_file() {
    let path = std::env::temp_dir().join("nonexistent-void-test-token.json");
    let loaded = load_refresh_token(&path).unwrap();
    assert_eq!(loaded, None);
}

// ── Manifest patching ─────────────────────────────────────────────────────

#[test]
fn has_expected_events_detects_complete() {
    let manifest = serde_json::json!({
        "settings": {
            "event_subscriptions": {
                "user_events": [
                    "message.channels",
                    "message.groups",
                    "message.im",
                    "message.mpim"
                ]
            }
        }
    });
    assert!(has_expected_events(&manifest));
}

#[test]
fn has_expected_events_detects_missing() {
    let manifest = serde_json::json!({
        "settings": {
            "event_subscriptions": {
                "user_events": ["message.channels"]
            }
        }
    });
    assert!(!has_expected_events(&manifest));
}

#[test]
fn has_expected_events_detects_empty() {
    let manifest = serde_json::json!({
        "settings": {}
    });
    assert!(!has_expected_events(&manifest));
}

#[test]
fn has_expected_events_detects_null_manifest() {
    let manifest = serde_json::json!({});
    assert!(!has_expected_events(&manifest));
}

#[test]
fn patch_event_subscriptions_adds_events() {
    let mut manifest = serde_json::json!({
        "settings": {}
    });
    patch_event_subscriptions(&mut manifest);
    assert!(has_expected_events(&manifest));
}

#[test]
fn patch_event_subscriptions_replaces_incomplete() {
    let mut manifest = serde_json::json!({
        "settings": {
            "event_subscriptions": {
                "user_events": ["message.channels"]
            }
        }
    });
    patch_event_subscriptions(&mut manifest);
    assert!(has_expected_events(&manifest));
}

#[test]
fn patch_preserves_other_manifest_fields() {
    let mut manifest = serde_json::json!({
        "display_information": { "name": "Void" },
        "settings": {
            "socket_mode_enabled": true,
            "event_subscriptions": {
                "user_events": []
            }
        }
    });
    patch_event_subscriptions(&mut manifest);
    assert!(has_expected_events(&manifest));
    assert_eq!(
        manifest["display_information"]["name"].as_str(),
        Some("Void")
    );
    assert_eq!(
        manifest["settings"]["socket_mode_enabled"].as_bool(),
        Some(true)
    );
}

// ── API call tests with wiremock ──────────────────────────────────────────

#[tokio::test]
async fn rotate_config_token_parses_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/tooling.tokens.rotate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "token": "xoxe.xoxp-new-access-token",
            "refresh_token": "xoxe-new-refresh-token"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let http = reqwest::Client::new();
    let result = rotate_config_token_with_url(&http, &mock_server.uri(), "xoxe-old-refresh").await;

    let rotation = result.unwrap();
    assert_eq!(rotation.token, "xoxe.xoxp-new-access-token");
    assert_eq!(rotation.refresh_token, "xoxe-new-refresh-token");
}

#[tokio::test]
async fn rotate_config_token_handles_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/tooling.tokens.rotate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": false,
            "error": "invalid_refresh_token"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let http = reqwest::Client::new();
    let result = rotate_config_token_with_url(&http, &mock_server.uri(), "xoxe-bad-token").await;

    let err = result.unwrap_err();
    assert!(err.to_string().contains("invalid_refresh_token"));
}

#[tokio::test]
async fn export_manifest_parses_response() {
    let mock_server = MockServer::start().await;

    let manifest = serde_json::json!({
        "settings": {
            "event_subscriptions": {
                "user_events": ["message.channels"]
            }
        }
    });

    Mock::given(method("POST"))
        .and(path("/apps.manifest.export"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "manifest": manifest
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let http = reqwest::Client::new();
    let result =
        export_manifest_with_url(&http, &mock_server.uri(), "xoxe.xoxp-token", "A0123456").await;

    assert_eq!(result.unwrap(), manifest);
}

#[tokio::test]
async fn update_manifest_sends_request() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/apps.manifest.update"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let http = reqwest::Client::new();
    let manifest = serde_json::json!({ "settings": {} });
    let result = update_manifest_with_url(
        &http,
        &mock_server.uri(),
        "xoxe.xoxp-token",
        "A0123456",
        &manifest,
    )
    .await;

    result.unwrap();
}
