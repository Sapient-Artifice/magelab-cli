use anyhow::Result;
use serde_json::{json, Map, Value};
use std::io::Write;

use crate::{client::headless::HeadlessClient, config::Config};

#[derive(Debug)]
pub struct HeadlessExitError {
    pub code: i32,
    message: String,
}

impl std::fmt::Display for HeadlessExitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for HeadlessExitError {}

pub fn process_exit_code(error: &anyhow::Error) -> Option<i32> {
    error
        .downcast_ref::<HeadlessExitError>()
        .map(|error| error.code)
}

pub enum SessionsCommand {
    List {
        json: bool,
    },
    Get {
        session_id: i64,
        json: bool,
    },
    Create {
        name: String,
        model: Option<String>,
        mcps: Vec<String>,
        json: bool,
    },
    Update {
        session_id: i64,
        name: Option<String>,
        model: Option<String>,
        mcps: Vec<String>,
        json: bool,
    },
}

pub enum ChatsCommand {
    List {
        session_id: Option<i64>,
        json: bool,
    },
    Create {
        session_id: i64,
        json: bool,
    },
    Switch {
        session_id: i64,
        chat_id: i64,
        json: bool,
    },
}

pub struct AskCommand {
    pub prompt: String,
    pub session_id: i64,
    pub chat_id: Option<i64>,
    pub new_chat: bool,
    pub model: Option<String>,
    pub system_message: Option<String>,
    pub developer_instructions: Option<String>,
    pub mcps: Vec<String>,
    pub json: bool,
    pub jsonl: bool,
}

pub async fn sessions(config: &Config, command: SessionsCommand) -> Result<()> {
    let mut client = HeadlessClient::resolve_and_connect(config, false).await?;
    let result = match command {
        SessionsCommand::List { json } => {
            let value = client.list_sessions().await?;
            print_value(&value, json, "sessions");
            Ok(())
        }
        SessionsCommand::Get { session_id, json } => {
            let value = client.get_session(session_id).await?;
            print_value(&value, json, "session");
            Ok(())
        }
        SessionsCommand::Create {
            name,
            model,
            mcps,
            json: json_output,
        } => {
            let state = session_state(model, mcps);
            let value = client.create_session(&name, state).await?;
            print_value(&value, json_output, "session created");
            Ok(())
        }
        SessionsCommand::Update {
            session_id,
            name,
            model,
            mcps,
            json: json_output,
        } => {
            let mut patch = Map::new();
            if let Some(name) = name {
                patch.insert("name".into(), json!(name));
            }
            if let Some(state) = session_state(model, mcps) {
                patch.insert("state".into(), state);
            }
            if patch.is_empty() {
                anyhow::bail!("Session update requires --name, --model, or --mcp");
            }
            let value = client
                .update_session(session_id, Value::Object(patch))
                .await?;
            print_value(&value, json_output, "session updated");
            Ok(())
        }
    };
    client.close().await;
    result
}

pub async fn chats(config: &Config, command: ChatsCommand) -> Result<()> {
    let mut client = HeadlessClient::resolve_and_connect(config, false).await?;
    let result = match command {
        ChatsCommand::List { session_id, json } => {
            if let Some(session_id) = session_id {
                client
                    .write_runtime_state(json!({"session_id": session_id}))
                    .await?;
            }
            let value = client.list_chats().await?;
            print_value(&value, json, "chats");
            Ok(())
        }
        ChatsCommand::Create { session_id, json } => {
            client
                .write_runtime_state(json!({"session_id": session_id}))
                .await?;
            let value = client.create_chat().await?;
            print_value(&value, json, "chat created");
            Ok(())
        }
        ChatsCommand::Switch {
            session_id,
            chat_id,
            json,
        } => {
            client
                .write_runtime_state(json!({
                    "session_id": session_id,
                    "chat_id": chat_id,
                }))
                .await?;
            let value = client.switch_chat(chat_id).await?;
            print_value(&value, json, "chat switched");
            Ok(())
        }
    };
    client.close().await;
    result
}

pub async fn ask(config: &Config, command: AskCommand) -> Result<()> {
    if command.new_chat && command.chat_id.is_some() {
        return Err(exit_error(2, "--new-chat conflicts with --chat"));
    }
    if !command.new_chat && command.chat_id.is_none() {
        return Err(exit_error(2, "Provide --chat <id> or --new-chat"));
    }

    let mut client = HeadlessClient::resolve_and_connect(config, false)
        .await
        .map_err(|error| exit_error(3, error))?;
    let mut state = Map::from_iter([("session_id".into(), json!(command.session_id))]);
    if let Some(chat_id) = command.chat_id {
        state.insert("chat_id".into(), json!(chat_id));
    }
    if let Some(model) = &command.model {
        state.insert("llm_model_name".into(), json!(model));
    }
    if let Some(system_message) = &command.system_message {
        state.insert("system_message".into(), json!(system_message));
    }
    if let Some(instructions) = &command.developer_instructions {
        state.insert("developer_instructions".into(), json!(instructions));
    }
    if !command.mcps.is_empty() {
        state.insert("mcps".into(), json!({"enabled_servers": command.mcps}));
    }

    let chat_id = client
        .prepare_conversation(Value::Object(state), command.new_chat, command.chat_id)
        .await
        .map_err(|error| exit_error(classify_operation_error(&error), error))?;

    let jsonl = command.jsonl;
    let human = !command.json && !jsonl;
    let result = client
        .run_turn(&command.prompt, |event| {
            if jsonl && event.get("type").and_then(Value::as_str) != Some("assistant_complete") {
                println!("{}", serde_json::to_string(&jsonl_event(event)).unwrap());
            } else if human {
                print_human_event(event);
            }
        })
        .await
        .map_err(|error| exit_error(classify_operation_error(&error), error))?;

    if human {
        println!();
    } else if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "client_request_id": result.client_request_id,
                "session_id": command.session_id,
                "chat_id": chat_id,
                "status": result.status,
                "text": result.text,
                "code": result.code,
                "error": result.error,
            }))?
        );
    } else {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "event": "assistant_complete",
                "client_request_id": result.client_request_id,
                "session_id": command.session_id,
                "chat_id": chat_id,
                "status": result.status,
                "text": result.text,
                "code": result.code,
                "error": result.error,
            }))?
        );
    }
    client.close().await;

    match result.status.as_str() {
        "completed" => Ok(()),
        "cancelled" => Err(exit_error(6, "Mage assistant turn was cancelled")),
        "error" => Err(exit_error(
            7,
            format!(
                "Mage assistant turn failed{}: {}",
                result
                    .code
                    .as_deref()
                    .map(|code| format!(" ({code})"))
                    .unwrap_or_default(),
                result.error.as_deref().unwrap_or("unknown error")
            ),
        )),
        status => Err(exit_error(
            7,
            format!("Mage returned unknown assistant status: {status}"),
        )),
    }
}

