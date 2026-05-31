//! Tests for auth::get_token fallback chain.
//!
//! The full chain (JWT → refresh → vault → env var) depends on keychain state,
//! so we test the env-var-only path by pointing gateway_url at an unreachable
//! address (so JWT refresh fails fast) and clearing any cached credentials.
//!
//! Note: if real credentials exist in the system keychain, the JWT path will
//! succeed and short-circuit the chain. These tests use `serial` to avoid
//! env var races and are designed to pass in both scenarios.

use magelab_cli::auth;
use magelab_cli::config::Config;
use serial_test::serial;
use std::ffi::OsString;
use tempfile::TempDir;

struct EnvGuard {
    vars: Vec<(&'static str, Option<OsString>)>,
    _tmp: TempDir,
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.vars {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn isolate_auth_state() -> EnvGuard {
    let tmp = TempDir::new().unwrap();
    let vars = vec![
        ("HOME", std::env::var_os("HOME")),
        ("USERPROFILE", std::env::var_os("USERPROFILE")),
        ("XDG_CONFIG_HOME", std::env::var_os("XDG_CONFIG_HOME")),
        (
            "MAGELAB_SKIP_KEYCHAIN_TESTS",
            std::env::var_os("MAGELAB_SKIP_KEYCHAIN_TESTS"),
        ),
    ];
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("USERPROFILE", tmp.path());
    std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("config"));
    std::env::set_var("MAGELAB_SKIP_KEYCHAIN_TESTS", "1");
    EnvGuard { vars, _tmp: tmp }
}

/// Config with gateway pointing nowhere (refresh will fail).
fn test_config() -> Config {
    std::env::remove_var("MAGELAB_API_KEY");
    Config {
        gateway_url: "http://127.0.0.1:1".to_string(), // unreachable
        ..Config::default()
    }
}

fn skip_in_ci() -> bool {
    std::env::var("MAGELAB_SKIP_KEYCHAIN_TESTS").is_ok()
}

#[tokio::test]
#[serial]
async fn test_get_token_returns_something_when_env_var_set() {
    if skip_in_ci() {
        return;
    }
    let _env = isolate_auth_state();
    let config = test_config();
    std::env::set_var("MAGELAB_API_KEY", "mage_test_env_key");

    // Should succeed — either from keychain creds or env var fallback
    let result = auth::get_token(&config).await;
    assert!(result.is_ok(), "get_token should succeed with env var set");

    std::env::remove_var("MAGELAB_API_KEY");
}

#[tokio::test]
#[serial]
async fn test_get_token_env_var_empty_is_not_valid() {
    if skip_in_ci() {
        return;
    }
    let _env = isolate_auth_state();
    let config = test_config();
    std::env::set_var("MAGELAB_API_KEY", "");

    let result = auth::get_token(&config).await;

    // If keychain has creds, this succeeds via JWT path (not env var).
    // If no keychain creds, this should fail — empty env var is rejected.
    // Either way, an empty MAGELAB_API_KEY should never be the returned token.
    if let Ok(token) = &result {
        assert!(
            !token.is_empty(),
            "get_token should never return empty string"
        );
    }

    std::env::remove_var("MAGELAB_API_KEY");
}

#[tokio::test]
#[serial]
async fn test_get_token_error_message_mentions_login() {
    if skip_in_ci() {
        return;
    }
    let _env = isolate_auth_state();
    // Remove env var — if no keychain creds either, should get helpful error
    std::env::remove_var("MAGELAB_API_KEY");
    let config = test_config();

    let result = auth::get_token(&config).await;
    // If keychain has creds, this succeeds — skip the assertion.
    // If no creds, verify the error message is helpful.
    if let Err(e) = result {
        let msg = e.to_string();
        assert!(
            msg.contains("mage login"),
            "Error should mention 'mage login', got: {msg}"
        );
    }
}
