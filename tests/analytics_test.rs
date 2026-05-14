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

#[test]
fn test_activation_skips_when_telemetry_disabled() {
    let mut config = magelab_cli::config::Config::default();
    config.telemetry = Some(false);
    assert!(!magelab_cli::analytics::should_track_activation(
        "user-1", &config
    ));
}
