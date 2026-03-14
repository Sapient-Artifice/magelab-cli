use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_version_flag() {
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("magelab"));
}

#[test]
fn test_help_flag() {
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("LLM chat and agentic tool use"));
}

#[test]
fn test_models_subcommand_without_key() {
    // Should fail gracefully without API key
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("models")
        .env_remove("MAGELAB_API_KEY")
        .assert()
        .failure();
}

#[test]
fn test_config_subcommand() {
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("config")
        .assert()
        .success()
        .stdout(predicate::str::contains("Config:"));
}

#[test]
fn test_conflicting_flags() {
    // --local and --remote together should still parse (last wins or auto)
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--local", "--remote", "--help"])
        .assert()
        .success();
}

#[test]
fn test_device_flag_shows_in_help() {
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--device"));
}

#[test]
fn test_login_status_flag() {
    // login --status should succeed and show auth info
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["login", "--status"])
        .env_remove("MAGELAB_API_KEY")
        .assert()
        .success()
        .stdout(predicate::str::contains("Auth:"));
}
