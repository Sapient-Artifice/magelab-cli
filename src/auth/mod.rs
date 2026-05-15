pub mod credentials;
pub mod oauth;
pub mod touchid;

use anyhow::Result;

use crate::config::Config;

/// Get the best available auth token.
///
/// Fallback chain: JWT → refresh → biometric refresh → vault → MAGELAB_API_KEY env var → error
pub async fn get_token(config: &Config) -> Result<String> {
    let creds = credentials::Credentials::load().unwrap_or_default();
    if let Some(token) = creds.try_get_valid_jwt(&config.gateway_url).await {
        return Ok(token);
    }
    if creds.access_token.is_some() {
        eprintln!("Warning: JWT expired and refresh failed. Falling back to API key.");
    }

    // Try vault
    match magelab_core::vault::Vault::open() {
        Ok(v) => {
            if let Ok(Some(key)) = v.get("magelab_api_key") {
                return Ok(key);
            }
        }
        Err(_) => {
            // Vault unavailable is expected — many users won't have the desktop installed
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
