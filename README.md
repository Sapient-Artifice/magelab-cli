# MageLab CLI

Infrastructure management tool for MageLab. Handles authentication, backend detection/launch, account management, device management, and configuration.

## Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)

## Building

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo install --path .         # Install `magelab` binary to ~/.cargo/bin
```

## Running

```bash
magelab --help                 # Show all commands
magelab version                # Print version

# Authentication
magelab login                  # Browser-based OAuth login (default)
magelab login --method magic   # Email magic-code login
magelab login --status         # Show current auth status
magelab logout                 # Clear stored credentials
magelab auth token             # Print JWT to stdout (for piping)

# Connection management
magelab connect                # Auto-resolve: local → relay → remote
magelab connect --json         # Output as JSON (for programmatic use)
magelab connect --local        # Force local backend only
magelab connect --relay        # Force relay mode only
magelab connect --remote       # Force remote REST mode only
magelab connect --no-launch    # Don't auto-launch backend

# Backend management
magelab launch                 # Start headless backend
magelab launch --wait          # Start and block until healthy
magelab status                 # Show backend health and auth info

# Device management
magelab devices                # List online relay devices
magelab devices --json         # Output as JSON
magelab devices bind <id>      # Bind to a specific device
magelab devices detach         # Unbind from current device

# Account info (requires auth)
magelab models                 # List available models
magelab usage                  # Show token usage summary
magelab balance                # Show account credit balance

# API key management
magelab keys list              # List API keys
magelab keys create            # Create a new API key
magelab keys revoke <id>       # Revoke an API key

# Configuration
magelab config                 # Show current config and file path
magelab config set <key> <val> # Set a config value
```

## Pi Extension

The CLI includes `@magelab/agent`, a [Pi coding agent](https://github.com/badlogic/pi-mono) extension that bridges MageLab backend tools (python, web search, image generation, subagents, etc.) into Pi.

### Quickstart

```bash
# 1. One command installs everything (Pi + extension + dependencies)
magelab setup-pi

# 2. Start MageLab backend (if not already running)
magelab launch --wait

# 3. Start Pi — MageLab tools auto-register
pi

# 4. Try a MageLab tool in Pi
#    "use run_python to calculate fibonacci(20)"
#    "use search_web to find Rust async patterns"
```

`setup-pi` will install Pi (via pnpm/npm) if it's not already installed, embed the extension files into `~/.pi/agent/extensions/magelab-agent/`, and install dependencies. Pi auto-discovers extensions from this directory.

### Manual Setup

If you prefer to link the extension from the repo source:

```bash
# Install Pi
pnpm install -g @mariozechner/pi-coding-agent

# Install extension dependencies
cd extension && pnpm install && cd ..

# Symlink into Pi's extension directory
mkdir -p ~/.pi/agent/extensions
ln -s "$(pwd)/extension" ~/.pi/agent/extensions/magelab-agent
```

### Managing the Extension

```bash
magelab setup-pi               # Install/reinstall
magelab setup-pi --uninstall   # Remove extension
```

### How It Works

On Pi startup, the extension calls `magelab connect --json --no-launch` to find the backend, opens a WebSocket, and registers all non-native backend tools with Pi. Tools like `read_file`, `write_file`, and `run_bash` are skipped (Pi handles those natively).

## Configuration

Config file: `~/.config/magelab/cli.toml`

```toml
gateway_url = "https://api.magelab.ai"
local_url = "http://127.0.0.1:11115"
default_model = "qwen-3-235b-a22b-instruct-2507"
api_key = "mage_..."
```

Credentials are stored in the system keychain (macOS Keychain, Linux secret service, Windows Credential Manager) with a file-based fallback at `~/.config/magelab/credentials.json`.

## Testing

### Unit and Integration Tests

The `tests/` directory contains Rust integration tests covering config, credentials, connection resolution, backend detection, remote client HTTP calls, OAuth, and CLI commands.

```bash
cargo test                     # Run all tests
cargo test <test_name>         # Run a specific test
cargo test -- --nocapture      # Show println output
```

Key test files:

| File | Coverage |
|------|----------|
| `integration_test.rs` | CLI binary smoke tests (version, help, config, auth) |
| `config_test.rs`, `config_set_test.rs` | Config loading, saving, `config set` |
| `credentials_test.rs` | Credential storage round-trip |
| `connect_test.rs`, `connect_resolve_test.rs` | Connection resolution logic |
| `connection_mode_test.rs` | `--local` / `--relay` / `--remote` flag handling |
| `detect_test.rs`, `detect_http_test.rs` | Backend health checks, headless launch, device discovery (uses `wiremock`) |
| `remote_test.rs`, `remote_http_test.rs` | `RemoteClient` REST calls (uses `wiremock`) |
| `oauth_test.rs`, `login_logout_test.rs` | OAuth PKCE flow, login/logout state transitions |
| `messages_test.rs` | WebSocket protocol message serialization |
| `settings_test.rs` | Runtime settings parsing |

### Login/Logout E2E Script

`tests/test-login-logout.sh` is a multi-phase bash script that tests the full auth lifecycle against a live gateway. It builds the CLI, installs it, and runs progressively deeper tests:

```bash
# Phase 1 only (offline — no gateway needed)
./tests/test-login-logout.sh

# Phase 1 + 2 (requires running gateway + web app via Docker)
./tests/test-login-logout.sh --gateway

# Phase 1 + 2 with custom URLs
./tests/test-login-logout.sh --gateway http://localhost:65535 --web-url http://localhost:3007

# Phase 1 + 2 + 3 (alternative login methods)
./tests/test-login-logout.sh --gateway --magic --email you@example.com
./tests/test-login-logout.sh --gateway --google
```

| Phase | What it tests | Requirements |
|-------|--------------|--------------|
| 1 | Build, install, version, config, logout idempotency, status, auth failure without credentials | None (offline) |
| 2 | Web browser OAuth login, JWT validation, token refresh, authenticated commands (`models`, `balance`, `usage`), post-logout command failure | Running gateway + web app |
| 3 | Magic auth (email code) and/or Google OAuth | `--magic` and/or `--google` flag |

## Linting and Formatting

```bash
cargo check                    # Type check
cargo clippy -- -D warnings    # Lint (warnings are errors in CI)
cargo fmt --check              # Check formatting
cargo fmt                      # Auto-format
```

## Connection Modes

The CLI resolves connections in priority order:

1. **Local** — connects to a MageLab backend running at `localhost:11115` (full tool use)
2. **Relay** — tunnels through the gateway to a user's device (full tool use)
3. **Remote** — REST calls to `api.magelab.ai` (chat only, requires API key)
