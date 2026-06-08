use anyhow::Result;

/// Push a MageLab access token into a running local backend.
pub async fn push_access_token_to_backend(local_url: &str, access_token: &str) -> Result<()> {
    let url = format!("{}/api/auth/set_token", local_url.trim_end_matches('/'));
    let body = serde_json::json!({ "access_token": access_token });

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Backend not running. Start it with `mage launch`"))?;

    if resp.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("Auth token push failed with status {}", resp.status())
    }
}
