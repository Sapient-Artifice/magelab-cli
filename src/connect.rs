use anyhow::Result;
use serde::Serialize;
use std::time::Duration;

use crate::auth;
use crate::config::Config;
use crate::detect;

/// Convert an HTTP(S) URL to its WebSocket equivalent.
/// http://host → ws://host/ws
/// https://host → wss://host/ws
pub fn to_ws_url(http_url: &str) -> String {
    let base = http_url.trim_end_matches('/');
    if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{}/ws", rest)
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{}/ws", rest)
    } else {
        // Already a ws/wss URL or unknown scheme — append /ws
        format!("{}/ws", base)
    }
}

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
            url: Some(to_ws_url(&config.local_url)),
            token: None,
            mode: "local".to_string(),
            model: Some(config.default_model.clone()),
        });
    }

    // 2. Try to launch local backend (unless --no-launch)
    if !no_launch {
        if let Some(home) = detect::find_magelab_home(config.magelab_home.as_deref()) {
            let port = detect::port_from_url(&config.local_url);
            if let Ok(child) = detect::launch_backend_headless(&home, port) {
                // Detach the child so it outlives this CLI invocation
                // without leaving a zombie (Unix) or being killed (Windows).
                std::mem::forget(child);
                if detect::wait_for_backend(&config.local_url, Duration::from_secs(15))
                    .await
                    .is_ok()
                {
                    return Ok(ConnectResult {
                        url: Some(to_ws_url(&config.local_url)),
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
            // Use bound device if configured, otherwise any online device
            let has_device = if let Some(ref bound) = config.default_device {
                devices.iter().any(|d| d == bound)
            } else {
                !devices.is_empty()
            };
            if has_device {
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
                    if let Err(e) = new_creds.save() {
                        // Log but don't fail — the in-memory token is still usable
                        eprintln!("[magelab] Warning: failed to persist refreshed token: {e}");
                    }
                    return new_creds.access_token;
                }
            }
            return None;
        }
        return Some(token.clone());
    }
    None
}
