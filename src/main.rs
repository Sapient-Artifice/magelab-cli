mod account;
mod auth;
mod client;
mod config;
mod connect;
mod detect;
mod settings;
mod ui;
mod vault;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use client::remote::RemoteClient;
use config::Config;

#[derive(Parser)]
#[command(
    name = "mage",
    version,
    about = "MageLab CLI — infrastructure management for MageLab"
)]
struct Cli {
    /// Skip Touch ID verification
    #[arg(long, global = true)]
    no_touchid: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate with MageLab (JWT expires after 1 hour; API key from config is used as fallback)
    Login {
        /// Login method: web (default), google (legacy), magic (legacy)
        #[arg(long, default_value = "web")]
        method: String,
        /// Show current auth status
        #[arg(long)]
        status: bool,
    },
    /// Clear stored credentials
    Logout,
    /// Print a fresh auth token to stdout
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// Resolve backend connection and print result
    Connect {
        /// Output as JSON (for programmatic use)
        #[arg(long)]
        json: bool,
        /// Skip headless backend auto-launch (query only)
        #[arg(long)]
        no_launch: bool,
        /// Force local mode only
        #[arg(long, conflicts_with_all = ["relay", "remote"])]
        local: bool,
        /// Force relay mode only
        #[arg(long, conflicts_with_all = ["local", "remote"])]
        relay: bool,
        /// Force remote REST mode only
        #[arg(long, conflicts_with_all = ["local", "relay"])]
        remote: bool,
    },
    /// Start the headless backend
    Launch {
        /// Block until backend is healthy, then print URL
        #[arg(long)]
        wait: bool,
    },
    /// Show backend health and connection info
    Status,
    /// Manage relay devices
    Devices {
        #[command(subcommand)]
        action: Option<DevicesAction>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List available models
    Models,
    /// Show token usage summary
    Usage,
    /// Show account credit balance
    Balance,
    /// Manage API keys
    Keys {
        #[command(subcommand)]
        action: KeysAction,
    },
    /// Read secrets from the desktop app's encrypted vault
    Vault {
        #[command(subcommand)]
        action: Option<VaultAction>,
    },
    /// Show or update CLI configuration (local config file)
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// Show or change backend runtime settings (via WebSocket)
    Settings {
        #[command(subcommand)]
        action: Option<SettingsAction>,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
    /// Install the @magelab/agent Pi extension
    SetupPi {
        /// Remove the extension instead of installing
        #[arg(long)]
        uninstall: bool,
        /// Link to repo source instead of embedding (for development)
        #[arg(long)]
        dev: bool,
    },
    /// Print version
    Version,
}

#[derive(Subcommand)]
enum AuthAction {
    /// Print auth token to stdout. Uses JWT if valid, attempts refresh if expired, falls back to API key from config.
    Token,
}

#[derive(Subcommand)]
enum DevicesAction {
    /// Bind to a specific device for relay
    Bind { device_id: String },
    /// Unbind from current device
    Detach,
}

#[derive(Subcommand)]
enum KeysAction {
    List,
    Create,
    Revoke { id: String },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set a config value
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum SettingsAction {
    /// Set a backend setting (model, voice)
    Set {
        /// Setting name: model, voice
        key: String,
        /// New value
        value: String,
    },
}

#[derive(Subcommand)]
enum VaultAction {
    /// Print a secret value to stdout
    Get {
        /// Secret key name (e.g., llm_api_key, magelab_api_key)
        key: String,
    },
    /// Push all vault secrets to the running local backend
    Push,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::load().unwrap_or_default();

    // Set Touch ID disable flag before any command dispatch
    auth::touchid::set_disabled(cli.no_touchid);

    match cli.command {
        Commands::Login { method, status } => {
            if status {
                return cmd_login_status(&config).await;
            }
            cmd_login(&config, &method).await
        }
        Commands::Logout => {
            auth::touchid::verify(auth::touchid::Tier::Sensitive, "log out")?;
            cmd_logout(&config)
        }
        Commands::Auth { action } => match action {
            AuthAction::Token => {
                auth::touchid::verify(auth::touchid::Tier::Cached, "access auth token")?;
                cmd_auth_token(&config).await
            }
        },
        Commands::Connect {
            json,
            no_launch,
            local,
            relay,
            remote,
        } => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "connect")?;
            cmd_connect(&config, json, no_launch, local, relay, remote).await
        }
        Commands::Launch { wait } => cmd_launch(&config, wait).await,
        Commands::Status => cmd_status(&config).await,
        Commands::Devices { action, json } => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "access devices")?;
            cmd_devices(&config, action, json).await
        }
        Commands::Models => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "access account info")?;
            cmd_account(&config, "models").await
        }
        Commands::Usage => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "access account info")?;
            cmd_account(&config, "usage").await
        }
        Commands::Balance => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "access account info")?;
            cmd_account(&config, "balance").await
        }
        Commands::Keys { action } => {
            match &action {
                KeysAction::Create | KeysAction::Revoke { .. } => {
                    auth::touchid::verify(auth::touchid::Tier::Sensitive, "manage API keys")?;
                }
                KeysAction::List => {
                    auth::touchid::verify(auth::touchid::Tier::Cached, "access API keys")?;
                }
            }
            cmd_keys(&config, action).await
        }
        Commands::Vault { action } => cmd_vault(&config, action).await,
        Commands::Config { action } => cmd_config(&mut config, action),
        Commands::Settings { action } => cmd_settings(&config, action).await,
        Commands::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "mage",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Commands::SetupPi { uninstall, dev } => cmd_setup_pi(uninstall, dev),
        Commands::Version => {
            println!("mage {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

async fn cmd_login(config: &Config, method: &str) -> Result<()> {
    let m: auth::oauth::LoginMethod = method.parse()?;
    auth::oauth::login_with_method(&config.gateway_url, &m).await?;
    Ok(())
}

async fn cmd_login_status(config: &Config) -> Result<()> {
    let creds = auth::credentials::Credentials::load().unwrap_or_default();
    if let Some(email) = &creds.email {
        println!("Logged in as: {}", email);
    } else {
        println!("Not logged in.");
    }
    if creds.access_token.is_some() {
        let valid = creds.is_token_valid();
        println!("Token: {}", if valid { "valid" } else { "expired" });
    }
    if let Some(key) = config.api_key() {
        let preview = if key.len() > 8 {
            format!("{}...{}", &key[..4], &key[key.len() - 4..])
        } else {
            "****".to_string()
        };
        println!("API key: {}", preview);
    }
    Ok(())
}

fn cmd_logout(config: &Config) -> Result<()> {
    auth::credentials::Credentials::clear()?;
    println!("Logged out.");
    if config.api_key().is_some() {
        println!("Note: API key in config is still active. Use 'mage keys revoke' to deactivate it.");
    }
    Ok(())
}

async fn cmd_auth_token(config: &Config) -> Result<()> {
    let creds = auth::credentials::Credentials::load()?;
    if !creds.is_token_valid() {
        if let Some(refresh) = &creds.refresh_token {
            let new_creds = auth::oauth::refresh_token(&config.gateway_url, refresh).await?;
            new_creds.save()?;
            if let Some(token) = &new_creds.access_token {
                print!("{}", token); // No newline — for piping
                return Ok(());
            }
        }
        anyhow::bail!("Token expired and refresh failed. Run: mage login");
    }
    match &creds.access_token {
        Some(token) => {
            print!("{}", token);
            Ok(())
        }
        None => anyhow::bail!("Not logged in. Run: mage login"),
    }
}

async fn cmd_connect(
    config: &Config,
    json: bool,
    no_launch: bool,
    local: bool,
    relay: bool,
    remote: bool,
) -> Result<()> {
    let result = if local {
        if detect::check_backend_health(&config.local_url).await {
            connect::ConnectResult {
                url: Some(connect::to_ws_url(&config.local_url)),
                token: None,
                mode: "local".to_string(),
                model: Some(config.default_model.clone()),
            }
        } else {
            connect::ConnectResult {
                url: None,
                token: None,
                mode: "none".to_string(),
                model: None,
            }
        }
    } else if relay || remote {
        let mut r = connect::resolve(config, true).await?;
        if relay && r.mode != "relay" {
            r = connect::ConnectResult {
                url: None,
                token: None,
                mode: "none".to_string(),
                model: None,
            };
        }
        if remote && r.mode != "remote" {
            if let Some(api_key) = config.api_key() {
                r = connect::ConnectResult {
                    url: Some(config.gateway_url.clone()),
                    token: Some(api_key),
                    mode: "remote".to_string(),
                    model: Some(config.default_model.clone()),
                };
            }
        }
        r
    } else {
        connect::resolve(config, no_launch).await?
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        match result.mode.as_str() {
            "local" => println!(
                "Connected: local backend ({})",
                result.url.as_deref().unwrap_or("?")
            ),
            "relay" => println!("Connected: relay via gateway"),
            "remote" => println!("Connected: gateway REST (chat only)"),
            "none" => println!("No connection available. Run: mage login"),
            _ => println!("Mode: {}", result.mode),
        }
        if let Some(model) = &result.model {
            println!("Model: {}", model);
        }
    }

    Ok(())
}

async fn cmd_launch(config: &Config, wait: bool) -> Result<()> {
    let home = detect::find_magelab_home(config.magelab_home.as_deref()).ok_or_else(|| {
        anyhow::anyhow!("MageLab installation not found. Set magelab_home in config.")
    })?;

    let port = detect::port_from_url(&config.local_url);
    let child = detect::launch_backend_headless(&home, port)?;
    // Detach the child so it outlives this CLI invocation
    std::mem::forget(child);

    if wait {
        let sp = ui::spinner("Starting backend...");
        detect::wait_for_backend(&config.local_url, std::time::Duration::from_secs(30)).await?;
        sp.finish_and_clear();
        ui::success(&format!("Backend ready at {}", config.local_url));
    } else {
        ui::success("Backend launched");
        ui::label("check", "mage status");
    }

    Ok(())
}

async fn cmd_status(config: &Config) -> Result<()> {
    let healthy = detect::check_backend_health(&config.local_url).await;
    println!(
        "Backend: {}",
        if healthy { "running" } else { "not running" }
    );
    println!("URL: {}", config.local_url);

    let creds = auth::credentials::Credentials::load().unwrap_or_default();
    let logged_in = creds.access_token.is_some() && creds.is_token_valid();
    println!(
        "Auth: {}",
        if logged_in {
            "logged in"
        } else {
            "not logged in"
        }
    );

    if let Some(key) = config.api_key() {
        println!("API key: configured ({}...)", &key[..4.min(key.len())]);
    }

    Ok(())
}

async fn cmd_devices(config: &Config, action: Option<DevicesAction>, json: bool) -> Result<()> {
    let creds = auth::credentials::Credentials::load()?;
    let jwt = creds
        .access_token
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run: mage login"))?;

    match action {
        None => {
            let devices = detect::discover_devices(&config.gateway_url, jwt).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&devices)?);
            } else if devices.is_empty() {
                println!("No devices online.");
            } else {
                println!("Online devices:");
                for d in &devices {
                    println!("  {}", d);
                }
            }
        }
        Some(DevicesAction::Bind { device_id }) => {
            let mut cfg = config.clone();
            cfg.default_device = Some(device_id.clone());
            cfg.save()?;
            println!("Bound to device: {}", device_id);
        }
        Some(DevicesAction::Detach) => {
            let mut cfg = config.clone();
            cfg.default_device = None;
            cfg.save()?;
            println!("Detached from device.");
        }
    }

    Ok(())
}

