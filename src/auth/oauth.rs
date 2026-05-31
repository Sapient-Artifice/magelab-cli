use anyhow::{Context, Result};
use std::io::{BufRead, IsTerminal, Write};
use std::net::TcpListener;
use std::str::FromStr;

use magelab_core::auth::{self as core_auth, pkce, Credentials, LOOPBACK_PORT};

// Re-export core auth functions for the public API (used by integration tests)
#[allow(unused_imports)]
pub use magelab_core::auth::exchange_cli_code;
#[allow(unused_imports)]
pub use magelab_core::auth::refresh_token;

/// Build CLI-specific success HTML (says "return to the terminal" / "mage cli").
fn cli_success_html() -> String {
    core_auth::login_success_html("return to the terminal", "mage cli")
}

/// Prompt for user input from stdin
fn prompt(label: &str) -> String {
    eprint!("{} ", label);
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap_or_default();
    input.trim().to_string()
}

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

/// Get the MageLab web app URL for browser-based login.
/// Defaults to https://magelab.ai. Override with MAGELAB_WEB_URL for local dev.
fn web_url() -> String {
    std::env::var("MAGELAB_WEB_URL")
        .unwrap_or_else(|_| "https://magelab.ai".to_string())
        .trim_end_matches('/')
        .to_string()
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
/// Security: the access token is AES-256-GCM encrypted in transit. The state
/// parameter IS visible in the browser address bar/history (it's part of the
/// returnTo URL), but exploiting this requires BOTH the state AND the encrypted
/// code (which only appears in the loopback redirect to 127.0.0.1:19872).
///
/// Flow:
///   1. CLI generates a random `state` and binds loopback on :19872
///   2. Browser opens to {web_url}/auth/sign-in → user authenticates
///   3. Web app encrypts token with AES-256-GCM (key = server_secret + state),
///      redirects to loopback with ?code=<encrypted>
///   4. CLI POSTs {code, state} back — web app re-derives the key and decrypts
async fn login_web() -> Result<Credentials> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", LOOPBACK_PORT))
        .context("Failed to start loopback server on port 19872 — is another instance running?")?;

    let state = pkce::generate_state();
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

    // State is verified implicitly: the web app encrypts the token using
    // AES-256-GCM with a key derived from (server_secret + state). If the
    // state is wrong, decryption fails in the POST exchange below.
    let sp = crate::ui::spinner("Exchanging code...");
    let creds = core_auth::exchange_cli_code(&base, &code, &state)
        .await
        .map_err(|e| {
            if let core_auth::AuthError::TokenExchange { status: 410, .. } = &e {
                anyhow::anyhow!(
                    "Login code expired — the 60-second window elapsed. Please try again."
                )
            } else {
                anyhow::anyhow!("{e}")
            }
        })?;
    sp.finish_and_clear();

    super::save_credentials(&creds)?;
    if let Some(ref email) = creds.email {
        crate::ui::success(&format!("Logged in as {email}"));
    }
    crate::ui::label("credentials", &Credentials::path()?.display().to_string());
    Ok(creds)
}

/// Wait for the web app to redirect to our loopback with an encrypted code.
/// Returns the code. State is verified implicitly during decryption in exchange_cli_code.
/// Times out after 3 minutes.
fn wait_for_code_callback(listener: TcpListener) -> Result<String> {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(180);
    listener.set_nonblocking(true)?;

    loop {
        // Accept next connection (poll with timeout)
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

        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut reader = std::io::BufReader::new(&stream);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            continue; // malformed connection, try next
        }

        // Drain remaining HTTP headers to prevent "connection reset" on
        // browsers that wait for the full request to be consumed.
        {
            let mut header = String::new();
            loop {
                header.clear();
                match reader.read_line(&mut header) {
                    Ok(0) => break,
                    Ok(_) if header.trim().is_empty() => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        }

        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 || parts[0] != "GET" {
            continue; // non-GET request, skip
        }

        let url = match url::Url::parse(&format!("http://localhost{}", parts[1])) {
            Ok(u) => u,
            Err(_) => continue,
        };

        if url.path() != "/callback" {
            // Favicon, prefetch, or other non-callback request — send 404, keep listening
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
            continue;
        }

        let code = url
            .query_pairs()
            .find(|(k, _)| k == "code")
            .map(|(_, v)| v.to_string())
            .context("No code in callback — login may have failed")?;

        let html = cli_success_html();
        let html_bytes = html.as_bytes();
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            html_bytes.len(),
        );
        stream.write_all(header.as_bytes()).ok();
        stream.write_all(html_bytes).ok();

        return Ok(code);
    }
}

/// Magic Auth login flow — exchanges code through the gateway
async fn login_magic_auth(gateway_url: &str) -> Result<Credentials> {
    let http = reqwest::Client::new();
    let cid = core_auth::client_id();

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
        .post(format!("{}/magic-auth", core_auth::auth_base_url(gateway_url)))
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
    let creds = core_auth::exchange_token(
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

    super::save_credentials(&creds)?;
    if let Some(ref email) = creds.email {
        crate::ui::success(&format!("Logged in as {email}"));
    }
    crate::ui::label("credentials", &Credentials::path()?.display().to_string());
    Ok(creds)
}

/// Google OAuth login flow — PKCE via shared loopback in magelab-core
async fn login_google(gateway_url: &str) -> Result<Credentials> {
    let sp = crate::ui::spinner("Opening browser for authentication...");
    let html = cli_success_html();
    let creds = core_auth::pkce_loopback_login(
        gateway_url,
        |url| {
            crate::ui::label("open", url);
            open::that(url).map_err(|e| core_auth::AuthError::Http(e.to_string()))
        },
        Some(&html),
        None,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    sp.finish_and_clear();

    super::save_credentials(&creds)?;
    if let Some(ref email) = creds.email {
        crate::ui::success(&format!("Logged in as {email}"));
    }
    crate::ui::label("credentials", &Credentials::path()?.display().to_string());
    Ok(creds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate_callback_path(path: &str) -> anyhow::Result<()> {
        let url = url::Url::parse(&format!("http://localhost{}", path))
            .context("Failed to parse callback URL")?;
        if url.path() != "/callback" {
            anyhow::bail!(
                "Unexpected request path '{}', expected /callback",
                url.path()
            );
        }
        Ok(())
    }

    #[test]
    fn callback_path_accepted() {
        assert!(validate_callback_path("/callback?code=abc123").is_ok());
        assert!(validate_callback_path("/callback?code=abc&state=xyz").is_ok());
        assert!(validate_callback_path("/callback").is_ok());
    }

    #[test]
    fn callback_path_rejected_for_prefix_attack() {
        assert!(validate_callback_path("/callbackevil?code=abc").is_err());
        assert!(validate_callback_path("/callback/extra?code=abc").is_err());
    }

    #[test]
    fn callback_path_rejected_for_wrong_path() {
        assert!(validate_callback_path("/?code=abc").is_err());
        assert!(validate_callback_path("/evil/callback?code=abc").is_err());
        assert!(validate_callback_path("/other?code=abc").is_err());
    }
}
