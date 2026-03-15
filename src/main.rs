mod account;
mod auth;
mod client;
mod config;
mod detect;
mod render;
mod repl;

use anyhow::Result;
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use render::tree::TreeRenderer;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

use client::local::{decode_message, encode_message, LocalClient};
use client::messages::{IncomingMessage, OutgoingMessage};
use client::remote::RemoteClient;
use config::Config;
use detect::ConnectionMode;
use repl::approval::ApprovalPolicy;
use repl::input::{parse_slash_command, SlashCommand};

#[derive(Parser)]
#[command(
    name = "magelab",
    version,
    about = "MageLab CLI — LLM chat and agentic tool use"
)]
struct Cli {
    /// Prompt for one-shot mode
    prompt: Option<String>,

    #[arg(short, long)]
    model: Option<String>,

    #[arg(long)]
    local: bool,

    #[arg(long)]
    remote: bool,

    #[arg(long)]
    yolo: bool,

    /// Target a specific desktop device by name (implies --remote)
    #[arg(long)]
    device: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate with MageLab
    Login {
        /// Login method: google (default) or magic (email code)
        #[arg(long, default_value = "magic")]
        method: String,
        /// Show current auth status instead of logging in
        #[arg(long)]
        status: bool,
    },
    /// Clear stored credentials
    Logout,
    Models,
    Usage,
    Balance,
    Keys {
        #[command(subcommand)]
        action: KeysAction,
    },
    Config,
}

