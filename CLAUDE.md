# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Rust CLI for MageLab — provides LLM chat and agentic tool use from the terminal. Binary name: `magelab`. Connects to either a local MageLab backend (WebSocket, full tool use) or the remote gateway API (REST/SSE, chat only).

## Build & Development Commands

```bash
cargo build                    # Build
cargo run                      # Run (REPL mode)
cargo run -- "prompt here"     # One-shot mode
cargo install --path .         # Install locally
```

## Quality Gates (CI mirrors these)

```bash
cargo check                    # Type check
cargo test                     # All tests
cargo clippy -- -D warnings    # Lint (warnings are errors)
cargo fmt --check              # Format check
cargo fmt                      # Auto-format
```

Run a single test file:
```bash
cargo test --test config_test
```

Run a single test by name:
```bash
cargo test test_name_here
```

## Architecture

Three connection modes resolved in `main.rs::resolve_auto_mode()`:
1. **Local** — WebSocket to `ws://127.0.0.1:11115/ws` (MageLab desktop backend). Full agentic tool use with approval flow.
2. **Relay** — WebSocket through `api.magelab.ai/v1/realtime/portal/ws` to a remote device. Same protocol as local. Uses JWT auth + ws-ticket.
3. **Remote REST** — SSE streaming to `api.magelab.ai/v1/chat/completions`. Chat only, no tools. Uses API key auth.

Auto mode tries: local health check → launch headless backend → JWT relay → API key REST → prompt login.

### Module Layout

| Module | Purpose |
|--------|---------|
| `src/main.rs` | CLI args (clap), mode resolution, REPL loops, WebSocket/SSE message processing |
| `src/client/` | `local.rs` (WebSocket encode/decode), `remote.rs` (REST/SSE client), `messages.rs` (all WS message types) |
| `src/render/` | Terminal output: `tree.rs` (tool execution tree), `highlight.rs` (syntax highlighting via syntect), `markdown.rs` (termimad), `results.rs` (tool result display), `stream.rs` (status/error printing) |
| `src/repl/` | `input.rs` (slash commands), `approval.rs` (tool approval policy — auto-approve list + yolo mode) |
| `src/auth/` | `oauth.rs` (Google OAuth via Supabase, device-code-like flow with local HTTP callback), `credentials.rs` (JWT storage in `~/.config/magelab/`) |
| `src/config.rs` | Config loading from `~/.config/magelab/cli.toml` |
| `src/detect.rs` | Backend discovery: health checks, headless launch, device discovery via gateway |
| `src/settings.rs` | Runtime settings parsed from backend's WebSocket config response |

### WebSocket Protocol

The CLI speaks the same JSON protocol as the MageLab desktop frontend. Key message types in `client/messages.rs`:
- **Outgoing**: `Chat`, `NewChat`, `GetRuntimeConfig`, `SetModel`, `ConfirmationResponse`, `GetChats`, `SetChat`
- **Incoming**: `AssistantStream` (token-by-token), `ConfirmationRequest` (tool approval), `ToolResult`, `SubagentUpdate`, `RuntimeConfig`, `Error`

### Tool Approval Flow

When the backend wants to execute a tool, it sends `ConfirmationRequest`. The CLI checks `ApprovalPolicy` (auto-approve list from config + `--yolo` flag). If not auto-approved, prompts the user interactively. Response sent back via `ConfirmationResponse`.

## Tests

Tests live in `tests/` as integration tests. They use `assert_cmd` for CLI invocation testing, `wiremock` for HTTP mocking, and `tempfile` for config isolation. No unit tests inside `src/` — all tests are external.

## Config

User config: `~/.config/magelab/cli.toml`
Credentials: `~/.config/magelab/credentials.json`
