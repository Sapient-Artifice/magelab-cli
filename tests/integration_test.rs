use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_version_flag() {
    Command::cargo_bin("mage")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("mage"));
}

#[test]
fn test_help_flag() {
    Command::cargo_bin("mage")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("LLM chat and agentic tool use"));
}

#[test]
fn test_models_subcommand_without_key() {
    // Should fail gracefully without API key
    Command::cargo_bin("mage")
        .unwrap()
        .arg("models")
        .env_remove("MAGELAB_API_KEY")
        .assert()
        .failure();
}

#[test]
fn test_config_subcommand() {
    Command::cargo_bin("mage")
        .unwrap()
        .arg("config")
        .assert()
        .success()
        .stdout(predicate::str::contains("Config:"));
}

#[test]
fn test_conflicting_flags() {
    // --local and --remote together should still parse (last wins or auto)
    Command::cargo_bin("mage")
        .unwrap()
        .args(["--local", "--remote", "--help"])
        .assert()
        .success();
}
