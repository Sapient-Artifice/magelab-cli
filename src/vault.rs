use std::path::PathBuf;

use zeroize::Zeroizing;

/// Known secret key names stored by the desktop app.
const SECRET_KEYS: &[&str] = &[
    "llm_api_key",
    "whisper_api_key",
    "tts_api_key",
    "vision_api_key",
    "image_api_key",
    "magelab_api_key",
];

/// Tauri app identifier — must match tauri.conf.json.
const APP_ID: &str = "com.magelab.dev";

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("vault file not found: {0}")]
    NotFound(PathBuf),
    #[error("salt file not found: {0}")]
    SaltNotFound(PathBuf),
    #[error("keychain unavailable: {0}")]
    KeychainUnavailable(String),
    #[error("failed to decrypt vault — wrong password or corrupted file")]
    DecryptionFailed,
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

pub struct VaultConfig {
    pub vault_path: PathBuf,
    pub salt_path: PathBuf,
    pub keychain_service: &'static str,
    pub keychain_username: &'static str,
    pub client_name: &'static str,
}

impl Default for VaultConfig {
    fn default() -> Self {
        let data_dir = dirs::data_dir().unwrap_or_default().join(APP_ID);
        let data_local_dir = dirs::data_local_dir().unwrap_or_default().join(APP_ID);
        Self {
            vault_path: data_dir.join("auth-vault.hold"),
            salt_path: data_local_dir.join("stronghold-salt.txt"),
            keychain_service: "magelab.stronghold",
            keychain_username: "vault_password",
            client_name: "auth",
        }
    }
}

pub struct Vault {
    config: VaultConfig,
    key_provider: iota_stronghold::KeyProvider,
    snapshot_path: iota_stronghold::SnapshotPath,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_vault_path_ends_with_auth_vault_hold() {
        let config = VaultConfig::default();
        assert!(config.vault_path.ends_with("auth-vault.hold"));
    }

    #[test]
    fn default_config_salt_path_ends_with_stronghold_salt_txt() {
        let config = VaultConfig::default();
        assert!(config.salt_path.ends_with("stronghold-salt.txt"));
    }

    #[test]
    fn default_config_client_name_is_auth() {
        let config = VaultConfig::default();
        assert_eq!(config.client_name, "auth");
    }

    #[test]
    fn secret_keys_contains_expected_entries() {
        assert!(SECRET_KEYS.contains(&"llm_api_key"));
        assert!(SECRET_KEYS.contains(&"magelab_api_key"));
        assert!(SECRET_KEYS.contains(&"whisper_api_key"));
    }

    #[test]
    fn vault_error_not_found_displays_path() {
        let err = VaultError::NotFound(std::path::PathBuf::from("/tmp/missing.hold"));
        assert!(err.to_string().contains("/tmp/missing.hold"));
    }

    #[test]
    fn vault_error_salt_not_found_displays_path() {
        let err = VaultError::SaltNotFound(std::path::PathBuf::from("/tmp/missing-salt.txt"));
        assert!(err.to_string().contains("/tmp/missing-salt.txt"));
    }
}
