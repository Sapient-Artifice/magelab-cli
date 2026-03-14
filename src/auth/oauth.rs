use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};
use std::net::TcpListener;

use super::credentials::Credentials;

/// Supabase auth base URL
const AUTH_URL: &str = "https://auth.magelab.ai/auth/v1";

/// Run the full OAuth login flow:
/// 1. Generate PKCE verifier + challenge
/// 2. Start loopback HTTP server
/// 3. Open browser to Supabase OAuth
/// 4. Handle callback with auth code
/// 5. Exchange code for tokens
/// 6. Save credentials
pub async fn login(_gateway_url: &str) -> Result<Credentials> {
    // Generate PKCE
    let verifier = generate_code_verifier();
    let challenge = generate_code_challenge(&verifier);
    let state = generate_state();

    // Start loopback server on random port
    let listener = TcpListener::bind("127.0.0.1:0").context("Failed to start loopback server")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{}/callback", port);

    // Build authorize URL
    let authorize_url = format!(
        "{}/authorize?provider=google&code_challenge={}&code_challenge_method=S256&redirect_uri={}&response_type=code&state={}&flow_type=pkce",
        AUTH_URL,
        urlencoded(&challenge),
        urlencoded(&redirect_uri),
        urlencoded(&state),
    );

    println!("Opening browser for login...");
    println!("If browser doesn't open, visit:\n{}\n", authorize_url);
    open::that(&authorize_url).ok();

    // Wait for callback
    println!("Waiting for authentication...");
    let (code, returned_state) = wait_for_callback(listener)?;

    // Validate state
    if returned_state != state {
        anyhow::bail!("OAuth state mismatch — possible CSRF attack");
    }

    // Exchange code for tokens
    println!("Exchanging authorization code...");
    let creds = exchange_code(&code, &verifier, &redirect_uri).await?;

    // Save
    creds.save()?;
    let path = Credentials::path()?;
    println!("Logged in! Credentials saved to {}", path.display());

    Ok(creds)
}

/// Generate a cryptographically random PKCE code verifier (43-128 chars, URL-safe)
fn generate_code_verifier() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Generate S256 code challenge from verifier
fn generate_code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

/// Generate a random state parameter for CSRF protection
fn generate_state() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

/// URL-encode a string
fn urlencoded(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Wait for the OAuth callback on the loopback server
fn wait_for_callback(listener: TcpListener) -> Result<(String, String)> {
    // Set a timeout so we don't wait forever
    listener.set_nonblocking(false)?;

    let (mut stream, _) = listener.accept().context("No callback received")?;

    let mut reader = std::io::BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    // Parse: GET /callback?code=...&state=... HTTP/1.1
    let path = request_line
        .split_whitespace()
        .nth(1)
        .context("Invalid HTTP request")?;

    let url = url::Url::parse(&format!("http://localhost{}", path))
        .context("Failed to parse callback URL")?;

    let code = url
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .context("No authorization code in callback")?;

    let state = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .unwrap_or_default();

    // Send success response
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h2>Login successful!</h2><p>You can close this tab and return to the terminal.</p></body></html>";
    stream.write_all(response.as_bytes()).ok();

    Ok((code, state))
}

/// Exchange authorization code for access + refresh tokens
async fn exchange_code(code: &str, code_verifier: &str, redirect_uri: &str) -> Result<Credentials> {
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/token?grant_type=authorization_code", AUTH_URL))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "code": code,
            "code_verifier": code_verifier,
            "redirect_uri": redirect_uri,
        }))
        .send()
        .await
        .context("Failed to exchange auth code")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Token exchange failed ({}): {}", status, body);
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse token response")?;

    let access_token = body["access_token"]
        .as_str()
        .context("No access_token in response")?
        .to_string();

    let refresh_token = body["refresh_token"].as_str().map(String::from);

    let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    let user_id = body["user"]["id"].as_str().map(String::from);

    Ok(Credentials {
        access_token: Some(access_token),
        refresh_token,
        expires_at: Some(expires_at),
        user_id,
    })
}

/// Refresh an expired access token using the refresh token
#[allow(dead_code)]
pub async fn refresh_token(refresh_token: &str) -> Result<Credentials> {
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/token?grant_type=refresh_token", AUTH_URL))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .context("Failed to refresh token")?;

    if !resp.status().is_success() {
        anyhow::bail!("Token refresh failed — please run `magelab login` again");
    }

    let body: serde_json::Value = resp.json().await?;

    let access_token = body["access_token"]
        .as_str()
        .context("No access_token in refresh response")?
        .to_string();

    let new_refresh = body["refresh_token"].as_str().map(String::from);
    let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;
    let user_id = body["user"]["id"].as_str().map(String::from);

    let creds = Credentials {
        access_token: Some(access_token),
        refresh_token: new_refresh,
        expires_at: Some(expires_at),
        user_id,
    };
    creds.save()?;

    Ok(creds)
}

/// Ensure we have a valid JWT — refresh if expired, login if no credentials
#[allow(dead_code)]
pub async fn ensure_valid_jwt(gateway_url: &str) -> Result<String> {
    let creds = Credentials::load()?;

    if creds.is_token_valid() {
        return Ok(creds.access_token.unwrap());
    }

    // Try refresh
    if let Some(ref rt) = creds.refresh_token {
        match refresh_token(rt).await {
            Ok(new_creds) => return Ok(new_creds.access_token.unwrap()),
            Err(_) => {
                // Refresh failed — need full login
            }
        }
    }

    // Need full login
    let new_creds = login(gateway_url).await?;
    Ok(new_creds.access_token.unwrap())
}
