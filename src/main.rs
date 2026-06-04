mod account;
mod analytics;
mod auth;
mod client;
mod config;
mod connect;
mod detect;
mod settings;
mod setup_pi;
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
        /// Probe this HTTP backend URL instead of config.local_url
        #[arg(long, conflicts_with = "ws")]
        url: Option<String>,
        /// Probe this WebSocket backend URL instead of config.local_url
        #[arg(long, conflicts_with = "url")]
        ws: Option<String>,
    },
    /// Start the headless backend
    Launch {
        /// Block until backend is healthy, then print URL
        #[arg(long)]
        wait: bool,
        /// Bind host for the headless backend
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Bind port for the headless backend (defaults to local_url port)
        #[arg(long)]
        port: Option<u16>,
        /// Print the resolved backend launch command without spawning it
        #[arg(long)]
        dry_run: bool,
        /// Allow binding the full-access backend to a non-localhost interface
        #[arg(long)]
        allow_network: bool,
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

impl VaultAction {
    fn into_vault_action(self) -> vault::VaultAction {
        match self {
            VaultAction::Get { key } => vault::VaultAction::Get { key },
            VaultAction::Push => vault::VaultAction::Push,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::load().unwrap_or_default();

    // Warn if plaintext API key exists in cli.toml (deprecated)
    if config.api_key.is_some() {
        eprintln!(
            "Warning: Plaintext API key in cli.toml is deprecated.\n\
             Set it in the desktop app or use MAGELAB_API_KEY env var instead."
        );
    }

    if config.telemetry() {
        analytics::init().await;
    }

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
            url,
            ws,
        } => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "connect")?;
            cmd_connect(&config, json, no_launch, local, relay, remote, url, ws).await
        }
        Commands::Launch {
            wait,
            host,
            port,
            dry_run,
            allow_network,
        } => cmd_launch(&mut config, wait, &host, port, dry_run, allow_network).await,
        Commands::Status => cmd_status(&config).await,
        Commands::Devices { action, json } => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "access devices")?;
            cmd_devices(&config, action, json).await
        }
        Commands::Models => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "access account info")?;
            if let Ok(creds) = auth::Credentials::load() {
                if let Some(uid) = &creds.user_id {
                    analytics::track_activation(uid, "models", &mut config).await;
                }
            }
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
        Commands::Vault { action } => {
            let va = match action {
                Some(a) => a.into_vault_action(),
                None => vault::VaultAction::List,
            };
            vault::cmd_vault(&mut config, va).await
        }
        Commands::Config { action } => cmd_config(&mut config, action),
        Commands::Settings { action } => cmd_settings(&config, action).await,
        Commands::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "mage", &mut std::io::stdout());
            Ok(())
        }
        Commands::SetupPi { uninstall, dev } => setup_pi::cmd_setup_pi(uninstall, dev),
        Commands::Version => {
            println!("mage {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

async fn cmd_login(config: &Config, method: &str) -> Result<()> {
    let m: auth::oauth::LoginMethod = method.parse()?;
    auth::oauth::login_with_method(&config.gateway_url, &m).await?;

    if let Ok(creds) = auth::Credentials::load() {
        if let Some(uid) = &creds.user_id {
            analytics::track(
                "user_signed_in",
                uid,
                serde_json::json!({ "auth_method": method }),
                config,
            )
            .await;
        }
    }

    Ok(())
}

async fn cmd_login_status(config: &Config) -> Result<()> {
    let creds = auth::Credentials::load().unwrap_or_default();
    if let Some(email) = &creds.email {
        println!("Logged in as: {}", email);
    } else {
        println!("Not logged in.");
    }
    if creds.access_token.is_some() {
        let valid = creds.is_token_valid();
        println!("Token: {}", if valid { "valid" } else { "expired" });
    }
    // Show vault status
    if let Ok(v) = magelab_core::vault::Vault::open() {
        if let Ok(keys) = v.list() {
            if !keys.is_empty() {
                println!("Vault: {} key(s)", keys.len());
            }
        }
    }
    if let Some(key) = config.api_key() {
        let preview = if key.len() > 8 {
            format!("{}...{}", &key[..4], &key[key.len() - 4..])
        } else {
            "****".to_string()
        };
        println!("API key (env): {}", preview);
    }
    Ok(())
}

fn cmd_logout(_config: &Config) -> Result<()> {
    auth::clear_credentials()?;
    println!("Logged out.");
    Ok(())
}

async fn cmd_auth_token(config: &Config) -> Result<()> {
    let creds = auth::Credentials::load()?;
    if !creds.is_token_valid() {
        if let Some(refresh) = &creds.refresh_token {
            let mut new_creds =
                magelab_core::auth::refresh_token(&config.gateway_url, refresh).await?;
            if new_creds.refresh_token.is_none() {
                new_creds.refresh_token = Some(refresh.clone());
            }
            auth::save_refreshed_credentials(&new_creds)?;
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
    url: Option<String>,
    ws: Option<String>,
) -> Result<()> {
    let has_url_override = url.is_some() || ws.is_some();
    let local_url = if let Some(ws_url) = ws {
        connect::direct_ws_to_http_url(&ws_url)?
    } else {
        url.unwrap_or_else(|| config.local_url.clone())
    };

    let result = if local {
        if detect::check_backend_health(&local_url).await {
            connect::ConnectResult {
                url: Some(connect::to_ws_url(&local_url)),
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
            if let Ok(token) = auth::get_token(config).await {
                r = connect::ConnectResult {
                    url: Some(config.gateway_url.clone()),
                    token: Some(token),
                    mode: "remote".to_string(),
                    model: Some(config.default_model.clone()),
                };
            }
        }
        r
    } else {
        connect::resolve_with_local_url(config, no_launch || has_url_override, &local_url).await?
    };

    // Track activation funnel: connect
    if result.mode != "none" {
        if let Ok(creds) = auth::Credentials::load() {
            if let Some(uid) = &creds.user_id {
                let backend_type = match result.mode.as_str() {
                    "local" | "relay" => "local",
                    _ => "cloud",
                };
                analytics::track(
                    "cli_connect_completed",
                    uid,
                    serde_json::json!({ "backend_type": backend_type }),
                    config,
                )
                .await;
            }
        }
    }

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

async fn cmd_launch(
    config: &mut Config,
    wait: bool,
    host: &str,
    port: Option<u16>,
    dry_run: bool,
    allow_network: bool,
) -> Result<()> {
    validate_launch_host(host, allow_network)?;

    let bundle = detect::find_backend_bundle(config.magelab_home.as_deref())?.ok_or_else(|| {
        anyhow::anyhow!("MageLab backend bundle not found. Set magelab_home or MAGELAB_API_DIR.")
    })?;

    let port = port.unwrap_or_else(|| detect::port_from_url(&config.local_url));
    let wait_url = wait_url(host, port);
    if dry_run {
        print_launch_plan(&bundle, host, port);
        return Ok(());
    }

    let mut child = detect::launch_backend_headless(&bundle, host, port, config.relay_enabled)?;

    if wait {
        let sp = ui::spinner("Starting backend...");
        if let Err(e) =
            detect::wait_for_backend(&wait_url, std::time::Duration::from_secs(30)).await
        {
            child.kill().ok();
            child.wait().ok();
            return Err(e);
        }
        // Detach the child so it outlives this CLI invocation after health succeeds.
        std::mem::forget(child);
        sp.finish_and_clear();
        ui::success(&format!("Backend ready at {}", wait_url));

        // Auto-push vault secrets (same as desktop's initSecrets startup flow)
        if let Ok(v) = magelab_core::vault::Vault::open() {
            if let Ok(secrets) = v.all_secrets() {
                if !secrets.is_empty() {
                    config.local_url = wait_url.clone();
                    if let Err(e) = vault::push_secrets(config).await {
                        eprintln!("Warning: vault push failed: {e}");
                    }
                }
            }
        }
    } else {
        // Fire-and-forget launch intentionally detaches immediately.
        std::mem::forget(child);
        ui::success("Backend launched");
        ui::label("check", "mage status");
    }

    if let Ok(creds) = auth::Credentials::load() {
        if let Some(uid) = &creds.user_id {
            analytics::track_activation(uid, "launch", config).await;
        }
    }

    Ok(())
}

fn validate_launch_host(host: &str, allow_network: bool) -> Result<()> {
    let local = matches!(host, "127.0.0.1" | "localhost" | "::1");
    if !local && !allow_network {
        anyhow::bail!(
            "Refusing to bind MageLab backend to {host}. This exposes full tool access to the network. Re-run with --allow-network if this is intentional."
        );
    }

    if !local {
        eprintln!(
            "Warning: binding to {host} exposes the MageLab backend (full tool access) to your network."
        );
    }

    Ok(())
}

fn wait_url(host: &str, port: u16) -> String {
    let health_host = if host == "0.0.0.0" { "127.0.0.1" } else { host };
    format!("http://{}:{}", health_host, port)
}

fn print_launch_plan(bundle: &detect::BackendBundle, host: &str, port: u16) {
    println!("Backend kind:  {:?}", bundle.kind);
    println!("Root:          {}", bundle.root.display());
    if let Some(api_dir) = &bundle.api_dir {
        println!("API dir:       {}", api_dir.display());
    }
    println!("Backend dir:   {}", bundle.backend_dir.display());
    println!("Python:        {}", bundle.python.display());
    println!("Host:          {}", host);
    println!("Port:          {}", port);
    if let Some(dir) = dirs::config_dir() {
        println!(
            "Log:           {}",
            dir.join("magelab").join("backend.log").display()
        );
    }
}

async fn cmd_status(config: &Config) -> Result<()> {
    let healthy = detect::check_backend_health(&config.local_url).await;
    println!(
        "Backend: {}",
        if healthy { "running" } else { "not running" }
    );
    println!("URL: {}", config.local_url);

    let creds = auth::Credentials::load().unwrap_or_default();
    let logged_in = creds.access_token.is_some() && creds.is_token_valid();
    println!(
        "Auth: {}",
        if logged_in {
            "logged in"
        } else {
            "not logged in"
        }
    );

    match magelab_core::vault::Vault::open() {
        Ok(v) => {
            if let Ok(keys) = v.list() {
                if !keys.is_empty() {
                    println!("Vault: {} key(s)", keys.len());
                }
            }
        }
        Err(_) => println!("Vault: not available"),
    }
    if let Some(key) = config.api_key() {
        println!(
            "API key (env): configured ({}...)",
            &key[..4.min(key.len())]
        );
    }

    Ok(())
}

async fn cmd_devices(config: &Config, action: Option<DevicesAction>, json: bool) -> Result<()> {
    let creds = auth::Credentials::load()?;
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
    let token = auth::get_token(config).await?;
    let client = RemoteClient::new(&config.gateway_url, &token);
    match kind {
        "models" => account::list_models(&client).await,
        "usage" => account::show_usage(&client).await,
        "balance" => account::show_balance(&client).await,
        _ => unreachable!(),
    }
}

async fn cmd_keys(config: &Config, action: KeysAction) -> Result<()> {
    let token = auth::get_token(config).await?;
    let client = RemoteClient::new(&config.gateway_url, &token);
    match action {
        KeysAction::List => account::list_keys(&client).await,
        KeysAction::Create => {
            let new_key = account::create_key(&client).await?;
            if let Some(key_value) = new_key {
                // Push new key to running backend (if running)
                let push_url = format!("{}/api/auth/push_secrets", config.local_url);
                let body = serde_json::json!({ "secrets": { "magelab_api_key": key_value } });
                match reqwest::Client::new()
                    .post(&push_url)
                    .json(&body)
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        println!("API key pushed to running backend.");
                    }
                    _ => {
                        println!("Set this key in the desktop app to persist it in the vault.");
                    }
                }
            }
            Ok(())
        }
        KeysAction::Revoke { id } => account::revoke_key(&client, &id).await,
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

    let (mut ws, _) = connect_async(&ws_url)
        .await
        .map_err(|_| anyhow::anyhow!("Backend not running. Start it with `mage launch`"))?;

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
