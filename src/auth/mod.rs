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
            if let Ok(mut new_creds) = magelab_core::auth::refresh_token(gateway_url, refresh).await
            {
                if new_creds.refresh_token.is_none() {
                    new_creds.refresh_token = Some(refresh.clone());
                }
                save_refreshed_credentials(&new_creds).ok();
                return new_creds.access_token;
            }
        }
    }
    None
}

/// Get the best available auth token.
///
/// Fallback chain: JWT → refresh → vault → MAGELAB_API_KEY env var → error
pub async fn get_token(config: &Config) -> Result<String> {
    let creds = Credentials::load().unwrap_or_default();
    if let Some(token) = get_valid_jwt(&creds, &config.gateway_url).await {
        return Ok(token);
    }
    if creds.access_token.is_some() {
        eprintln!("Warning: JWT expired and refresh failed. Falling back to API key.");
    }

    // Try vault
    if let Ok(v) = magelab_core::vault::Vault::open() {
        if let Ok(Some(key)) = v.get("magelab_api_key") {
            return Ok(key);
        }
    }

    // Env var fallback
    if let Ok(key) = std::env::var("MAGELAB_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }

    anyhow::bail!("Not authenticated. Run: mage login")
}
