use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::touchid;

const KEYCHAIN_SERVICE: &str = "magelab-cli";
const KEYCHAIN_ACCOUNT: &str = "default";

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
    #[serde(default)]
    pub email: Option<String>,
}

#[allow(dead_code)]
impl Credentials {
    /// Path to credentials file: ~/.config/magelab/credentials.json
    pub fn path() -> Result<PathBuf> {
        let base = dirs::config_dir().context("Could not determine config directory")?;
        Ok(base.join("magelab").join("credentials.json"))
    }

    /// Load credentials — try keychain first, then file fallback
    pub fn load() -> Result<Self> {
        // Try keychain
        if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            if let Ok(json) = entry.get_password() {
                if let Ok(creds) = serde_json::from_str::<Credentials>(&json) {
                    return Ok(creds);
                }
            }
        }

        // File fallback
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

    /// Save credentials — try keychain first, then file fallback
    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string(self)?;

        // Try keychain
        if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            if entry.set_password(&json).is_ok() {
                // If Touch ID is available and we have a refresh token,
                // store it in the biometric-protected keychain item
                if let Some(ref rt) = self.refresh_token {
                    if touchid::is_available() {
                        if let Err(e) = touchid::store_secure(rt) {
                            eprintln!("Warning: Could not store credentials in biometric keychain. Touch ID refresh will not be available. ({})", e);
                        }
                    }
                }
                return Ok(());
            }
        }

        // File fallback
        eprintln!("Warning: No system keychain available — storing tokens in credentials file");
        let path = Self::path()?;
        let dir = path.parent().context("Invalid credentials path")?;
        std::fs::create_dir_all(dir)?;
        std::fs::write(&path, &json)?;

        // Set file permissions to 0600 on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Clear stored credentials from both keychain and file
    pub fn clear() -> Result<()> {
        // Clear Touch ID biometric item and session cache
        touchid::clear()?;

        // Try keychain
        if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            entry.delete_credential().ok();
        }

        // File
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
