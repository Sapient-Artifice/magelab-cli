use assert_cmd::Command;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn no_touchid_flag_accepted_by_version() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["--no-touchid", "version"])
        .assert()
        .success()
        .stdout(predicates::str::contains("mage"));
}

#[test]
fn no_touchid_flag_accepted_by_status() {
    if std::env::var("MAGELAB_SKIP_KEYCHAIN_TESTS").is_ok() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    Command::cargo_bin("mage")
        .unwrap()
        .args(["--no-touchid", "status"])
        .timeout(Duration::from_secs(10))
        .env("HOME", tmp.path())
        .env("USERPROFILE", tmp.path())
        .env("MAGELAB_SKIP_KEYCHAIN_TESTS", "1")
        .assert()
        .success();
}

#[test]
fn no_touchid_flag_accepted_by_config() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["--no-touchid", "config"])
        .assert()
        .success();
}

#[test]
fn no_touchid_flag_accepted_by_login_status() {
    if std::env::var("MAGELAB_SKIP_KEYCHAIN_TESTS").is_ok() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    Command::cargo_bin("mage")
        .unwrap()
        .args(["--no-touchid", "login", "--status"])
        .timeout(Duration::from_secs(10))
        .env("HOME", tmp.path())
        .env("USERPROFILE", tmp.path())
        .env("MAGELAB_SKIP_KEYCHAIN_TESTS", "1")
        .assert()
        .success();
}

#[test]
fn no_touchid_flag_accepted_by_completions() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["--no-touchid", "completions", "bash"])
        .assert()
        .success();
}

#[test]
fn no_touchid_flag_is_global() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["--no-touchid", "version"])
        .assert()
        .success();
}
