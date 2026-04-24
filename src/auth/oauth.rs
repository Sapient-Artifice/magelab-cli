use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::io::{BufRead, IsTerminal, Write};
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
    /// Google OAuth via browser (legacy — requires gateway WorkOS API key)
    Google,
    /// Magic auth code via email (legacy — requires gateway WorkOS API key)
    MagicAuth,
    /// Web-based login via the MageLab web app (browser → encrypted code exchange)
    #[default]
    Web,
}

impl FromStr for LoginMethod {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "google" => Ok(Self::Google),
            "magic" | "email" | "magic-auth" => Ok(Self::MagicAuth),
            "web" | "browser" => Ok(Self::Web),
            _ => anyhow::bail!(
                "Unknown login method '{}'. Use 'google', 'magic', or 'web'.",
                s
            ),
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
    std::env::var("MAGELAB_AUTH_URL")
        .unwrap_or_else(|_| format!("{}/v1/auth", gateway_url.trim_end_matches('/')))
}

/// Get the MageLab web app URL for browser-based login.
/// Defaults to https://magelab.ai. Override with MAGELAB_WEB_URL for local dev.
fn web_url() -> String {
    std::env::var("MAGELAB_WEB_URL")
        .unwrap_or_else(|_| "https://magelab.ai".to_string())
        .trim_end_matches('/')
        .to_string()
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
        LoginMethod::Web => login_web().await,
    }
}

/// Web-based login — two-phase code exchange via the MageLab web app.
///
/// Security: the access token never appears in URLs, browser history, or logs.
///
/// Flow:
///   1. CLI generates a random `state` and binds loopback on :19872
///   2. Browser opens to {web_url}/auth/sign-in → user authenticates
///   3. Web app mints a one-time code, redirects to loopback with ?code=...&state=...
///   4. CLI verifies state, then POSTs the code back to exchange for the actual token
async fn login_web() -> Result<Credentials> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", LOOPBACK_PORT))
        .context("Failed to start loopback server on port 19872 — is another instance running?")?;

    let state = generate_state();
    let loopback_url = format!("http://127.0.0.1:{}/callback", LOOPBACK_PORT);
    let base = web_url();

    let cli_token_url = format!(
        "{}/api/auth/cli-token?redirect={}&state={}",
        base,
        urlencoding::encode(&loopback_url),
        urlencoding::encode(&state)
    );
    let sign_in_url = format!(
        "{}/auth/sign-in?returnTo={}",
        base,
        urlencoding::encode(&cli_token_url)
    );

    crate::ui::label("open", &sign_in_url);
    open::that(&sign_in_url).ok();

    let sp = crate::ui::spinner("Waiting for authentication...");
    let code = wait_for_code_callback(listener)?;
    sp.finish_and_clear();

    let sp = crate::ui::spinner("Exchanging code...");
    let creds = exchange_cli_code(&base, &code, &state).await?;
    sp.finish_and_clear();

    creds.save()?;
    if let Some(ref email) = creds.email {
        crate::ui::success(&format!("Logged in as {email}"));
    }
    crate::ui::label("credentials", &Credentials::path()?.display().to_string());
    Ok(creds)
}

/// Wait for the web app to redirect to our loopback with an encrypted code.
/// State is NOT in the URL — only the CLI knows it.
/// Times out after 3 minutes.
fn wait_for_code_callback(listener: TcpListener) -> Result<String> {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(180);
    listener.set_nonblocking(true)?;

    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(conn) => break conn,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    anyhow::bail!(
                        "Login timed out — no response received within 3 minutes. Try again."
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e).context("Failed to accept callback connection"),
        }
    };

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
        .context("No code in callback — login may have failed")?;

    let response = concat!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n",
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">",
        "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">",
        "<meta name=\"referrer\" content=\"no-referrer\">",
        "<title>Mage Lab — Login Successful</title>",
        "<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">",
        "<link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>",
        "<link href=\"https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600&display=swap\" rel=\"stylesheet\">",
        "<style>",
        "*{margin:0;padding:0;box-sizing:border-box}",
        "body{background:#09090b;color:#fafafa;font-family:'Geist',system-ui,sans-serif;",
        "min-height:100vh;display:flex;align-items:center;justify-content:center}",
        ".card{text-align:center;max-width:400px;padding:3rem 2rem}",
        ".icon{width:48px;height:48px;margin:0 auto 1.5rem;border-radius:12px;",
        "background:linear-gradient(135deg,#8b5cf6,#7c3aed);",
        "display:flex;align-items:center;justify-content:center}",
        ".icon svg{width:24px;height:24px;fill:none;stroke:#fff;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}",
        "h1{font-size:1.25rem;font-weight:600;margin-bottom:.5rem;letter-spacing:-.01em}",
        "p{color:#a1a1aa;font-size:.875rem;line-height:1.5}",
        ".hint{margin-top:1.5rem;padding-top:1.5rem;border-top:1px solid rgba(255,255,255,.06);",
        "color:#71717a;font-size:.75rem;font-family:'Geist Mono',monospace}",
        "</style></head><body>",
        "<div class=\"card\">",
        "<div class=\"icon\"><svg viewBox=\"0 0 24 24\"><polyline points=\"20 6 9 17 4 12\"/></svg></div>",
        "<h1>Login successful</h1>",
        "<p>You can close this tab and return to the terminal.</p>",
        "<div class=\"hint\">magelab cli</div>",
        "</div></body></html>",
    );
    stream.write_all(response.as_bytes()).ok();

    Ok(code)
}

