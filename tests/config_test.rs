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

    let mut config = magelab_cli::config::Config::default();
    config.api_key = Some("mage_saved".to_string());
    config.save_to(&config_path).unwrap();

    let loaded = magelab_cli::config::Config::load_from(&config_path).unwrap();
    assert_eq!(loaded.api_key, Some("mage_saved".to_string()));
}
