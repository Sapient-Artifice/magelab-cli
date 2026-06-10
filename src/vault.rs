use anyhow::Result;

use crate::analytics;
use crate::auth;
use crate::config::Config;

/// Vault subcommand actions
pub enum VaultAction {
    /// List all vault keys
    List,
    /// Print a secret value to stdout
    Get { key: String },
}

pub async fn cmd_vault(config: &mut Config, action: VaultAction) -> Result<()> {
    match action {
        VaultAction::List => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "list vault keys")?;
            let vault = magelab_core::vault::Vault::open().map_err(|e| match e {
                magelab_core::vault::VaultError::NotFound(_) => {
                    anyhow::anyhow!("No vault found. Open the desktop app to create one.")
                }
                magelab_core::vault::VaultError::KeychainUnavailable(_) => anyhow::anyhow!(
                    "Vault exists but no password in keychain. Open the desktop app first, or set MAGELAB_VAULT_PASSWORD env var."
                ),
                other => anyhow::anyhow!("{other}"),
            })?;

            let keys = vault.list()?;
            if keys.is_empty() {
                println!("Vault is empty. Store secrets in the desktop app.");
            } else {
                for key in &keys {
                    println!("{}", key);
                }
            }
            Ok(())
        }
        VaultAction::Get { key } => {
            auth::touchid::verify(auth::touchid::Tier::Sensitive, "read vault secret")?;
            if let Ok(creds) = auth::Credentials::load() {
                if let Some(uid) = &creds.user_id {
                    analytics::track_activation(uid, "vault_get", config).await;
                }
            }
            let vault = magelab_core::vault::Vault::open()?;
            match vault.get(&key)? {
                Some(value) => {
                    print!("{}", value); // No newline — for piping
                    Ok(())
                }
                None => anyhow::bail!("Key '{}' not found in vault", key),
            }
        }
    }
}
