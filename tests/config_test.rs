use std::io::Write;

use serial_test::serial;
use tempfile::TempDir;

#[test]
fn test_default_config() {
    let config = magelab_cli::config::Config::default();
    assert_eq!(config.gateway_url, "https://api.magelab.ai");
    assert_eq!(config.local_url, "http://127.0.0.1:11115");
    assert_eq!(config.prefer, "auto");
    assert!(config.auto_approve.contains(&"read_file".to_string()));
}

#[test]
fn test_load_config_from_toml() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("cli.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    writeln!(
        f,
        r#"
api_key = "mage_test123"
default_model = "gpt-4o"
gateway_url = "https://custom.api.com"
prefer = "remote"
auto_approve = ["read_file"]
"#
    )
    .unwrap();

    let config = magelab_cli::config::Config::load_from(config_path).unwrap();
    assert_eq!(config.default_model, "gpt-4o");
    assert_eq!(config.gateway_url, "https://custom.api.com");
    assert_eq!(config.prefer, "remote");
}

#[test]
fn test_save_config() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("cli.toml");

    let config = magelab_cli::config::Config::default();
    config.save_to(&config_path).unwrap();

    let loaded = magelab_cli::config::Config::load_from(&config_path).unwrap();
    assert_eq!(loaded.default_model, config.default_model);
}

#[test]
fn test_legacy_api_key_deserializes_but_is_not_serialized() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("cli.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    writeln!(f, r#"api_key = "mage_old_key""#).unwrap();

    let loaded = magelab_cli::config::Config::load_from(&config_path).unwrap();
    loaded.save_to(&config_path).unwrap();

    let contents = std::fs::read_to_string(&config_path).unwrap();
    assert!(!contents.contains("api_key"));
    assert!(!contents.contains("mage_old_key"));
}

#[test]
#[serial]
fn test_api_key_method_only_returns_env_var() {
    let config = magelab_cli::config::Config::default();

    std::env::remove_var("MAGELAB_API_KEY");
    assert_eq!(config.api_key(), None);

    std::env::set_var("MAGELAB_API_KEY", "env_value");
    assert_eq!(config.api_key().as_deref(), Some("env_value"));
    std::env::remove_var("MAGELAB_API_KEY");
}

#[test]
fn test_default_device_none_by_default() {
    let config = magelab_cli::config::Config::default();
    assert!(config.default_device.is_none());
}

#[test]
fn test_config_with_default_device() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("cli.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    writeln!(f, r#"default_device = "macbook-pro""#).unwrap();

    let config = magelab_cli::config::Config::load_from(config_path).unwrap();
    assert_eq!(config.default_device.as_deref(), Some("macbook-pro"));
}

#[test]
fn test_telemetry_defaults_to_true() {
    let config = magelab_cli::config::Config::default();
    assert_eq!(config.telemetry(), true);
}

#[test]
fn test_telemetry_deserialized_from_toml() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli.toml");
    std::fs::write(&path, "telemetry = false\n").unwrap();

    let config = magelab_cli::config::Config::load_from(&path).unwrap();
    assert_eq!(config.telemetry(), false);
}

#[test]
fn test_telemetry_missing_defaults_to_true() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli.toml");
    std::fs::write(&path, "default_model = \"test\"\n").unwrap();

    let config = magelab_cli::config::Config::load_from(&path).unwrap();
    assert_eq!(config.telemetry(), true);
}

#[test]
fn test_activated_user_id_defaults_to_none() {
    let config = magelab_cli::config::Config::default();
    assert!(config.activated_user_id.is_none());
}

#[test]
fn test_activated_user_id_round_trips() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli.toml");

    let mut config = magelab_cli::config::Config::default();
    config.activated_user_id = Some("uuid-123".to_string());
    config.save_to(&path).unwrap();

    let loaded = magelab_cli::config::Config::load_from(&path).unwrap();
    assert_eq!(loaded.activated_user_id.as_deref(), Some("uuid-123"));
}
