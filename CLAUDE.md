# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## What This Is

Rust CLI for MageLab — infrastructure management tool. Binary name: `mage`. Handles auth, backend detection/launch, account management, config, settings, and device management. NOT a coding agent — the agent experience is provided by the Pi coding agent with the `@magelab/agent` extension.

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
mage login/logout              # WorkOS OAuth or magic auth
mage auth token                # Print JWT to stdout (for Pi extension)
mage connect [--json]          # Resolve backend connection
mage launch [--wait]           # Start headless backend
mage status                    # Health check
mage settings                  # Show backend runtime settings (via WebSocket)
mage settings set model <val>  # Change backend model
mage settings set voice <val>  # Change backend voice
mage devices                   # List/bind/detach relay devices
mage models/usage/balance      # Account info
mage keys list/create/revoke
mage config [set <k> <v>]      # CLI config (~/.config/magelab/cli.toml)
mage completions <shell>       # Generate shell completions
mage setup-pi [--dev]          # Install @magelab/agent Pi extension
mage version
```

### Module Layout

| Module | Purpose |
|--------|---------|
| `src/main.rs` | CLI args (clap), subcommand dispatch |
| `src/connect.rs` | Connection resolution: local → launch → relay → remote → none |
| `src/auth/` | `oauth.rs` (WorkOS PKCE + magic auth + web login), `credentials.rs` (keychain + file storage) |
| `src/auth/touchid/` | macOS biometric authentication for sensitive operations |
| `src/client/` | `remote.rs` (REST client for Gateway API) |
| `src/detect.rs` | Backend discovery, health check, headless launch, device discovery |
| `src/config.rs` | Config loading/saving from `~/.config/magelab/cli.toml` |
| `src/settings.rs` | Runtime settings parsed from backend WebSocket responses |
| `src/account.rs` | Models, usage, balance, API key management |
| `src/ui.rs` | Spinner, animated prompt, terminal UI helpers |

## Config

User config: `~/.config/magelab/cli.toml`
Credentials: `~/.config/magelab/credentials.json` (or system keychain)

## Code Style

- **Pure functions by default.** Extract logic into pure functions with injectable dependencies (e.g. `exists`, `home`) so they're testable without mocking the filesystem or environment. Side-effectful wrappers call the pure core.
- **TDD where it makes sense.** Write failing tests before implementation for non-trivial logic. Unit tests for pure functions; integration tests only where real I/O is unavoidable. Always write tests for existing behavior before modifying it.

## Design Spec

See: `docs/superpowers/specs/2026-04-22-cli-pi-strategy-design.md`
