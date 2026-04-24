use magelab_cli::auth::oauth::LoginMethod;

#[test]
fn test_login_method_from_str() {
    assert_eq!(
        "google".parse::<LoginMethod>().unwrap(),
        LoginMethod::Google
    );
    assert_eq!(
        "magic".parse::<LoginMethod>().unwrap(),
        LoginMethod::MagicAuth
    );
    assert_eq!("web".parse::<LoginMethod>().unwrap(), LoginMethod::Web);
    assert_eq!("browser".parse::<LoginMethod>().unwrap(), LoginMethod::Web);
    assert!("invalid".parse::<LoginMethod>().is_err());
}

#[test]
fn test_login_method_default_is_web() {
    assert_eq!(LoginMethod::default(), LoginMethod::Web);
}

#[test]
fn test_credentials_email_field() {
    let json = r#"{"access_token":"tok","email":"max@magelab.ai"}"#;
    let creds: magelab_cli::auth::credentials::Credentials = serde_json::from_str(json).unwrap();
    assert_eq!(creds.email.as_deref(), Some("max@magelab.ai"));
    assert_eq!(creds.access_token.as_deref(), Some("tok"));
}

#[test]
fn test_credentials_backwards_compatible() {
    let json = r#"{"access_token":"tok","refresh_token":"ref","expires_at":999999999}"#;
    let creds: magelab_cli::auth::credentials::Credentials = serde_json::from_str(json).unwrap();
    assert!(creds.email.is_none());
    assert!(creds.has_token());
}
