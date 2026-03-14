use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[allow(dead_code)]
/// Stored authentication credentials
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Credentials {
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub user_id: Option<String>,
}

#[allow(dead_code)]
impl Credentials {
    /// Path to credentials file: ~/.config/magelab/credentials.json
    pub fn path() -> Result<PathBuf> {
        let base = dirs::config_dir().context("Could not determine config directory")?;
        Ok(base.join("magelab").join("credentials.json"))
    }

    /// Load credentials from disk, returning default if missing
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let creds: Credentials =
            serde_json::from_str(&contents).with_context(|| "Failed to parse credentials")?;
        Ok(creds)
    }

    /// Save credentials to disk
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        let dir = path.parent().context("Invalid credentials path")?;
        std::fs::create_dir_all(dir)?;
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;
        Ok(())
    }

    /// Clear stored credentials
    pub fn clear() -> Result<()> {
        let path = Self::path()?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Check if we have a JWT (may be expired)
    pub fn has_token(&self) -> bool {
        self.access_token.is_some()
    }

    /// Check if the token is likely still valid (with 60s buffer)
    pub fn is_token_valid(&self) -> bool {
        match (&self.access_token, self.expires_at) {
            (Some(_), Some(exp)) => {
                let now = chrono::Utc::now().timestamp();
                exp > now + 60
            }
            (Some(_), None) => true, // No expiry info, assume valid
            _ => false,
        }
    }
}
