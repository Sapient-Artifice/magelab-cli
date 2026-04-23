use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};
use std::net::TcpListener;
use std::str::FromStr;
use workos::sso::ClientId;
use workos::user_management::{
    CodeChallenge, ConnectionSelector, GetAuthorizationUrl, GetAuthorizationUrlParams,
    OauthProvider, Provider,
};
use workos::{ApiKey, WorkOs};

use super::credentials::Credentials;

/// Prompt for user input from stdin
fn prompt(label: &str) -> String {
    eprint!("{} ", label);
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap_or_default();
    input.trim().to_string()
}

/// WorkOS Client ID (shared with web frontend)
const DEFAULT_CLIENT_ID: &str = "client_01KKJ9GJKHDMW63A3RCV56KVZ6";

/// Fixed loopback port (must match WorkOS redirect URI config)
const LOOPBACK_PORT: u16 = 19872;

/// URL for new account signup
const SIGNUP_URL: &str = "https://magelab.ai/signup";

/// Login method selection
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LoginMethod {
    /// Google OAuth via browser
    Google,
    /// Magic auth code via email (default for CLI — no browser needed)
    #[default]
    MagicAuth,
}

impl FromStr for LoginMethod {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "google" => Ok(Self::Google),
            "magic" | "email" | "magic-auth" => Ok(Self::MagicAuth),
            _ => anyhow::bail!("Unknown login method '{}'. Use 'google' or 'magic'.", s),
        }
    }
}

/// Get WorkOS Client ID from env or default
fn client_id() -> String {
    std::env::var("WORKOS_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string())
}

/// Get auth base URL. Defaults to {gateway_url}/v1/auth.
/// Override with MAGELAB_AUTH_URL for testing against the web app
/// (e.g. MAGELAB_AUTH_URL=http://localhost:3007/api/auth)
fn auth_base_url(gateway_url: &str) -> String {
    std::env::var("MAGELAB_AUTH_URL").unwrap_or_else(|_| {
        format!("{}/v1/auth", gateway_url.trim_end_matches('/'))
    })
}

/// Run the login flow using the default method (Google).
pub async fn login(gateway_url: &str) -> Result<Credentials> {
    login_with_method(gateway_url, &LoginMethod::default()).await
}

/// Run the login flow with a specific method.
pub async fn login_with_method(gateway_url: &str, method: &LoginMethod) -> Result<Credentials> {
    match method {
        LoginMethod::Google => login_google(gateway_url).await,
        LoginMethod::MagicAuth => login_magic_auth(gateway_url).await,
    }
}

/// Magic Auth login — public entry point
pub async fn magic_login(gateway_url: &str) -> Result<Credentials> {
    login_magic_auth(gateway_url).await
}

/// Magic Auth login flow — exchanges code through the gateway
async fn login_magic_auth(gateway_url: &str) -> Result<Credentials> {
    let http = reqwest::Client::new();
    let cid = client_id();

    let email = prompt("Email:");
    if email.is_empty() {
        anyhow::bail!("Email is required");
    }

    // Send magic auth code via gateway
    eprintln!("Sending login code to {}...", email);
    let resp = http
        .post(format!(
            "{}/magic-auth",
            auth_base_url(gateway_url)
        ))
        .json(&serde_json::json!({
            "email": email,
            "client_id": cid,
        }))
        .send()
        .await
        .context("Failed to send magic auth code")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if body.contains("user_not_found") || status.as_u16() == 404 {
            eprintln!("No MageLab account found for {}.", email);
            println!();
            println!("  1) Sign in with Google (creates account automatically)");
            println!("  2) Sign up at {}", SIGNUP_URL);
            println!();
            let choice = prompt("Choose [1/2]:");
            match choice.as_str() {
                "1" | "" => return login_google(gateway_url).await,
                _ => {
                    eprintln!("Visit {} to create an account, then try again.", SIGNUP_URL);
                    anyhow::bail!("Account not found");
                }
            }
        }
        anyhow::bail!("Failed to send magic auth code ({}): {}", status, body);
    }

    // Prompt for code
    eprintln!("Code sent! Check your inbox.");
    let code_input = prompt("Code:");
    if code_input.is_empty() {
        anyhow::bail!("Code is required");
    }

    // Exchange magic auth code for tokens via gateway
    eprintln!("Authenticating...");
    let creds = exchange_token(
        gateway_url,
        &serde_json::json!({
            "grant_type": "urn:workos:oauth:grant-type:magic-auth",
            "code": code_input,
            "email": email,
            "client_id": cid,
        }),
    )
    .await?;

    creds.save()?;
    if let Some(ref email) = creds.email {
        eprintln!("Logged in as {}!", email);
    }
    eprintln!("Credentials saved to {}", Credentials::path()?.display());
    Ok(creds)
}

