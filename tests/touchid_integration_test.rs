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

#[test]
fn no_touchid_flag_accepted_by_login_status() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "login", "--status"])
        .assert()
        .success();
}

#[test]
fn no_touchid_flag_accepted_by_completions() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "completions", "bash"])
        .assert()
        .success();
}

#[test]
fn no_touchid_flag_is_global() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "version"])
        .assert()
        .success();
}
