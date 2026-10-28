use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::detect;
use crate::ui;

// -- Extension files embedded at compile time --

const EXT_PACKAGE_JSON: &str = include_str!("../extension/package.json");
const EXT_TSCONFIG: &str = include_str!("../extension/tsconfig.json");
const EXT_INDEX_TS: &str = include_str!("../extension/src/index.ts");
const EXT_BINARY_TS: &str = include_str!("../extension/src/binary.ts");
const EXT_CONNECTION_TS: &str = include_str!("../extension/src/connection.ts");
const EXT_WEBSOCKET_TS: &str = include_str!("../extension/src/websocket.ts");
const EXT_TOOLS_TS: &str = include_str!("../extension/src/tools.ts");
const EXT_GATEWAY_TS: &str = include_str!("../extension/src/gateway.ts");
const EXT_COMMANDS_TS: &str = include_str!("../extension/src/commands.ts");
const EXT_VALIDATION_TS: &str = include_str!("../extension/src/validation.ts");
const EXT_PROTOCOL_TS: &str = include_str!("../extension/src/protocol.ts");
const EXT_CLIENT_INDEX_TS: &str = include_str!("../extension/src/client/index.ts");
const EXT_CLIENT_ERRORS_TS: &str = include_str!("../extension/src/client/errors.ts");
const EXT_CLIENT_COORDINATOR_TS: &str = include_str!("../extension/src/client/coordinator.ts");

fn command_path(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let path = Path::new(name);
        if path.extension().is_some() {
            return PathBuf::from(name);
        }

        let path_ext = std::env::var_os("PATHEXT")
            .and_then(|v| v.into_string().ok())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
        let extensions: Vec<String> = path_ext
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| ext.to_ascii_lowercase())
            .collect();

        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                for ext in &extensions {
                    let candidate = dir.join(format!("{name}{ext}"));
                    if candidate.exists() {
                        return candidate;
                    }
                }
            }
        }
    }

    PathBuf::from(name)
}

fn command_succeeds(name: &str, args: &[&str]) -> bool {
    std::process::Command::new(command_path(name))
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn runtime_package_json() -> Result<String> {
    let mut manifest: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(EXT_PACKAGE_JSON)?;
    manifest.remove("devDependencies");
    manifest.remove("scripts");
    Ok(format!("{}\n", serde_json::to_string_pretty(&manifest)?))
}

fn install_packages(manager: &str, args: &[&str], directory: Option<&Path>) -> Result<()> {
    let mut command = std::process::Command::new(command_path(manager));
    command.args(args);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    // Drain stderr while waiting: package managers can exceed a pipe's buffer
    // even when their installation fails before downloading anything.
    let output = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .with_context(|| format!("Could not start {manager}"))?;
    if !output.status.success() {
        let tail = &output.stderr[output.stderr.len().saturating_sub(4096)..];
        anyhow::bail!(
            "{manager} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(tail).trim()
        );
    }
    Ok(())
}

fn remove_extension(path: &Path) -> std::io::Result<bool> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTypeExt;
            // Windows directory links require RemoveDirectory, including
            // dangling links. Never follow the link into a development repo.
            if metadata.file_type().is_symlink_dir() {
                std::fs::remove_dir(path)?;
            } else {
                std::fs::remove_file(path)?;
            }
        }
        #[cfg(not(windows))]
        std::fs::remove_file(path)?;
    } else if metadata.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(true)
}

