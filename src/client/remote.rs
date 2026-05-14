use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

pub struct RemoteClient {
    client: Client,
    gateway_url: String,
    token: String,
}

impl RemoteClient {
    pub fn new(gateway_url: &str, token: &str) -> Self {
        Self {
            client: Client::new(),
            gateway_url: gateway_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    #[allow(dead_code)]
    pub fn gateway_url(&self) -> &str {
        &self.gateway_url
    }

    /// List available models
    pub async fn list_models(&self) -> Result<Value> {
        self.get("/v1/models").await
    }

    /// Get usage summary
    pub async fn usage_summary(&self) -> Result<Value> {
        self.get("/v1/usage/summary").await
    }

    /// Get balance
    pub async fn balance(&self) -> Result<Value> {
        self.get("/v1/usage/balance").await
    }

    /// List API keys
    pub async fn list_keys(&self) -> Result<Value> {
        self.post("/v1/list-api-keys", &json!({})).await
    }

    /// Generate a new API key
    pub async fn generate_key(&self) -> Result<Value> {
        self.post("/v1/generate-api-key", &json!({})).await
    }

    /// Revoke an API key
    pub async fn revoke_key(&self, key_id: &str) -> Result<Value> {
        self.post("/v1/revoke-api-key", &json!({"key_id": key_id}))
            .await
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.gateway_url, path);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .with_context(|| format!("Request failed: GET {}", path))?;
        resp.json()
            .await
            .with_context(|| format!("Failed to parse response from {}", path))
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let url = format!("{}{}", self.gateway_url, path);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(body)
            .send()
            .await
            .with_context(|| format!("Request failed: POST {}", path))?;
        resp.json()
            .await
            .with_context(|| format!("Failed to parse response from {}", path))
    }
}
