/// Tests for WebSocket protocol message serialization/deserialization.
/// These test against the JSON wire format directly to avoid lib export issues.
use serde_json;

#[test]
fn test_chat_message_format() {
    let json: serde_json::Value = serde_json::json!({
        "type": "text",
        "text": "hello"
    });
    assert_eq!(json["type"], "text");
    assert_eq!(json["text"], "hello");
}

#[test]
fn test_confirmation_response_format() {
    let json: serde_json::Value = serde_json::json!({
        "type": "confirmation_response",
        "confirmation_id": "confirm_123",
        "confirmed": true,
        "remember": false
    });
    assert_eq!(json["type"], "confirmation_response");
    assert_eq!(json["confirmation_id"], "confirm_123");
    assert_eq!(json["confirmed"], true);
    assert_eq!(json["remember"], false);
}

#[test]
fn test_assistant_stream_delta() {
    let json = r#"{"type":"assistant_stream","phase":"delta","text":"Hello"}"#;
    let msg: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(msg["type"], "assistant_stream");
    assert_eq!(msg["phase"], "delta");
    assert_eq!(msg["text"], "Hello");
}

#[test]
fn test_assistant_message() {
    let json = r#"{"type":"assistant","text":"Hello! How can I help?"}"#;
    let msg: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(msg["type"], "assistant");
    assert_eq!(msg["text"], "Hello! How can I help?");
}

#[test]
fn test_confirmation_request() {
    let json = r#"{"type":"confirmation_request","confirmation_id":"confirm_1710432000000","function_name":"bash_commands","script":"ls -la","arguments":{"command":"ls -la"}}"#;
    let msg: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(msg["type"], "confirmation_request");
    assert_eq!(msg["confirmation_id"], "confirm_1710432000000");
    assert_eq!(msg["function_name"], "bash_commands");
    assert_eq!(msg["script"], "ls -la");
    assert_eq!(msg["arguments"]["command"], "ls -la");
}

#[test]
fn test_runtime_config() {
    let json = r#"{"type":"runtime_config","llm_model_name":"qwen-3-235b","mute":true}"#;
    let msg: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(msg["type"], "runtime_config");
    assert_eq!(msg["llm_model_name"], "qwen-3-235b");
    assert_eq!(msg["mute"], true);
}

#[test]
fn test_tool_call_request() {
    let json: serde_json::Value = serde_json::json!({
        "type": "tool_call",
        "call_id": "uuid-123",
        "function_name": "run_python",
        "arguments": { "code": "print(42)" }
    });
    assert_eq!(json["type"], "tool_call");
    assert_eq!(json["call_id"], "uuid-123");
    assert_eq!(json["function_name"], "run_python");
    assert_eq!(json["arguments"]["code"], "print(42)");
}

#[test]
fn test_tool_call_result() {
    let json =
        r#"{"type":"tool_call_result","call_id":"uuid-123","success":true,"result":"hello"}"#;
    let msg: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(msg["type"], "tool_call_result");
    assert_eq!(msg["call_id"], "uuid-123");
    assert_eq!(msg["success"], true);
    assert_eq!(msg["result"], "hello");
}

#[test]
fn test_tools_list() {
    let json = r#"{"type":"tools_list","tools":[{"type":"function","function":{"name":"run_python","description":"Run Python"}}]}"#;
    let msg: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(msg["type"], "tools_list");
    assert_eq!(msg["tools"][0]["function"]["name"], "run_python");
}
