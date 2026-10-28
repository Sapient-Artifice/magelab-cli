use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

#[cfg(unix)]
fn fake_command(bin_dir: &std::path::Path, name: &str, body: &str) {
    let path = bin_dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn isolated_installer(home: &std::path::Path) -> Command {
    let mut command = Command::cargo_bin("mage").unwrap();
    command
        .arg("setup-pi")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("PATH", home.join("bin"))
        .env("MAGE_TEST_PACKAGE_LOG", home.join("package-manager.log"))
        .env("MAGE_TEST_INSTALL_CWD", home.join("install-cwd"))
        .timeout(std::time::Duration::from_secs(10));
    command
}

#[cfg(unix)]
const RECORD_INSTALL: &str = r#"
printf '%s' "${0##*/}" >> "$MAGE_TEST_PACKAGE_LOG"
printf ' %s' "$@" >> "$MAGE_TEST_PACKAGE_LOG"
printf '\n' >> "$MAGE_TEST_PACKAGE_LOG"
pwd > "$MAGE_TEST_INSTALL_CWD"
"#;

#[cfg(unix)]
const NOISY_FAILURE: &str = r#"
i=0
while [ "$i" -lt 20000 ]; do
    printf 'A package-manager diagnostic long enough to fill stderr pipes.\n' >&2
    i=$((i + 1))
done
exit 1
"#;

#[cfg(unix)]
fn installed_manifest(home: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(
        &fs::read_to_string(home.join(".pi/agent/extensions/magelab-agent/package.json")).unwrap(),
    )
    .unwrap()
}

#[cfg(unix)]
fn assert_runtime_manifest(home: &std::path::Path) {
    let installed = installed_manifest(home);
    let mut source: serde_json::Value =
        serde_json::from_str(include_str!("../extension/package.json")).unwrap();
    source.as_object_mut().unwrap().remove("scripts");
    source.as_object_mut().unwrap().remove("devDependencies");
    assert_eq!(
        installed, source,
        "all runtime dependencies and entrypoints must survive packaging"
    );
    assert!(installed.get("scripts").is_none());
    assert!(installed.get("devDependencies").is_none());
}

#[test]
fn test_setup_pi_help() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["setup-pi", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pi extension"))
        .stdout(predicate::str::contains("--uninstall"));
}

#[test]
fn test_setup_pi_shows_in_help() {
    Command::cargo_bin("mage")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("setup-pi"));
}

#[test]
fn test_setup_pi_detects_missing_pi_and_npm() {
    // Override PATH to ensure neither `pi` nor `npm`/`pnpm` are found
    Command::cargo_bin("mage")
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
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let pnpm = bin_dir.join(if cfg!(windows) { "pnpm.cmd" } else { "pnpm" });
    fs::write(
        &pnpm,
        if cfg!(windows) {
            "@echo 9.0.0\r\n"
        } else {
            "#!/bin/sh\necho 9.0.0\n"
        },
    )
    .unwrap();
    #[cfg(unix)]
    fs::set_permissions(&pnpm, fs::Permissions::from_mode(0o755)).unwrap();

    Command::cargo_bin("mage")
        .unwrap()
        .arg("setup-pi")
        .env("PATH", &bin_dir)
        .env("HOME", tmp.path())
        .env("USERPROFILE", tmp.path())
        .timeout(std::time::Duration::from_secs(10))
        .write_stdin("n\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Install Pi with"));
}

#[test]
fn test_setup_pi_uninstall_when_not_installed() {
    let tmp = TempDir::new().unwrap();

    Command::cargo_bin("mage")
        .unwrap()
        .args(["setup-pi", "--uninstall"])
        .env("HOME", tmp.path())
        .env("USERPROFILE", tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("not installed"));
}

#[test]
#[cfg_attr(target_os = "windows", ignore)] // dirs::home_dir() doesn't respect USERPROFILE overrides reliably
fn test_setup_pi_uninstall_removes_directory() {
    let tmp = TempDir::new().unwrap();
    let ext_dir = tmp.path().join(".pi/agent/extensions/magelab-agent/src");
    fs::create_dir_all(&ext_dir).unwrap();
    fs::write(ext_dir.join("index.ts"), "// test").unwrap();

    Command::cargo_bin("mage")
        .unwrap()
        .args(["setup-pi", "--uninstall"])
        .env("HOME", tmp.path())
        .env("USERPROFILE", tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Removed"));

    assert!(
        !tmp.path()
            .join(".pi/agent/extensions/magelab-agent")
            .exists(),
        "Extension directory should be removed"
    );
}

#[test]
#[cfg(unix)]
fn test_setup_pi_embeds_reusable_client_modules() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fake_command(&bin_dir, "pi", "exit 0");
    fake_command(&bin_dir, "pnpm", RECORD_INSTALL);
    // A successful preferred manager must not invoke the fallback.
    fake_command(&bin_dir, "npm", &format!("{RECORD_INSTALL}\nexit 1"));
    isolated_installer(tmp.path()).assert().success();
    assert_eq!(
        fs::read_to_string(tmp.path().join("package-manager.log")).unwrap(),
        "pnpm install --prod\n"
    );
    let extension = tmp.path().join(".pi/agent/extensions/magelab-agent");
    assert_eq!(
        fs::read_to_string(tmp.path().join("install-cwd"))
            .unwrap()
            .trim(),
        extension.to_str().unwrap()
    );
    assert_runtime_manifest(tmp.path());

    let client_dir = tmp
        .path()
        .join(".pi/agent/extensions/magelab-agent/src/client");
    for module in ["index.ts", "errors.ts", "coordinator.ts"] {
        assert!(
            client_dir.join(module).is_file(),
            "embedded client module {module} should be installed"
        );
    }
}

