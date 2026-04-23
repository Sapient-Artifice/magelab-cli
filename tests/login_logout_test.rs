//! Integration tests for login/logout flows against a mock gateway.
//!
//! These test the token exchange, credential storage, refresh, and logout
//! by standing up a wiremock server that mimics the gateway's /v1/auth endpoints.

use assert_cmd::Command;
use magelab_cli::auth::credentials::Credentials;
use magelab_cli::auth::oauth;
use predicates::prelude::*;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Helper: build a code-exchange response like the web app's /api/auth/cli-token returns.
fn cli_token_response(email: &str) -> serde_json::Value {
    serde_json::json!({
        "access_token": "test_web_token_xyz789",
        "email": email,
        "user_id": "user_01WEBTEST",
    })
}

/// Helper: build a JSON token response like the gateway returns.
fn token_response(email: &str) -> serde_json::Value {
    serde_json::json!({
        "access_token": "test_access_token_abc123",
        "refresh_token": "test_refresh_token_xyz789",
        "expires_in": 3600,
        "user": {
            "id": "user_01TEST",
            "email": email,
        }
    })
}

/// Refresh-token flow: exchanges a refresh token for new credentials via the gateway,
/// then verifies the returned credentials are correct.
#[tokio::test]
async fn test_refresh_token_exchange() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/auth/token"))
        .and(body_string_contains("refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_response("max@magelab.ai")))
        .expect(1)
        .mount(&mock_server)
        .await;

    let gateway_url = mock_server.uri();
    let creds = oauth::refresh_token(&gateway_url, "old_refresh_token")
        .await
        .expect("refresh_token should succeed against mock");

    assert_eq!(creds.access_token.as_deref(), Some("test_access_token_abc123"));
    assert_eq!(creds.refresh_token.as_deref(), Some("test_refresh_token_xyz789"));
    assert_eq!(creds.email.as_deref(), Some("max@magelab.ai"));
    assert_eq!(creds.user_id.as_deref(), Some("user_01TEST"));
    assert!(creds.is_token_valid(), "fresh token should be valid");
}

/// Refresh-token flow with a gateway error should return an error, not panic.
#[tokio::test]
async fn test_refresh_token_gateway_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/auth/token"))
        .respond_with(ResponseTemplate::new(401).set_body_string("invalid_grant"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let gateway_url = mock_server.uri();
    let result = oauth::refresh_token(&gateway_url, "bad_token").await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("401") || err_msg.contains("invalid_grant"),
        "error should mention the failure: {err_msg}"
    );
}

/// Refresh-token flow when gateway is unreachable should return a connection error.
#[tokio::test]
async fn test_refresh_token_unreachable_gateway() {
    // Use a port that nothing is listening on
    let result = oauth::refresh_token("http://127.0.0.1:19999", "some_token").await;
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("connect"),
        "error should indicate a connection failure"
    );
}

/// Credential save/load round-trip via file (not keychain, to avoid CI issues).
/// Uses a tempdir to avoid touching real credentials.
#[test]
fn test_credentials_save_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let cred_path = tmp.path().join("credentials.json");

    let creds = Credentials {
        access_token: Some("tok_roundtrip".into()),
        refresh_token: Some("ref_roundtrip".into()),
        expires_at: Some(chrono::Utc::now().timestamp() + 3600),
        user_id: Some("user_rt".into()),
        email: Some("test@example.com".into()),
    };

    // Write directly to file (bypasses keychain)
    let json = serde_json::to_string(&creds).unwrap();
    std::fs::write(&cred_path, &json).unwrap();

    // Read back
    let contents = std::fs::read_to_string(&cred_path).unwrap();
    let loaded: Credentials = serde_json::from_str(&contents).unwrap();

    assert_eq!(loaded.access_token.as_deref(), Some("tok_roundtrip"));
    assert_eq!(loaded.refresh_token.as_deref(), Some("ref_roundtrip"));
    assert_eq!(loaded.email.as_deref(), Some("test@example.com"));
    assert!(loaded.is_token_valid());
}

/// Expired credentials should report as invalid.
#[test]
fn test_expired_credentials_are_invalid() {
    let creds = Credentials {
        access_token: Some("expired_tok".into()),
        refresh_token: None,
        expires_at: Some(1000), // epoch + 1000s, long in the past
        user_id: None,
        email: None,
    };

    assert!(!creds.is_token_valid());
    assert!(creds.has_token());
}

