use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn explicit_config_reads_displays_and_saves_only_selected_file() {
    let temporary = tempfile::tempdir().unwrap();
    let selected = temporary.path().join("selected config.toml");
    let other = temporary.path().join("other.toml");
    let original = "local_url = \"http://127.0.0.1:4321\"\ntelemetry = false\n";
    std::fs::write(&selected, original).unwrap();
    std::fs::write(&other, original).unwrap();

    Command::cargo_bin("mage")
        .unwrap()
        .arg("--config")
        .arg(&selected)
        .arg("config")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Config file: {}",
            selected.display()
        )))
        .stdout(predicate::str::contains("http://127.0.0.1:4321"));

    // Global flags must also work after the nested command.
    Command::cargo_bin("mage")
        .unwrap()
        .args([
            "config",
            "set",
            "local_url",
            "http://127.0.0.1:9876",
            "--config",
        ])
        .arg(&selected)
        .assert()
        .success();
    let saved = magelab_cli::config::Config::load_from(&selected).unwrap();
    assert_eq!(saved.local_url, "http://127.0.0.1:9876");
    assert_eq!(std::fs::read_to_string(&other).unwrap(), original);
    assert!(!std::fs::read_to_string(&selected)
        .unwrap()
        .contains("config_path"));

    Command::cargo_bin("mage")
        .unwrap()
        .arg("--config")
        .arg(&selected)
        .arg("config")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Config file: {}",
            selected.display()
        )))
        .stdout(predicate::str::contains("http://127.0.0.1:9876"));
}

#[test]
fn missing_explicit_config_fails_without_creating_a_file() {
    let temporary = tempfile::tempdir().unwrap();
    let missing = temporary.path().join("missing.toml");
    Command::cargo_bin("mage")
        .unwrap()
        .arg("--config")
        .arg(&missing)
        .args(["config", "set", "local_url", "http://127.0.0.1:9876"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to read"));
    assert!(!missing.exists());
}

#[test]
fn malformed_explicit_config_fails_without_overwriting_it() {
    let temporary = tempfile::tempdir().unwrap();
    let malformed = temporary.path().join("malformed.toml");
    let original = "[invalid TOML";
    std::fs::write(&malformed, original).unwrap();
    Command::cargo_bin("mage")
        .unwrap()
        .arg("--config")
        .arg(&malformed)
        .args(["config", "set", "local_url", "http://127.0.0.1:9876"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to parse cli.toml"));
    assert_eq!(std::fs::read_to_string(&malformed).unwrap(), original);
}
