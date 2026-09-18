//! Withings OAuth2: browser flow, disk-cached tokens, rotation-safe refresh.
//!
//! Two Withings quirks shape this module:
//!
//! - the authorization code expires after about 30 seconds, so the exchange
//!   runs immediately after the redirect lands, never after a prompt;
//! - every refresh returns a *new* refresh token and invalidates the old one,
//!   so the token file — not the config — is the source of truth and is
//!   rewritten on each refresh.

use std::io::{BufRead, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::envelope::unwrap_body;
use crate::error::WithingsError;

pub const AUTHORIZE_URL: &str = "https://account.withings.com/oauth2_user/authorize2";
pub const DEFAULT_API_BASE: &str = "https://wbsapi.withings.net";

/// Withings only accepts redirect URIs registered with the app, so the port is
/// fixed rather than ephemeral. Register exactly this value in the dashboard.
pub const OAUTH_REDIRECT_URI: &str = "http://localhost:8765/callback";
pub const OAUTH_PORT: u16 = 8765;
pub const OAUTH_SCOPES: &str = "user.info,user.metrics,user.activity";

/// Refresh this long before the access token actually expires.
pub const REFRESH_MARGIN_SECS: i64 = 120;

/// Withings OAuth2 state, cached to disk.
#[derive(Clone, Serialize, Deserialize)]
pub struct TokenCache {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds. Withings access tokens live three hours.
    pub expires_at: i64,
    #[serde(default)]
    pub userid: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

// Manual `Debug` so the tokens are never dumped by `{:?}`.
impl std::fmt::Debug for TokenCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenCache")
            .field(
                "access_token",
                &void_core::config::redact_token(&self.access_token),
            )
            .field(
                "refresh_token",
                &void_core::config::redact_token(&self.refresh_token),
            )
            .field("expires_at", &self.expires_at)
            .field("userid", &self.userid)
            .field("scope", &self.scope)
            .finish()
    }
}

impl TokenCache {
    pub fn load(path: &Path) -> Result<Self, WithingsError> {
        debug!(path = %path.display(), "loading Withings token cache");
        let content = std::fs::read_to_string(path).map_err(|e| {
            WithingsError::Auth(format!(
                "no Withings token at {} ({e}) — run `void setup` to authorize",
                path.display()
            ))
        })?;
        serde_json::from_str(&content).map_err(|e| WithingsError::Decode(e.to_string()))
    }

    pub fn save(&self, path: &Path) -> Result<(), WithingsError> {
        debug!(path = %path.display(), "saving Withings token cache");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content =
            serde_json::to_string_pretty(self).map_err(|e| WithingsError::Decode(e.to_string()))?;
        // Holds OAuth access/refresh tokens — keep it owner-only.
        void_core::config::write_secure(path, content)?;
        Ok(())
    }

    /// True when the access token is expired or close enough that a call would
    /// race the expiry.
    pub fn needs_refresh(&self, now: i64) -> bool {
        self.expires_at - REFRESH_MARGIN_SECS <= now
    }
}

pub fn token_cache_path(store_path: &Path, connection_id: &str) -> PathBuf {
    store_path.join(format!("{connection_id}-withings-token.json"))
}

pub fn token_url(api_base: &str) -> String {
    format!("{}/v2/oauth2", api_base.trim_end_matches('/'))
}

pub fn authorize_url(client_id: &str, state: &str, redirect_uri: &str) -> String {
    format!(
        "{AUTHORIZE_URL}?response_type=code&client_id={}&state={}&scope={}&redirect_uri={}",
        urlencoding::encode(client_id),
        urlencoding::encode(state),
        urlencoding::encode(OAUTH_SCOPES),
        urlencoding::encode(redirect_uri),
    )
}

/// Run the full browser flow: open the consent page, catch the redirect on the
/// fixed loopback port, and exchange the code before it expires.
pub async fn authorize_interactive(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    api_base: &str,
) -> Result<TokenCache, WithingsError> {
    let listener = TcpListener::bind(("127.0.0.1", OAUTH_PORT)).map_err(|e| {
        WithingsError::Auth(format!(
            "could not listen on 127.0.0.1:{OAUTH_PORT} for the Withings redirect: {e}"
        ))
    })?;

    let state = format!(
        "{:x}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let auth_url = authorize_url(client_id, &state, OAUTH_REDIRECT_URI);

    eprintln!("\nOpening browser for Withings authorization...");
    eprintln!("If it doesn't open, visit this URL manually:\n{auth_url}\n");
    open::that(&auth_url).ok();

    let code = wait_for_auth_code(&listener, &state)?;
    info!("Withings authorization code received");

    // Withings invalidates the code after ~30s: exchange right now.
    exchange_code(http, client_id, client_secret, &code, api_base).await
}

/// How long to keep the loopback server up waiting for the redirect.
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);

