use std::collections::HashMap;
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

/// Derive the encryption key using Argon2id, matching tauri-plugin-stronghold's derivation.
///
/// Uses rust-argon2 default config: Argon2id, mem=19MiB, t=2, p=1, hash_length=32.
fn derive_key(password: &str, salt: &[u8]) -> Result<Vec<u8>, VaultError> {
    argon2::hash_raw(password.as_bytes(), salt, &argon2::Config::default())
        .map_err(|e| VaultError::Other(anyhow::anyhow!("argon2 derivation failed: {e}")))
}

impl Vault {
    /// Open an existing vault for reading.
    pub fn open() -> Result<Self, VaultError> {
        Self::open_with_config(VaultConfig::default())
    }

    pub fn open_with_config(config: VaultConfig) -> Result<Self, VaultError> {
        if !config.vault_path.exists() {
            return Err(VaultError::NotFound(config.vault_path));
        }
        if !config.salt_path.exists() {
            return Err(VaultError::SaltNotFound(config.salt_path));
        }

        // Read password from OS keychain
        let password = {
            let entry = keyring::Entry::new(config.keychain_service, config.keychain_username)
                .map_err(|e| VaultError::KeychainUnavailable(format!("keyring init: {e}")))?;
            entry
                .get_password()
                .map_err(|e| VaultError::KeychainUnavailable(format!("keyring get: {e}")))?
        };

        // Read salt and derive key
        let salt = std::fs::read(&config.salt_path)
            .map_err(|e| VaultError::Other(anyhow::anyhow!("read salt: {e}")))?;
        let key = derive_key(&password, &salt)?;

        let key_provider = iota_stronghold::KeyProvider::try_from(Zeroizing::new(key))
            .map_err(|e| VaultError::Other(anyhow::anyhow!("key provider: {e}")))?;
        let snapshot_path = iota_stronghold::SnapshotPath::from_path(&config.vault_path);

        // Verify we can load the snapshot (validates the derived key)
        let stronghold = iota_stronghold::Stronghold::default();
        stronghold
            .load_snapshot(&key_provider, &snapshot_path)
            .map_err(|_| VaultError::DecryptionFailed)?;

        Ok(Self {
            config,
            key_provider,
            snapshot_path,
        })
    }