async fn cmd_account(config: &Config, kind: &str) -> Result<()> {
    let token = get_token(config).await?;
    let client = RemoteClient::new(&config.gateway_url, &token);
    match kind {
        "models" => account::list_models(&client).await,
        "usage" => account::show_usage(&client).await,
        "balance" => account::show_balance(&client).await,
        _ => unreachable!(),
    }
}

async fn cmd_keys(config: &Config, action: KeysAction) -> Result<()> {
    let token = get_token(config).await?;
    let client = RemoteClient::new(&config.gateway_url, &token);
    match action {
        KeysAction::List => account::list_keys(&client).await,
        KeysAction::Create => account::create_key(&client).await,
        KeysAction::Revoke { id } => account::revoke_key(&client, &id).await,
    }
}

async fn cmd_vault(config: &Config, action: Option<VaultAction>) -> Result<()> {
    match action {
        None => {
            // mage vault — list key names
            auth::touchid::verify(auth::touchid::Tier::Cached, "list vault keys")?;
            let vault = vault::Vault::open().map_err(|e| match e {
                vault::VaultError::NotFound(_) => {
                    anyhow::anyhow!("No vault found. Open the desktop app to create one.")
                }
                vault::VaultError::KeychainUnavailable(_) => anyhow::anyhow!(
                    "Vault exists but no password in keychain. Open the desktop app first, or set MAGELAB_VAULT_PASSWORD env var."
                ),
                other => anyhow::anyhow!("{other}"),
            })?;

            let keys = vault.list()?;
            if keys.is_empty() {
                println!("Vault is empty. Store secrets in the desktop app.");
            } else {
                for key in &keys {
                    println!("{}", key);
                }
            }
            Ok(())
        }
        Some(VaultAction::Get { key }) => {
            auth::touchid::verify(auth::touchid::Tier::Sensitive, "read vault secret")?;
            let vault = vault::Vault::open()?;
            match vault.get(&key)? {
                Some(value) => {
                    print!("{}", value); // No newline — for piping
                    Ok(())
                }
                None => anyhow::bail!("Key '{}' not found in vault", key),
            }
        }
        Some(VaultAction::Push) => {
            auth::touchid::verify(auth::touchid::Tier::Sensitive, "push vault secrets")?;
            push_vault_secrets(config).await
        }
    }
}

