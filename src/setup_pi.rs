use anyhow::Result;
use std::path::PathBuf;

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
        use std::path::Path;

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

pub fn cmd_setup_pi(uninstall: bool, dev: bool) -> Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let ext_dir = home.join(".pi/agent/extensions/magelab-agent");

    if uninstall {
        if ext_dir.exists() || ext_dir.is_symlink() {
            if ext_dir.is_symlink() {
                std::fs::remove_file(&ext_dir)?;
            } else {
                std::fs::remove_dir_all(&ext_dir)?;
            }
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
        let pi_ok = std::process::Command::new(command_path(pkg_mgr))
            .args(["install", "-g", "@mariozechner/pi-coding-agent"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        sp.finish_and_clear();

        if !pi_ok {
            anyhow::bail!(
                "Failed to install Pi. Try manually:\n  {pkg_mgr} install -g @mariozechner/pi-coding-agent"
            );
        }
        ui::success("Pi coding agent installed");
    }

    // Remove existing install (file or symlink) before reinstalling
    if ext_dir.exists() || ext_dir.is_symlink() {
        if ext_dir.is_symlink() {
            std::fs::remove_file(&ext_dir)?;
        } else {
            std::fs::remove_dir_all(&ext_dir)?;
        }
    }

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
        std::fs::write(ext_dir.join("package.json"), EXT_PACKAGE_JSON)?;
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
        std::fs::write(src_dir.join("binary.ts"), EXT_BINARY_TS)?;
        std::fs::write(client_dir.join("index.ts"), EXT_CLIENT_INDEX_TS)?;
        std::fs::write(client_dir.join("errors.ts"), EXT_CLIENT_ERRORS_TS)?;
        std::fs::write(client_dir.join("coordinator.ts"), EXT_CLIENT_COORDINATOR_TS)?;

        sp.set_message("Installing dependencies...");

        // Try pnpm first, fall back to npm
        let install_result = std::process::Command::new("pnpm")
            .arg("install")
            .current_dir(&ext_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status();

        let ok = match install_result {
            Ok(s) if s.success() => true,
            _ => {
                // Fall back to npm
                std::process::Command::new("npm")
                    .arg("install")
                    .current_dir(&ext_dir)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::piped())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            }
        };

        sp.finish_and_clear();

        if !ok {
            anyhow::bail!(
                "Failed to install dependencies. Ensure pnpm or npm is available.\n\
                 Extension files written to: {}\n\
                 Run manually: cd {} && pnpm install",
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
