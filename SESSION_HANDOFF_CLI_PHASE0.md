# MageLab CLI Phase 0-6 + Pi Extension — Session Handoff

## Quick Status
- Context: ~190K/200K tokens used
- Time invested: ~6 hours
- Completion: Phases 0, 1, 2, 3, 6 done. Auth flow broken.
- Next session estimate: 1-2 hours (auth fix + polish)

## Completed Work (Committed)

### Phase 0: CLI Infrastructure
All infrastructure commands implemented: login/logout, auth token, connect --json, launch --wait, status, devices, models/usage/balance, keys, config, version, setup-pi, completions.

### Phase 1: Pi Extension MVP + Post-MVP
- Extension at `extension/` — symlinked to `~/.pi/agent/extensions/magelab-agent/`
- Registers 26+ backend tools with Pi via WebSocket `get_tools`/`tool_call` protocol
- Permission gate: Always/Session/No dialog for tool confirmations
- Reconnection via partysocket (auto backoff)
- Gateway provider auto-configuration via `gateway.ts`

### Phase 2: Skills & Commands
- `resources_discover` handler registers `~/Mage/Skills/` with Pi
- `/magelab`, `/chats`, `/chat`, `/newchat`, `/backend-model` commands
- Skill slash commands auto-discovered from `commands/*.md`

### Phase 3: Backend Events
- SubagentUpdate/SubagentComplete → Pi status line
- Notify/OpenUrl/OpenFile → Pi notifications
- Tool execution status via `setWorkingMessage`

### Phase 6: Protocol Schema
- `schemas/websocket/protocol.json` — 41 message types
- `schemas/codegen.ts` — generates Rust + TypeScript from schema
- Both `messages.rs` and `protocol.ts` are auto-generated

### Custom Pi Provider (Key Architecture)
- MageLab backend registered as `magelab-backend` Pi provider via `streamSimple`
- User messages go through Pi's native UX (user bubble, assistant bubble)
- Backend agent handles LLM + tool execution, streams response back
- `~/.pi/agent/settings.json` sets `magelab-backend` as default provider
- Pi's own LLM is NOT called — `before_agent_start` sends to backend, but **cannot cancel Pi's agent turn** (Pi doesn't support cancel in `BeforeAgentStartEventResult`)
- Current workaround: backend response arrives via `sendMessage` but Pi also tries to call its own provider (hitting 402 if Gateway has no credits)

### Backend WebSocket Handlers
- PR Sapient-Artifice/mage-lab#302 adds `get_tools` and `tool_call` to Python backend
- Backend must be on branch `feat/ws-tool-call-protocol` for extension to work
- 5 Python tests pass

## Uncommitted Work
```
$ git status --short
?? .DS_Store
```
Everything is committed and pushed.

## Known Bugs to Fix

### 1. Auth flow redirect loop (HIGH PRIORITY)
**Root cause:** CLI opens `{web_url}/auth/sign-in?returnTo={web_url}/api/auth/cli-token?redirect=...`
The `/api/auth/cli-token` endpoint **does not exist** in the SaaS frontend. The SaaS frontend uses Supabase auth (`/auth/callback`), not a custom CLI token exchange.

The user believes the CLI should be hitting the **Next.js web app** (port 3007) not the SaaS frontend. The Next.js app may have or need a `/api/auth/cli-token` endpoint.

**Relevant code:**
- CLI login flow: `src/auth/oauth.rs:108` (`login_web()`)
- `web_url()` function determines which URL to open
- SaaS frontend auth: `magelab-saas-frontend/src/routes/auth/callback/+server.ts` (Supabase-based)
- The `MAGELAB_WEB_URL` env var overrides the web URL

**Fix approach:** Either:
a) Add `/api/auth/cli-token` endpoint to the correct web app (Next.js at port 3007)
b) Change CLI to use WorkOS directly (bypass web app entirely)
c) Check if the Next.js web app already has this endpoint

### 2. Pi agent turn not fully cancellable (MEDIUM)
**Root cause:** `BeforeAgentStartEventResult` doesn't support `cancel: true`. When using `magelab-backend` provider, Pi still tries to call the provider. If Gateway has no credits → 402 error shows alongside backend response.
**Workaround:** Use `--provider magelab-backend --model magelab-agent` which uses the custom `streamSimple` handler and bypasses the Gateway entirely.

### 3. Leaked API key in git history (HIGH - SECURITY)
**Root cause:** `.claude/settings.local.json` was committed with tool call history containing a real API key
**Status:** File removed from tracking, added to .gitignore. Key `mage_ngrubugAKSQSAZw55_-xhLYJUfvjnOa4d9tO-_3X-1E` is still in git history.
**Fix:** User must revoke this key and generate a new one. Consider `git filter-branch` or BFG to scrub history.

### 4. Backend agent modifies repo files (MEDIUM)
**Root cause:** When testing `/magelab` command, the backend agent used `write_file` and `run_bash` to create files in the repo (`pibonaci.rs`, modified `Cargo.toml`, `main.rs`).
**Fix:** Permission gate should deny `write_file` by default. Or run Pi from a different directory than the repo.

## Non-Obvious System Knowledge

