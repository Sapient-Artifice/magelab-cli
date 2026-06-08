use magelab_cli::backend_auth;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_push_access_token_success() {
    let server = MockServer::start().await;
    let expected_body = serde_json::json!({ "access_token": "jwt-test" });

    Mock::given(method("POST"))
        .and(path("/api/auth/set_token"))
        .and(body_json(&expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let result = backend_auth::push_access_token_to_backend(&server.uri(), "jwt-test").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_push_access_token_server_error_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/auth/set_token"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;

    let result = backend_auth::push_access_token_to_backend(&server.uri(), "jwt-test").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("403"));
}
