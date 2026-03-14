use magelab_cli::config::Config;

#[test]
fn test_needs_setup_when_no_key() {
    std::env::remove_var("MAGELAB_API_KEY");
    let config = Config::default();
    assert!(config.needs_remote_setup());
}

#[test]
fn test_no_setup_needed_with_key() {
    let mut config = Config::default();
    config.api_key = Some("mage_test".into());
    assert!(!config.needs_remote_setup());
}

#[test]
fn test_api_key_method_prefers_config() {
    // Without env var set, api_key() should return the config value
    std::env::remove_var("MAGELAB_API_KEY");
    let mut config = Config::default();
    config.api_key = Some("mage_from_config".into());
    assert_eq!(config.api_key(), Some("mage_from_config".to_string()));
}
