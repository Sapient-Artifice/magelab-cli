use magelab_cli::client::messages::*;

#[test]
fn test_serialize_chat_request() {
    let msg = OutgoingMessage::Chat {
        text: "hello".into(),
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["type"], "text");
    assert_eq!(json["text"], "hello");
}

#[test]
fn test_deserialize_stream_delta() {
    let json = r#"{"type":"assistant_stream","phase":"delta","text":"Hello"}"#;
    let msg: IncomingMessage = serde_json::from_str(json).unwrap();
    match msg {
        IncomingMessage::AssistantStream { phase, text, .. } => {
            assert_eq!(phase, "delta");
            assert_eq!(text.unwrap(), "Hello");
        }
        _ => panic!("Expected AssistantStream"),
    }
}

#[test]
fn test_deserialize_assistant_complete() {
    let json = r#"{"type":"assistant","text":"Hello! How can I help?"}"#;
    let msg: IncomingMessage = serde_json::from_str(json).unwrap();
    match msg {
        IncomingMessage::Assistant { text } => {
            assert_eq!(text.unwrap(), "Hello! How can I help?");
        }
        _ => panic!("Expected Assistant"),
    }
}

#[test]
fn test_deserialize_assistant_complete_signal() {
    let json = r#"{"type":"assistant_complete"}"#;
    let msg: IncomingMessage = serde_json::from_str(json).unwrap();
    assert!(matches!(msg, IncomingMessage::AssistantComplete { .. }));
}

#[test]
fn test_deserialize_confirmation_request_with_id() {
    let json = r#"{"type":"confirmation_request","confirmation_id":"confirm_1710432000000","function_name":"bash_commands","script":"ls -la","arguments":{"command":"ls -la"}}"#;
    let msg: IncomingMessage = serde_json::from_str(json).unwrap();
    match msg {
        IncomingMessage::ConfirmationRequest {
            confirmation_id,
            function_name,
            script,
            arguments,
        } => {
            assert_eq!(confirmation_id, "confirm_1710432000000");
            assert_eq!(function_name, "bash_commands");
            assert_eq!(script.unwrap(), "ls -la");
            assert!(arguments.contains_key("command"));
        }
        _ => panic!("Expected ConfirmationRequest"),
    }
}

#[test]
fn test_serialize_confirmation_response_with_id() {
    let msg = OutgoingMessage::ConfirmationResponse {
        confirmation_id: "confirm_1710432000000".into(),
        confirmed: true,
        remember: false,
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["type"], "confirmation_response");
    assert_eq!(json["confirmation_id"], "confirm_1710432000000");
    assert_eq!(json["confirmed"], true);
    assert_eq!(json["remember"], false);
}

#[test]
fn test_deserialize_runtime_config() {
    let json = r#"{"type":"runtime_config","llm_model_name":"qwen-3-235b","mute":true}"#;
    let msg: IncomingMessage = serde_json::from_str(json).unwrap();
    match msg {
        IncomingMessage::RuntimeConfig(config) => {
            assert_eq!(
                config.get("llm_model_name").and_then(|v| v.as_str()),
                Some("qwen-3-235b")
            );
        }
        _ => panic!("Expected RuntimeConfig"),
    }
}
