use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::messages::{IncomingMessage, OutgoingMessage};

pub struct LocalClient {
    ws_url: String,
}

impl LocalClient {
    pub fn new(local_url: &str) -> Self {
        let ws_url = local_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        let ws_url = format!("{}/ws", ws_url.trim_end_matches('/'));
        Self { ws_url }
    }

    /// Connect and return split sink/stream for the caller to drive
    pub async fn connect(
        &self,
    ) -> Result<(
        impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error>,
        impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>,
    )> {
        let (ws_stream, _) = connect_async(&self.ws_url)
            .await
            .context("Failed to connect to local backend WebSocket")?;

        Ok(ws_stream.split())
    }
}

/// Serialize an outgoing message to a WebSocket text frame
pub fn encode_message(msg: &OutgoingMessage) -> Result<Message> {
    let json = serde_json::to_string(msg)?;
    Ok(Message::Text(json))
}

/// Deserialize an incoming WebSocket text frame
pub fn decode_message(msg: &Message) -> Result<Option<IncomingMessage>> {
    match msg {
        Message::Text(text) => {
            let incoming: IncomingMessage = serde_json::from_str(text).with_context(|| {
                format!(
                    "Failed to parse WS message: {}",
                    &text[..text.len().min(200)]
                )
            })?;
            Ok(Some(incoming))
        }
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => anyhow::bail!("WebSocket connection closed by server"),
        _ => Ok(None),
    }
}