/// Google OAuth login flow — builds auth URL locally, exchanges code through gateway
async fn login_google(gateway_url: &str) -> Result<Credentials> {
    // Build auth URL using WorkOS SDK (client-side, no API key needed)
    let key: ApiKey = "sk_placeholder".to_string().into();
    let workos = WorkOs::new(&key);
    let cid_str = client_id();
    let cid: ClientId = cid_str.clone().into();

    let verifier = generate_code_verifier();
    let challenge = generate_code_challenge(&verifier);
    let state = generate_state();

    let listener = TcpListener::bind(format!("127.0.0.1:{}", LOOPBACK_PORT))
        .context("Failed to start loopback server on port 19872 — is another instance running?")?;
    let redirect_uri = format!("http://127.0.0.1:{}/callback", LOOPBACK_PORT);

    let provider = Provider::Oauth(OauthProvider::GoogleOAuth);
    let params = GetAuthorizationUrlParams {
        client_id: &cid,
        redirect_uri: &redirect_uri,
        connection_selector: ConnectionSelector::Provider(&provider),
        state: Some(&state),
        code_challenge: Some(CodeChallenge::S256(&challenge)),
        login_hint: None,
        domain_hint: None,
    };

    let authorize_url = workos
        .user_management()
        .get_authorization_url(&params)
        .context("Failed to build WorkOS authorization URL")?;

    eprintln!("Opening browser for login...");
    eprintln!("If browser doesn't open, visit:\n{}\n", authorize_url);
    open::that(authorize_url.as_str()).ok();

    eprintln!("Waiting for authentication...");
    let (code, returned_state) = wait_for_callback(listener)?;

    if returned_state != state {
        anyhow::bail!("OAuth state mismatch — possible CSRF attack");
    }

    // Exchange code for tokens via gateway (NOT directly with WorkOS)
    eprintln!("Exchanging authorization code...");
    let creds = exchange_token(
        gateway_url,
        &serde_json::json!({
            "grant_type": "authorization_code",
            "code": code,
            "code_verifier": verifier,
            "redirect_uri": redirect_uri,
            "client_id": cid_str,
        }),
    )
    .await?;

    creds.save()?;
    if let Some(ref email) = creds.email {
        eprintln!("Logged in as {}!", email);
    }
    eprintln!("Credentials saved to {}", Credentials::path()?.display());
    Ok(creds)
}

/// Exchange a grant (auth code, refresh token, magic auth) for credentials
/// via the gateway's /v1/auth/token endpoint.
async fn exchange_token(
    gateway_url: &str,
    body: &serde_json::Value,
) -> Result<Credentials> {
    let http = reqwest::Client::new();
    let url = format!("{}/token", auth_base_url(gateway_url));

    let resp = http
        .post(&url)
        .json(body)
        .send()
        .await
        .context("Failed to connect to gateway for token exchange")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Token exchange failed ({}): {}", status, body);
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse token response")?;

    let access_token = data["access_token"]
        .as_str()
        .context("No access_token in response")?
        .to_string();

    let refresh_token = data["refresh_token"].as_str().map(String::from);
    let email = data["user"]["email"].as_str().map(String::from);
    let user_id = data["user"]["id"].as_str().map(String::from);

    let expires_in = data["expires_in"].as_i64().unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    Ok(Credentials {
        access_token: Some(access_token),
        refresh_token,
        expires_at: Some(expires_at),
        user_id,
        email,
    })
}

fn generate_code_verifier() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

fn generate_state() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn wait_for_callback(listener: TcpListener) -> Result<(String, String)> {
    listener.set_nonblocking(false)?;
    let (mut stream, _) = listener.accept().context("No callback received")?;

    let mut reader = std::io::BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

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

    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h2>Login successful!</h2><p>You can close this tab and return to the terminal.</p></body></html>";
    stream.write_all(response.as_bytes()).ok();

    Ok((code, state))
}

/// Refresh an expired access token via the gateway
#[allow(dead_code)]
pub async fn refresh_token(gateway_url: &str, rt: &str) -> Result<Credentials> {
    let creds = exchange_token(
        gateway_url,
        &serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": rt,
            "client_id": client_id(),
        }),
    )
    .await?;
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

    if let Some(ref rt) = creds.refresh_token {
        if let Ok(new_creds) = refresh_token(gateway_url, rt).await {
            return Ok(new_creds.access_token.unwrap());
        }
    }

    let new_creds = login(gateway_url).await?;
    Ok(new_creds.access_token.unwrap())
}