/// Serve the loopback port until the redirect arrives, then extract `code`.
///
/// Browsers open more than one connection to a page: a speculative preconnect,
/// a `/favicon.ico` fetch, sometimes an HTTPS probe. Answering only the first
/// connection loses the real redirect, so every request is read and anything
/// that is not the callback gets a 404 while the wait continues.
fn wait_for_auth_code(
    listener: &TcpListener,
    expected_state: &str,
) -> Result<String, WithingsError> {
    listener.set_nonblocking(false).ok();
    let deadline = Instant::now() + CALLBACK_TIMEOUT;

    loop {
        if Instant::now() >= deadline {
            return Err(WithingsError::Auth(
                "timed out waiting for the Withings redirect — re-run `void setup` to try again"
                    .into(),
            ));
        }

        let (mut stream, _) = listener
            .accept()
            .map_err(|e| WithingsError::Auth(format!("failed to accept the redirect: {e}")))?;
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok();

        let mut reader = std::io::BufReader::new(&stream);
        let mut request_line = String::new();
        // A preconnect socket carries no bytes: drop it and keep waiting.
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            debug!("ignoring an empty connection on the callback port");
            continue;
        }

        let path = match request_line.split_whitespace().nth(1) {
            Some(path) => path.to_string(),
            None => {
                debug!(request = %request_line.trim(), "ignoring a malformed request");
                continue;
            }
        };

        if !is_callback_request(&path) {
            debug!(path = %path, "ignoring a request that is not the redirect");
            respond(&mut stream, "HTTP/1.1 404 Not Found", NOT_FOUND_HTML);
            continue;
        }

        let outcome = parse_callback_path(&path, expected_state);
        match &outcome {
            Ok(_) => respond(&mut stream, "HTTP/1.1 200 OK", SUCCESS_HTML),
            Err(_) => respond(&mut stream, "HTTP/1.1 400 Bad Request", FAILURE_HTML),
        }
        return outcome;
    }
}

const SUCCESS_HTML: &str = "<!DOCTYPE html><html><body><h2>Withings authorization complete</h2>\
    <p>You can close this tab and return to your terminal.</p></body></html>";
const FAILURE_HTML: &str = "<!DOCTYPE html><html><body><h2>Withings authorization failed</h2>\
    <p>Return to your terminal for next steps.</p></body></html>";
const NOT_FOUND_HTML: &str =
    "<!DOCTYPE html><html><body><p>Waiting for the Withings redirect.</p></body></html>";

/// Only the OAuth redirect carries `code` or `error`; everything else the
/// browser asks for on this port is noise.
fn is_callback_request(path: &str) -> bool {
    let query = match path.split_once('?') {
        Some((_, query)) => query,
        None => return false,
    };
    query
        .split('&')
        .filter_map(|pair| pair.split('=').next())
        .any(|key| key == "code" || key == "error")
}

fn respond(stream: &mut std::net::TcpStream, status_line: &str, body: &str) {
    let response = format!(
        "{status_line}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).ok();
}

/// Validate the redirect query and return the authorization code.
pub fn parse_callback_path(path: &str, expected_state: &str) -> Result<String, WithingsError> {
    let url = url::Url::parse(&format!("http://localhost{path}"))
        .map_err(|e| WithingsError::Auth(format!("unparseable redirect URL: {e}")))?;

    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.to_string()),
            "state" => state = Some(value.to_string()),
            "error" => error = Some(value.to_string()),
            _ => {}
        }
    }

    if let Some(error) = error {
        return Err(WithingsError::Auth(format!(
            "Withings authorization error: {error}"
        )));
    }
    match state {
        Some(state) if state == expected_state => {}
        Some(_) => return Err(WithingsError::Auth("OAuth state mismatch".into())),
        None => return Err(WithingsError::Auth("redirect is missing state".into())),
    }
    code.ok_or_else(|| {
        WithingsError::Auth("no authorization code in redirect (did you deny access?)".into())
    })
}

