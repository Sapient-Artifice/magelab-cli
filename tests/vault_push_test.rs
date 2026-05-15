use std::collections::HashMap;

use magelab_cli::vault;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_secrets() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("llm_api_key".to_string(), "sk-test-123".to_string());
    m.insert("magelab_api_key".to_string(), "mage_test".to_string());
    m
}

#[tokio::test]
async fn test_push_secrets_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/push_secrets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let result = vault::push_secrets_to_backend(&server.uri(), &test_secrets()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_push_secrets_server_error_returns_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/push_secrets"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    let result = vault::push_secrets_to_backend(&server.uri(), &test_secrets()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("500"));
}

#[tokio::test]
async fn test_push_secrets_backend_not_running() {
    // Point at an address that will refuse connections
    let result = vault::push_secrets_to_backend("http://127.0.0.1:1", &test_secrets()).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Backend not running"));
}

#[tokio::test]
async fn test_push_secrets_sends_correct_payload() {
    let server = MockServer::start().await;
    let secrets = test_secrets();
    let expected_body = serde_json::json!({ "secrets": secrets });

    Mock::given(method("POST"))
        .and(path("/api/auth/push_secrets"))
        .and(body_json(&expected_body))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let result = vault::push_secrets_to_backend(&server.uri(), &secrets).await;
    assert!(result.is_ok());
    // wiremock will panic on drop if the expected body didn't match
}

#[tokio::test]
async fn test_push_secrets_403_returns_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/push_secrets"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;

    let result = vault::push_secrets_to_backend(&server.uri(), &test_secrets()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("403"));
}
