use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};
use std::net::TcpListener;
use std::str::FromStr;
use workos::sso::{AuthorizationCode, ClientId};
use workos::user_management::{
    AuthenticateWithCode, AuthenticateWithCodeParams, AuthenticateWithMagicAuth,
    AuthenticateWithMagicAuthParams, AuthenticateWithRefreshToken,
    AuthenticateWithRefreshTokenParams, CodeChallenge, ConnectionSelector, CreateMagicAuth,
    CreateMagicAuthParams, GetAuthorizationUrl, GetAuthorizationUrlParams, OauthProvider, Provider,
};
use workos::{ApiKey, WorkOs};

use super::credentials::Credentials;

/// WorkOS Client ID (shared with web frontend)
const DEFAULT_CLIENT_ID: &str = "client_01KKJ9GJKHDMW63A3RCV56KVZ6";

/// Fixed loopback port (must match WorkOS redirect URI config)
const LOOPBACK_PORT: u16 = 19872;

/// URL for new account signup
const SIGNUP_URL: &str = "https://magelab.ai/signup";

/// Login method selection
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginMethod {
    /// Google OAuth via browser
    Google,
    /// Magic auth code via email
    MagicAuth,
}

impl Default for LoginMethod {
    fn default() -> Self {
        Self::Google
    }
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

/// Get WorkOS API Key from env (required for SDK calls)
fn api_key() -> String {
    std::env::var("WORKOS_API_KEY").unwrap_or_default()
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

/// Magic Auth login flow
async fn login_magic_auth(_gateway_url: &str) -> Result<Credentials> {
    let key: ApiKey = api_key().into();
    let workos = WorkOs::new(&key);
    let cid: ClientId = client_id().into();

    print!("Email: ");
    std::io::stdout().flush()?;
    let mut email = String::new();
    std::io::stdin().lock().read_line(&mut email)?;
    let email = email.trim();
    if email.is_empty() {
        anyhow::bail!("Email is required");
    }

    println!("Sending login code to {}...", email);
    let create_params = CreateMagicAuthParams {
        email,
        invitation_token: None,
    };

    let magic_auth_result = workos
        .user_management()
        .create_magic_auth(&create_params)
        .await;

    if let Err(ref e) = magic_auth_result {
        let err_str = format!("{:?}", e);
        if err_str.contains("user_not_found") || err_str.contains("404") {
            println!("\nNo MageLab account found for {}.", email);
            println!("Create one at: {}", SIGNUP_URL);
            println!("\nOr sign in with Google: magelab login --method google");
            anyhow::bail!("Account not found");
        }
    }

    magic_auth_result.map_err(|e| anyhow::anyhow!("Failed to send magic auth code: {:?}", e))?;

    println!("Check your inbox for a login code.");
    print!("Code: ");
    std::io::stdout().flush()?;
    let mut code_input = String::new();
    std::io::stdin().lock().read_line(&mut code_input)?;
    let code_input = code_input.trim().to_string();
    if code_input.is_empty() {
        anyhow::bail!("Code is required");
    }

    let magic_code: workos::user_management::MagicAuthCode = code_input.into();
    let auth_params = AuthenticateWithMagicAuthParams {
        client_id: &cid,
        code: &magic_code,
        email,
        invitation_token: None,
        ip_address: None,
        user_agent: None,
    };

    let response = workos
        .user_management()
        .authenticate_with_magic_auth(&auth_params)
        .await
        .map_err(|e| {
            let err_str = format!("{:?}", e);
            if err_str.contains("user_not_found") || err_str.contains("404") {
                anyhow::anyhow!(
                    "No MageLab account found for {}.\nCreate one at: {}",
                    email,
                    SIGNUP_URL
                )
            } else {
                anyhow::anyhow!("Magic auth failed: {:?}", e)
            }
        })?;

    let expires_at = chrono::Utc::now().timestamp() + 3600;
    let creds = Credentials {
        access_token: Some(response.access_token.to_string()),
        refresh_token: Some(response.refresh_token.to_string()),
        expires_at: Some(expires_at),
        user_id: Some(response.user.id.to_string()),
        email: Some(response.user.email.clone()),
    };

    creds.save()?;
    let path = Credentials::path()?;
    println!("Logged in as {}!", response.user.email);
    println!("Credentials saved to {}", path.display());
    Ok(creds)
}

/// Google OAuth login flow via WorkOS AuthKit
async fn login_google(_gateway_url: &str) -> Result<Credentials> {
    let key: ApiKey = api_key().into();
    let workos = WorkOs::new(&key);
    let cid: ClientId = client_id().into();

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

    println!("Opening browser for login...");
    println!("If browser doesn't open, visit:\n{}\n", authorize_url);
    open::that(authorize_url.as_str()).ok();

    println!("Waiting for authentication...");
    let (code, returned_state) = wait_for_callback(listener)?;

    if returned_state != state {
        anyhow::bail!("OAuth state mismatch — possible CSRF attack");
    }

    println!("Exchanging authorization code...");
    let auth_code: AuthorizationCode = code.into();
    let auth_params = AuthenticateWithCodeParams {
        client_id: &cid,
        code: &auth_code,
        code_verifier: Some(&verifier),
        invitation_token: None,
        ip_address: None,
        user_agent: None,
    };

    let response = workos
        .user_management()
        .authenticate_with_code(&auth_params)
        .await
        .map_err(|e| anyhow::anyhow!("WorkOS authentication failed: {:?}", e))?;

    let expires_at = chrono::Utc::now().timestamp() + 3600;
    let creds = Credentials {
        access_token: Some(response.access_token.to_string()),
        refresh_token: Some(response.refresh_token.to_string()),
        expires_at: Some(expires_at),
        user_id: Some(response.user.id.to_string()),
        email: Some(response.user.email.clone()),
    };

    creds.save()?;
    let path = Credentials::path()?;
    println!("Logged in as {}!", response.user.email);
    println!("Credentials saved to {}", path.display());
    Ok(creds)
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

#[allow(dead_code)]
pub async fn refresh_token(rt: &str) -> Result<Credentials> {
    let key: ApiKey = api_key().into();
    let workos = WorkOs::new(&key);
    let cid: ClientId = client_id().into();
    let refresh: workos::user_management::RefreshToken = rt.to_string().into();

    let params = AuthenticateWithRefreshTokenParams {
        client_id: &cid,
        refresh_token: &refresh,
        organization_id: None,
        ip_address: None,
        user_agent: None,
    };

    let response = workos
        .user_management()
        .authenticate_with_refresh_token(&params)
        .await
        .map_err(|e| anyhow::anyhow!("WorkOS token refresh failed: {:?}", e))?;

    let expires_at = chrono::Utc::now().timestamp() + 3600;
    let creds = Credentials {
        access_token: Some(response.access_token.to_string()),
        refresh_token: Some(response.refresh_token.to_string()),
        expires_at: Some(expires_at),
        user_id: Some(response.user.id.to_string()),
        email: Some(response.user.email.clone()),
    };
    creds.save()?;
    Ok(creds)
}

#[allow(dead_code)]
pub async fn ensure_valid_jwt(gateway_url: &str) -> Result<String> {
    let creds = Credentials::load()?;

    if creds.is_token_valid() {
        return Ok(creds.access_token.unwrap());
    }

    if let Some(ref rt) = creds.refresh_token {
        match refresh_token(rt).await {
            Ok(new_creds) => return Ok(new_creds.access_token.unwrap()),
            Err(_) => {}
        }
    }

    let new_creds = login(gateway_url).await?;
    Ok(new_creds.access_token.unwrap())
}
