# MageLab CLI

LLM chat and agentic tool use from the terminal.

## Install

```bash
cargo install --path .
```

## Usage

```bash
mage                          # REPL mode
mage "explain this error"     # One-shot chat
mage -m gpt-4o "..."         # Choose model
mage --local                  # Force local backend
mage --remote                 # Force gateway API
mage --yolo                   # Auto-approve all tools

mage models                   # List available models
mage usage                    # Show usage summary
mage balance                  # Show account balance
mage keys list                # List API keys
mage config                   # Show configuration
```

## Configuration

Config file: `~/.config/magelab/cli.toml`

```toml
api_key = "mage_..."
default_model = "qwen-3-235b-a22b-instruct-2507"
gateway_url = "https://api.magelab.ai"
prefer = "auto"  # auto | local | remote
auto_approve = ["read_file", "search_files", "BraveSearch"]
```

## Connection Modes

**Local mode** — connects to `ws://127.0.0.1:11115/ws` for full agentic tool use (files, shell, Python, search). If the backend isn't running, the CLI can launch it in headless mode.

**Remote mode** — connects to `api.magelab.ai` for chat-only mode via REST/SSE. Requires an API key.
