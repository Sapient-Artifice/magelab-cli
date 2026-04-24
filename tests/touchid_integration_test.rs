use assert_cmd::Command;

#[test]
fn no_touchid_flag_accepted_by_version() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "version"])
        .assert()
        .success()
        .stdout(predicates::str::contains("magelab"));
}

#[test]
fn no_touchid_flag_accepted_by_status() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "status"])
        .assert()
        .success();
}

#[test]
fn no_touchid_flag_accepted_by_config() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "config"])
        .assert()
        .success();
}
