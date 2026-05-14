use tempfile::TempDir;

#[test]
fn test_config_set_value_updates_field() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli.toml");
    std::fs::write(&path, "default_model = \"old-model\"\n").unwrap();

    let mut config = magelab_cli::config::Config::load_from(&path).unwrap();
    let result = config.set_value("default_model", "new-model");
    assert!(result.is_ok());
    assert_eq!(config.default_model, "new-model");

    config.save_to(&path).unwrap();
    let reloaded = magelab_cli::config::Config::load_from(&path).unwrap();
    assert_eq!(reloaded.default_model, "new-model");
}

#[test]
fn test_config_set_value_rejects_unknown_key() {
    let mut config = magelab_cli::config::Config::default();
    let result = config.set_value("nonexistent_key", "value");
    assert!(result.is_err());
}

#[test]
fn test_config_set_gateway_url() {
    let mut config = magelab_cli::config::Config::default();
    config
        .set_value("gateway_url", "https://custom.api.com")
        .unwrap();
    assert_eq!(config.gateway_url, "https://custom.api.com");
}

#[test]
fn test_config_set_api_key_is_rejected() {
    let mut config = magelab_cli::config::Config::default();
    let result = config.set_value("api_key", "mage_test123");
    assert!(result.is_err());
}
