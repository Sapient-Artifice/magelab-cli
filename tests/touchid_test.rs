use magelab_cli::auth::touchid::{self, Tier};

#[test]
fn is_available_returns_false_when_disabled() {
    touchid::set_disabled(true);
    assert!(!touchid::is_available());
    // Reset for other tests
    touchid::set_disabled(false);
}

#[test]
fn set_disabled_can_be_toggled() {
    touchid::set_disabled(true);
    assert!(!touchid::is_available());
    touchid::set_disabled(false);
    // On non-macOS CI, is_available() is still false (no hardware)
    // but the flag itself was toggled successfully
}

#[test]
fn verify_returns_ok_when_not_available() {
    // When Touch ID is not available (no hardware or disabled),
    // verify() should return Ok(()) — graceful fallback
    touchid::set_disabled(true);
    let result = touchid::verify(Tier::Sensitive, "test");
    assert!(result.is_ok());
    let result = touchid::verify(Tier::Cached, "test");
    assert!(result.is_ok());
    touchid::set_disabled(false);
}

#[test]
fn clear_returns_ok_when_not_available() {
    touchid::set_disabled(true);
    let result = touchid::clear();
    assert!(result.is_ok());
    touchid::set_disabled(false);
}

mod session_cache_tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;

    use magelab_cli::auth::touchid::session_cache;

    #[test]
    fn cache_is_invalid_when_file_missing() {
        let dir = TempDir::new().unwrap();
        assert!(!session_cache::is_valid_in(dir.path()));
    }

    #[test]
    fn cache_is_valid_after_touch() {
        let dir = TempDir::new().unwrap();
        session_cache::touch_in(dir.path()).unwrap();
        assert!(session_cache::is_valid_in(dir.path()));
    }

    #[test]
    fn cache_is_invalid_after_expiry() {
        let dir = TempDir::new().unwrap();
        let cache_path = dir.path().join("touchid-session");
        let old_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 360;
        fs::write(&cache_path, old_ts.to_string()).unwrap();
        assert!(!session_cache::is_valid_in(dir.path()));
    }

    #[test]
    fn cache_is_invalid_with_garbage_content() {
        let dir = TempDir::new().unwrap();
        let cache_path = dir.path().join("touchid-session");
        fs::write(&cache_path, "not-a-number").unwrap();
        assert!(!session_cache::is_valid_in(dir.path()));
    }

    #[test]
    fn cache_is_deleted_by_delete() {
        let dir = TempDir::new().unwrap();
        session_cache::touch_in(dir.path()).unwrap();
        assert!(dir.path().join("touchid-session").exists());
        session_cache::delete_in(dir.path());
        assert!(!dir.path().join("touchid-session").exists());
    }

    #[test]
    fn cache_file_has_restrictive_permissions() {
        let dir = TempDir::new().unwrap();
        session_cache::touch_in(dir.path()).unwrap();
        let cache_path = dir.path().join("touchid-session");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::metadata(&cache_path).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn cache_respects_custom_ttl_env() {
        let dir = TempDir::new().unwrap();
        let cache_path = dir.path().join("touchid-session");
        let recent_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 2;
        fs::write(&cache_path, recent_ts.to_string()).unwrap();

        // With default TTL (300s), this should be valid
        assert!(session_cache::is_valid_in(dir.path()));

        // With TTL of 1 second, this should be invalid
        std::env::set_var("MAGELAB_TOUCHID_TTL", "1");
        assert!(!session_cache::is_valid_in(dir.path()));
        std::env::remove_var("MAGELAB_TOUCHID_TTL");
    }
}

mod credential_integration_tests {
    use magelab_cli::auth::credentials::Credentials;
    use magelab_cli::auth::touchid;

    #[test]
    fn save_with_touchid_disabled_preserves_refresh_token_in_regular_store() {
        touchid::set_disabled(true);

        let creds = Credentials {
            access_token: Some("test-access".to_string()),
            refresh_token: Some("test-refresh".to_string()),
            expires_at: Some(9999999999),
            user_id: Some("user-1".to_string()),
            email: Some("test@example.com".to_string()),
        };

        let result = creds.save();
        assert!(result.is_ok());

        let loaded = Credentials::load().unwrap();
        assert_eq!(loaded.refresh_token.as_deref(), Some("test-refresh"));

        // Clean up
        Credentials::clear().ok();
        touchid::set_disabled(false);
    }
}
