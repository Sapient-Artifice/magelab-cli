use magelab_cli::account;
use magelab_cli::client::remote::RemoteClient;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_list_models_displays_table() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {"id": "qwen-3-235b", "owned_by": "alibaba"},
                {"id": "gpt-4o", "owned_by": "openai"}
            ]
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    let result = account::list_models(&client).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_list_models_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": []
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    let result = account::list_models(&client).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_show_usage_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/usage/summary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_requests": 100,
            "total_tokens_used": 50000,
            "total_cost": 0.5678
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    let result = account::show_usage(&client).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_show_usage_missing_fields() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/usage/summary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    // Should handle missing fields gracefully (no panic)
    let result = account::show_usage(&client).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_show_balance_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/usage/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "available_credit": 9.5,
            "total_credit": 10.0,
            "total_cost": 0.5
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    let result = account::show_balance(&client).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_list_keys_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/list-api-keys"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "api_keys": [
                {"id": "1", "key_preview": "mage_abc...xyz", "is_revoked": false, "created_at": "2026-01-01"},
                {"id": "2", "key_preview": "mage_def...uvw", "is_revoked": true, "created_at": "2026-02-01"}
            ]
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    let result = account::list_keys(&client).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_create_key_returns_key_value() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/generate-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "api_key": "mage_brand_new_key"
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    let result = account::create_key(&client).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Some("mage_brand_new_key".to_string()));
}

#[tokio::test]
async fn test_create_key_returns_none_when_no_key_in_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/generate-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "ok"
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    let result = account::create_key(&client).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None);
}

#[tokio::test]
async fn test_revoke_key_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/revoke-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "revoked"
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    let result = account::revoke_key(&client, "key_42").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_list_models_network_error() {
    // Use a port that's not listening
    let client = RemoteClient::new("http://127.0.0.1:1", "tok");
    let result = account::list_models(&client).await;
    assert!(result.is_err());
}
