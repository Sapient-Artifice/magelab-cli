use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_version_command() {
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("magelab "));
}

#[test]
fn test_help_shows_subcommands() {
    Command::cargo_bin("magelab")
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
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["connect", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--no-launch"));
}

#[test]
fn test_config_shows_path() {
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("config")
        .assert()
        .success()
        .stdout(predicate::str::contains("Config file:"));
}

#[test]
fn test_auth_token_fails_when_not_logged_in() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["auth", "token"])
        .assert()
        .failure();
}

#[test]
fn test_login_status_when_not_logged_in() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["login", "--status"])
        .env_remove("MAGELAB_API_KEY")
        .assert()
        .success()
        .stdout(predicate::str::contains("Not logged in"));
}

#[test]
fn test_models_without_auth_fails() {
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("models")
        .env_remove("MAGELAB_API_KEY")
        .assert()
        .failure();
}
