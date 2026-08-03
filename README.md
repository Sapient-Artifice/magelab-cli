# MageLab CLI

Infrastructure management tool for MageLab. Handles CLI authentication, headless backend detection and launch, account management, device management, and configuration.

The installed binary is `mage`.

## Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)

## Building

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo install --path .         # Install `mage` binary to ~/.cargo/bin
```

## Running

```bash
mage --help                    # Show all commands
mage version                   # Print version

# Authentication
mage login                     # Browser-based OAuth login (default)
mage login --method magic      # Email magic-code login
mage login --status            # Show current auth status
mage logout                    # Clear stored credentials
mage auth token                # Print JWT to stdout (for piping)

# Connection management
mage connect                   # Auto-resolve: local → launch → relay → remote
mage connect --json            # Output as JSON (for programmatic use)
mage connect --local           # Force local backend only
mage connect --relay           # Force relay mode only
mage connect --remote          # Force remote REST mode only
mage connect --no-launch       # Don't auto-launch backend
mage connect --url <http-url>  # Probe a running backend without changing config
mage connect --ws <ws-url>     # Probe a running WebSocket backend

# Backend management
mage launch                    # Start headless backend
mage launch --wait             # Start and block until healthy
mage launch --dry-run          # Print resolved backend bundle and command inputs
mage launch --host 0.0.0.0 --allow-network
                               # Bind beyond localhost (exposes full tool access)
mage launch --port 8787        # Override local_url port for this launch
mage status                    # Show backend health and auth info

# Device management
mage devices                   # List online relay devices
mage devices --json            # Output as JSON
mage devices bind <id>         # Bind to a specific device
mage devices detach            # Unbind from current device

# Account info (requires auth)
mage models                    # List available models
mage usage                     # Show token usage summary
mage balance                   # Show account credit balance

# API key management
mage keys list                 # List API keys
mage keys create               # Create a new API key
mage keys revoke <id>          # Revoke an API key

# Configuration
mage config                    # Show current config and file path
mage config set <key> <val>    # Set a config value

# Headless sessions and assistant turns (Mage v0.12.0+)
mage sessions list --json
mage sessions create --name CRM --mcp pipedrive --json
mage sessions update 42 --model model-name --mcp pipedrive --json
mage chats create --session 42 --json
mage chats switch --session 42 --chat 91 --json
mage ask "Fetch deal 2" --session 42 --chat 91 --jsonl
mage storage health --json
mage protocol capabilities --json
```

## Login, Launch, and Connect

These commands intentionally do different jobs:

- `mage login` signs the CLI into the MageLab cloud account. It does not start the desktop app or backend.
- `mage launch` starts the backend headlessly. It does not open the desktop UI and does not perform interactive sign-in.
- `mage connect` finds a usable backend connection, optionally launching a local backend when allowed.

Recommended authenticated headless flow:

```bash
mage login
mage launch --wait
mage connect
```

`mage launch --wait` starts the backend, waits for health, and then pushes available vault secrets to the running backend. If you launch first and sign in afterward, push secrets explicitly:

```bash
mage launch --wait
mage login
mage vault push
```

To connect to an already-running backend on a non-default port or host:

```bash
mage connect --url http://127.0.0.1:8787
mage connect --ws ws://127.0.0.1:8787/ws
```

For remote machines, the backend must be launched with a reachable bind address, such as `--host 0.0.0.0 --allow-network`, and firewall plus browser origin settings must allow the client. Binding beyond localhost exposes the backend's full tool access to the network, so the CLI requires the explicit `--allow-network` opt-in.

### Headless Backend Discovery

`mage launch` supports both development and packaged layouts.

Development repo layout:

```text
<mage-lab>/backend/main.py
<mage-lab>/backend/.venv/bin/python
```

Dev repo discovery also requires a repository sentinel such as `.git`, `pyproject.toml`, `backend/pyproject.toml`, or `package.json`. This prevents accidentally launching an unrelated `backend/main.py` found while walking nearby directories.

Packaged API layout:

```text
<install-root>/bin/api/backend/main.py
<install-root>/bin/api/python/bin/python3
```

macOS app bundle layout:

```text
/Applications/magelab.app/Contents/Resources/bin/api/backend/main.py
/Applications/magelab.app/Contents/Resources/bin/api/python/bin/python3
```

Discovery order:

1. `MAGELAB_API_DIR`
2. `MAGELAB_HOME`
3. `magelab_home` from `~/.config/magelab/cli.toml`
4. nearby development layouts
5. packaged platform defaults

`magelab_home` should point to an install root or bundled API directory, not directly to `backend/main.py`.

Examples:

```bash
MAGELAB_HOME=/path/to/mage-lab mage launch --wait
MAGELAB_API_DIR=/Applications/magelab.app/Contents/Resources/bin/api mage launch --dry-run
mage config set magelab_home /Applications/magelab.app
```

## Headless Client Commands

The headless commands implement Mage's acknowledged WebSocket flow. Runtime
state is confirmed before chat creation or selection, prompts carry a unique
`client_request_id`, and a turn remains open until the matching
`assistant_complete` event arrives. A stream `end` event is not treated as the
end of a tool-using turn.

Create a session with programmatic MCP selection and start its first chat:

```bash
mage sessions create --name CRM --mcp pipedrive --mcp hubspot --json
mage chats create --session 42 --json
```

Run a turn against an existing chat:

```bash
mage ask "Summarize the current deal" \
  --session 42 \
  --chat 91 \
  --model model-name \
  --mcp pipedrive \
  --jsonl
