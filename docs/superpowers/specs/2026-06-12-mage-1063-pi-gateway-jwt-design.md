# MAGE-1063 — Pi Gateway provider from browser-login JWT

**Date:** 2026-06-12
**Status:** Approved
**Jira:** https://argoaeropspace.atlassian.net/browse/MAGE-1063

## Problem

After `mage setup-pi` and `pi --provider magelab`, a normally signed-in user
(default browser sign-in) gets "No models available." Pi loads but has no
MageLab model.

The Pi extension's `ensureGatewayProvider` only writes a usable provider when a
static key is present. Its JWT-fallback branch builds the provider config in a
local variable but **never persists it** (`shouldPersist = false` → no write),
so Pi never sees a `magelab` provider. The only working path requires the user
to export `MAGELAB_API_KEY`, which then lands in `~/.pi/agent/models.json` at
rest in plaintext.

A separate blocker (Workaround A — the `magelab` vs `mage` binary name) is
already fixed by `binary.ts` (`findMageBinary`).

## Core idea

Pi resolves `apiKey` references **at request time** (`"!cmd"` — shell command,
executed per request, no caching; also `"$VAR"` env interpolation).

We always write a single **command reference**, never a literal secret:

```
apiKey: "!<abs-path-to-mage> auth token"
```

`mage auth token` resolves the credential chain
(JWT → refresh → vault *(interactive only, round 4)* → `MAGELAB_API_KEY` env),
so one reference covers the browser-login JWT **and** static keys regardless of
where Pi is launched from. `models.json` never contains a literal secret.

> **Why not `"$MAGELAB_API_KEY"` when the env var is set?** That choice is made
> at setup time but resolved by Pi at request time. If the var was present when
> `setup-pi` ran but absent in Pi's later launch environment (GUI launch, a
> different shell), Pi interpolates it to empty → 401 with no fallback. The
> command form is strictly more robust — `mage auth token` already prefers the
> env var internally — so we use it unconditionally.

### TouchID caveat (corrected)

`touchid::verify(Tier::Cached, …)` is a **no-op when stdin is not a TTY**
(`is_available()` returns false). Pi resolves `"!mage auth token"` in a
non-interactive subprocess, so per-request token fetches **do not** prompt for
biometrics — there is no "one prompt per 5 minutes" gate on that path;
interactive `mage auth token` still prompts (5-minute session cache).

Because the biometric gate cannot fire non-interactively, the **vault fallback
is interactive-only** (round 4): a non-TTY caller — Pi's per-request
invocation, or any local process spawning `mage auth token` — can only ever
receive a short-lived JWT or its own `MAGELAB_API_KEY` env var, never the
long-lived vault API key. A vault-only static-key user must either `mage login`
or export `MAGELAB_API_KEY` for Pi.

## Components

### TypeScript — `extension/src/gateway.ts` (refactored for testable pure core)

- **`gatewayApiKeyRef(mageBin)`** *(pure)* — returns
  `` `!${shellArg(mageBin)} auth token` ``. `shellArg` leaves shell-safe paths
  bare and single-quotes anything else (handles spaces in `$HOME`).
- **`buildGatewayConfig(existing, apiKeyRef, models)`** *(pure)* — merges the
  `magelab` provider into the existing config, **overwriting** any prior
  `magelab` entry (migrates an old literal key/JWT → reference form), preserving
  all other providers.
- **`ensureGatewayProvider(deps?)`** *(thin IO wrapper, injectable)* — deps
  `{ home, mageBin, getToken, fetchImpl }` default to the real implementations.
  Reads `models.json`; resolves a **concrete** token via `getToken` only for the
  `GET /models` fetch (falls back to the known-model list); calls the pure
  builders; writes `models.json` and `chmod`s dir `0o700` / file `0o600`. Always
  persists. Injecting `getToken`/`fetchImpl` keeps tests hermetic (no network, no
  subprocess).
- **Remove** `readStaticApiKey` (read the cli.toml `api_key` field, now removed).

### TypeScript — `extension/src/binary.ts`

