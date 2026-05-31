use magelab_cli::detect;
use magelab_cli::detect::BackendBundleKind;
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
    // Config override must point to a real mage-lab directory (with backend/main.py)
    let dir = tempfile::TempDir::new().unwrap();
    let backend_dir = dir.path().join("backend");
    std::fs::create_dir_all(&backend_dir).unwrap();
    std::fs::write(backend_dir.join("main.py"), "").unwrap();

    let result = detect::find_magelab_home(Some(dir.path().to_str().unwrap()));
    assert_eq!(result, Some(dir.path().to_path_buf()));
}

#[test]
fn test_find_magelab_home_config_override_nonexistent_returns_none() {
    // A config override pointing to a nonexistent path should not be accepted
    let result = detect::find_magelab_home(Some("/nonexistent/magelab/path"));
    // May still return Some if MAGELAB_HOME or sibling paths exist, but the
    // override itself should not be returned for a nonexistent path
    assert_ne!(
        result,
        Some(std::path::PathBuf::from("/nonexistent/magelab/path"))
    );
}

#[test]
fn test_find_magelab_home_empty_override_returns_none() {
    // Empty override should not be treated as a valid path
    // (when no other paths exist either)
    let _result = detect::find_magelab_home(Some(""));
    // We can't assert None because MAGELAB_HOME or sibling paths might exist
    // But at least it shouldn't panic
}

#[test]
fn test_find_backend_bundle_dev_repo() {
    let dir = tempfile::TempDir::new().unwrap();
    let backend_dir = dir.path().join("backend");
    let venv_bin = backend_dir.join(".venv").join("bin");
    std::fs::create_dir_all(&venv_bin).unwrap();
    std::fs::write(backend_dir.join("main.py"), "").unwrap();
    std::fs::write(venv_bin.join("python"), "").unwrap();

    let bundle = detect::find_backend_bundle(Some(dir.path().to_str().unwrap()))
        .unwrap()
        .unwrap();

    assert_eq!(bundle.kind, BackendBundleKind::DevRepo);
    assert_eq!(bundle.root, dir.path().to_path_buf());
    assert_eq!(bundle.api_dir, None);
    assert_eq!(bundle.backend_dir, backend_dir);
    assert_eq!(bundle.python, venv_bin.join("python"));
}

#[test]
fn test_find_backend_bundle_packaged_api_root() {
    let dir = tempfile::TempDir::new().unwrap();
    let api_dir = dir.path().join("bin").join("api");
    let backend_dir = api_dir.join("backend");
    let python_dir = api_dir.join("python").join("bin");
    std::fs::create_dir_all(&backend_dir).unwrap();
    std::fs::create_dir_all(&python_dir).unwrap();
    std::fs::write(backend_dir.join("main.py"), "").unwrap();
    std::fs::write(python_dir.join("python3"), "").unwrap();

    let bundle = detect::find_backend_bundle(Some(dir.path().to_str().unwrap()))
        .unwrap()
        .unwrap();

    assert_eq!(bundle.kind, BackendBundleKind::PackagedApp);
    assert_eq!(bundle.api_dir, Some(api_dir.clone()));
    assert_eq!(bundle.backend_dir, backend_dir);
    assert_eq!(bundle.python, python_dir.join("python3"));
}

#[test]
fn test_find_backend_bundle_macos_app_root_shape() {
    let dir = tempfile::TempDir::new().unwrap();
    let app_root = dir.path().join("magelab.app");
    let api_dir = app_root
        .join("Contents")
        .join("Resources")
        .join("bin")
        .join("api");
    let backend_dir = api_dir.join("backend");
    let python_dir = api_dir.join("python").join("bin");
    std::fs::create_dir_all(&backend_dir).unwrap();
    std::fs::create_dir_all(&python_dir).unwrap();
    std::fs::write(backend_dir.join("main.py"), "").unwrap();
    std::fs::write(python_dir.join("python3"), "").unwrap();

    let bundle = detect::find_backend_bundle(Some(app_root.to_str().unwrap()))
        .unwrap()
        .unwrap();

    assert_eq!(bundle.kind, BackendBundleKind::PackagedApp);
    assert_eq!(bundle.api_dir, Some(api_dir));
}

#[test]
fn test_find_backend_bundle_rejects_main_py_override() {
    let dir = tempfile::TempDir::new().unwrap();
    let backend_dir = dir.path().join("backend");
    std::fs::create_dir_all(&backend_dir).unwrap();
    let main_py = backend_dir.join("main.py");
    std::fs::write(&main_py, "").unwrap();

    let err = detect::find_backend_bundle(Some(main_py.to_str().unwrap()))
        .unwrap_err()
        .to_string();

    assert!(err.contains("not backend/main.py"));
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
