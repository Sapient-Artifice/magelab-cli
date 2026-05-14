use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_vault_help() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["vault", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("get"))
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("delete"));
}

#[test]
fn test_vault_list_subcommand_exists() {
    // vault list should succeed (returns empty list with stub impl)
    Command::cargo_bin("mage")
        .unwrap()
        .args(["--no-touchid", "vault", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No secrets stored"));
}

#[test]
fn test_vault_get_missing_key() {
    // vault get should exit 1 for missing key (stub returns None)
    Command::cargo_bin("mage")
        .unwrap()
        .args(["--no-touchid", "vault", "get", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_help_shows_vault() {
    Command::cargo_bin("mage")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("vault"));
}
