use anyhow::Result;
use serde::Serialize;
use std::time::Duration;

use crate::auth;
use crate::config::Config;
use crate::detect;

#[derive(Debug, Serialize)]
pub struct ConnectResult {
    pub url: Option<String>,
    pub token: Option<String>,
    pub mode: String,
    pub model: Option<String>,
}

/// Resolve connection mode and return machine-readable result.
///
/// When `no_launch` is true, skips headless backend auto-launch
/// (pure query, no side effects).
pub async fn resolve(config: &Config, no_launch: bool) -> Result<ConnectResult> {
    // 1. Check if local backend is already running
    if detect::check_backend_health(&config.local_url).await {
        return Ok(ConnectResult {
            url: Some(format!(
                "ws://{}/ws",
                config.local_url.trim_start_matches("http://")
            )),
            token: None,
            mode: "local".to_string(),
            model: Some(config.default_model.clone()),
        });
    }

    // 2. Try to launch local backend (unless --no-launch)
    if !no_launch {
        if let Some(home) = detect::find_magelab_home(config.magelab_home.as_deref()) {
            if let Ok(_child) = detect::launch_backend_headless(&home) {
                if detect::wait_for_backend(&config.local_url, Duration::from_secs(15))
                    .await
                    .is_ok()
                {
                    return Ok(ConnectResult {
                        url: Some(format!(
                            "ws://{}/ws",
                            config.local_url.trim_start_matches("http://")
                        )),
                        token: None,
                        mode: "local".to_string(),
                        model: Some(config.default_model.clone()),
                    });
                }
            }
        }
    }

    // 3. Check for JWT → try relay
    let creds = auth::credentials::Credentials::load().unwrap_or_default();
    if let Some(jwt) = get_valid_jwt(&creds, &config.gateway_url).await {
        if let Ok(devices) = detect::discover_devices(&config.gateway_url, &jwt).await {
            if !devices.is_empty() {
                // Get ws-ticket for relay connection
                if let Ok(ticket) = detect::get_ws_ticket(&config.gateway_url, &jwt).await {
                    let url = format!(
                        "{}/v1/realtime/portal/ws?ws_ticket={}",
                        config
                            .gateway_url
                            .replace("https://", "wss://")
                            .replace("http://", "ws://"),
                        ticket
                    );
                    return Ok(ConnectResult {
                        url: Some(url),
                        token: Some(jwt),
                        mode: "relay".to_string(),
                        model: Some(config.default_model.clone()),
                    });
                }
            }
        }
    }

    // 4. Check for API key → REST (chat only)
    if let Some(api_key) = config.api_key() {
        return Ok(ConnectResult {
            url: Some(config.gateway_url.clone()),
            token: Some(api_key),
            mode: "remote".to_string(),
            model: Some(config.default_model.clone()),
        });
    }

    // 5. Nothing available
    Ok(ConnectResult {
        url: None,
        token: None,
        mode: "none".to_string(),
        model: None,
    })
}

/// Try to get a valid JWT, refreshing if expired
async fn get_valid_jwt(
    creds: &auth::credentials::Credentials,
    gateway_url: &str,
) -> Option<String> {
    if let Some(token) = &creds.access_token {
        if !creds.is_token_valid() {
            // Try refresh
            if let Some(refresh) = &creds.refresh_token {
                if let Ok(new_creds) = auth::oauth::refresh_token(gateway_url, refresh).await {
                    let _ = new_creds.save();
                    return new_creds.access_token;
                }
            }
            return None;
        }
        return Some(token.clone());
    }
    None
}
