use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Map, Value};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{config::Config, connect};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const TURN_TIMEOUT: Duration = Duration::from_secs(120);
const CANCELLATION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct TurnResult {
    pub client_request_id: String,
    pub status: String,
    pub text: String,
    pub code: Option<String>,
    pub error: Option<String>,
}

pub struct HeadlessClient {
    socket: Socket,
    http_base_url: Option<String>,
    token: Option<String>,
    http: reqwest::Client,
}

impl HeadlessClient {
    pub async fn resolve_and_connect(config: &Config, no_launch: bool) -> Result<Self> {
        let resolved = connect::resolve(config, no_launch).await?;
        let ws_url = resolved
            .url
            .filter(|_| matches!(resolved.mode.as_str(), "local" | "relay"))
            .ok_or_else(|| {
                anyhow::anyhow!("No headless Mage WebSocket is available. Run: mage launch --wait")
            })?;
        let http_base_url = (resolved.mode == "local").then(|| connect::ws_to_http_url(&ws_url));
        Self::connect(&ws_url, resolved.token, http_base_url).await
    }

    pub async fn connect(
        ws_url: &str,
        token: Option<String>,
        http_base_url: Option<String>,
    ) -> Result<Self> {
        let authenticated_url = append_token(ws_url, token.as_deref())?;
        let (socket, _) = connect_async(&authenticated_url)
            .await
            .with_context(|| format!("Could not connect to Mage WebSocket at {ws_url}"))?;
        Ok(Self {
            socket,
            http_base_url,
            token,
            http: reqwest::Client::new(),
        })
    }

    pub async fn close(&mut self) {
        self.socket.close(None).await.ok();
    }

    pub async fn list_sessions(&self) -> Result<Value> {
        self.http_json(reqwest::Method::GET, "/api/sessions", None)
            .await
    }

    pub async fn get_session(&self, session_id: i64) -> Result<Value> {
        ensure_positive(session_id, "session id")?;
        let payload = self.list_sessions().await?;
        payload
            .get("sessions")
            .and_then(Value::as_array)
            .and_then(|sessions| {
                sessions
                    .iter()
                    .find(|session| session.get("id").and_then(Value::as_i64) == Some(session_id))
            })
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Mage session {session_id} was not found"))
    }

    pub async fn create_session(&self, name: &str, state: Option<Value>) -> Result<Value> {
        let mut body = Map::from_iter([("name".to_string(), json!(name))]);
        if let Some(state) = &state {
            body.insert("state".to_string(), state.clone());
        }
        let result = self
            .http_json(
                reqwest::Method::POST,
                "/api/sessions",
                Some(Value::Object(body)),
            )
            .await?;
        if let Some(state) = &state {
            ensure_session_state_applied(&result, state)?;
        }
        Ok(result)
    }

    pub async fn update_session(&self, session_id: i64, patch: Value) -> Result<Value> {
        ensure_positive(session_id, "session id")?;
        let requested_state = patch.get("state").cloned();
        let result = self
            .http_json(
                reqwest::Method::PATCH,
                &format!("/api/sessions/{session_id}"),
                Some(patch),
            )
            .await?;
        if let Some(state) = &requested_state {
            ensure_session_state_applied(&result, state)?;
        }
        Ok(result)
    }

    pub async fn storage_health(&self) -> Result<Value> {
        self.http_json(reqwest::Method::GET, "/health", None).await
    }

    pub async fn list_chats(&mut self) -> Result<Value> {
        self.send_json(&json!({"type": "get_chats"})).await?;
        self.wait_for_type("chat_list_result", OPERATION_TIMEOUT)
            .await
    }

    pub async fn write_runtime_state(&mut self, state: Value) -> Result<Value> {
        let request_id = new_id();
        let result = self
            .request_by_id(
                json!({
                    "type": "write_runtime_state",
                    "request_id": request_id,
                    "state": state,
                }),
                "runtime_state_write_result",
                &request_id,
            )
            .await?;
        ensure_ok(&result, "Runtime state write")?;
        if let Some(warnings) = result.get("warnings").and_then(Value::as_array) {
            if !warnings.is_empty() {
                anyhow::bail!(
                    "Runtime state was saved with reconciliation warnings: {}",
                    warnings
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("; ")
                );
            }
        }
        Ok(result)
    }

    pub async fn create_chat(&mut self) -> Result<Value> {
        let request_id = new_id();
        let result = self
            .request_by_id(
                json!({"type": "new_chat", "request_id": request_id}),
                "new_chat_result",
                &request_id,
            )
            .await?;
        ensure_ok(&result, "Chat creation")?;
        if result.get("chat_id").and_then(Value::as_i64).is_none() {
            anyhow::bail!("Chat creation response omitted chat_id");
        }
        Ok(result)
    }

