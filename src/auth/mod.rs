pub mod touchid;

/// Thin OAuth wrapper — interactive login flows live here; portable token
/// exchange logic is delegated to `magelab_core::auth`.
pub mod oauth;

use anyhow::Result;

use crate::config::Config;

// Re-export core's Credentials as the CLI's canonical type.
pub use magelab_core::auth::Credentials;

/// Save credentials to the canonical auth store.
pub fn save_credentials(creds: &Credentials) -> Result<()> {
    creds.save()?;
    Ok(())
}

/// Save credentials returned by a refresh operation to the canonical store.
pub(crate) fn save_refreshed_credentials(creds: &Credentials) -> Result<()> {
    creds.save()?;
    Ok(())
}

/// Clear credentials from the canonical store and legacy biometric items.
pub fn clear_credentials() -> Result<()> {
    touchid::clear()?;
    Credentials::clear()?;
    Ok(())
}

/// Try to get a valid JWT, refreshing through the canonical credentials.
pub async fn get_valid_jwt(creds: &Credentials, gateway_url: &str) -> Option<String> {
    if let Some(token) = &creds.access_token {
        if creds.is_token_valid() {
            return Some(token.clone());
        }

        if let Some(refresh) = &creds.refresh_token {
            match magelab_core::auth::refresh_token(gateway_url, refresh).await {
                Ok(mut new_creds) => {
                    if new_creds.refresh_token.is_none() {
                        new_creds.refresh_token = Some(refresh.clone());
                    }
                    save_refreshed_credentials(&new_creds).ok();
                    return new_creds.access_token;
                }
                Err(_) => {
                    // Refresh failed. A concurrent `mage auth token` (Pi resolves
                    // "!mage auth token" per request) may have just rotated the
                    // single-use refresh token and saved fresh credentials. Re-read
                    // once (bypassing this process's cache): if the stored creds
                    // CHANGED, a concurrent winner wrote them — use them if valid.
                    // If unchanged, the token is simply expired/revoked (no winner),
                    // so bail immediately rather than adding latency to the
                    // per-request hot path. (The tight simultaneous window where the
                    // winner hasn't saved yet is picked up on Pi's next request.)
                    if let Ok(reloaded) = Credentials::reload() {
                        let changed = reloaded.access_token != creds.access_token
                            || reloaded.expires_at != creds.expires_at;
                        if changed && reloaded.is_token_valid() {
                            return reloaded.access_token;
                        }
                    }
                }
            }
        }
    }
    None
}

/// Get the best available auth token.
///
/// Fallback chain: JWT → refresh → vault (interactive only) →
/// MAGELAB_API_KEY env var → error
pub async fn get_token(config: &Config) -> Result<String> {
    let creds = Credentials::load().unwrap_or_default();
    if let Some(token) = get_valid_jwt(&creds, &config.gateway_url).await {
        return Ok(token);
    }
    // No stderr warning here: `mage auth token` is invoked per request by the
    // Pi extension, and Pi may capture/treat subprocess stderr as failure.
    // Stay silent and fall through to the next credential source.

    // Static fallback. The vault (long-lived API key) is only consulted
    // interactively: TouchID cannot gate a non-TTY caller, so without this
    // check any local process could harvest the vault key by spawning
    // `mage auth token`. The env var remains available to non-interactive
    // callers — it is the caller's own environment.
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdin());
    if let Some(token) = magelab_core::auth::static_token_fallback(
        interactive,
        || {
            magelab_core::vault::Vault::open()
                .ok()
                .and_then(|v| v.get("magelab_api_key").ok().flatten())
        },
        std::env::var("MAGELAB_API_KEY").ok(),
    ) {
        return Ok(token);
    }

    anyhow::bail!("Not authenticated. Run: mage login")
}
