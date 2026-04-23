mod account;
mod auth;
mod client;
mod config;
mod connect;
mod detect;
mod settings;

use anyhow::Result;
use clap::{Parser, Subcommand};

use client::remote::RemoteClient;
use config::Config;

#[derive(Parser)]
#[command(
    name = "magelab",
    version,
    about = "MageLab CLI — infrastructure management for MageLab"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate with MageLab
    Login {
        /// Login method: web (browser), google, or magic (email code)
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
        #[arg(long)]
        local: bool,
        /// Force relay mode only
        #[arg(long)]
        relay: bool,
        /// Force remote REST mode only
        #[arg(long)]
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
    /// Show or update configuration
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// Print version
    Version,
}

#[derive(Subcommand)]
enum AuthAction {
    /// Print a fresh JWT to stdout (refreshes if expired)
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::load().unwrap_or_default();

    match cli.command {
        Commands::Login { method, status } => {
            if status {
                return cmd_login_status(&config).await;
            }
            cmd_login(&config, &method).await
        }
        Commands::Logout => cmd_logout(),
        Commands::Auth { action } => match action {
            AuthAction::Token => cmd_auth_token(&config).await,
        },
        Commands::Connect {
            json,
            no_launch,
            local,
            relay,
            remote,
        } => cmd_connect(&config, json, no_launch, local, relay, remote).await,
        Commands::Launch { wait } => cmd_launch(&config, wait).await,
        Commands::Status => cmd_status(&config).await,
        Commands::Devices { action, json } => cmd_devices(&config, action, json).await,
        Commands::Models => cmd_account(&config, "models").await,
        Commands::Usage => cmd_account(&config, "usage").await,
        Commands::Balance => cmd_account(&config, "balance").await,
        Commands::Keys { action } => cmd_keys(&config, action).await,
        Commands::Config { action } => cmd_config(&mut config, action),
        Commands::Version => {
            println!("magelab {}", env!("CARGO_PKG_VERSION"));
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

fn cmd_logout() -> Result<()> {
    auth::credentials::Credentials::clear()?;
    println!("Logged out.");
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
        anyhow::bail!("Token expired and refresh failed. Run: magelab login");
    }
    match &creds.access_token {
        Some(token) => {
            print!("{}", token);
            Ok(())
        }
        None => anyhow::bail!("Not logged in. Run: magelab login"),
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
                url: Some(format!(
                    "ws://{}/ws",
                    config.local_url.trim_start_matches("http://")
                )),
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
            "none" => println!("No connection available. Run: magelab login"),
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

    let _child = detect::launch_backend_headless(&home)?;
    eprintln!("Backend starting...");

    if wait {
        detect::wait_for_backend(&config.local_url, std::time::Duration::from_secs(30)).await?;
        println!("{}", config.local_url);
    } else {
        println!("Backend launched. Use 'magelab status' to check health.");
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
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run: magelab login"))?;

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

/// Get the best available token (JWT preferred, API key fallback)
async fn get_token(config: &Config) -> Result<String> {
    let creds = auth::credentials::Credentials::load().unwrap_or_default();
    if let Some(token) = &creds.access_token {
        if creds.is_token_valid() {
            return Ok(token.clone());
        }
        if let Some(refresh) = &creds.refresh_token {
            if let Ok(new_creds) = auth::oauth::refresh_token(&config.gateway_url, refresh).await {
                let _ = new_creds.save();
                if let Some(t) = new_creds.access_token {
                    return Ok(t);
                }
            }
        }
    }
    config
        .api_key()
        .ok_or_else(|| anyhow::anyhow!("Not authenticated. Run: magelab login"))
}
