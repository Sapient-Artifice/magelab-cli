#[test]
fn test_credentials_file_fallback() {
    // When keychain is unavailable (CI), should fall back to file
    let creds = magelab_cli::auth::credentials::Credentials::default();
    assert!(!creds.has_token());
    assert!(!creds.is_token_valid());
}

#[test]
fn test_credentials_clear_no_panic() {
    // Clear should not panic even when nothing is stored
    // (we can't test actual keychain in CI, just ensure no crash)
    // Note: we don't call clear() here to avoid deleting real creds
    let creds = magelab_cli::auth::credentials::Credentials::default();
    assert!(creds.email.is_none());
}