- **Pi's `input` event `action: "handled"` hides user message:** The user's typed text disappears entirely. Must use `action: "continue"` to preserve the native user bubble display.
- **Pi's `registerTool` doesn't auto-activate tools:** Tools are registered but not active. Must call `pi.setActiveTools([...active, ...newTools])` in `session_start` handler (can't call during extension load).
- **Pi's `before_agent_start` can't cancel:** `BeforeAgentStartEventResult` only has `message` and `systemPrompt` fields, no `cancel`. `ctx.abort()` doesn't prevent the provider request either.
- **`streamSimple` is the right approach:** Register a custom provider that sends to the backend via WebSocket and streams the response. This gives native Pi UX (user/assistant bubbles, streaming).
- **Backend `stream: False` by default:** The backend sends a single `assistant` message, not streaming tokens. The extension handles both modes.
- **`setup-pi` without `--dev` copies embedded files:** Files are embedded in the Rust binary at compile time. `--dev` symlinks to the repo source instead.
- **`assistant_stream` vs `assistant` messages:** Backend sends `assistant_stream` (phase: start/delta/end) when streaming, `assistant` (text: full response) when not. Extension handles both.
- **`tool_debug` messages contain user prompt text:** Must filter to `message_type === "tool_call"` only, or the user's input replaces the "Working..." spinner text.
- **Pi jiti loader aliases:** `@mariozechner/pi-tui`, `@sinclair/typebox`, `@mariozechner/pi-coding-agent` are available via Pi's jiti virtual modules. Sub-path imports (like `/dist/modes/...`) need `require.resolve` to find the package root first.

## Key Architecture Decisions

- **Backend as Pi provider (not input interceptor):** Registering `magelab-backend` as a `streamSimple` provider is cleaner than intercepting `input` events. Pi handles all UX natively.
- **Symlink for dev, embed for release:** `setup-pi --dev` symlinks to repo source for instant iteration. `setup-pi` (without --dev) embeds files in the binary for distribution.
- **JSON Schema as protocol source of truth:** All 41 WebSocket message types defined in `schemas/websocket/protocol.json`, codegen produces Rust + TypeScript.
- **Skills loaded directly by Pi, not bridged:** Pi's `resources_discover` reads `~/Mage/Skills/` directly. No backend involvement needed for skills.

## Failed Approaches (Do NOT Retry)

- **`input` event with `action: "handled"`:** User message disappears. Can't display it without triggering Pi's LLM.
- **`before_agent_start` with `cancel: true`:** Not supported by Pi's API.
- **`before_provider_request` returning null:** Crashes Pi with "Cannot read properties of null (reading 'stream')".
- **`ctx.abort()` in `before_agent_start`:** Doesn't prevent the provider request from firing.
- **`Markdown` TUI component without theme:** Crashes with "Cannot read properties of undefined (reading 'listBullet')". Must pass a `MarkdownTheme` object.
- **Importing `@mariozechner/pi-coding-agent/dist/modes/interactive/theme/theme.js`:** Jiti resolves the package alias to `index.js` then appends the path, creating a wrong path. Must use `require.resolve` to find the package root first.

## Files Modified

```
$ git log --oneline -20
cb3e0d2 fix: add trailing newline to login_logout_test.rs
dfa8fab fix: remove leaked API key from .claude/settings.local.json
1820044 fix: remove agent-injected test that times out in CI
b6e49df fix: apply cargo fmt to all source files
5f12687 fix: add explicit type to rng.gen() for Windows compatibility
a245a99 fix: restore CI — fix codegen test compat, clean up agent damage
4198aea fix(ext): only show tool_call debug messages in working spinner
c1e4aef fix(ext): use setWorkingMessage for tool/subagent status during streaming
4c8434b fix(ext): handle non-streaming backend responses and Esc abort
600b902 feat(ext): MageLab backend as Pi custom provider with streaming
1797b90 feat(ext): three-option permission gate (Always/Session/No)
e797698 fix(ext): remove duplicate response display
071a562 fix: redirect backend stdout/stderr to ~/.config/magelab/backend.log
1862994 fix(ext): rename /model to /backend-model to avoid Pi builtin conflict
d9c92d8 feat(ext): add /chats, /chat, /newchat, /model commands and tool display
df4f701 feat(ext): add /magelab command, skill commands, and agent streaming (Phase 2)
3c31796 feat: add schema-first WebSocket protocol with codegen (Phase 6)
92a3724 feat(ext): add subagent status display and backend notifications (Phase 3)
bcf308d feat(ext): add permission gate for tool confirmations
1050677 feat: add @magelab/agent Pi extension and setup-pi command

$ git status --short
?? .DS_Store
```

## Resume Commands

```bash
# 1. Check state
cd ~/magelab/magelab-cli
git log --oneline -5
git status

# 2. Start backend (must be on feat/ws-tool-call-protocol branch)
cd ~/magelab/mage-lab
git checkout feat/ws-tool-call-protocol
cd backend
SKIP_FRONTEND_BUILD=1 .venv/bin/python -m uvicorn main:app --host 127.0.0.1 --port 11115 --log-level warning &

# 3. Ensure extension is symlinked
cd ~/magelab/magelab-cli
ls -la ~/.pi/agent/extensions/magelab-agent
# Should be: symlink -> /Users/maxcarlsonold/magelab/magelab-cli/extension

# 4. Start Pi
pi --provider magelab-backend --model magelab-agent
# Or just `pi` if settings.json is configured

# 5. Run tests
cargo test
cd extension && pnpm test
```

## Testing Checklist
- [x] Extension connects to backend and registers 26 tools
- [x] /magelab command sends to backend agent
- [x] Permission gate (Always/Session/No) works
- [x] Subagent status shows in Pi
- [x] Backend notifications forwarded
- [x] WebSocket reconnection with backoff
- [x] Schema codegen (Rust + TS)
- [x] Skills discovered via resources_discover
- [x] Skill slash commands registered
- [x] Custom provider streams responses
- [ ] Auth flow (login_web) works end-to-end
- [ ] Gateway balance check on startup
- [ ] CI fully green on all platforms
- [ ] Streaming tokens display incrementally (backend has stream: False)