```

Or create a confirmed chat immediately before the prompt:

```bash
mage ask "Start a new analysis" --session 42 --new-chat --json
```

In `--json` mode, stdout contains only the final JSON result. In `--jsonl`
mode, stdout contains one JSON object per correlated assistant event followed by
an `assistant_complete` envelope. Human diagnostics and errors use stderr.

`mage ask` uses these exit codes:

| Code | Meaning |
|------|---------|
| `0` | Assistant completed successfully |
| `2` | Invalid CLI/setup arguments |
| `3` | Connection lost or unavailable |
| `4` | Runtime/chat setup failed |
| `5` | Operation or turn timed out |
| `6` | Turn was cancelled |
| `7` | Assistant returned an error terminal status |

`mage protocol capabilities --json` reports the client contract, not an
unverified backend claim. Compatibility is feature-probed from required echoed
request identifiers, terminal status events, and persisted session state. The
client does not infer safety from a version string; an older backend fails with
an explicit v0.12.0 compatibility error.

### Concurrency boundary

Mage v0.12.0 still has one mutable active runtime. One CLI invocation runs one
acknowledged setup-and-turn sequence, but separate processes are not coordinated
automatically. Services that share one Mage backend must use a process-wide—or,
across service replicas, distributed—serialization coordinator covering runtime
setup through `assistant_complete`.

Do not use a subprocess-per-request `mage ask` wrapper as a high-throughput Node
integration. The reusable TypeScript client under `extension/src/client/` is the
long-lived Node/Pi integration path; the CLI is intended for operators, shell
automation, CI, and protocol diagnostics.

These commands operate on Mage sessions, chats, runtime state, and MCP server
names. They do not implement Drupal entity mapping, tenant authorization, or
customer-specific prompt policy.

See [Persistent Node.js Headless Client Example](docs/node-headless-client-example.md)
for the supported long-lived service pattern and application responsibility
boundary.

## Pi Extension

The CLI includes `@magelab/agent`, a [Pi coding agent](https://github.com/badlogic/pi-mono) extension that bridges MageLab backend tools (python, web search, image generation, subagents, etc.) into Pi.

### Quickstart

```bash
# 1. One command installs everything (Pi + extension + dependencies)
mage setup-pi

# 2. Sign in and start MageLab backend (if not already running)
mage login
mage launch --wait

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
mage setup-pi                  # Install/reinstall
mage setup-pi --uninstall      # Remove extension
```

### How It Works

On Pi startup, the extension calls `mage connect --json --no-launch` to find an already-running backend, opens a WebSocket, and registers all non-native backend tools with Pi. Tools like `read_file`, `write_file`, and `run_bash` are skipped (Pi handles those natively).

## Configuration

Config file: `~/.config/magelab/cli.toml`

```toml
gateway_url = "https://api.magelab.ai"
local_url = "http://127.0.0.1:11115"
default_model = "qwen-3-235b-a22b-instruct-2507"
magelab_home = "/Applications/magelab.app"
```

Credentials use the system keychain by default (macOS Keychain, Linux secret service, Windows Credential Manager). Non-keychain modes must be explicit:

- `MAGELAB_AUTH_MODE=keychain` or unset: use the system keychain.
- `MAGELAB_AUTH_MODE=file`: use the platform config credentials file with restrictive file permissions for headless systems (`~/.config/magelab/credentials.json` on macOS/Linux, `%APPDATA%\magelab\credentials.json` on Windows).
- `MAGELAB_AUTH_MODE=env`: read `MAGELAB_ACCESS_TOKEN` and optionally `MAGELAB_REFRESH_TOKEN` without writing credentials.

Production runs should use keychain mode. File mode is intended for headless or CI-style environments where a secure OS credential store is unavailable.

Plaintext `api_key` in `cli.toml` is deprecated. Prefer the desktop app vault or `MAGELAB_API_KEY`.

## Testing

### Unit and Integration Tests

The `tests/` directory contains Rust integration tests covering config, credentials, connection resolution, backend detection, remote client HTTP calls, OAuth, and CLI commands.

```bash
cargo test -p magelab-cli                     # Run all tests from the workspace root
cargo test -p magelab-cli <test_name>         # Run a specific test
cargo test -p magelab-cli -- --nocapture      # Show println output
```

Key test files:

| File | Coverage |
|------|----------|
| `integration_test.rs` | CLI binary smoke tests (version, help, config, auth) |
| `config_test.rs`, `config_set_test.rs` | Config loading, saving, `config set` |
| `credentials_test.rs` | Credential storage round-trip |
| `connect_test.rs`, `connect_resolve_test.rs` | Connection resolution logic |
| `detect_test.rs`, `detect_http_test.rs` | Backend health checks, headless launch, device discovery (uses `wiremock`) |
| `remote_test.rs`, `remote_http_test.rs` | `RemoteClient` REST calls (uses `wiremock`) |
| `oauth_test.rs`, `login_logout_test.rs` | OAuth PKCE flow, login/logout state transitions |
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
cargo check -p magelab-cli                    # Type check from the workspace root
cargo clippy -p magelab-cli -- -D warnings    # Lint (warnings are errors in CI)
cargo fmt --check                             # Check formatting
cargo fmt                                     # Auto-format
```

## Connection Modes

The CLI resolves connections in priority order:

1. **Explicit local URL** — probes `mage connect --url` or `mage connect --ws` without mutating config
2. **Configured local** — connects to the backend at `local_url`, default `http://127.0.0.1:11115`
3. **Headless launch** — starts a local backend unless `--no-launch` was set
4. **Relay** — tunnels through the gateway to a user's device (full tool use)
5. **Remote** — REST calls to `api.magelab.ai` (chat only, requires auth/API key)
