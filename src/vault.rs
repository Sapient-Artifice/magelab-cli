use anyhow::Result;

use crate::analytics;
use crate::auth;
use crate::config::Config;
use crate::ui;

/// Vault subcommand actions
pub enum VaultAction {
    /// List all vault keys
    List,
    /// Print a secret value to stdout
    Get { key: String },
    /// Push all vault secrets to the running local backend
    Push,
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
            if let Ok(creds) = auth::credentials::Credentials::load() {
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
        VaultAction::Push => {
            auth::touchid::verify(auth::touchid::Tier::Sensitive, "push vault secrets")?;
            push_secrets(config).await
        }
    }
}

pub async fn push_secrets(config: &Config) -> Result<()> {
    let vault = magelab_core::vault::Vault::open()?;
    let secrets = vault.all_secrets()?;

    if secrets.is_empty() {
        println!("No secrets in vault to push.");
        return Ok(());
    }

    push_secrets_to_backend(&config.local_url, &secrets).await
}

/// Push a set of secrets to the backend's push_secrets endpoint.
/// Separated from push_secrets() for testability.
pub async fn push_secrets_to_backend(
    local_url: &str,
    secrets: &std::collections::HashMap<String, String>,
) -> Result<()> {
    let url = format!("{}/api/auth/push_secrets", local_url);
    let body = serde_json::json!({ "secrets": secrets });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Backend not running. Start it with `mage launch`"))?;

    if resp.status().is_success() {
        ui::success(&format!("Pushed {} secret(s) to backend", secrets.len()));
        Ok(())
    } else {
        anyhow::bail!("Push failed with status {}", resp.status())
    }
}