#[derive(Subcommand)]
enum KeysAction {
    List,
    Create,
    Revoke { id: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::load().unwrap_or_default();

    // --device implies --remote
    let mode = if cli.device.is_some() {
        ConnectionMode::Remote
    } else {
        ConnectionMode::from_flags(cli.local, cli.remote)
    };
    let model = cli.model.clone().unwrap_or(config.default_model.clone());

    // Handle subcommands (all require remote/API key)
    if let Some(cmd) = &cli.command {
        return handle_subcommand(cmd, &mut config).await;
    }

    // Determine connection mode
    let resolved = match &mode {
        ConnectionMode::Local => detect::ResolvedConnection::Local,
        ConnectionMode::Remote => resolve_remote_mode(&config, cli.device.as_deref()).await?,
        ConnectionMode::Auto => resolve_auto_mode(&config).await?,
    };

    match resolved {
        detect::ResolvedConnection::Local => run_local_mode(&config, &cli, &model).await,
        detect::ResolvedConnection::RemoteRest => {
            if config.needs_remote_setup() {
                config.run_first_setup()?;
            }
            run_remote_mode(&config, &cli, &model).await
        }
        detect::ResolvedConnection::RemoteRelay { jwt } => {
            run_relay_mode(&config, &cli, &model, &jwt).await
        }
    }
}

/// Smart connection resolver:
///   local health → headless launch → JWT + devices → REST fallback → login prompt
async fn resolve_auto_mode(config: &Config) -> Result<detect::ResolvedConnection> {
    // 1. Check if local backend is already running
    if detect::check_backend_health(&config.local_url).await {
        render::stream::print_status("Local backend detected.");
        return Ok(detect::ResolvedConnection::Local);
    }

    // 2. Try to find and launch local backend
    if let Some(home) = detect::find_magelab_home(config.magelab_home.as_deref()) {
        render::stream::print_status(&format!(
            "Local backend found at {}. Starting...",
            home.display()
        ));
        if let Ok(_child) = detect::launch_backend_headless(&home) {
            if detect::wait_for_backend(&config.local_url, Duration::from_secs(15))
                .await
                .is_ok()
            {
                render::stream::print_status("Backend ready.");
                return Ok(detect::ResolvedConnection::Local);
            }
        }
        render::stream::print_status("Failed to start local backend, trying remote...");
    }

    // 3. Check for JWT (Supabase auth) → try relay
    let creds = auth::credentials::Credentials::load().unwrap_or_default();
    if let Some(jwt) = try_get_valid_jwt(&creds, &config.gateway_url).await {
        // Check for online devices
        if let Ok(devices) = detect::discover_devices(&config.gateway_url, &jwt).await {
            if !devices.is_empty() {
                render::stream::print_status(&format!(
                    "Remote device online ({}). Connecting via relay...",
                    devices[0]
                ));
                return Ok(detect::ResolvedConnection::RemoteRelay { jwt });
            }
        }
        // JWT valid but no devices — fall through to REST
    }

    // 4. Check for API key → REST/SSE (chat only)
    if config.api_key().is_some() {
        render::stream::print_status("Using remote API (chat-only mode).");
        return Ok(detect::ResolvedConnection::RemoteRest);
    }

    // 5. No auth at all → offer to login
    println!("No local backend found and no credentials configured.\n");
    println!("  1) Log in with Google   (magelab login)");
    println!("  2) Enter an API key     (get one at magelab.ai/dashboard)\n");
    let choice = render::stream::animated_prompt("Choose [1/2]:");

    match choice.as_str() {
        "1" | "" => {
            auth::oauth::login(&config.gateway_url).await?;
            // After login, retry JWT path
            let creds = auth::credentials::Credentials::load().unwrap_or_default();
            if let Some(_jwt) = creds.access_token {
                return Ok(detect::ResolvedConnection::RemoteRest);
            }
            anyhow::bail!("Login completed but no credentials found. Try again.")
        }
        "2" => {
            anyhow::bail!("Run: magelab config  — then add api_key to ~/.config/magelab/cli.toml")
        }
        _ => {
            anyhow::bail!("No authentication configured")
        }
    }
}

/// Resolve --remote: try relay (JWT + device) first, fall back to REST
async fn resolve_remote_mode(
    config: &Config,
    device: Option<&str>,
) -> Result<detect::ResolvedConnection> {
    // Try JWT auth for relay
    let creds = auth::credentials::Credentials::load().unwrap_or_default();
    if let Some(jwt) = try_get_valid_jwt(&creds, &config.gateway_url).await {
        // Check for devices
        match detect::discover_devices(&config.gateway_url, &jwt).await {
            Ok(devices) if !devices.is_empty() => {
                let target = device
                    .or(config.default_device.as_deref())
                    .unwrap_or(&devices[0]);

                if let Some(d) = device {
                    if !devices.iter().any(|dev| dev == d) {
                        render::stream::print_error(&format!(
                            "Device \"{}\" is not online. Available: {}",
                            d,
                            devices.join(", ")
                        ));
                        anyhow::bail!("Device not found");
                    }
                }

                let _ = target; // used for future device targeting
                return Ok(detect::ResolvedConnection::RemoteRelay { jwt });
            }
            Ok(_) => {
                // No devices — fall through to REST
            }
            Err(_) => {
                // Device check failed — fall through to REST
            }
        }
    }

    // Fall back to REST/SSE (chat only)
    if config.api_key().is_some() {
        return Ok(detect::ResolvedConnection::RemoteRest);
    }

    // No auth at all
    println!("Run `magelab login` to authenticate, or set api_key in cli.toml");
    anyhow::bail!("No authentication configured")
}

/// Try to get a valid JWT from stored credentials, refreshing if needed
async fn try_get_valid_jwt(
    creds: &auth::credentials::Credentials,
    gateway_url: &str,
) -> Option<String> {
    if creds.is_token_valid() {
        return creds.access_token.clone();
    }
    if let Some(ref rt) = creds.refresh_token {
        if let Ok(new_creds) = auth::oauth::refresh_token(gateway_url, rt).await {
            return new_creds.access_token;
        }
    }
    None
}

async fn handle_subcommand(cmd: &Commands, config: &mut Config) -> Result<()> {
    // Commands that don't need an API key
    match cmd {
        Commands::Config => {
            let path = Config::path()?;
            println!("Config: {}", path.display());
            if path.exists() {
                let contents = std::fs::read_to_string(&path)?;
                println!("{}", contents);
            } else {
                println!("(not yet created — run magelab to set up)");
            }
            return Ok(());
        }
        Commands::Login { method, status } => {
            if *status {
                let creds = auth::credentials::Credentials::load().unwrap_or_default();
                if let Some(ref email) = creds.email {
                    println!("Auth:    {} (WorkOS)", email);
                } else if creds.has_token() {
                    println!("Auth:    logged in (WorkOS)");
                } else {
                    println!("Auth:    not logged in");
                }

                if creds.is_token_valid() {
                    let remaining = creds.expires_at.unwrap_or(0) - chrono::Utc::now().timestamp();
                    let mins = remaining / 60;
                    println!("Token:   valid, expires in {} minutes", mins);
                } else if creds.has_token() {
                    println!("Token:   expired");
                } else {
                    println!("Token:   none");
                }

                println!("Storage: credentials file");

                if config.api_key().is_some() {
                    let key = config.api_key().unwrap();
                    let preview = if key.len() > 8 {
                        format!("{}...{}", &key[..4], &key[key.len() - 4..])
                    } else {
                        "configured".to_string()
                    };
                    println!("API key: configured ({})", preview);
                } else {
                    println!("API key: not configured");
                }
            } else {
                let login_method: auth::oauth::LoginMethod = method.parse()?;
                auth::oauth::login_with_method(&config.gateway_url, &login_method).await?;
            }
            return Ok(());
        }
        Commands::Logout => {
            auth::credentials::Credentials::clear()?;
            println!("Logged out. Credentials removed.");
            return Ok(());
        }
        _ => {}
    }

    if config.needs_remote_setup() {
        config.run_first_setup()?;
    }
    let api_key = config.api_key().unwrap();
    let client = RemoteClient::new(&config.gateway_url, &api_key);

    match cmd {
        Commands::Models => account::list_models(&client).await,
        Commands::Usage => account::show_usage(&client).await,
        Commands::Balance => account::show_balance(&client).await,
        Commands::Keys { action } => match action {
            KeysAction::List => account::list_keys(&client).await,
            KeysAction::Create => account::create_key(&client).await,
            KeysAction::Revoke { id } => account::revoke_key(&client, id).await,
        },
        Commands::Config | Commands::Login { .. } | Commands::Logout => unreachable!(),
    }
}

async fn run_local_mode(config: &Config, cli: &Cli, model: &str) -> Result<()> {
    let client = LocalClient::new(&config.local_url);
    let (mut sink, mut stream) = client.connect().await?;

    // Fetch runtime config on connect and wait for response
    let msg = encode_message(&OutgoingMessage::GetRuntimeConfig)?;
    sink.send(msg).await?;
    let backend_model = drain_until_runtime_config(&mut stream).await;

    let approval = ApprovalPolicy::new(config.auto_approve.clone(), cli.yolo);
    let display_model = backend_model.as_deref().unwrap_or(model);

    println!("\u{1f5a5} Connected to local backend");

    // One-shot mode
    if let Some(prompt) = &cli.prompt {
        let stdin_content = read_stdin_if_piped();
        let full_prompt = match stdin_content {
            Some(stdin) => format!("{}\n\n{}", prompt, stdin),
            None => prompt.clone(),
        };

        let msg = encode_message(&OutgoingMessage::Chat { text: full_prompt })?;
        sink.send(msg).await?;

        return process_ws_response(&mut stream, &mut sink, &approval).await;
    }

    // REPL mode
    println!("MageLab v{} ({})", env!("CARGO_PKG_VERSION"), display_model);
    println!("Type /help for commands, Ctrl+D to exit.\n");

    let mut rl = rustyline::DefaultEditor::new()?;

    loop {
        let readline = rl.readline("> ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                rl.add_history_entry(trimmed)?;

                // Check for slash commands
                if let Some(cmd) = parse_slash_command(trimmed) {
                    match cmd {
                        SlashCommand::Quit => break,
                        SlashCommand::Help => repl::input::print_help(),
                        SlashCommand::Clear => {
                            let msg = encode_message(&OutgoingMessage::NewChat)?;
                            sink.send(msg).await?;
                            println!("Conversation cleared.");
                        }
                        SlashCommand::Model(m) => {
                            let msg =
                                encode_message(&OutgoingMessage::SetModel { model: m.clone() })?;
                            sink.send(msg).await?;
                            match drain_next_message(&mut stream).await {
                                Some(IncomingMessage::SetModelResult {
                                    success: true,
                                    model: Some(name),
                                    ..
                                }) => println!("Switched to {}.", name),
                                Some(IncomingMessage::SetModelResult {
                                    success: false, ..
                                }) => render::stream::print_error("Failed to switch model"),
                                _ => println!("Switched to {}.", m),
                            }
                        }
                        SlashCommand::Mode => {
                            println!("local mode (WebSocket to {})", config.local_url);
                        }
                        SlashCommand::Tools => {
                            let msg = encode_message(&OutgoingMessage::GetRuntimeConfig)?;
                            sink.send(msg).await?;
                            match drain_next_message(&mut stream).await {
                                Some(IncomingMessage::RuntimeConfig(config_map)) => {
                                    if let Some(funcs) = config_map
                                        .get("enabled_functions")
                                        .and_then(|v| v.as_array())
                                    {
                                        println!("Enabled tools:");
                                        for f in funcs {
                                            if let Some(name) = f.as_str() {
                                                println!("  ✓ {}", name);
                                            }
                                        }
                                    } else {
                                        println!("No tools info available.");
                                    }
                                }
                                _ => println!("No response from backend."),
                            }
                        }
                        SlashCommand::Yolo => {
                            println!("Yolo mode toggled (restart with --yolo for persistent).");
                        }
                        SlashCommand::Chats => {
                            let msg = encode_message(&OutgoingMessage::GetChats)?;
                            sink.send(msg).await?;
                            match drain_next_message(&mut stream).await {
                                Some(IncomingMessage::ChatListResult {
                                    chats,
                                    history_path,
                                    ..
                                }) => {
                                    let current = history_path.as_deref().unwrap_or("");
                                    println!("Chat histories:");
                                    for c in &chats {
                                        let marker = if c == current { " (active)" } else { "" };
                                        // Show just the filename, not full path
                                        let name = c
                                            .rsplit('/')
                                            .next()
                                            .unwrap_or(c)
                                            .trim_end_matches(".json");
                                        println!("  {}{}", name, marker);
                                    }
                                }
                                _ => println!("No response from backend."),
                            }
                        }
                        SlashCommand::Chat(name) => {
                            // Try to match by filename substring
                            let path = if name.ends_with(".json") {
                                name.clone()
                            } else {
                                format!("{}.json", name)
                            };
                            let msg =
                                encode_message(&OutgoingMessage::SetChat { history_path: path })?;
                            sink.send(msg).await?;
                            match drain_next_message(&mut stream).await {
                                Some(IncomingMessage::ChatSwitchResult {
                                    ok: true,
                                    history_path: Some(hp),
                                    ..
                                }) => {
                                    let name = hp
                                        .rsplit('/')
                                        .next()
                                        .unwrap_or(&hp)
                                        .trim_end_matches(".json");
                                    println!("Switched to chat: {}", name);
                                }
                                Some(IncomingMessage::ChatSwitchResult {
                                    ok: false,
                                    error: Some(err),
                                    ..
                                }) => {
                                    render::stream::print_error(&err);
                                }
                                _ => render::stream::print_error("Failed to switch chat"),
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Send chat message
                let msg = encode_message(&OutgoingMessage::Chat {
                    text: trimmed.to_string(),
                })?;
                sink.send(msg).await?;

                process_ws_response(&mut stream, &mut sink, &approval).await?;
                println!(); // blank line after response
            }
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(rustyline::error::ReadlineError::Interrupted) => break,
            Err(e) => {
                render::stream::print_error(&format!("Input error: {}", e));
                break;
            }
        }
    }

    Ok(())
}

/// Connect to relay WebSocket: get ticket, connect, fetch runtime config.
/// Returns (sink, stream, backend_model).
async fn connect_relay(
    config: &Config,
    jwt: &str,
) -> Result<(
    futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    Option<String>,
)> {
    let ticket = detect::get_ws_ticket(&config.gateway_url, jwt).await?;
    let gateway_ws = config
        .gateway_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    let relay_url = format!(
        "{}/v1/realtime/portal/ws?ws_ticket={}",
        gateway_ws.trim_end_matches('/'),
        ticket
    );

    let (ws_stream, _) = tokio_tungstenite::connect_async(&relay_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to relay: {}", e))?;
    let (mut sink, mut stream) = futures_util::StreamExt::split(ws_stream);

    let msg = encode_message(&OutgoingMessage::GetRuntimeConfig)?;
    futures_util::SinkExt::send(&mut sink, msg).await?;
    let backend_model = drain_until_runtime_config(&mut stream).await;

    Ok((sink, stream, backend_model))
}

/// Reconnect backoff delays in seconds
const RECONNECT_BACKOFF: [u64; 3] = [1, 3, 5];

async fn run_relay_mode(config: &Config, cli: &Cli, model: &str, jwt: &str) -> Result<()> {
    let (mut sink, mut stream, backend_model) = connect_relay(config, jwt).await?;
    let display_model = backend_model.as_deref().unwrap_or(model);

    println!("\u{26a1} Connected via relay (full tools)");

    let approval = ApprovalPolicy::new(config.auto_approve.clone(), cli.yolo);

    // One-shot mode
    if let Some(prompt) = &cli.prompt {
        let stdin_content = read_stdin_if_piped();
        let full_prompt = match stdin_content {
            Some(stdin) => format!("{}\n\n{}", prompt, stdin),
            None => prompt.clone(),
        };

        let msg = encode_message(&OutgoingMessage::Chat { text: full_prompt })?;
        futures_util::SinkExt::send(&mut sink, msg).await?;
        return process_ws_response(&mut stream, &mut sink, &approval).await;
    }

    // REPL mode
    println!("MageLab v{} ({})", env!("CARGO_PKG_VERSION"), display_model);
    println!("Type /help for commands, Ctrl+D to exit.\n");

    let mut rl = rustyline::DefaultEditor::new()?;

    loop {
        let readline = rl.readline("> ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                rl.add_history_entry(trimmed)?;

                if let Some(cmd) = parse_slash_command(trimmed) {
                    match cmd {
                        SlashCommand::Quit => break,
                        SlashCommand::Help => repl::input::print_help(),
                        SlashCommand::Clear => {
                            let msg = encode_message(&OutgoingMessage::NewChat)?;
                            futures_util::SinkExt::send(&mut sink, msg).await?;
                            println!("Conversation cleared.");
                        }
                        SlashCommand::Mode => {
                            println!("relay mode (WebSocket via {})", config.gateway_url);
                        }
                        SlashCommand::Status => {
                            println!("Mode:   relay (full tools)");
                            println!("Via:    {}", config.gateway_url);
                        }
                        SlashCommand::Devices => {
                            let msg = Message::Text(
                                serde_json::json!({"type": "list_devices"}).to_string(),
                            );
                            futures_util::SinkExt::send(&mut sink, msg).await?;
                            println!("(device list will arrive via broker_status)");
                        }
                        SlashCommand::Bind(device) => {
                            let msg = Message::Text(
                                serde_json::json!({"type": "bind", "device_id": device})
                                    .to_string(),
                            );
                            futures_util::SinkExt::send(&mut sink, msg).await?;
                            println!("Binding to {}...", device);
                        }
                        SlashCommand::Detach => {
                            let msg = Message::Text(
                                serde_json::json!({"type": "detach"}).to_string(),
                            );
                            futures_util::SinkExt::send(&mut sink, msg).await?;
                            println!("Detached from device.");
                        }
                        _ => println!("Command not yet supported in relay mode."),
                    }
                    continue;
                }

                let msg = encode_message(&OutgoingMessage::Chat {
                    text: trimmed.to_string(),
                })?;

                // Send and process, with reconnect on failure
                let send_result =
                    futures_util::SinkExt::send(&mut sink, msg.clone()).await;
                let chat_ok = match send_result {
                    Ok(()) => {
                        process_ws_response(&mut stream, &mut sink, &approval)
                            .await
                            .is_ok()
                    }
                    Err(_) => false,
                };

                if !chat_ok {
                    // Attempt reconnect with backoff
                    let mut reconnected = false;
                    for (attempt, delay) in RECONNECT_BACKOFF.iter().enumerate() {
                        render::stream::print_status(&format!(
                            "Connection lost. Reconnecting ({}/{})...",
                            attempt + 1,
                            RECONNECT_BACKOFF.len()
                        ));
                        tokio::time::sleep(Duration::from_secs(*delay)).await;

                        // Refresh JWT in case it rotated
                        let creds =
                            auth::credentials::Credentials::load().unwrap_or_default();
                        if let Some(fresh_jwt) =
                            try_get_valid_jwt(&creds, &config.gateway_url).await
                        {
                            match connect_relay(config, &fresh_jwt).await {
                                Ok((new_sink, new_stream, _)) => {
                                    sink = new_sink;
                                    stream = new_stream;
                                    render::stream::print_status("Reconnected.");
                                    reconnected = true;
                                    break;
                                }
                                Err(_) => continue,
                            }
                        }
                    }
                    if !reconnected {
                        render::stream::print_error(
                            "Failed to reconnect after 3 attempts.",
                        );
                        break;
                    }
                } else {
                    println!();
                }
            }
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(rustyline::error::ReadlineError::Interrupted) => break,
            Err(e) => {
                render::stream::print_error(&format!("Input error: {}", e));
                break;
            }
        }
    }

    Ok(())
}

async fn run_remote_mode(config: &Config, cli: &Cli, model: &str) -> Result<()> {
    let api_key = config.api_key().unwrap();
    let client = RemoteClient::new(&config.gateway_url, &api_key);

    println!("\u{2601} Connected to gateway (chat only)");

    // One-shot mode
    if let Some(prompt) = &cli.prompt {
        let stdin_content = read_stdin_if_piped();
        let full_prompt = match stdin_content {
            Some(stdin) => format!("{}\n\n{}", prompt, stdin),
            None => prompt.clone(),
        };

        let messages = vec![("user".to_string(), full_prompt)];
        let resp = client.chat_stream(&messages, model).await?;
        stream_sse_response(resp).await?;
        println!();
        return Ok(());
    }

    // REPL mode (remote — chat only, no tools)
    println!("MageLab v{} ({})", env!("CARGO_PKG_VERSION"), model);
    println!("Type /help for commands, Ctrl+D to exit.\n");

    let mut rl = rustyline::DefaultEditor::new()?;
    let mut history: Vec<(String, String)> = Vec::new();
    let mut current_model = model.to_string();

    loop {
        let readline = rl.readline("> ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                rl.add_history_entry(trimmed)?;

                if let Some(cmd) = parse_slash_command(trimmed) {
                    match cmd {
                        SlashCommand::Quit => break,
                        SlashCommand::Help => repl::input::print_help(),
                        SlashCommand::Clear => {
                            history.clear();
                            println!("Conversation cleared.");
                        }
                        SlashCommand::Model(m) => {
                            current_model = m.clone();
                            println!("Switched to {}.", m);
                        }
                        SlashCommand::Mode => {
                            println!("remote mode (REST to {})", config.gateway_url);
                        }
                        _ => println!("Command not available in remote mode."),
                    }
                    continue;
                }

                history.push(("user".into(), trimmed.to_string()));

                let resp = client.chat_stream(&history, &current_model).await?;
                let assistant_text = stream_sse_response(resp).await?;
                println!();

                history.push(("assistant".into(), assistant_text));
            }
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(rustyline::error::ReadlineError::Interrupted) => break,
            Err(e) => {
                render::stream::print_error(&format!("Input error: {}", e));
                break;
            }
        }
    }

    Ok(())
}

/// Create a mage-themed spinner for waiting states
pub fn make_spinner(msg: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&[
                "\x1b[38;5;141m🜁\x1b[0m",
                "\x1b[38;5;135m🜁\x1b[0m",
                "\x1b[38;5;128m🜂\x1b[0m",
                "\x1b[38;5;93m🜂\x1b[0m",
                "\x1b[38;5;57m🜃\x1b[0m",
                "\x1b[38;5;55m🜃\x1b[0m",
                "\x1b[38;5;57m🜄\x1b[0m",
                "\x1b[38;5;91m🜄\x1b[0m",
                "\x1b[38;5;93m🜁\x1b[0m",
                "\x1b[38;5;128m🜁\x1b[0m",
                "\x1b[38;5;135m🜂\x1b[0m",
                "\x1b[38;5;141m🜂\x1b[0m",
                "\x1b[38;5;177m🜃\x1b[0m",
                "\x1b[38;5;141m🜃\x1b[0m",
                "\x1b[38;5;135m🜄\x1b[0m",
                "\x1b[38;5;128m🜄\x1b[0m",
                "\x1b[38;5;141m🜁\x1b[0m",
            ])
            .template("{spinner} {msg}")
            .unwrap(),
    );
    spinner.set_message(msg.to_string());
    spinner.enable_steady_tick(Duration::from_millis(60));
    spinner
}

/// Response timeout — if no message arrives within this duration, give up
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);

