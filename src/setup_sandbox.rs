//! Local entry point for the installed backend's versioned sandbox installer.

use anyhow::Result;
use clap::Subcommand;

use crate::config::Config;

#[derive(Debug, Subcommand)]
pub enum SetupCommand {
    /// Install and verify sandbox support on this Ubuntu or Arch Linux machine
    Sandbox {
        /// Check installed dependencies and confinement without installing anything
        #[arg(long, conflicts_with_all = ["dry_run", "yes"])]
        check: bool,
        /// Show the installation plan without changing anything
        #[arg(long, conflicts_with_all = ["check", "yes"])]
        dry_run: bool,
        /// Accept the displayed installation plan for unattended setup
        #[arg(long, conflicts_with_all = ["check", "dry_run"])]
        yes: bool,
    },
}

#[derive(Debug)]
pub struct SetupExitError {
    code: i32,
}

impl std::fmt::Display for SetupExitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Sandbox setup exited with status {}", self.code)
    }
}

impl std::error::Error for SetupExitError {}

pub fn process_exit_code(error: &anyhow::Error) -> Option<i32> {
    error
        .downcast_ref::<SetupExitError>()
        .map(|error| error.code)
}

pub fn require_supported_platform() -> Result<()> {
    if !cfg!(target_os = "linux") {
        anyhow::bail!("Sandbox setup currently supports Ubuntu and Arch Linux only");
    }
    Ok(())
}

pub fn run(config: &Config, command: &SetupCommand) -> Result<()> {
    require_supported_platform()?;
    run_supported(config, command)
}

#[cfg(not(target_os = "linux"))]
fn run_supported(_config: &Config, _command: &SetupCommand) -> Result<()> {
    unreachable!("unsupported platforms are rejected before backend discovery")
}

#[cfg(target_os = "linux")]
fn run_supported(config: &Config, command: &SetupCommand) -> Result<()> {
    use anyhow::Context;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};

    let bundle = crate::detect::find_backend_bundle(config.magelab_home.as_deref())?.context(
        "MageLab backend bundle not found. Install MageLab or set magelab_home or MAGELAB_API_DIR.",
    )?;
    // Resolve relative overrides before changing cwd. Preserve Python symlinks:
    // resolving a virtualenv interpreter to its target can lose the virtualenv.
    let backend_dir = std::path::absolute(&bundle.backend_dir)?;
    let python = if bundle.python.components().count() > 1 {
        std::path::absolute(&bundle.python)?
    } else {
        bundle.python
    };
    let helper = backend_dir.join("scripts/setup_sandbox.py");
    if !helper.is_file() {
        anyhow::bail!(
            "This MageLab backend does not include sandbox setup. Upgrade MageLab before running this command. Expected helper: {}",
            helper.display()
        );
    }

    let mut process = Command::new(&python);
    process
        .arg("-I")
        .arg("-B")
        .arg(&helper)
        .current_dir(&backend_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    match command {
        SetupCommand::Sandbox {
            check,
            dry_run,
            yes,
        } => {
            for (selected, flag) in [
                (*check, "--check"),
                (*dry_run, "--dry-run"),
                (*yes, "--yes"),
            ] {
                if selected {
                    process.arg(flag);
                }
            }
        }
    }
    // Only the versioned helper chooses apt/pacman operations. The CLI never
    // launches itself, the helper, or the Mage backend through sudo, and never
    // connects to config.local_url / a remote backend for host installation.
    let status = process
        .status()
        .with_context(|| format!("Could not run sandbox setup using {}", python.display()))?;
    if !status.success() {
        return Err(SetupExitError {
            code: status
                .code()
                .unwrap_or_else(|| status.signal().map_or(1, |signal| 128 + signal)),
        }
        .into());
    }
    Ok(())
}
