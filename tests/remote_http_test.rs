use magelab_cli::client::remote::RemoteClient;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_list_models_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("Authorization", "Bearer test_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {"id": "qwen-3-235b", "owned_by": "alibaba"},
                {"id": "gpt-4o", "owned_by": "openai"}
            ]
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "test_token");
    let result = client.list_models().await.unwrap();
    let models = result["data"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["id"], "qwen-3-235b");
}

#[tokio::test]
async fn test_usage_summary_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/usage/summary"))
        .and(header("Authorization", "Bearer api_key_123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_requests": 42,
            "total_tokens_used": 10000,
            "total_cost": 0.1234
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "api_key_123");
    let result = client.usage_summary().await.unwrap();
    assert_eq!(result["total_requests"], 42);
    assert_eq!(result["total_tokens_used"], 10000);
}

#[tokio::test]
async fn test_balance_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/usage/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "available_credit": 9.50,
            "total_credit": 10.00,
            "total_cost": 0.50
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    let result = client.balance().await.unwrap();
    assert_eq!(result["available_credit"], 9.50);
}

#[tokio::test]
async fn test_list_keys_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/list-api-keys"))
        .and(header("Authorization", "Bearer jwt_tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "api_keys": [
                {"id": "1", "key_preview": "mage_abc...xyz", "is_revoked": false, "created_at": "2026-01-01"}
            ]
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "jwt_tok");
    let result = client.list_keys().await.unwrap();
    let keys = result["api_keys"].as_array().unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0]["key_preview"], "mage_abc...xyz");
}

#[tokio::test]
async fn test_generate_key_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/generate-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "api_key": "mage_new_key_abc123"
        })))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    let result = client.generate_key().await.unwrap();
    assert_eq!(result["api_key"], "mage_new_key_abc123");
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
    let result = client.revoke_key("key_42").await.unwrap();
    assert_eq!(result["status"], "revoked");
}

#[tokio::test]
async fn test_list_models_server_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "tok");
    // Server returns 500 but reqwest still parses it (json parse will fail)
    let result = client.list_models().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_auth_header_uses_bearer_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("Authorization", "Bearer my_secret_jwt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .expect(1)
        .mount(&server)
        .await;

    let client = RemoteClient::new(&server.uri(), "my_secret_jwt");
    client.list_models().await.unwrap();
    // If the header didn't match, wiremock would return 404 and the test would fail
}

#[tokio::test]
async fn test_trailing_slash_in_gateway_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;

    // Pass URL with trailing slash
    let client = RemoteClient::new(&format!("{}/", server.uri()), "tok");
    let result = client.list_models().await.unwrap();
    assert!(result["data"].as_array().unwrap().is_empty());
}