/// Exchange an authorization code for the first token pair.
pub async fn exchange_code(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    code: &str,
    api_base: &str,
) -> Result<TokenCache, WithingsError> {
    request_token(
        http,
        api_base,
        &[
            ("action", "requesttoken"),
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("redirect_uri", OAUTH_REDIRECT_URI),
        ],
    )
    .await
}

/// Trade the current refresh token for a fresh pair. The old refresh token is
/// dead as soon as this returns.
pub async fn refresh_tokens(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
    api_base: &str,
) -> Result<TokenCache, WithingsError> {
    request_token(
        http,
        api_base,
        &[
            ("action", "requesttoken"),
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
        ],
    )
    .await
}

async fn request_token(
    http: &reqwest::Client,
    api_base: &str,
    form: &[(&str, &str)],
) -> Result<TokenCache, WithingsError> {
    let raw = http
        .post(token_url(api_base))
        .form(form)
        .send()
        .await?
        .text()
        .await?;
    parse_token_response(&raw, chrono::Utc::now().timestamp())
}

/// Turn a token response into a cache entry with an absolute expiry.
pub fn parse_token_response(raw: &str, now: i64) -> Result<TokenCache, WithingsError> {
    let body = unwrap_body(raw)?;

    let access_token = body
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| WithingsError::Auth("token response has no access_token".into()))?
        .to_string();
    let refresh_token = body
        .get("refresh_token")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| WithingsError::Auth("token response has no refresh_token".into()))?
        .to_string();
    let expires_in = body
        .get("expires_in")
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(10_800);

    Ok(TokenCache {
        access_token,
        refresh_token,
        expires_at: now + expires_in,
        userid: body.get("userid").map(value_to_string),
        scope: body
            .get("scope")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    })
}