    pub async fn switch_chat(&mut self, chat_id: i64) -> Result<Value> {
        ensure_positive(chat_id, "chat id")?;
        let request_id = new_id();
        let result = self
            .request_by_id(
                json!({
                    "type": "set_chat",
                    "chat_id": chat_id,
                    "request_id": request_id,
                }),
                "chat_switch_result",
                &request_id,
            )
            .await?;
        ensure_ok(&result, "Chat switch")?;
        Ok(result)
    }

    pub async fn prepare_conversation(
        &mut self,
        state: Value,
        create_chat: bool,
        chat_id: Option<i64>,
    ) -> Result<i64> {
        self.write_runtime_state(state).await?;
        if create_chat {
            return self
                .create_chat()
                .await?
                .get("chat_id")
                .and_then(Value::as_i64)
                .ok_or_else(|| anyhow::anyhow!("Chat creation response omitted chat_id"));
        }
        let chat_id = chat_id.ok_or_else(|| {
            anyhow::anyhow!("A chat id or --new-chat is required for an assistant turn")
        })?;
        self.switch_chat(chat_id).await?;
        Ok(chat_id)
    }

    pub async fn run_turn<F>(&mut self, text: &str, mut on_event: F) -> Result<TurnResult>
    where
        F: FnMut(&Value),
    {
        if text.trim().is_empty() {
            anyhow::bail!("Assistant prompt cannot be empty");
        }
        let client_request_id = new_id();
        self.send_json(&json!({
            "type": "text",
            "text": text,
            "client_request_id": client_request_id,
        }))
        .await?;

        let mut deadline = tokio::time::Instant::now() + TURN_TIMEOUT;
        let mut buffered = String::new();
        let mut cancellation_sent = false;

        loop {
            let next = tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c(), if !cancellation_sent => {
                    cancellation_sent = true;
                    self.send_json(&json!({"type": "control", "action": "stop"})).await?;
                    deadline = tokio::time::Instant::now() + CANCELLATION_TIMEOUT;
                    continue;
                }
                next = tokio::time::timeout_at(deadline, self.socket.next()) => next,
            };

            let raw = next
                .map_err(|_| {
                    if cancellation_sent {
                        anyhow::anyhow!("Assistant cancellation timed out after 10 seconds")
                    } else {
                        anyhow::anyhow!("Assistant turn timed out after 120 seconds")
                    }
                })?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Mage connection closed before assistant completion; turn outcome is unknown"
                    )
                })??;
            let Some(event) = message_json(raw)? else {
                continue;
            };
            let event_type = event.get("type").and_then(Value::as_str);
            let event_id = event.get("client_request_id").and_then(Value::as_str);

            if event_type == Some("assistant_complete") && event_id.is_none() {
                anyhow::bail!(
                    "Mage backend did not correlate assistant_complete; v0.12.0 or newer is required"
                );
            }
            if event_id != Some(client_request_id.as_str()) {
                continue;
            }

            on_event(&event);
            match event_type {
                Some("assistant_stream")
                    if event.get("phase").and_then(Value::as_str) == Some("delta") =>
                {
                    if let Some(delta) = event
                        .get("token")
                        .or_else(|| event.get("text"))
                        .or_else(|| event.get("content"))
                        .and_then(Value::as_str)
                    {
                        buffered.push_str(delta);
                    }
                }
                Some("assistant") => {
                    if let Some(text) = event.get("text").and_then(Value::as_str) {
                        buffered.push_str(text);
                    }
                }
                Some("assistant_complete") => {
                    return Ok(TurnResult {
                        client_request_id,
                        status: event
                            .get("status")
                            .and_then(Value::as_str)
                            .unwrap_or("completed")
                            .to_string(),
                        text: buffered,
                        code: event
                            .get("code")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        error: event
                            .get("error")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    });
                }
                _ => {}
            }
        }
    }

    async fn request_by_id(
        &mut self,
        message: Value,
        expected_type: &str,
        request_id: &str,
    ) -> Result<Value> {
        self.send_json(&message).await?;
        let deadline = tokio::time::Instant::now() + OPERATION_TIMEOUT;
        loop {
            let event = self.next_json(deadline).await?;
            if event.get("type").and_then(Value::as_str) == Some(expected_type) {
                match event.get("request_id").and_then(Value::as_str) {
                    Some(response_id) if response_id == request_id => return Ok(event),
                    None => anyhow::bail!(
                        "Mage backend response {expected_type} did not echo request_id; v0.12.0 or newer is required"
                    ),
                    Some(_) => {}
                }
            }
        }
    }

    async fn wait_for_type(&mut self, expected_type: &str, timeout: Duration) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let event = self.next_json(deadline).await?;
            if event.get("type").and_then(Value::as_str) == Some(expected_type) {
                return Ok(event);
            }
        }
    }

    async fn next_json(&mut self, deadline: tokio::time::Instant) -> Result<Value> {
        loop {
            let raw = tokio::time::timeout_at(deadline, self.socket.next())
                .await
                .map_err(|_| anyhow::anyhow!("Mage operation timed out"))?
                .ok_or_else(|| anyhow::anyhow!("Mage WebSocket closed"))??;
            if let Some(value) = message_json(raw)? {
                return Ok(value);
            }
        }
    }

    async fn send_json(&mut self, value: &Value) -> Result<()> {
        self.socket
            .send(Message::Text(value.to_string()))
            .await
            .context("Failed to send Mage WebSocket message")
    }

    async fn http_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let base = self.http_base_url.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "This operation requires a direct local Mage HTTP connection; relay mode is not supported"
            )
        })?;
        let mut request = self
            .http
            .request(method, format!("{}{}", base.trim_end_matches('/'), path));
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.context("Mage HTTP request failed")?;
        let status = response.status();
        let payload: Value = response
            .json()
            .await
            .context("Mage returned an invalid JSON response")?;
        if !status.is_success() {
            anyhow::bail!(
                "Mage HTTP {}: {}",
                status.as_u16(),
                payload
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("request failed")
            );
        }
        Ok(payload)
    }
}

