use assert_cmd::Command;
use predicates::prelude::*;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_version_command() {
    Command::cargo_bin("mage")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("mage "));
}

#[test]
fn test_help_shows_subcommands() {
    Command::cargo_bin("mage")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("login"))
        .stdout(predicate::str::contains("connect"))
        .stdout(predicate::str::contains("launch"))
        .stdout(predicate::str::contains("models"))
        .stdout(predicate::str::contains("config"));
}

#[test]
fn test_connect_help() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["connect", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--no-launch"));
}

#[test]
fn test_launch_help() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["launch", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("--allow-network"));
}

#[test]
fn test_config_shows_path() {
    Command::cargo_bin("mage")
        .unwrap()
        .arg("config")
        .assert()
        .success()
        .stdout(predicate::str::contains("Config file:"));
}

#[test]
fn test_auth_token_fails_when_not_logged_in() {
    if std::env::var("MAGELAB_SKIP_KEYCHAIN_TESTS").is_ok() {
        eprintln!("Skipping: keychain access may hang in CI");
        return;
    }
    let tmp = TempDir::new().unwrap();
    Command::cargo_bin("mage")
        .unwrap()
        .args(["auth", "token"])
        .timeout(Duration::from_secs(10))
        .env("HOME", tmp.path())
        .env("USERPROFILE", tmp.path())
        .env("MAGELAB_SKIP_KEYCHAIN_TESTS", "1")
        .env_remove("MAGELAB_API_KEY")
        .assert()
        .failure();
}

#[test]
#[ignore] // Flaky: depends on real keychain state; use test_login_status_after_logout instead
fn test_login_status_when_not_logged_in() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["login", "--status"])
        .env_remove("MAGELAB_API_KEY")
        .assert()
        .success()
        .stdout(predicate::str::contains("Not logged in"));
}

#[test]
fn test_models_without_auth_fails() {
    if std::env::var("MAGELAB_SKIP_KEYCHAIN_TESTS").is_ok() {
        eprintln!("Skipping: keychain access may hang in CI");
        return;
    }
    let tmp = TempDir::new().unwrap();
    Command::cargo_bin("mage")
        .unwrap()
        .arg("models")
        .timeout(Duration::from_secs(10))
        .env("HOME", tmp.path())
        .env("USERPROFILE", tmp.path())
        .env("MAGELAB_SKIP_KEYCHAIN_TESTS", "1")
        .env_remove("MAGELAB_API_KEY")
        .assert()
        .failure();
}