async fn push_vault_secrets(config: &Config) -> Result<()> {
    let vault = vault::Vault::open()?;
    let secrets = vault.all_secrets()?;

    if secrets.is_empty() {
        println!("No secrets in vault to push.");
        return Ok(());
    }

    let url = format!("{}/api/auth/push_secrets", config.local_url);
    let body = serde_json::json!({ "secrets": secrets });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Backend not running. Start it with `mage launch`"))?;

    if resp.status().is_success() {
        ui::success(&format!("Pushed {} secret(s) to backend", secrets.len()));
        Ok(())
    } else {
        anyhow::bail!("Push failed with status {}", resp.status())
    }
}

fn cmd_config(config: &mut Config, action: Option<ConfigAction>) -> Result<()> {
    match action {
        None => {
            let path = Config::path()?;
            println!("Config file: {}", path.display());
            println!();
            if path.exists() {
                print!("{}", std::fs::read_to_string(&path)?);
            } else {
                println!("(no config file — using defaults)");
            }
            Ok(())
        }
        Some(ConfigAction::Set { key, value }) => {
            config.set_value(&key, &value)?;
            config.save()?;
            println!("Set {} = {}", key, value);
            Ok(())
        }
    }
}

/// Show or change backend runtime settings via WebSocket
async fn cmd_settings(config: &Config, action: Option<SettingsAction>) -> Result<()> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::connect_async;

    // Validate key early before connecting
    if let Some(SettingsAction::Set { ref key, .. }) = action {
        match key.as_str() {
            "model" | "voice" => {}
            _ => anyhow::bail!("Unknown setting '{}'. Available: model, voice", key),
        }
    }

    let ws_url = connect::to_ws_url(&config.local_url);

    let (mut ws, _) = connect_async(&ws_url).await.map_err(|_| {
        anyhow::anyhow!("Backend not running. Start it with `mage launch`")
    })?;

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut got_response = false;

    match action {
        None => {
            let msg = serde_json::json!({"type": "get_runtime_config"});
            ws.send(tokio_tungstenite::tungstenite::Message::Text(
                msg.to_string(),
            ))
            .await?;

            while let Ok(Some(Ok(raw))) = tokio::time::timeout_at(deadline, ws.next()).await {
                if let tokio_tungstenite::tungstenite::Message::Text(text) = raw {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    if v.get("type").and_then(|t| t.as_str()) == Some("runtime_config") {
                        let map: std::collections::HashMap<String, serde_json::Value> =
                            serde_json::from_value(v).unwrap_or_default();
                        let s = settings::RuntimeSettings::from_config_map(&map);
                        println!("Model:    {}", s.model);
                        println!("Provider: {}", s.provider);
                        println!("Endpoint: {}", s.endpoint);
                        println!("Mute:     {}", s.mute);
                        // voice_model isn't in RuntimeSettings yet, get from raw map
                        if let Some(voice) = map.get("voice_model").and_then(|v| v.as_str()) {
                            println!("Voice:    {}", voice);
                        }
                        if let Some(tts) = map.get("tts_stream").and_then(|v| v.as_bool()) {
                            println!("TTS:      {}", if tts { "streaming" } else { "buffered" });
                        }
                        if let Some(chat) = map.get("chat_id").and_then(|v| v.as_str()) {
                            println!("Chat:     {}", chat);
                        }
                        got_response = true;
                        break;
                    }
                }
            }
        }
        Some(SettingsAction::Set { key, value }) => {
            let msg = match key.as_str() {
                "model" => serde_json::json!({"type": "set_model", "model": value}),
                "voice" => serde_json::json!({"type": "set_voice", "voice": value}),
                _ => unreachable!(),
            };
            let expect_type = format!("set_{}_result", key);

            ws.send(tokio_tungstenite::tungstenite::Message::Text(
                msg.to_string(),
            ))
            .await?;

            while let Ok(Some(Ok(raw))) = tokio::time::timeout_at(deadline, ws.next()).await {
                if let tokio_tungstenite::tungstenite::Message::Text(text) = raw {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    if v.get("type").and_then(|t| t.as_str()) == Some(&expect_type) {
                        if v["ok"].as_bool() == Some(true) {
                            ui::success(&format!("{} → {}", key, value));
                        } else {
                            anyhow::bail!("Backend rejected update");
                        }
                        got_response = true;
                        break;
                    }
                }
            }
        }
    }

    ws.close(None).await.ok();
    if !got_response {
        anyhow::bail!("Backend did not respond within 10 seconds");
    }
    Ok(())
}

