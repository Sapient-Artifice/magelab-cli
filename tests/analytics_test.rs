#[test]
fn test_common_properties_populated() {
    let props = magelab_cli::analytics::common_properties();
    assert_eq!(props.get("platform").and_then(|v| v.as_str()), Some("cli"));
    assert!(props.get("cli_version").is_some());
    assert!(props.get("os").is_some());
    assert!(props.get("arch").is_some());
}

#[test]
fn test_activation_skips_when_already_activated() {
    let mut config = magelab_cli::config::Config::default();
    config.activated_user_id = Some("user-1".to_string());

    assert!(!magelab_cli::analytics::should_track_activation(
        "user-1", &config
    ));
}

#[test]
fn test_activation_fires_for_new_user() {
    let config = magelab_cli::config::Config::default();
    assert!(magelab_cli::analytics::should_track_activation(
        "user-1", &config
    ));
}

#[test]
fn test_activation_fires_for_different_user() {
    let mut config = magelab_cli::config::Config::default();
    config.activated_user_id = Some("user-1".to_string());
    assert!(magelab_cli::analytics::should_track_activation(
        "user-2", &config
    ));
}

#[tokio::test]
async fn test_track_activation_does_not_persist_on_capture_failure() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("cli.toml");
    let mut config = magelab_cli::config::Config::load_with_path(&path).unwrap();

    // PostHog not initialized — capture will fail, activation should NOT persist
    magelab_cli::analytics::track_activation("user-42", "models", &mut config).await;

    assert_eq!(config.activated_user_id, None);

    // Config file should not contain activated_user_id either
    let reloaded = magelab_cli::config::Config::load_with_path(&path).unwrap();
    assert_eq!(reloaded.activated_user_id, None);
}

#[tokio::test]
async fn test_track_activation_is_idempotent() {
    let mut config = magelab_cli::config::Config::default();
    config.activated_user_id = Some("user-42".to_string());

    // Should be a no-op — already activated for this user
    magelab_cli::analytics::track_activation("user-42", "models", &mut config).await;

    // Still the same user
    assert_eq!(config.activated_user_id.as_deref(), Some("user-42"));
}

#[test]
fn test_config_save_uses_config_path() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("cli.toml");
    let mut config = magelab_cli::config::Config::load_with_path(&path).unwrap();
    config.default_model = "test-model".to_string();
    config.save().unwrap();

    // Verify it wrote to the temp path, not the default
    let reloaded = magelab_cli::config::Config::load_with_path(&path).unwrap();
    assert_eq!(reloaded.default_model, "test-model");
}

#[test]
fn test_activation_skips_when_telemetry_disabled() {
    let mut config = magelab_cli::config::Config::default();
    config.telemetry = Some(false);
    assert!(!magelab_cli::analytics::should_track_activation(
        "user-1", &config
    ));
}
