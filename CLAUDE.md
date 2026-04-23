# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## What This Is

Rust CLI for MageLab — infrastructure management tool. Binary name: `magelab`. Handles auth, backend detection/launch, account management, config, and device management. NOT a coding agent — the agent experience is provided by the Pi coding agent with the `@magelab/agent` extension.

## Build & Development Commands

```bash
cargo build                    # Build
cargo install --path .         # Install locally
cargo run -- version           # Check version
cargo run -- --help            # Show all commands
```

## Quality Gates (CI mirrors these)

```bash
cargo check                    # Type check
cargo test                     # All tests
cargo clippy -- -D warnings    # Lint (warnings are errors)
cargo fmt --check              # Format check
```

## Architecture

Lean infrastructure CLI. No REPL, no rendering, no streaming.

### Commands

```
magelab login/logout           # WorkOS OAuth or magic auth
magelab auth token             # Print JWT to stdout (for Pi extension)
magelab connect [--json]       # Resolve backend connection
magelab launch [--wait]        # Start headless backend
magelab status                 # Health check
magelab devices                # List/bind/detach relay devices
magelab models/usage/balance   # Account info
magelab keys list/create/revoke
magelab config [set <k> <v>]
magelab version
```

### Module Layout

| Module | Purpose |
|--------|---------|
| `src/main.rs` | CLI args (clap), subcommand dispatch |
| `src/connect.rs` | Connection resolution: local → launch → relay → remote → none |
| `src/auth/` | `oauth.rs` (WorkOS PKCE + magic auth), `credentials.rs` (keychain + file storage) |
| `src/client/` | `remote.rs` (REST client for Gateway API), `messages.rs` (WebSocket protocol types) |
| `src/detect.rs` | Backend discovery, health check, headless launch, device discovery |
| `src/config.rs` | Config loading/saving from `~/.config/magelab/cli.toml` |
| `src/account.rs` | Models, usage, balance, API key management |
| `src/settings.rs` | Runtime config parsing from backend responses |

## Config

User config: `~/.config/magelab/cli.toml`
Credentials: `~/.config/magelab/credentials.json` (or system keychain)

## Design Spec

See: `docs/superpowers/specs/2026-04-22-cli-pi-strategy-design.md`