- **`findMageBinary`** also walks `PATH` on **POSIX** (not just Windows), so a
  Homebrew/`/usr/local/bin` install or a GUI-launched Pi without `~/.cargo/bin`
  on PATH still resolves to an absolute path. Falls back to bare `mage` only when
  nothing is found.
- **`runMage(args)`** — shared exec helper (binary lookup + `execFile`), reused
  by `connection.ts`, `gateway.ts`, and `index.ts` (DRY).

### TypeScript — `extension/src/index.ts`

- One `session_start` notification handler registered **eagerly** (before the
  async init), driven by a `startupNotice` flag — late registration after awaits
  could miss the event.
- Binary-missing detection keys solely on `err.code === "ENOENT"` (no brittle
  `"not found"` substring match).

### Rust — `src/main.rs` / `src/auth/mod.rs` / `crates/magelab-core`

- `cmd_auth_token` delegates to `auth::get_token(config)` (chain:
  JWT → refresh → vault *(interactive only)* → `MAGELAB_API_KEY` env → error),
  newline-free stdout.
- `get_token` no longer prints a stderr warning on refresh failure
  (per-request command — Pi may treat stderr as failure).
- **Concurrent-refresh mitigation:** on refresh failure, `get_valid_jwt`
  re-reads credentials via the new `Credentials::reload()` (cache-bypassing) and
  uses them if valid — so a process that lost a single-use refresh-token
  rotation race picks up the winner's freshly-saved token instead of erroring.
  (A cross-process lock would close the residual window; reload-retry handles the
  common staggered case without one.)
- **Removed** the vestigial `Config.api_key` field and its deprecation warning
  (the CLI already reads the static key from env/vault only).

## Error handling & edge cases

- **Not logged in / no credential** — `mage auth token` exits non-zero; Pi
  surfaces an auth error; the extension already notifies "Run: mage login". The
  `GET /models` fetch falls back to the static known-model list, so the provider
  entry is still written and works once the user logs in (no need to re-run
  setup).
- **TouchID** — see the corrected caveat above: the per-request, non-interactive
  path bypasses biometrics entirely.
- **Migration** — an existing `models.json` with an embedded JWT or static key is
  overwritten with the reference form on the next launch.
- **`$HOME` with spaces** — `shellArg` single-quotes the path; relies on Pi
  executing `"!cmd"` through a shell (per pi coding-agent docs).

## Testing (TDD)

- **TS unit:** `gatewayApiKeyRef` (bare path, path with spaces, never a literal/
  `$`); `buildGatewayConfig` (writes ref not literal, migrates old literal,
  preserves other providers, no mutation); `findMageBinary` POSIX PATH walk.
- **TS (hermetic):** `ensureGatewayProvider` with injected `getToken`/`fetchImpl`
  — always writes a `!`-command reference, never a literal; migrates a literal
  key **and** a realistic 3-segment JWT; preserves other providers; `0600` perms;
  never touches network/subprocess.
- **Rust:** `magelab-core` compiles with `Credentials::reload()`
  (`cargo check -p magelab-core` ✓). The CLI crate's gates (`cargo test/clippy`)
  must run in CI — local `cargo` is blocked by an `aws-lc-sys` build-script issue
  on the current macOS SDK, unrelated to these changes.

## Review hardening (round 2)

- **Bounded refresh-race retry** — `get_valid_jwt` retries `Credentials::reload()`
  up to 3× with ~75 ms backoff on refresh failure, giving a concurrent winner's
  write time to land. A cross-process lock remains the only full fix (follow-up).
- **`reload()` deduplicated + documented** — `load()` and `reload()` share a
  private `load_uncached()`; `reload()`'s doc now states it refreshes the cache
  (it does not silently "bypass" it).
- **Executable-file resolution** — `findMageBinary`'s default existence check
  requires a regular, executable file (`accessSync(X_OK)`), and `getConnection`
  normalizes `ENOENT`/`EACCES`/`EISDIR`/`ENOTDIR` → "CLI not found", so a
  non-executable/dir `mage` on PATH no longer causes 5 confusing retries.
