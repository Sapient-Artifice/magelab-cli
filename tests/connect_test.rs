use serde_json::Value;

#[test]
fn test_connect_result_serializes_local() {
    let result = magelab_cli::connect::ConnectResult {
        url: Some("ws://127.0.0.1:11115/ws".to_string()),
        token: None,
        mode: "local".to_string(),
        model: Some("qwen-3-235b".to_string()),
    };
    let json: Value = serde_json::to_value(&result).unwrap();
    assert_eq!(json["mode"], "local");
    assert_eq!(json["url"], "ws://127.0.0.1:11115/ws");
    assert!(json["token"].is_null());
}

#[test]
fn test_connect_result_serializes_relay() {
    let result = magelab_cli::connect::ConnectResult {
        url: Some("wss://api.magelab.ai/v1/realtime/portal/ws?ws_ticket=abc".to_string()),
        token: Some("eyJ...".to_string()),
        mode: "relay".to_string(),
        model: None,
    };
    let json: Value = serde_json::to_value(&result).unwrap();
    assert_eq!(json["mode"], "relay");
    assert!(json["token"].is_string());
}

#[test]
fn test_connect_result_serializes_remote() {
    let result = magelab_cli::connect::ConnectResult {
        url: Some("https://api.magelab.ai".to_string()),
        token: Some("mage_abc123".to_string()),
        mode: "remote".to_string(),
        model: None,
    };
    let json: Value = serde_json::to_value(&result).unwrap();
    assert_eq!(json["mode"], "remote");
}

#[test]
fn test_connect_result_serializes_none() {
    let result = magelab_cli::connect::ConnectResult {
        url: None,
        token: None,
        mode: "none".to_string(),
        model: None,
    };
    let json: Value = serde_json::to_value(&result).unwrap();
    assert_eq!(json["mode"], "none");
    assert!(json["url"].is_null());
}

#[test]
fn test_ws_to_http_url() {
    assert_eq!(
        magelab_cli::connect::direct_ws_to_http_url("ws://127.0.0.1:8787/ws").unwrap(),
        "http://127.0.0.1:8787"
    );
    assert_eq!(
        magelab_cli::connect::direct_ws_to_http_url("wss://example.com/ws").unwrap(),
        "https://example.com"
    );
}

#[test]
fn test_direct_ws_to_http_url_rejects_relay_url() {
    let err = magelab_cli::connect::direct_ws_to_http_url(
        "wss://api.magelab.ai/v1/realtime/portal/ws?ws_ticket=abc",
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("directly to a backend /ws endpoint"));
}
