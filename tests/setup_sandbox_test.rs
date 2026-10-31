use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn sandbox_setup_help_describes_modes() {
    Command::cargo_bin("mage")
        .unwrap()
        .args(["setup", "sandbox", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Ubuntu or Arch Linux"))
        .stdout(predicate::str::contains("--check"))
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("--yes"));
}

#[test]
fn sandbox_setup_modes_are_mutually_exclusive() {
    for flags in [
        ["--check", "--dry-run"],
        ["--check", "--yes"],
        ["--dry-run", "--yes"],
    ] {
        Command::cargo_bin("mage")
            .unwrap()
            .args(["setup", "sandbox"])
            .args(flags)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("cannot be used with"));
    }
}

#[cfg(not(target_os = "linux"))]
#[test]
fn unsupported_platform_is_rejected_before_config_or_backend_discovery() {
    Command::cargo_bin("mage")
        .unwrap()
        .args([
            "--config",
            "/missing-sandbox-config.toml",
            "setup",
            "sandbox",
            "--check",
        ])
        .env("MAGELAB_API_DIR", "/missing-sandbox-backend")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Ubuntu and Arch Linux only"));
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct Installation {
        temp: TempDir,
        api: PathBuf,
        helper: PathBuf,
        home: PathBuf,
        log: PathBuf,
    }

    impl Installation {
        fn new(python_body: &str) -> Self {
            let temp = TempDir::new().unwrap();
            // Paths must remain individual argv elements, never shell input.
            let api = temp.path().join("Mage Lab $(not-a-command); bundle/api");
            let backend = api.join("backend");
            let helper = backend.join("scripts/setup_sandbox.py");
            let python = api.join("python/bin/python3");
            fs::create_dir_all(helper.parent().unwrap()).unwrap();
            fs::create_dir_all(python.parent().unwrap()).unwrap();
            fs::write(backend.join("main.py"), "# backend fixture\n").unwrap();
            fs::write(&helper, "# helper fixture\n").unwrap();
            fs::write(&python, format!("#!/bin/sh\n{python_body}\n")).unwrap();
            fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).unwrap();
            let home = temp.path().join("home");
            fs::create_dir(&home).unwrap();
            let log = temp.path().join("python-argv");
            Self {
                temp,
                api,
                helper,
                home,
                log,
            }
        }

        fn command(&self) -> Command {
            let mut command = Command::cargo_bin("mage").unwrap();
            command
                .args(["setup", "sandbox"])
                .env("MAGELAB_API_DIR", &self.api)
                .env_remove("MAGELAB_HOME")
                .env("HOME", &self.home)
                .env("XDG_CONFIG_HOME", self.home.join("config"))
                .env("XDG_DATA_HOME", self.home.join("data"))
                .env("PATH", "/not-used-for-bundled-python")
                .env("MAGE_TEST_SETUP_LOG", &self.log)
                .current_dir(self.temp.path())
                .timeout(std::time::Duration::from_secs(10));
            command
        }

        fn assert_no_user_state(&self) {
            assert_eq!(fs::read_dir(&self.home).unwrap().count(), 0);
        }
    }

    const RECORD_ARGS: &str = r#"
printf '%s\n' "$PWD" "$@" > "$MAGE_TEST_SETUP_LOG"
printf 'helper stdout\n'
printf 'helper stderr\n' >&2
"#;

    #[test]
    fn runs_bundled_python_with_isolation_and_preserves_each_argument() {
        for flag in [None, Some("--check"), Some("--dry-run"), Some("--yes")] {
            let install = Installation::new(RECORD_ARGS);
            let mut command = install.command();
            if let Some(flag) = flag {
                command.arg(flag);
            }
            command
                .assert()
                .success()
                .stdout(predicate::str::contains("helper stdout"))
                .stderr(predicate::str::contains("helper stderr"));
            let actual = fs::read_to_string(&install.log).unwrap();
            let mut expected = format!(
                "{}\n-I\n{}\n",
                install.api.join("backend").display(),
                install.helper.display()
            );
            if let Some(flag) = flag {
                expected.push_str(flag);
                expected.push('\n');
            }
            assert_eq!(actual, expected);
            install.assert_no_user_state();
        }
    }

    #[test]
    fn forwards_interactive_input_to_helper() {
        let install = Installation::new("read -r answer\nprintf 'answer=%s\\n' \"$answer\"");
        install
            .command()
            .write_stdin("yes please\n")
            .assert()
            .success()
            .stdout(predicate::str::contains("answer=yes please"));
    }

    #[test]
    fn relative_bundle_override_resolves_before_changing_directory() {
        let install = Installation::new(RECORD_ARGS);
        let relative_api = install.api.strip_prefix(install.temp.path()).unwrap();
        install
            .command()
            .env("MAGELAB_API_DIR", relative_api)
            .arg("--check")
            .assert()
            .success();
        let actual = fs::read_to_string(&install.log).unwrap();
        assert_eq!(
            actual,
            format!(
                "{}\n-I\n{}\n--check\n",
                install.api.join("backend").display(),
                install.helper.display()
            )
        );
    }

    #[test]
    fn preserves_helper_exit_code() {
        let install = Installation::new("printf 'probe failed\\n' >&2\nexit 42");
        install
            .command()
            .arg("--check")
            .assert()
            .code(42)
            .stderr(predicate::str::contains("probe failed"));
        install.assert_no_user_state();
    }

    #[test]
    fn reports_signal_exit_as_shell_status() {
        let install = Installation::new("kill -TERM $$");
        install.command().arg("--check").assert().code(143);
    }

    #[test]
    fn old_backend_fails_before_running_python() {
        let install = Installation::new(RECORD_ARGS);
        fs::remove_file(&install.helper).unwrap();
        install
            .command()
            .arg("--yes")
            .assert()
            .failure()
            .stderr(predicate::str::contains("Upgrade MageLab"));
        assert!(!install.log.exists());
        install.assert_no_user_state();
    }

    #[test]
    fn invalid_explicit_backend_does_not_fall_back() {
        let install = Installation::new(RECORD_ARGS);
        install
            .command()
            .env("MAGELAB_API_DIR", install.temp.path().join("missing"))
            .arg("--check")
            .assert()
            .failure()
            .stderr(predicate::str::contains("MAGELAB_API_DIR"));
        assert!(!install.log.exists());
        install.assert_no_user_state();
    }

    #[test]
    fn configured_local_bundle_is_used_even_when_connection_prefers_remote() {
        let install = Installation::new(RECORD_ARGS);
        let config_dir = install.home.join("config/magelab");
        fs::create_dir_all(&config_dir).unwrap();
        let config_file = config_dir.join("cli.toml");
        let config = format!(
            "magelab_home = '{}'\nprefer = 'remote'\nlocal_url = 'http://127.0.0.1:1'\ngateway_url = 'http://127.0.0.1:1'\ntelemetry = true\n",
            install.api.display()
        );
        fs::write(&config_file, &config).unwrap();
        install
            .command()
            .env_remove("MAGELAB_API_DIR")
            .arg("--dry-run")
            .assert()
            .success();
        assert!(install.log.exists());
        assert_eq!(fs::read_to_string(config_file).unwrap(), config);
        assert_eq!(fs::read_dir(config_dir).unwrap().count(), 1);
        assert_eq!(fs::read_dir(&install.home).unwrap().count(), 1);
    }
}