fn append_token(ws_url: &str, token: Option<&str>) -> Result<String> {
    let mut url = url::Url::parse(ws_url)?;
    if let Some(token) = token {
        url.query_pairs_mut().append_pair("token", token);
    }
    Ok(url.to_string())
}

fn message_json(message: Message) -> Result<Option<Value>> {
    match message {
        Message::Text(text) => Ok(serde_json::from_str(&text).ok()),
        Message::Close(_) => anyhow::bail!("Mage WebSocket closed"),
        Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => Ok(None),
    }
}

fn ensure_ok(value: &Value, operation: &str) -> Result<()> {
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    anyhow::bail!(
        "{} failed{}: {}",
        operation,
        value
            .get("code")
            .and_then(Value::as_str)
            .map(|code| format!(" ({code})"))
            .unwrap_or_default(),
        value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("backend rejected operation")
    )
}

fn ensure_positive(value: i64, field: &str) -> Result<()> {
    if value <= 0 {
        anyhow::bail!("{field} must be a positive integer");
    }
    Ok(())
}

fn ensure_session_state_applied(response: &Value, expected: &Value) -> Result<()> {
    let actual = response.pointer("/session/state").ok_or_else(|| {
        anyhow::anyhow!("Session response omitted state; Mage v0.12.0 or newer is required")
    })?;
    if !contains_partial(actual, expected) {
        anyhow::bail!(
            "Mage did not persist the requested partial session state; v0.12.0 or newer is required"
        );
    }
    Ok(())
}

fn contains_partial(actual: &Value, expected: &Value) -> bool {
    match expected {
        Value::Object(expected) => expected.iter().all(|(key, value)| {
            actual
                .get(key)
                .map(|actual| contains_partial(actual, value))
                .unwrap_or(false)
        }),
        Value::Array(expected) => actual
            .as_array()
            .map(|actual| {
                actual.len() == expected.len()
                    && expected
                        .iter()
                        .zip(actual)
                        .all(|(expected, actual)| contains_partial(actual, expected))
            })
            .unwrap_or(false),
        _ => actual == expected,
    }
}

fn new_id() -> String {
    format!("{:032x}", rand::random::<u128>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_token_without_dropping_existing_ticket() {
        let result = append_token("wss://example.test/ws?ws_ticket=abc", Some("secret")).unwrap();
        let parsed = url::Url::parse(&result).unwrap();
        let query: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
        assert_eq!(query.get("ws_ticket").map(|v| v.as_ref()), Some("abc"));
        assert_eq!(query.get("token").map(|v| v.as_ref()), Some("secret"));
    }

    #[test]
    fn ensure_ok_preserves_backend_code_and_error() {
        let error = ensure_ok(
            &json!({"ok": false, "code": "session_not_found", "error": "missing"}),
            "setup",
        )
        .unwrap_err();
        assert!(error.to_string().contains("session_not_found"));
        assert!(error.to_string().contains("missing"));
    }
}