/// Logout via CLI should succeed even when not logged in.
#[test]
fn test_logout_succeeds_when_not_logged_in() {
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("logout")
        .assert()
        .success()
        .stdout(predicate::str::contains("Logged out"));
}

/// Login --status after logout should show "Not logged in".
#[test]
fn test_login_status_after_logout() {
    // Logout first to ensure clean state
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("logout")
        .assert()
        .success();

    Command::cargo_bin("magelab")
        .unwrap()
        .args(["login", "--status"])
        .env_remove("MAGELAB_API_KEY")
        .assert()
        .success()
        .stdout(predicate::str::contains("Not logged in"));
}

/// Login --status shows API key preview when MAGELAB_API_KEY is set.
#[test]
fn test_login_status_shows_api_key() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["login", "--status"])
        .env("MAGELAB_API_KEY", "sk-test-1234567890abcdef")
        .assert()
        .success()
        .stdout(predicate::str::contains("API key: sk-t...cdef"));
}

/// Code exchange: valid code + matching state → credentials returned.
#[tokio::test]
async fn test_cli_code_exchange_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/auth/cli-token"))
        .and(body_string_contains("test_code_abc"))
        .and(body_string_contains("test_state_123"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(cli_token_response("max@magelab.ai")),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let creds = oauth::exchange_cli_code(&mock_server.uri(), "test_code_abc", "test_state_123")
        .await
        .expect("code exchange should succeed");

    assert_eq!(creds.access_token.as_deref(), Some("test_web_token_xyz789"));
    assert_eq!(creds.email.as_deref(), Some("max@magelab.ai"));
    assert_eq!(creds.user_id.as_deref(), Some("user_01WEBTEST"));
    assert!(creds.is_token_valid(), "fresh token should be valid");
    // Web flow doesn't provide refresh tokens
    assert!(creds.refresh_token.is_none());
}

/// Code exchange: expired/invalid code → error.
#[tokio::test]
async fn test_cli_code_exchange_invalid_code() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/auth/cli-token"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(serde_json::json!({"error": "Invalid or expired code"})),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let result =
        oauth::exchange_cli_code(&mock_server.uri(), "bad_code", "some_state").await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("400") || err_msg.contains("expired"),
        "error should mention failure: {err_msg}"
    );
}

/// Code exchange: state mismatch → error from server.
#[tokio::test]
async fn test_cli_code_exchange_state_mismatch() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/auth/cli-token"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(serde_json::json!({"error": "State mismatch"})),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let result =
        oauth::exchange_cli_code(&mock_server.uri(), "some_code", "wrong_state").await;

    assert!(result.is_err());
}

/// Code exchange: web app unreachable → connection error.
#[tokio::test]
async fn test_cli_code_exchange_unreachable() {
    let result =
        oauth::exchange_cli_code("http://127.0.0.1:19999", "some_code", "some_state").await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("connect") || err_msg.contains("exchange") || err_msg.contains("web app"),
        "error should indicate connection failure: {err_msg}"
    );
}

/// Code exchange: server returns 200 but missing access_token field → error.
#[tokio::test]
async fn test_cli_code_exchange_missing_token_field() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/auth/cli-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"email": "max@magelab.ai"})),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let result =
        oauth::exchange_cli_code(&mock_server.uri(), "code", "state").await;

    assert!(result.is_err(), "should fail when access_token is missing");
}

/// ensure_valid_jwt should fail cleanly when there are no stored credentials.
#[tokio::test]
async fn test_ensure_valid_jwt_no_credentials() {
    // This will try to load credentials (may find none or expired ones)
    // and then fail since no refresh token or login is possible non-interactively.
    // We just verify it doesn't panic.
    let result = oauth::ensure_valid_jwt("http://127.0.0.1:19999").await;
    // Either it finds existing valid creds (unlikely in CI) or errors out
    if result.is_err() {
        let msg = result.unwrap_err().to_string();
        // Should get a credentials error or connection error, not a panic
        assert!(
            msg.contains("login")
                || msg.contains("connect")
                || msg.contains("credentials")
                || msg.contains("parse")
                || msg.contains("Email")
                || msg.contains("loopback")
                || msg.contains("19872"),
            "unexpected error: {msg}"
        );
    }
}