/// Get the best available token (JWT preferred, API key fallback)
async fn get_token(config: &Config) -> Result<String> {
    let creds = auth::credentials::Credentials::load().unwrap_or_default();
    if let Some(token) = creds.try_get_valid_jwt(&config.gateway_url).await {
        return Ok(token);
    }
    if creds.access_token.is_some() {
        eprintln!("Warning: JWT expired and refresh failed. Falling back to API key.");
    }
    config
        .api_key()
        .ok_or_else(|| anyhow::anyhow!("Not authenticated. Run: mage login"))
}

// -- Extension files embedded at compile time --

const EXT_PACKAGE_JSON: &str = include_str!("../extension/package.json");
const EXT_TSCONFIG: &str = include_str!("../extension/tsconfig.json");
const EXT_INDEX_TS: &str = include_str!("../extension/src/index.ts");
const EXT_CONNECTION_TS: &str = include_str!("../extension/src/connection.ts");
const EXT_WEBSOCKET_TS: &str = include_str!("../extension/src/websocket.ts");
const EXT_TOOLS_TS: &str = include_str!("../extension/src/tools.ts");
const EXT_GATEWAY_TS: &str = include_str!("../extension/src/gateway.ts");

fn cmd_setup_pi(uninstall: bool, dev: bool) -> Result<()> {
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
    let pi_installed = std::process::Command::new("pi")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !pi_installed {
        println!("Pi coding agent is not installed.");
        println!();

        // Check if npm/pnpm is available
        let has_pnpm = std::process::Command::new("pnpm")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        let has_npm = std::process::Command::new("npm")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

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
        let pi_ok = std::process::Command::new(pkg_mgr)
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
        std::fs::create_dir_all(&src_dir)?;

        // Write embedded files
        std::fs::write(ext_dir.join("package.json"), EXT_PACKAGE_JSON)?;
        std::fs::write(ext_dir.join("tsconfig.json"), EXT_TSCONFIG)?;
        std::fs::write(src_dir.join("index.ts"), EXT_INDEX_TS)?;
        std::fs::write(src_dir.join("connection.ts"), EXT_CONNECTION_TS)?;
        std::fs::write(src_dir.join("websocket.ts"), EXT_WEBSOCKET_TS)?;
        std::fs::write(src_dir.join("tools.ts"), EXT_TOOLS_TS)?;
        std::fs::write(src_dir.join("gateway.ts"), EXT_GATEWAY_TS)?;

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
    let backend_running = std::net::TcpStream::connect_timeout(
        &addr,
        std::time::Duration::from_millis(500),
    )
    .is_ok();

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
