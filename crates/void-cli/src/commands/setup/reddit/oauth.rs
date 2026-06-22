use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use void_reddit::api::{RedditClient, OAUTH_REDIRECT_URI};

pub(crate) const REDIRECT_URI: &str = OAUTH_REDIRECT_URI;
const OAUTH_PORT: u16 = 8765;
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);

const SUCCESS_HTML: &str = "<html><body><h1>Reddit authorization complete</h1><p>You can close this window and return to the terminal.</p></body></html>";
const ERROR_HTML: &str = "<html><body><h1>Reddit authorization failed</h1><p>Return to the terminal for next steps.</p></body></html>";

pub(crate) async fn obtain_refresh_token(
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<String> {
    let client = RedditClient::new(client_id, client_secret);
    let state = uuid::Uuid::new_v4().to_string();
    let auth_url = client.authorize_url(&state, REDIRECT_URI);

    let code = match try_loopback_callback(&auth_url, &state).await {
        Ok(code) => code,
        Err(loopback_err) => {
            eprintln!("Could not complete browser callback: {loopback_err}");
            eprintln!();
            eprintln!("Open this URL in your browser and approve access:");
            eprintln!("{auth_url}");
            eprintln!();
            let pasted = crate::commands::setup::prompt::prompt(
                "Paste the authorization code or full redirect URL here: ",
            );
            extract_code_from_input(&pasted, &state)?
        }
    };

    let tokens = client
        .exchange_authorization_code(&code, REDIRECT_URI)
        .await?;
    tokens
        .refresh_token
        .ok_or_else(|| anyhow::anyhow!("Reddit did not return a refresh token"))
}

async fn try_loopback_callback(auth_url: &str, state: &str) -> anyhow::Result<String> {
    let listener = match TcpListener::bind(format!("127.0.0.1:{OAUTH_PORT}")).await {
        Ok(listener) => listener,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            anyhow::bail!("port {OAUTH_PORT} is already in use");
        }
        Err(e) => return Err(e.into()),
    };

    eprintln!("Opening browser for Reddit authorization...");
    if let Err(e) = open::that(auth_url) {
        eprintln!("Could not open browser automatically: {e}");
        eprintln!("Open this URL manually: {auth_url}");
    }

    let result = tokio::time::timeout(CALLBACK_TIMEOUT, accept_oauth_callback(listener, state))
        .await
        .map_err(|_| anyhow::anyhow!("timed out waiting for Reddit authorization callback"))??;

    Ok(result)
}

async fn accept_oauth_callback(
    listener: TcpListener,
    expected_state: &str,
) -> anyhow::Result<String> {
    let (mut stream, _) = listener.accept().await?;
    let mut buffer = vec![0_u8; 8192];
    let n = stream.read(&mut buffer).await?;
    let request = String::from_utf8_lossy(&buffer[..n]);
    let request_line = request
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid OAuth callback request"))?;

    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("invalid OAuth callback request line"))?;

    match parse_callback_path(path, expected_state) {
        Ok(code) => {
            stream
                .write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{SUCCESS_HTML}", SUCCESS_HTML.len()).as_bytes())
                .await?;
            Ok(code)
        }
        Err(err) => {
            stream
                .write_all(format!("HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{ERROR_HTML}", ERROR_HTML.len()).as_bytes())
                .await?;
            Err(err)
        }
    }
}

pub(crate) fn parse_callback_path(path: &str, expected_state: &str) -> anyhow::Result<String> {
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or(path);

    let params = parse_query(query);
    if let Some(error) = params.get("error") {
        anyhow::bail!("Reddit authorization error: {error}");
    }

    let state = params
        .get("state")
        .ok_or_else(|| anyhow::anyhow!("OAuth callback missing state"))?;
    if state != expected_state {
        anyhow::bail!("OAuth state mismatch");
    }

    params
        .get("code")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("OAuth callback missing code"))
}

pub(crate) fn extract_code_from_input(input: &str, expected_state: &str) -> anyhow::Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("authorization code is empty");
    }

    if trimmed.contains("code=") {
        let path = trimmed.split_once('?').map(|(_, q)| q).unwrap_or(trimmed);
        return parse_callback_path(&format!("/?{path}"), expected_state);
    }

    Ok(trimmed.to_string())
}

fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((
                key.to_string(),
                urlencoding::decode(value).unwrap_or_default().into_owned(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_callback_path_extracts_code_and_validates_state() {
        let code = parse_callback_path("/?code=abc123&state=xyz", "xyz").unwrap();
        assert_eq!(code, "abc123");
    }

    #[test]
    fn parse_callback_path_rejects_state_mismatch() {
        let err = parse_callback_path("/?code=abc123&state=bad", "expected")
            .unwrap_err()
            .to_string();
        assert!(err.contains("state mismatch"));
    }

    #[test]
    fn extract_code_from_input_accepts_full_redirect_url() {
        let code =
            extract_code_from_input("http://localhost:8765/?code=manual-code&state=xyz", "xyz")
                .unwrap();
        assert_eq!(code, "manual-code");
    }

    #[test]
    fn extract_code_from_input_accepts_raw_code() {
        let code = extract_code_from_input("manual-code", "ignored").unwrap();
        assert_eq!(code, "manual-code");
    }
}