- **No TouchID at startup** — the startup model-list token fetch uses
  `mage --no-touchid auth token` (read-only list, token not persisted), so Pi
  launch never triggers a biometric prompt.
- **`runMage` hardened + injectable** — adds `{ timeout: 15s, maxBuffer: 1 MiB }`
  and optional `bin`/`exec` injection; `getConnection` takes an injectable runner
  (hermetic tests).
- **Notice timing** — `flushStartupNotice` notifies immediately if `session_start`
  already fired, else when the notice is set — covering both orderings.
- **Permissions** — only `models.json` is `chmod 0600`; `~/.pi/agent` (Pi-owned)
  is left untouched.
- **Windows quoting** — KNOWN LIMITATION: POSIX single-quoting in `shellArg` does
  not apply to cmd.exe; spaced Windows install paths are unsupported pending
  confirmation of Pi's Windows `"!cmd"` semantics.

## Review hardening (round 3)

- **Refresh-retry latency removed** — the round-2 sleep-loop added ~150 ms to the
  per-request path for *every* expired/revoked-token user (no concurrent winner).
  Replaced with a single cache-bypassing `reload()`: use the reloaded token only
  if the stored creds **changed** (a winner wrote them) and are valid; otherwise
  bail immediately. Zero added latency on the common path; no `tokio::sleep`.
- **EACCES distinct remediation** — a present-but-non-executable `mage` now yields
  "CLI found but not executable — chmod +x" (still tagged ENOENT to short-circuit
  retries) instead of the misleading "Run: mage setup-pi". index.ts surfaces the
  message verbatim.
- **Production resolver tested** — added real-filesystem tests for the default
  `isExecutableFile` path (executable vs non-executable vs directory) and a
  `runMage` test asserting arg/timeout/maxBuffer pass-through.
- **Single `session_start` handler** — consolidated the eager notice handler and
  the late tool-activation handler into one registration driven by an `onReady`
  callback + stored ctx, so tool activation is never dropped if `session_start`
  fires during the connect delay, and behavior no longer depends on whether
  `pi.on` is additive or last-wins.
- **`shellArg`** drops `%`/`,` from the bare-safe set (`%` is a cmd.exe sigil).
- **`KNOWN_MODELS` fallback** now logs a `console.warn` so a drifted static list
  is diagnosable. Removed a dead `const name` in the `tool_result` handler and the
  flaky real-binary `connection.test` block (superseded by hermetic tests).

### Noted, not changed
- `findMageBinary`'s `accessSync(X_OK)` is an existence check on Windows (Node
  semantics) — correct, not a regression.
- Unbounded `socket.on` listener accumulation in `streamSimple` is a pre-existing
  leak unrelated to MAGE-1063 (tracked separately).

## Review hardening (round 4 — security review findings)

- **Vault fallback is interactive-only** — TouchID is a no-op for non-TTY
  callers, so the unconditional vault fallback let *any* local process harvest
  the long-lived vault API key by spawning `mage auth token`. `get_token` now
  routes the static fallback through the pure, tested
  `magelab_core::auth::static_token_fallback(interactive, vault, env)` with
  `interactive = stdin.is_terminal()`: non-interactive callers are limited to a
  short-lived JWT or their own `MAGELAB_API_KEY` env var. (TDD: the test
  asserts the vault closure is never invoked when non-interactive.)
- **Atomic credential saves** — `Credentials::save_to_file` previously used
  truncate-in-place `std::fs::write` (+ chmod after, leaving a brief 0644
  window). Concurrent per-request `mage` processes could interleave and let a
  reader observe a truncated `credentials.json` — a 4-writer stress test
  reproduced a 128-byte partial read of a 32 KB payload within 30 ms. Saves now
  go through `write_json_atomically`: uniquely-named temp file in the same
  directory, created `0600`, fsynced, then renamed over the destination. This
  also shrinks (not closes) the round-3 race window — the winner's save lands
  as a single atomic event.

## Out of scope

- The binary-name fix (`binary.ts`) — already done.
- Changing how the desktop app or web cli-token route issue/refresh JWTs.