/// `userid` comes back as a number from some endpoints and a string from others.
fn value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN_JSON: &str = r#"{
        "status": 0,
        "body": {
            "userid": "12345",
            "access_token": "acc-1",
            "refresh_token": "ref-1",
            "scope": "user.metrics,user.activity",
            "expires_in": 10800,
            "token_type": "Bearer"
        }
    }"#;

    #[test]
    fn parses_a_token_response_into_an_absolute_expiry() {
        let tokens = parse_token_response(TOKEN_JSON, 1_000).unwrap();
        assert_eq!(tokens.access_token, "acc-1");
        assert_eq!(tokens.refresh_token, "ref-1");
        assert_eq!(tokens.expires_at, 11_800);
        assert_eq!(tokens.userid.as_deref(), Some("12345"));
    }

    #[test]
    fn accepts_a_numeric_userid() {
        let raw = r#"{"status":0,"body":{"userid":42,"access_token":"a","refresh_token":"r"}}"#;
        let tokens = parse_token_response(raw, 0).unwrap();
        assert_eq!(tokens.userid.as_deref(), Some("42"));
        // Missing expires_in falls back to the documented three hours.
        assert_eq!(tokens.expires_at, 10_800);
    }

    #[test]
    fn rejects_a_token_response_without_a_refresh_token() {
        let raw = r#"{"status":0,"body":{"access_token":"a","expires_in":10}}"#;
        let err = parse_token_response(raw, 0).unwrap_err();
        assert!(err.to_string().contains("no refresh_token"));
    }

    #[test]
    fn surfaces_an_api_error_from_the_token_endpoint() {
        let raw = r#"{"status":503,"body":{},"error":"Invalid Params"}"#;
        let err = parse_token_response(raw, 0).unwrap_err();
        assert!(err.to_string().contains("Invalid Params"));
    }

    #[test]
    fn needs_refresh_respects_the_margin() {
        let tokens = parse_token_response(TOKEN_JSON, 0).unwrap();
        assert!(!tokens.needs_refresh(10_000));
        assert!(tokens.needs_refresh(10_800 - REFRESH_MARGIN_SECS));
        assert!(tokens.needs_refresh(20_000));
    }

    #[test]
    fn token_cache_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = token_cache_path(dir.path(), "health");
        assert!(path.ends_with("health-withings-token.json"));

        let tokens = parse_token_response(TOKEN_JSON, 0).unwrap();
        tokens.save(&path).unwrap();
        let loaded = TokenCache::load(&path).unwrap();
        assert_eq!(loaded.access_token, "acc-1");
        assert_eq!(loaded.refresh_token, "ref-1");
    }

    #[test]
    fn loading_a_missing_token_points_at_setup() {
        let dir = tempfile::tempdir().unwrap();
        let err = TokenCache::load(&token_cache_path(dir.path(), "none")).unwrap_err();
        assert!(err.to_string().contains("void setup"));
    }

    #[test]
    fn debug_never_prints_the_tokens() {
        let tokens = parse_token_response(TOKEN_JSON, 0).unwrap();
        let debug = format!("{tokens:?}");
        assert!(!debug.contains("acc-1"));
        assert!(!debug.contains("ref-1"));
    }

    #[test]
    fn authorize_url_carries_scopes_and_redirect() {
        let url = authorize_url("cid", "st-1", OAUTH_REDIRECT_URI);
        assert!(url.starts_with(AUTHORIZE_URL));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("state=st-1"));
        assert!(url.contains("user.metrics"));
        assert!(url.contains("localhost%3A8765%2Fcallback"));
    }

    #[test]
    fn only_requests_carrying_code_or_error_count_as_the_redirect() {
        assert!(is_callback_request("/callback?code=abc&state=st"));
        assert!(is_callback_request(
            "/callback?error=access_denied&state=st"
        ));
        assert!(!is_callback_request("/favicon.ico"));
        assert!(!is_callback_request("/"));
        assert!(!is_callback_request("/callback"));
        // A query that merely mentions the word is not a code parameter.
        assert!(!is_callback_request("/callback?decoded=1"));
    }

    #[test]
    fn browser_noise_before_the_redirect_is_skipped() {
        use std::io::{Read, Write};
        use std::net::TcpStream;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let browser = std::thread::spawn(move || {
            // A speculative preconnect: opened, never written to.
            let preconnect = TcpStream::connect(("127.0.0.1", port)).unwrap();
            drop(preconnect);

            // The favicon fetch every browser makes.
            let mut favicon = TcpStream::connect(("127.0.0.1", port)).unwrap();
            favicon
                .write_all(b"GET /favicon.ico HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            let mut favicon_response = String::new();
            favicon.read_to_string(&mut favicon_response).ok();

            // The redirect that actually matters.
            let mut redirect = TcpStream::connect(("127.0.0.1", port)).unwrap();
            redirect
                .write_all(
                    b"GET /callback?code=the-code&state=st HTTP/1.1\r\nHost: localhost\r\n\r\n",
                )
                .unwrap();
            let mut redirect_response = String::new();
            redirect.read_to_string(&mut redirect_response).ok();

            (favicon_response, redirect_response)
        });

        let code = wait_for_auth_code(&listener, "st").unwrap();
        assert_eq!(code, "the-code");

        let (favicon_response, redirect_response) = browser.join().unwrap();
        assert!(favicon_response.contains("404 Not Found"));
        assert!(redirect_response.contains("200 OK"));
        assert!(redirect_response.contains("authorization complete"));
    }

    #[test]
    fn a_redirect_with_a_bad_state_answers_400_and_fails() {
        use std::io::{Read, Write};
        use std::net::TcpStream;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let browser = std::thread::spawn(move || {
            let mut redirect = TcpStream::connect(("127.0.0.1", port)).unwrap();
            redirect
                .write_all(b"GET /callback?code=c&state=wrong HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            let mut response = String::new();
            redirect.read_to_string(&mut response).ok();
            response
        });

        let err = wait_for_auth_code(&listener, "expected").unwrap_err();
        assert!(err.to_string().contains("state mismatch"));
        assert!(browser.join().unwrap().contains("400 Bad Request"));
    }

    #[test]
    fn callback_path_yields_the_code() {
        let code = parse_callback_path("/callback?code=abc&state=st", "st").unwrap();
        assert_eq!(code, "abc");
    }

    #[test]
    fn callback_path_rejects_a_state_mismatch() {
        let err = parse_callback_path("/callback?code=abc&state=other", "st").unwrap_err();
        assert!(err.to_string().contains("state mismatch"));
    }

    #[test]
    fn callback_path_surfaces_a_denial() {
        let err = parse_callback_path("/callback?error=access_denied&state=st", "st").unwrap_err();
        assert!(err.to_string().contains("access_denied"));
    }

    #[test]
    fn token_url_is_derived_from_the_api_base() {
        assert_eq!(
            token_url("https://wbsapi.withings.net/"),
            "https://wbsapi.withings.net/v2/oauth2"
        );
    }
}