/// Wait for the next meaningful message, skipping pings/transcriptions/tool_debug.
/// Returns None on timeout (5s) or error.
async fn drain_next_message<S>(stream: &mut S) -> Option<IncomingMessage>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let timeout = Duration::from_secs(5);
    loop {
        match tokio::time::timeout(timeout, stream.next()).await {
            Ok(Some(Ok(msg))) => {
                if let Ok(Some(incoming)) = decode_message(&msg) {
                    match &incoming {
                        IncomingMessage::Ping { .. }
                        | IncomingMessage::Transcription { .. }
                        | IncomingMessage::ToolDebug { .. } => continue,
                        _ => return Some(incoming),
                    }
                }
            }
            _ => return None,
        }
    }
}

/// Wait for the initial runtime_config response.
/// Returns the backend's active model name if found.
async fn drain_until_runtime_config<S>(stream: &mut S) -> Option<String>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match drain_next_message(stream).await {
            Some(IncomingMessage::RuntimeConfig(config)) => {
                return config
                    .get("llm_model_name")
                    .and_then(|v| v.as_str())
                    .map(String::from);
            }
            Some(_) => continue, // Skip other messages
            None => return None, // Timeout
        }
    }
}

/// Process WebSocket messages until the assistant response is complete
async fn process_ws_response<S, K>(
    stream: &mut S,
    sink: &mut K,
    approval: &ApprovalPolicy,
) -> Result<()>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    K: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let mut streaming = false;
    let spinner = make_spinner("Thinking...");
    let mut tree = TreeRenderer::new();
    let mut last_tool_id: Option<String> = None;
    let mut highlighter = render::highlight::StreamProcessor::new();

    loop {
        let msg_result = tokio::time::timeout(RESPONSE_TIMEOUT, stream.next()).await;

        let msg_result = match msg_result {
            Ok(Some(msg_result)) => msg_result,
            Ok(None) => {
                tree.clear();
                spinner.finish_and_clear();
                render::stream::print_error("Connection closed by backend");
                return Ok(());
            }
            Err(_) => {
                tree.clear();
                spinner.finish_and_clear();
                render::stream::print_error(
                    "Response timed out (120s). The backend may be down or out of credits.",
                );
                return Ok(());
            }
        };

        let msg = match msg_result {
            Ok(msg) => msg,
            Err(e) => {
                tree.clear();
                spinner.finish_and_clear();
                render::stream::print_error(&format!("WebSocket error: {}", e));
                return Ok(());
            }
        };

        let Some(incoming) = decode_message(&msg)? else {
            continue;
        };

        match &incoming {
            IncomingMessage::AssistantStream {
                phase,
                token,
                text,
                content,
                ..
            } => match phase.as_str() {
                "start" => {
                    spinner.finish_and_clear();
                    tree.clear();
                    streaming = true;
                }
                "delta" | "token" => {
                    let tok = text.as_ref().or(token.as_ref());
                    if let Some(t) = tok {
                        highlighter.push_token(t);
                    }
                }
                "end" => {
                    if streaming {
                        highlighter.flush();
                        render::stream::end_stream();
                    }
                    if let Some(c) = content {
                        if !streaming {
                            spinner.finish_and_clear();
                            tree.clear();
                            render::markdown::render(c);
                        }
                    }
                    return Ok(());
                }
                _ => {}
            },
            IncomingMessage::Assistant { text } => {
                spinner.finish_and_clear();
                tree.clear();
                if let Some(t) = text {
                    render::markdown::render(t);
                }
            }
            IncomingMessage::AssistantComplete { .. } => {
                spinner.finish_and_clear();
                // Render final tree state if it has nodes
                if !tree.is_empty() {
                    tree.render();
                    println!(); // blank line after tree
                }
                return Ok(());
            }
            IncomingMessage::Transcription { .. } => {}
            IncomingMessage::ToolDebug { .. } => {}
            IncomingMessage::ConfirmationRequest {
                confirmation_id,
                function_name,
                script,
                arguments,
            } => {
                spinner.finish_and_clear();
                let args_str = serde_json::to_string(arguments).unwrap_or_default();
                let display = script.as_deref().unwrap_or(&args_str);
                let label = format!("{}: {}", function_name, truncate(display, 60));

                // Add to tree
                tree.push(confirmation_id, &label);
                tree.render();

                if approval.is_auto_approved(function_name) {
                    // Auto-approve — mark as running
                    let response = encode_message(&OutgoingMessage::ConfirmationResponse {
                        confirmation_id: confirmation_id.clone(),
                        confirmed: true,
                        remember: false,
                    })?;
                    sink.send(response).await?;
                    last_tool_id = Some(confirmation_id.clone());
                } else {
                    // Need user input — clear tree, prompt, then restore
                    tree.clear();
                    let approved =
                        approval.prompt_approval(function_name, script.as_deref(), &args_str);
                    if !approved {
                        tree.fail(confirmation_id, "denied");
                    }
                    let response = encode_message(&OutgoingMessage::ConfirmationResponse {
                        confirmation_id: confirmation_id.clone(),
                        confirmed: approved,
                        remember: false,
                    })?;
                    sink.send(response).await?;
                    if approved {
                        last_tool_id = Some(confirmation_id.clone());
                    }
                    tree.render();
                }
            }
            IncomingMessage::ToolResult {
                function_name,
                result,
            } => {
                let preview = result
                    .as_ref()
                    .map(|r| render::results::smart_preview(function_name, r));
                // Complete the matching tree node
                if let Some(ref id) = last_tool_id {
                    tree.complete(id, preview.as_deref());
                }
                last_tool_id = None;
                tree.render();

                // Show full result for substantial output
                if let Some(r) = result {
                    render::results::display_full_result(function_name, r);
                }
            }
            IncomingMessage::SubagentUpdate {
                task_id,
                name,
                status,
                progress,
                ..
            } => {
                spinner.finish_and_clear();
                let label = match progress {
                    Some(p) if !p.is_empty() => format!("{} ({})", name, p),
                    _ => name.clone(),
                };
                // Check if node exists, update or create
                if tree.contains(task_id) {
                    tree.update_status(
                        task_id,
                        match status.as_deref() {
                            Some("completed") => render::tree::NodeStatus::Completed,
                            Some("failed") => render::tree::NodeStatus::Failed,
                            Some("cancelled") => render::tree::NodeStatus::Cancelled,
                            _ => render::tree::NodeStatus::Running,
                        },
                    );
                } else {
                    tree.push(task_id, &label);
                }
                tree.render();
            }
            IncomingMessage::SubagentComplete { task_id, error, .. } => {
                if let Some(err) = error {
                    tree.fail(task_id, err);
                } else {
                    tree.complete(task_id, None);
                }
                tree.render();
            }
            IncomingMessage::RuntimeConfig(_) => {}
            IncomingMessage::Error { message } => {
                spinner.finish_and_clear();
                tree.clear();
                render::stream::print_error(message.as_deref().unwrap_or("Unknown error"));
                return Ok(());
            }
            IncomingMessage::Notify { title, body } => {
                spinner.finish_and_clear();
                tree.clear();
                render::stream::print_status(&format!("[{}] {}", title, body));
            }
            IncomingMessage::OpenUrl { url } => {
                render::stream::print_status(&format!("Opening: {}", url));
                open::that(url).ok();
            }
            IncomingMessage::OpenFile { filepath } => {
                render::stream::print_status(&format!("Opening: {}", filepath));
                open::that(filepath).ok();
            }
            IncomingMessage::TokenCount { total_count, .. } => {
                render::stream::print_status(&format!("[{} tokens]", total_count));
            }
            IncomingMessage::BrokerStatus {
                devices,
                bound_device_id,
                ..
            } => {
                if !devices.is_empty() {
                    println!("Online devices:");
                    for d in devices {
                        let marker = if Some(d) == bound_device_id.as_ref() {
                            " (bound)"
                        } else {
                            ""
                        };
                        println!("  {}{}", d, marker);
                    }
                }
            }
            IncomingMessage::BindResult {
                bound_device_id: Some(d),
            } => {
                println!("\u{26a1} Bound to {}", d);
            }
            IncomingMessage::BindResult { .. } => {}
            IncomingMessage::BrokerError { code, message } => {
                render::stream::print_error(&format!("Broker error ({}): {}", code, message));
            }
            IncomingMessage::Ping { .. } => {}
            IncomingMessage::Unknown => {
                let msg_str = msg.to_string();
                eprintln!(
                    "[debug] Unknown message: {}",
                    &msg_str[..msg_str.len().min(200)]
                );
            }
            _ => {}
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max.saturating_sub(3)])
    } else {
        s.to_string()
    }
}

/// Stream an SSE response and return the accumulated text
async fn stream_sse_response(resp: reqwest::Response) -> Result<String> {
    let mut accumulated = String::new();
    let mut bytes_stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut highlighter = render::highlight::StreamProcessor::new();

    while let Some(chunk) = bytes_stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete lines
        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim().to_string();
            buffer = buffer[newline_pos + 1..].to_string();

            if let Some(token) = client::remote::parse_sse_delta(&line) {
                highlighter.push_token(&token);
                accumulated.push_str(&token);
            }
        }
    }

    highlighter.flush();
    Ok(accumulated)
}

/// Read stdin if it's piped (not a terminal)
fn read_stdin_if_piped() -> Option<String> {
    use std::io::{IsTerminal, Read};
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    if buf.is_empty() {
        None
    } else {
        Some(buf)
    }
}
