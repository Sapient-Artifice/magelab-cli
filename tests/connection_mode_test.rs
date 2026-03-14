use magelab_cli::config::Config;
use magelab_cli::detect::ConnectionMode;
use std::io::Write;
use tempfile::TempDir;

#[test]
fn test_connection_mode_from_flags() {
    assert_eq!(ConnectionMode::from_flags(true, false), ConnectionMode::Local);
    assert_eq!(ConnectionMode::from_flags(false, true), ConnectionMode::Remote);
    assert_eq!(ConnectionMode::from_flags(false, false), ConnectionMode::Auto);
}

#[test]
fn test_config_default_device() {
    let config = Config::default();
    assert!(config.default_device.is_none());
}

#[test]
fn test_config_with_default_device() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("cli.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    writeln!(f, r#"default_device = "macbook-pro""#).unwrap();

    let config = Config::load_from(config_path).unwrap();
    assert_eq!(config.default_device.as_deref(), Some("macbook-pro"));
}

#[test]
fn test_config_roundtrip_with_device() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("cli.toml");

    let mut config = Config::default();
    config.default_device = Some("imac-studio".to_string());
    config.save_to(&config_path).unwrap();

    let loaded = Config::load_from(&config_path).unwrap();
    assert_eq!(loaded.default_device.as_deref(), Some("imac-studio"));
}

#[test]
fn test_credentials_default_has_no_email() {
    let creds = magelab_cli::auth::credentials::Credentials::default();
    assert!(creds.email.is_none());
    assert!(!creds.has_token());
    assert!(!creds.is_token_valid());
}

#[test]
fn test_credentials_with_email_parses() {
    let json = r#"{
        "access_token": "jwt_tok",
        "refresh_token": "ref_tok",
        "expires_at": 9999999999,
        "user_id": "user_123",
        "email": "dev@magelab.ai"
    }"#;
    let creds: magelab_cli::auth::credentials::Credentials =
        serde_json::from_str(json).unwrap();
    assert_eq!(creds.email.as_deref(), Some("dev@magelab.ai"));
    assert!(creds.has_token());
    assert!(creds.is_token_valid());
}

#[test]
fn test_credentials_expired_token() {
    let json = r#"{
        "access_token": "jwt_tok",
        "expires_at": 1000
    }"#;
    let creds: magelab_cli::auth::credentials::Credentials =
        serde_json::from_str(json).unwrap();
    assert!(creds.has_token());
    assert!(!creds.is_token_valid()); // expired long ago
}

#[test]
fn test_login_method_parsing() {
    use magelab_cli::auth::oauth::LoginMethod;
    assert_eq!("google".parse::<LoginMethod>().unwrap(), LoginMethod::Google);
    assert_eq!("magic".parse::<LoginMethod>().unwrap(), LoginMethod::MagicAuth);
    assert_eq!("email".parse::<LoginMethod>().unwrap(), LoginMethod::MagicAuth);
    assert!("unknown".parse::<LoginMethod>().is_err());
}
