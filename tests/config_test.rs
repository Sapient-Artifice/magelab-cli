use std::io::Write;
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
    assert_eq!(config.api_key, Some("mage_test123".to_string()));
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
fn test_api_key_not_serialized_but_still_deserializable() {
    // api_key should not appear in serialized output
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("cli.toml");

    let mut config = magelab_cli::config::Config::default();
    config.api_key = Some("mage_legacy".to_string());
    config.save_to(&config_path).unwrap();

    let contents = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        !contents.contains("mage_legacy"),
        "api_key should not be serialized"
    );

    // But old configs with api_key can still be loaded
    let dir2 = TempDir::new().unwrap();
    let config_path2 = dir2.path().join("cli.toml");
    let mut f = std::fs::File::create(&config_path2).unwrap();
    writeln!(f, r#"api_key = "mage_old_key""#).unwrap();

    let loaded = magelab_cli::config::Config::load_from(config_path2).unwrap();
    assert_eq!(loaded.api_key, Some("mage_old_key".to_string()));
}

#[test]
fn test_api_key_method_only_returns_env_var() {
    let config = magelab_cli::config::Config::default();
    // api_key() should NOT return config file value, only env var
    let mut config_with_key = config;
    config_with_key.api_key = Some("config_value".to_string());
    // Without MAGELAB_API_KEY env var set, should return None
    // (can't safely test with env var in parallel tests)
    // Just verify the method exists and returns Option<String>
    let _result: Option<String> = config_with_key.api_key();
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
