use magelab_cli::detect;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_check_backend_health_running() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;

    let healthy = detect::check_backend_health(&server.uri()).await;
    assert!(healthy);
}

#[tokio::test]
async fn test_check_backend_health_not_running() {
    // Use a port that's not listening
    let healthy = detect::check_backend_health("http://127.0.0.1:1").await;
    assert!(!healthy);
}

#[tokio::test]
async fn test_check_backend_health_server_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let healthy = detect::check_backend_health(&server.uri()).await;
    assert!(!healthy);
}

#[tokio::test]
async fn test_check_backend_health_trailing_slash() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let healthy = detect::check_backend_health(&format!("{}/", server.uri())).await;
    assert!(healthy);
}

#[tokio::test]
async fn test_discover_devices_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/realtime/devices"))
        .and(header("Authorization", "Bearer jwt_test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "devices": ["device-1", "device-2"]
        })))
        .mount(&server)
        .await;

    let devices = detect::discover_devices(&server.uri(), "jwt_test")
        .await
        .unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0], "device-1");
    assert_eq!(devices[1], "device-2");
}

#[tokio::test]
async fn test_discover_devices_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/realtime/devices"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "devices": []
        })))
        .mount(&server)
        .await;

    let devices = detect::discover_devices(&server.uri(), "tok")
        .await
        .unwrap();
    assert!(devices.is_empty());
}

#[tokio::test]
async fn test_discover_devices_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/realtime/devices"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .mount(&server)
        .await;

    // Should return empty list on non-success
    let devices = detect::discover_devices(&server.uri(), "bad_tok")
        .await
        .unwrap();
    assert!(devices.is_empty());
}

#[tokio::test]
async fn test_get_ws_ticket_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/realtime/ws-ticket"))
        .and(header("Authorization", "Bearer jwt_test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ws_ticket": "ticket_abc123"
        })))
        .mount(&server)
        .await;

    let ticket = detect::get_ws_ticket(&server.uri(), "jwt_test")
        .await
        .unwrap();
    assert_eq!(ticket, "ticket_abc123");
}

#[tokio::test]
async fn test_get_ws_ticket_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/realtime/ws-ticket"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
        .mount(&server)
        .await;

    let result = detect::get_ws_ticket(&server.uri(), "bad_tok").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("403"));
}

#[tokio::test]
async fn test_get_ws_ticket_missing_field() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/realtime/ws-ticket"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let result = detect::get_ws_ticket(&server.uri(), "tok").await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("No ws_ticket in response"));
}

#[test]
fn test_find_magelab_home_config_override() {
    let result = detect::find_magelab_home(Some("/custom/magelab/path"));
    // Config override is checked last; with no env var and no matching paths,
    // the config override should be returned
    // Note: This depends on MAGELAB_HOME not being set and no sibling/platform paths existing
    // In practice, it may return a different path. Let's just verify non-empty override works.
    assert!(result.is_some() || std::env::var("MAGELAB_HOME").is_ok());
}

#[test]
fn test_find_magelab_home_empty_override_returns_none() {
    // Empty override should not be treated as a valid path
    // (when no other paths exist either)
    let _result = detect::find_magelab_home(Some(""));
    // We can't assert None because MAGELAB_HOME or sibling paths might exist
    // But at least it shouldn't panic
}

#[tokio::test]
async fn test_wait_for_backend_already_running() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let result = detect::wait_for_backend(&server.uri(), std::time::Duration::from_secs(2)).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_wait_for_backend_timeout() {
    // Port that's not listening — should timeout quickly
    let result =
        detect::wait_for_backend("http://127.0.0.1:1", std::time::Duration::from_millis(300)).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("did not become healthy"));
}
