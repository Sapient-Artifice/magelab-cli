use magelab_cli::config::Config;
use magelab_cli::connect;
use serial_test::serial;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Create a config pointing at the given local URL with no API key or magelab_home.
/// Also clears MAGELAB_API_KEY env var to avoid test pollution.
fn config_with_local(local_url: &str) -> Config {
    std::env::remove_var("MAGELAB_API_KEY");
    Config {
        local_url: local_url.to_string(),
        gateway_url: "https://api.magelab.ai".to_string(),
        api_key: None,
        magelab_home: None,
        ..Config::default()
    }
}

#[tokio::test]
#[serial]
async fn test_resolve_local_backend_running() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let config = config_with_local(&server.uri());
    let result = connect::resolve(&config, true).await.unwrap();

    assert_eq!(result.mode, "local");
    assert!(result.url.is_some());
    assert!(result.url.unwrap().contains("/ws"));
    assert!(result.token.is_none());
    assert!(result.model.is_some());
}

#[tokio::test]
#[serial]
async fn test_resolve_falls_to_remote_with_api_key() {
    // No local backend, no JWT, but has API key via env var
    std::env::set_var("MAGELAB_API_KEY", "mage_test_key");
    let config = config_with_local("http://127.0.0.1:1"); // won't connect

    let result = connect::resolve(&config, true).await.unwrap();

    std::env::remove_var("MAGELAB_API_KEY");

    assert_eq!(result.mode, "remote");
    assert_eq!(result.token.as_deref(), Some("mage_test_key"));
    assert!(result.url.is_some());
}

#[tokio::test]
#[serial]
async fn test_resolve_returns_none_when_nothing_available() {
    std::env::remove_var("MAGELAB_API_KEY");
    // No local backend, no JWT, no API key
    let config = config_with_local("http://127.0.0.1:1");

    let result = connect::resolve(&config, true).await.unwrap();

    assert_eq!(result.mode, "none");
    assert!(result.url.is_none());
    assert!(result.token.is_none());
    assert!(result.model.is_none());
}

#[tokio::test]
#[serial]
async fn test_resolve_no_launch_skips_backend_launch() {
    std::env::remove_var("MAGELAB_API_KEY");
    // With no_launch=true, should not try to start a backend
    let config = config_with_local("http://127.0.0.1:1");

    let result = connect::resolve(&config, true).await.unwrap();

    // Should fall through to none since no local, no JWT, no API key
    assert_eq!(result.mode, "none");
}

#[tokio::test]
#[serial]
async fn test_resolve_remote_includes_gateway_url() {
    std::env::set_var("MAGELAB_API_KEY", "mage_key");
    let mut config = config_with_local("http://127.0.0.1:1");
    config.gateway_url = "https://custom-gateway.example.com".to_string();

    let result = connect::resolve(&config, true).await.unwrap();

    std::env::remove_var("MAGELAB_API_KEY");

    assert_eq!(result.mode, "remote");
    assert_eq!(
        result.url.as_deref(),
        Some("https://custom-gateway.example.com")
    );
}

#[tokio::test]
#[serial]
async fn test_resolve_local_uses_configured_model() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let mut config = config_with_local(&server.uri());
    config.default_model = "my-custom-model".to_string();

    let result = connect::resolve(&config, true).await.unwrap();

    assert_eq!(result.mode, "local");
    assert_eq!(result.model.as_deref(), Some("my-custom-model"));
}