/// Exchange an encrypted CLI code for credentials via POST to the web app.
pub async fn exchange_cli_code(web_base: &str, code: &str, state: &str) -> Result<Credentials> {
    let http = reqwest::Client::new();
    let url = format!("{}/api/auth/cli-token", web_base);

    let resp = http
        .post(&url)
        .json(&serde_json::json!({ "code": code, "state": state }))
        .send()
        .await
        .context("Failed to exchange code with web app")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Code exchange failed ({}): {}", status, body);
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse code exchange response")?;

    let access_token = data["access_token"]
        .as_str()
        .context("No access_token in response")?
        .to_string();

    let email = data["email"].as_str().map(String::from);
    let user_id = data["user_id"].as_str().map(String::from);

    // WorkOS access tokens typically expire in 5 minutes
    let expires_at = chrono::Utc::now().timestamp() + 300;

    Ok(Credentials {
        access_token: Some(access_token),
        refresh_token: None,
        expires_at: Some(expires_at),
        user_id,
        email,
    })
}


/// Magic Auth login flow — exchanges code through the gateway
async fn login_magic_auth(gateway_url: &str) -> Result<Credentials> {
    let http = reqwest::Client::new();
    let cid = client_id();

    let email = if std::io::stdin().is_terminal() {
        crate::ui::animated_prompt("Email:")
    } else {
        prompt("Email:")
    };
    if email.is_empty() {
        anyhow::bail!("Email is required");
    }

    let sp = crate::ui::spinner(&format!("Sending code to {email}..."));
    let resp = http
        .post(format!("{}/magic-auth", auth_base_url(gateway_url)))
        .json(&serde_json::json!({
            "email": email,
            "client_id": cid,
        }))
        .send()
        .await
        .context("Failed to send magic auth code")?;

    if !resp.status().is_success() {
        sp.finish_and_clear();
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if body.contains("user_not_found") || status.as_u16() == 404 {
            eprintln!("No MageLab account found for {}.", email);
            println!();
            println!("  1) Sign in with Google (creates account automatically)");
            println!("  2) Sign up at {}", SIGNUP_URL);
            println!();
            let choice = if std::io::stdin().is_terminal() {
                crate::ui::animated_prompt("Choose [1/2]:")
            } else {
                prompt("Choose [1/2]:")
            };
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

    sp.finish_with_message("Code sent! Check your inbox.");
    let code_input = if std::io::stdin().is_terminal() {
        crate::ui::animated_prompt("Code:")
    } else {
        prompt("Code:")
    };
    if code_input.is_empty() {
        anyhow::bail!("Code is required");
    }

    let sp = crate::ui::spinner("Authenticating...");
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
    sp.finish_and_clear();

    creds.save()?;
    if let Some(ref email) = creds.email {
        crate::ui::success(&format!("Logged in as {email}"));
    }
    crate::ui::label("credentials", &Credentials::path()?.display().to_string());
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

    crate::ui::label("open", authorize_url.as_str());
    open::that(authorize_url.as_str()).ok();

    let sp = crate::ui::spinner("Waiting for authentication...");
    let (code, returned_state) = wait_for_callback(listener)?;
    sp.finish_and_clear();

    if returned_state != state {
        anyhow::bail!("OAuth state mismatch — possible CSRF attack");
    }

    let sp = crate::ui::spinner("Exchanging authorization code...");
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
    sp.finish_and_clear();

    creds.save()?;
    if let Some(ref email) = creds.email {
        crate::ui::success(&format!("Logged in as {email}"));
    }
    crate::ui::label("credentials", &Credentials::path()?.display().to_string());
    Ok(creds)
}

/// Exchange a grant (auth code, refresh token, magic auth) for credentials
/// via the gateway's /v1/auth/token endpoint.
async fn exchange_token(gateway_url: &str, body: &serde_json::Value) -> Result<Credentials> {
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
