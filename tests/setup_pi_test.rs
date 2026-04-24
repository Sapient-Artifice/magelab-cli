use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_setup_pi_help() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["setup-pi", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pi extension"))
        .stdout(predicate::str::contains("--uninstall"));
}

#[test]
fn test_setup_pi_shows_in_help() {
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("setup-pi"));
}

#[test]
fn test_setup_pi_detects_missing_pi_and_npm() {
    // Override PATH to ensure neither `pi` nor `npm`/`pnpm` are found
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("setup-pi")
        .env("PATH", "/nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains("Pi coding agent is not installed"))
        .stdout(predicate::str::contains("Node.js"));
}

#[test]
fn test_setup_pi_offers_install_when_npm_available() {
    // Pi not on PATH but npm is — should prompt to install
    // We pass "n" via stdin to decline
    Command::cargo_bin("magelab")
        .unwrap()
        .arg("setup-pi")
        .env("PATH", format!("/nonexistent:{}", std::env::var("PATH").unwrap_or_default()))
        .write_stdin("n\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Install Pi with"));
}

#[test]
fn test_setup_pi_uninstall_when_not_installed() {
    let tmp = TempDir::new().unwrap();

    Command::cargo_bin("magelab")
        .unwrap()
        .args(["setup-pi", "--uninstall"])
        .env("HOME", tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("not installed"));
}

#[test]
fn test_setup_pi_uninstall_removes_directory() {
    let tmp = TempDir::new().unwrap();
    let ext_dir = tmp.path().join(".pi/agent/extensions/magelab-agent/src");
    fs::create_dir_all(&ext_dir).unwrap();
    fs::write(ext_dir.join("index.ts"), "// test").unwrap();

    Command::cargo_bin("magelab")
        .unwrap()
        .args(["setup-pi", "--uninstall"])
        .env("HOME", tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Removed"));

    assert!(
        !tmp.path().join(".pi/agent/extensions/magelab-agent").exists(),
        "Extension directory should be removed"
    );
}