pub async fn storage_health(config: &Config, json_output: bool) -> Result<()> {
    let mut client = HeadlessClient::resolve_and_connect(config, true).await?;
    let value = client.storage_health().await?;
    print_value(&value, json_output, "storage health");
    client.close().await;
    Ok(())
}

pub fn protocol_capabilities(json_output: bool) -> Result<()> {
    let value = json!({
        "schema": "magelab-websocket-v0.12",
        "client": {
            "request_correlation": true,
            "assistant_turn_correlation": true,
            "terminal_status": ["completed", "cancelled", "error"],
            "runtime_state_write": true,
            "session_mcp_state": "mcps.enabled_servers",
            "automatic_turn_replay": false,
            "concurrency_mode": "serialized_global_runtime"
        },
        "required_backend_events": [
            "runtime_state_write_result",
            "new_chat_result",
            "chat_switch_result",
            "assistant_complete"
        ]
    });
    print_value(&value, json_output, "protocol capabilities");
    Ok(())
}

fn session_state(model: Option<String>, mcps: Vec<String>) -> Option<Value> {
    let mut state = Map::new();
    if let Some(model) = model {
        state.insert("provider".into(), json!({"llm_model_name": model}));
    }
    if !mcps.is_empty() {
        state.insert("mcps".into(), json!({"enabled_servers": mcps}));
    }
    (!state.is_empty()).then_some(Value::Object(state))
}

fn classify_operation_error(error: &anyhow::Error) -> i32 {
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("timed out") {
        5
    } else if message.contains("connect") || message.contains("websocket closed") {
        3
    } else {
        4
    }
}

fn exit_error(code: i32, message: impl std::fmt::Display) -> anyhow::Error {
    HeadlessExitError {
        code,
        message: message.to_string(),
    }
    .into()
}

fn print_value(value: &Value, json_output: bool, label: &str) {
    if json_output {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    } else {
        println!(
            "{}: {}",
            label,
            serde_json::to_string_pretty(value).unwrap()
        );
    }
}

fn jsonl_event(event: &Value) -> Value {
    let mut output = event.clone();
    if let Some(object) = output.as_object_mut() {
        let normalized = match event.get("type").and_then(Value::as_str) {
            Some("assistant_stream")
                if event.get("phase").and_then(Value::as_str) == Some("delta") =>
            {
                "assistant_delta"
            }
            Some(kind) => kind,
            None => "unknown",
        };
        object.insert("event".into(), json!(normalized));
        object.remove("type");
    }
    output
}

fn print_human_event(event: &Value) {
    let text = match event.get("type").and_then(Value::as_str) {
        Some("assistant_stream") if event.get("phase").and_then(Value::as_str) == Some("delta") => {
            event
                .get("token")
                .or_else(|| event.get("text"))
                .or_else(|| event.get("content"))
                .and_then(Value::as_str)
        }
        Some("assistant") => event.get("text").and_then(Value::as_str),
        _ => None,
    };
    if let Some(text) = text {
        print!("{text}");
        std::io::stdout().flush().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_uses_canonical_mcp_shape() {
        assert_eq!(
            session_state(None, vec!["pipedrive".into()]),
            Some(json!({"mcps": {"enabled_servers": ["pipedrive"]}}))
        );
    }

    #[test]
    fn jsonl_delta_has_stable_event_name() {
        assert_eq!(
            jsonl_event(&json!({
                "type": "assistant_stream",
                "phase": "delta",
                "token": "hello",
                "client_request_id": "turn-1"
            })),
            json!({
                "event": "assistant_delta",
                "phase": "delta",
                "token": "hello",
                "client_request_id": "turn-1"
            })
        );
    }

    #[test]
    fn headless_exit_codes_are_preserved_through_anyhow() {
        let error = exit_error(6, "cancelled");
        assert_eq!(process_exit_code(&error), Some(6));
    }

    #[test]
    fn operation_errors_distinguish_timeout_connection_and_setup() {
        assert_eq!(classify_operation_error(&anyhow::anyhow!("timed out")), 5);
        assert_eq!(
            classify_operation_error(&anyhow::anyhow!("WebSocket closed")),
            3
        );
        assert_eq!(
            classify_operation_error(&anyhow::anyhow!("chat rejected")),
            4
        );
    }
}