    /// Get a secret by key name. Returns None if the key doesn't exist in the store.
    ///
    /// Loads a fresh snapshot each call to pick up changes made by the desktop app.
    pub fn get(&self, key: &str) -> Result<Option<String>, VaultError> {
        let stronghold = iota_stronghold::Stronghold::default();
        let client = stronghold
            .load_client_from_snapshot(
                self.config.client_name,
                &self.key_provider,
                &self.snapshot_path,
            )
            .map_err(|e| VaultError::Other(anyhow::anyhow!("load client: {e}")))?;

        let store = client.store();
        match store.get(key.as_bytes()) {
            Ok(Some(data)) => {
                let value = String::from_utf8(data)
                    .map_err(|e| VaultError::Other(anyhow::anyhow!("UTF-8 decode: {e}")))?;
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(trimmed))
                }
            }
            Ok(None) => Ok(None),
            Err(e) => Err(VaultError::Other(anyhow::anyhow!("store get: {e}"))),
        }
    }

    /// List all stored key names (no values).
    pub fn list(&self) -> Result<Vec<String>, VaultError> {
        let all_keys = self.discover_keys()?;
        let stronghold = iota_stronghold::Stronghold::default();
        let client = stronghold
            .load_client_from_snapshot(
                self.config.client_name,
                &self.key_provider,
                &self.snapshot_path,
            )
            .map_err(|e| VaultError::Other(anyhow::anyhow!("load client: {e}")))?;

        let store = client.store();
        let mut present = Vec::new();
        for key in &all_keys {
            if let Ok(Some(data)) = store.get(key.as_bytes()) {
                let value = String::from_utf8_lossy(&data);
                if !value.trim().is_empty() {
                    present.push(key.clone());
                }
            }
        }
        Ok(present)
    }

    /// Read all secrets as key-value pairs.
    pub fn all_secrets(&self) -> Result<HashMap<String, String>, VaultError> {
        let all_keys = self.discover_keys()?;
        let stronghold = iota_stronghold::Stronghold::default();
        let client = stronghold
            .load_client_from_snapshot(
                self.config.client_name,
                &self.key_provider,
                &self.snapshot_path,
            )
            .map_err(|e| VaultError::Other(anyhow::anyhow!("load client: {e}")))?;

        let store = client.store();
        let mut secrets = HashMap::new();
        for key in &all_keys {
            if let Ok(Some(data)) = store.get(key.as_bytes()) {
                if let Ok(value) = String::from_utf8(data) {
                    let trimmed = value.trim().to_string();
                    if !trimmed.is_empty() {
                        secrets.insert(key.clone(), trimmed);
                    }
                }
            }
        }
        Ok(secrets)
    }

    /// Build the full list of keys to probe: static SECRET_KEYS + dynamic keys from __vault_keys.
    fn discover_keys(&self) -> Result<Vec<String>, VaultError> {
        let mut keys: Vec<String> = SECRET_KEYS.iter().map(|s| s.to_string()).collect();

        // Try to read __vault_keys metadata for dynamic provider_api_key:* entries
        let stronghold = iota_stronghold::Stronghold::default();
        if let Ok(client) = stronghold.load_client_from_snapshot(
            self.config.client_name,
            &self.key_provider,
            &self.snapshot_path,
        ) {
            let store = client.store();
            if let Ok(Some(data)) = store.get(b"__vault_keys") {
                if let Ok(json_str) = String::from_utf8(data) {
                    if let Ok(stored_keys) = serde_json::from_str::<Vec<String>>(&json_str) {
                        for k in stored_keys {
                            if !keys.contains(&k) && !k.starts_with("__") && k != "refresh_token" {
                                keys.push(k);
                            }
                        }
                    }
                }
            }
        }

        Ok(keys)
    }
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

    #[test]
    fn derive_key_produces_32_byte_output() {
        let key = derive_key("test-password", b"some-salt-value").unwrap();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn derive_key_is_deterministic() {
        let salt = b"test-salt-value!"; // min 8 bytes for argon2
        let key1 = derive_key("password", salt).unwrap();
        let key2 = derive_key("password", salt).unwrap();
        assert_eq!(key1, key2);
    }

    #[test]
    fn derive_key_different_passwords_produce_different_keys() {
        let salt = b"test-salt-value!";
        let key1 = derive_key("password1", salt).unwrap();
        let key2 = derive_key("password2", salt).unwrap();
        assert_ne!(key1, key2);
    }

    #[test]
    fn open_returns_not_found_when_vault_missing() {
        let config = VaultConfig {
            vault_path: PathBuf::from("/nonexistent/vault.hold"),
            salt_path: PathBuf::from("/nonexistent/salt.txt"),
            keychain_service: "test",
            keychain_username: "test",
            client_name: "auth",
        };
        match Vault::open_with_config(config) {
            Err(VaultError::NotFound(p)) => assert!(p.to_str().unwrap().contains("nonexistent")),
            other => panic!("expected NotFound, got: {:?}", other.err()),
        }
    }

    #[test]
    fn open_returns_salt_not_found_when_vault_exists_but_salt_missing() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault.hold");
        std::fs::write(&vault_path, b"fake-vault-data").unwrap();

        let config = VaultConfig {
            vault_path,
            salt_path: PathBuf::from("/nonexistent/salt.txt"),
            keychain_service: "test",
            keychain_username: "test",
            client_name: "auth",
        };
        match Vault::open_with_config(config) {
            Err(VaultError::SaltNotFound(p)) => {
                assert!(p.to_str().unwrap().contains("nonexistent"))
            }
            other => panic!("expected SaltNotFound, got: {:?}", other.err()),
        }
    }
}
