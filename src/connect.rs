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

/// Convert a WebSocket endpoint back to the HTTP base URL used for health checks.
pub fn ws_to_http_url(ws_url: &str) -> String {
    let base = ws_url.trim_end_matches('/');
    let base = base.strip_suffix("/ws").unwrap_or(base);
    if let Some(rest) = base.strip_prefix("wss://") {
        format!("https://{}", rest)
    } else if let Some(rest) = base.strip_prefix("ws://") {
        format!("http://{}", rest)
    } else {
        base.to_string()
    }
}

/// Validate and convert a direct backend WebSocket URL for local health checks.
///
/// This intentionally rejects relay/gateway-style paths and query strings. The
/// connect --ws flag is only for direct backend endpoints such as
/// ws://127.0.0.1:8787/ws.
pub fn direct_ws_to_http_url(ws_url: &str) -> Result<String> {
    let parsed = url::Url::parse(ws_url)?;
    match parsed.scheme() {
        "ws" | "wss" => {}
        _ => anyhow::bail!("--ws must use a ws:// or wss:// URL"),
    }

    if parsed.path() != "/ws" || parsed.query().is_some() || parsed.fragment().is_some() {
        anyhow::bail!("--ws must point directly to a backend /ws endpoint");
    }

    Ok(ws_to_http_url(ws_url))
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
    resolve_with_local_url(config, no_launch, &config.local_url).await
}

pub async fn resolve_with_local_url(
    config: &Config,
    no_launch: bool,
    local_url: &str,
) -> Result<ConnectResult> {
    // 1. Check if local backend is already running
    if detect::check_backend_health(local_url).await {
        return Ok(ConnectResult {
            url: Some(to_ws_url(local_url)),
            token: None,
            mode: "local".to_string(),
            model: Some(config.default_model.clone()),
        });
    }

    // 2. Try to launch local backend (unless --no-launch)
    if !no_launch {
        if let Some(bundle) = detect::find_backend_bundle(config.magelab_home.as_deref())? {
            let port = detect::port_from_url(local_url);
            let control_secret = detect::generate_backend_control_secret();
            if let Ok(mut child) = detect::launch_backend_headless(
                &bundle,
                "127.0.0.1",
                port,
                config.relay_enabled,
                &control_secret,
            ) {
                if detect::wait_for_backend(local_url, Duration::from_secs(15))
                    .await
                    .is_ok()
                {
                    // Detach only after health succeeds so failed launches are cleaned up.
                    std::mem::forget(child);
                    return Ok(ConnectResult {
                        url: Some(to_ws_url(local_url)),
                        token: None,
                        mode: "local".to_string(),
                        model: Some(config.default_model.clone()),
                    });
                }
                child.kill().ok();
                child.wait().ok();
            }
        }
    }

    // 3. Check for JWT → try relay
    let creds = auth::Credentials::load().unwrap_or_default();
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

/// Try to get a valid JWT, refreshing if expired (with biometric fallback)
async fn get_valid_jwt(creds: &auth::Credentials, gateway_url: &str) -> Option<String> {
    auth::get_valid_jwt(creds, gateway_url).await
}