pub fn cmd_setup_pi(uninstall: bool, dev: bool) -> Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let ext_dir = home.join(".pi/agent/extensions/magelab-agent");

    if uninstall {
        if remove_extension(&ext_dir)? {
            ui::success("Removed Pi extension");
            ui::label("path", &ext_dir.display().to_string());
        } else {
            println!("Extension not installed.");
        }
        return Ok(());
    }

    // Check if Pi is installed, offer to install if not
    let pi_installed = command_succeeds("pi", &["--version"]);

    if !pi_installed {
        println!("Pi coding agent is not installed.");
        println!();

        // Check if npm/pnpm is available
        let has_pnpm = command_succeeds("pnpm", &["--version"]);
        let has_npm = command_succeeds("npm", &["--version"]);

        if !has_pnpm && !has_npm {
            println!("Neither pnpm nor npm found. Install Node.js first:");
            println!("  https://nodejs.org/");
            println!();
            println!("Then run: mage setup-pi");
            return Ok(());
        }

        let pkg_mgr = if has_pnpm { "pnpm" } else { "npm" };
        print!("Install Pi with {pkg_mgr}? [Y/n] ");
        std::io::Write::flush(&mut std::io::stdout())?;

        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        let answer = answer.trim().to_lowercase();

        if !answer.is_empty() && answer != "y" && answer != "yes" {
            println!();
            println!("Install Pi manually:");
            println!("  {pkg_mgr} install -g @mariozechner/pi-coding-agent");
            println!();
            println!("Then run: mage setup-pi");
            return Ok(());
        }

        let sp = ui::spinner("Installing Pi coding agent...");
        let pi_result = install_packages(
            pkg_mgr,
            &["install", "-g", "@mariozechner/pi-coding-agent"],
            None,
        );

        sp.finish_and_clear();

        if let Err(error) = pi_result {
            anyhow::bail!(
                "Failed to install Pi: {error:#}\nTry manually:\n  {pkg_mgr} install -g @mariozechner/pi-coding-agent"
            );
        }
        ui::success("Pi coding agent installed");
    }

    // Remove existing install (file or symlink) before reinstalling
    remove_extension(&ext_dir)?;

    if dev {
        // Dev mode: symlink to the repo's extension/ directory
        let cli_dir = std::env::current_dir()?;
        let ext_source = {
            let candidate = cli_dir.join("extension");
            if candidate.join("src/index.ts").exists() {
                candidate
            } else {
                anyhow::bail!(
                    "Run from the magelab-cli repo directory, or use --dev from a directory containing extension/src/index.ts"
                );
            }
        };

        let extensions_dir = ext_dir.parent().unwrap();
        std::fs::create_dir_all(extensions_dir)?;

        #[cfg(unix)]
        std::os::unix::fs::symlink(&ext_source, &ext_dir)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&ext_source, &ext_dir)?;

        ui::success("Pi extension linked (dev mode)");
        ui::label("symlink", &ext_dir.display().to_string());
        ui::label("target", &ext_source.display().to_string());
    } else {
        let sp = ui::spinner("Installing @magelab/agent extension...");

        // Create directory structure
        let src_dir = ext_dir.join("src");
        let client_dir = src_dir.join("client");
        std::fs::create_dir_all(&src_dir)?;
        std::fs::create_dir_all(&client_dir)?;

        // Write embedded files
        std::fs::write(ext_dir.join("package.json"), runtime_package_json()?)?;
        std::fs::write(ext_dir.join("tsconfig.json"), EXT_TSCONFIG)?;
        std::fs::write(src_dir.join("index.ts"), EXT_INDEX_TS)?;
        std::fs::write(src_dir.join("binary.ts"), EXT_BINARY_TS)?;
        std::fs::write(src_dir.join("connection.ts"), EXT_CONNECTION_TS)?;
        std::fs::write(src_dir.join("websocket.ts"), EXT_WEBSOCKET_TS)?;
        std::fs::write(src_dir.join("tools.ts"), EXT_TOOLS_TS)?;
        std::fs::write(src_dir.join("gateway.ts"), EXT_GATEWAY_TS)?;
        std::fs::write(src_dir.join("commands.ts"), EXT_COMMANDS_TS)?;
        std::fs::write(src_dir.join("validation.ts"), EXT_VALIDATION_TS)?;
        std::fs::write(src_dir.join("protocol.ts"), EXT_PROTOCOL_TS)?;
        std::fs::write(client_dir.join("index.ts"), EXT_CLIENT_INDEX_TS)?;
        std::fs::write(client_dir.join("errors.ts"), EXT_CLIENT_ERRORS_TS)?;
        std::fs::write(client_dir.join("coordinator.ts"), EXT_CLIENT_COORDINATOR_TS)?;

        sp.set_message("Installing dependencies...");

        // Try pnpm first, fall back to npm
        let install_result = match install_packages("pnpm", &["install", "--prod"], Some(&ext_dir))
        {
            Ok(()) => Ok(()),
            Err(pnpm_error) => install_packages(
                "npm",
                &["install", "--omit=dev", "--no-audit", "--no-fund"],
                Some(&ext_dir),
            )
            .with_context(|| format!("pnpm installation failed: {pnpm_error:#}")),
        };

        sp.finish_and_clear();

        if let Err(error) = install_result {
            anyhow::bail!(
                "Failed to install runtime dependencies: {error:#}\n\
                 Extension files written to: {}\n\
                 Run manually: cd {} && npm install --omit=dev --no-audit --no-fund",
                ext_dir.display(),
                ext_dir.display()
            );
        }

        ui::success("Pi extension installed");
        ui::label("path", &ext_dir.display().to_string());
    }

    // Check if backend is running (quick TCP probe)
    let config = Config::load().unwrap_or_default();
    let port = detect::port_from_url(&config.local_url);
    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    let backend_running =
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(500)).is_ok();

    println!();
    println!("  Quickstart");
    println!("  ----------");
    if !backend_running {
        println!("  1. Start MageLab backend:");
        println!("     mage launch --wait");
        println!("  2. Start Pi (MageLab tools auto-register):");
        println!("     pi");
    } else {
        ui::label("backend", &format!("running at 127.0.0.1:{}", port));
        println!("  1. Start Pi (MageLab tools auto-register):");
        println!("     pi");
    }
    println!();
    println!("  Try a MageLab tool in Pi:");
    println!("     \"use run_python to calculate fibonacci(20)\"");
    println!("     \"use search_web to find Rust async patterns\"");
    println!();

    Ok(())
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::remove_extension;
    use std::fs;
    use std::path::Path;

    fn directory_link(source: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(source, link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(source, link).unwrap();
    }

    #[test]
    fn removing_development_link_preserves_source_directory() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let link = temp.path().join("installed");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("package.json"), "source manifest").unwrap();
        directory_link(&source, &link);
        assert!(remove_extension(&link).unwrap());
        assert!(fs::symlink_metadata(&link).is_err());
        assert_eq!(
            fs::read_to_string(source.join("package.json")).unwrap(),
            "source manifest"
        );
        assert!(!remove_extension(&link).unwrap());
    }

    #[test]
    fn removing_dangling_development_link_succeeds() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let link = temp.path().join("installed");
        fs::create_dir(&source).unwrap();
        directory_link(&source, &link);
        fs::remove_dir(&source).unwrap();
        assert!(remove_extension(&link).unwrap());
        assert!(fs::symlink_metadata(&link).is_err());
        assert!(!source.exists());
    }
}
