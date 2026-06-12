use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_help_shows_vault_subcommand() {
    Command::cargo_bin("mage")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("vault"));
}

#[test]
fn test_vault_help() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["vault", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("get"))
        .stdout(predicate::str::contains("push").not());
}

#[test]
fn test_vault_get_help() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["vault", "get", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("key"));
}
