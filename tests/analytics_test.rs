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
async fn test_track_activation_persists_user_id() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("cli.toml");
    let mut config = magelab_cli::config::Config::default();
    config.save_to(&path).unwrap();

    // PostHog client not initialized — capture will silently fail, but
    // activated_user_id should still be persisted
    magelab_cli::analytics::track_activation("user-42", "models", &mut config).await;

    assert_eq!(config.activated_user_id.as_deref(), Some("user-42"));
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
fn test_activation_skips_when_telemetry_disabled() {
    let mut config = magelab_cli::config::Config::default();
    config.telemetry = Some(false);
    assert!(!magelab_cli::analytics::should_track_activation(
        "user-1", &config
    ));
}
