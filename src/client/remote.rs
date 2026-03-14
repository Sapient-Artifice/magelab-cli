use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

pub struct RemoteClient {
    client: Client,
    gateway_url: String,
    api_key: String,
}

impl RemoteClient {
    pub fn new(gateway_url: &str, api_key: &str) -> Self {
        Self {
            client: Client::new(),
            gateway_url: gateway_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }

    #[allow(dead_code)]
    pub fn gateway_url(&self) -> &str {
        &self.gateway_url
    }

    /// Send a chat completion request with SSE streaming
    pub async fn chat_stream(
        &self,
        messages: &[(String, String)],
        model: &str,
    ) -> Result<reqwest::Response> {
        let body = build_chat_body(messages, model, true);
        let url = format!("{}/v1/chat/completions", self.gateway_url);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send chat request to gateway")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Gateway returned {}: {}", status, body);
        }

        Ok(resp)
    }

    /// List available models
    pub async fn list_models(&self) -> Result<Value> {
        let url = format!("{}/v1/models", self.gateway_url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .context("Failed to fetch models")?;
        resp.json().await.context("Failed to parse models response")
    }

    /// Get usage summary
    pub async fn usage_summary(&self) -> Result<Value> {
        let url = format!("{}/v1/usage/summary", self.gateway_url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;
        resp.json().await.context("Failed to parse usage response")
    }

    /// Get balance
    pub async fn balance(&self) -> Result<Value> {
        let url = format!("{}/v1/usage/balance", self.gateway_url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;
        resp.json()
            .await
            .context("Failed to parse balance response")
    }

    /// List API keys
    pub async fn list_keys(&self) -> Result<Value> {
        let url = format!("{}/v1/list-api-keys", self.gateway_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({}))
            .send()
            .await?;
        resp.json().await.context("Failed to parse keys response")
    }

    /// Generate a new API key
    pub async fn generate_key(&self) -> Result<Value> {
        let url = format!("{}/v1/generate-api-key", self.gateway_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({}))
            .send()
            .await?;
        resp.json().await.context("Failed to parse key response")
    }

    /// Revoke an API key
    pub async fn revoke_key(&self, key_id: &str) -> Result<Value> {
        let url = format!("{}/v1/revoke-api-key", self.gateway_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({"key_id": key_id}))
            .send()
            .await?;
        resp.json().await.context("Failed to parse revoke response")
    }
}

/// Build a chat completion request body
pub fn build_chat_body(messages: &[(String, String)], model: &str, stream: bool) -> Value {
    let msgs: Vec<Value> = messages
        .iter()
        .map(|(role, content)| json!({"role": role, "content": content}))
        .collect();

    json!({
        "model": model,
        "messages": msgs,
        "stream": stream,
    })
}

/// Parse a single SSE data line into a delta token
pub fn parse_sse_delta(line: &str) -> Option<String> {
    let data = line.strip_prefix("data: ")?;
    if data == "[DONE]" {
        return None;
    }
    let v: Value = serde_json::from_str(data).ok()?;
    v["choices"][0]["delta"]["content"]
        .as_str()
        .map(String::from)
}