#[test]
#[cfg(unix)]
fn test_setup_pi_falls_back_to_production_npm_when_pnpm_is_missing() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fake_command(&bin_dir, "pi", "exit 0");
    fake_command(&bin_dir, "npm", RECORD_INSTALL);
    isolated_installer(tmp.path()).assert().success();
    assert_eq!(
        fs::read_to_string(tmp.path().join("package-manager.log")).unwrap(),
        "npm install --omit=dev --no-audit --no-fund\n"
    );
    assert_runtime_manifest(tmp.path());
}

#[test]
#[cfg(unix)]
fn test_setup_pi_drains_failed_pnpm_stderr_before_npm_fallback() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fake_command(&bin_dir, "pi", "exit 0");
    fake_command(
        &bin_dir,
        "pnpm",
        &format!("{RECORD_INSTALL}\n{NOISY_FAILURE}"),
    );
    fake_command(&bin_dir, "npm", RECORD_INSTALL);
    isolated_installer(tmp.path()).assert().success();
    assert_eq!(
        fs::read_to_string(tmp.path().join("package-manager.log")).unwrap(),
        "pnpm install --prod\nnpm install --omit=dev --no-audit --no-fund\n"
    );
    assert_runtime_manifest(tmp.path());
}

#[test]
#[cfg(unix)]
fn test_setup_pi_drains_global_pi_installer_errors_without_installing_extension() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    // Only the fake manager exists. No actual global package command is run.
    fake_command(
        &bin_dir,
        "pnpm",
        &format!(
            r#"
if [ "$1" = "--version" ]; then exit 0; fi
{RECORD_INSTALL}
{NOISY_FAILURE}"#
        ),
    );
    let result = isolated_installer(tmp.path())
        .write_stdin("y\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to install Pi"));
    assert!(
        result.get_output().stderr.len() < 8192,
        "installer diagnostics should be bounded"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("package-manager.log")).unwrap(),
        "pnpm install -g @mariozechner/pi-coding-agent\n"
    );
    assert!(!tmp
        .path()
        .join(".pi/agent/extensions/magelab-agent")
        .exists());
}

#[test]
#[cfg(unix)]
fn test_setup_pi_reports_both_install_failures_with_runtime_retry_command() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fake_command(&bin_dir, "pi", "exit 0");
    fake_command(&bin_dir, "pnpm", "echo pnpm-fixture-error >&2\nexit 1");
    fake_command(&bin_dir, "npm", "echo npm-fixture-error >&2\nexit 2");
    isolated_installer(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("pnpm-fixture-error"))
        .stderr(predicate::str::contains("npm-fixture-error"))
        .stderr(predicate::str::contains(
            "npm install --omit=dev --no-audit --no-fund",
        ));
    assert_runtime_manifest(tmp.path());
}

#[test]
#[cfg(unix)]
fn test_setup_pi_dev_links_original_manifest_without_installing_packages() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fake_command(&bin_dir, "pi", "exit 0");
    fake_command(&bin_dir, "pnpm", &format!("{RECORD_INSTALL}\nexit 1"));
    fake_command(&bin_dir, "npm", &format!("{RECORD_INSTALL}\nexit 1"));
    let source = tmp.path().join("repository/extension");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("src/index.ts"), "// Development entrypoint").unwrap();
    fs::write(
        source.join("package.json"),
        include_str!("../extension/package.json"),
    )
    .unwrap();
    isolated_installer(tmp.path())
        .arg("--dev")
        .current_dir(source.parent().unwrap())
        .assert()
        .success();
    let extension = tmp.path().join(".pi/agent/extensions/magelab-agent");
    assert_eq!(fs::read_link(&extension).unwrap(), source);
    assert!(installed_manifest(tmp.path())
        .get("devDependencies")
        .is_some());
    assert!(installed_manifest(tmp.path()).get("scripts").is_some());
    assert!(!tmp.path().join("package-manager.log").exists());

    // Reinstalling the packaged extension replaces the link, not its target.
    fake_command(&bin_dir, "pnpm", RECORD_INSTALL);
    fs::create_dir_all(source.join("node_modules/vitest")).unwrap();
    fs::write(source.join("node_modules/vitest/source-marker"), "keep").unwrap();
    isolated_installer(tmp.path()).assert().success();
    assert!(!extension.is_symlink());
    assert!(source.join("node_modules/vitest/source-marker").is_file());
    assert_eq!(
        fs::read_to_string(source.join("package.json")).unwrap(),
        include_str!("../extension/package.json")
    );
    assert_runtime_manifest(tmp.path());
    assert!(!extension.join("node_modules/vitest").exists());

    // Old packaged installs may contain development dependencies. They must
    // also disappear on reinstall rather than survive a production install.
    fs::create_dir_all(extension.join("node_modules/vitest")).unwrap();
    fs::write(extension.join("node_modules/vitest/old-marker"), "remove").unwrap();
    isolated_installer(tmp.path()).assert().success();
    assert_runtime_manifest(tmp.path());
    assert!(!extension.join("node_modules/vitest").exists());
}
