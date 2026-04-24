# Touch ID Integration for magelab CLI

**Date:** 2026-04-24
**Status:** Approved design

## Overview

Add macOS Touch ID support to the magelab CLI to protect credential access, gate sensitive operations, and enable biometric-gated token refresh. Touch ID is auto-detected when hardware is present and the session is interactive. No configuration required. A `--no-touchid` global flag provides an escape hatch for scripts and CI.

## Use Cases

1. **Protect credential access** -- require Touch ID before the CLI reads the stored JWT from keychain. Prevents silent token extraction by unauthorized processes.
2. **Gate sensitive operations** -- require Touch ID before destructive or high-privilege commands (`keys create`, `keys revoke`, `logout`), while letting read-only commands use a session cache.
3. **Biometric-gated token refresh** -- when the JWT expires, Touch ID unlocks a biometric-protected refresh token in the Keychain, which silently exchanges for a new JWT via the gateway. Falls back to browser login if the refresh token itself is expired.

## Approach

Use the `security-framework` Rust crate to create Keychain items with `kSecAccessControlBiometryCurrentSet`. The refresh token is stored in a biometric-protected Keychain item separate from the regular credentials. The OS triggers the Touch ID prompt automatically when the item is accessed.

On non-macOS platforms, all biometric code compiles to no-ops. Existing behavior is unchanged.

## Architecture

```
+--------------------------------------------------+
|                  CLI Commands                     |
|  (main.rs -- login, auth token, keys, logout)     |
+------------------------+-------------------------+
                         |
              +----------v----------+
              |   credentials.rs    |
              |   load() / save()   |
              +----------+----------+
                         |
              +----------v----------+
              |     touchid.rs      |
              |  +----------------+ |
              |  | verify()       | |  <- session cache check, then OS prompt
              |  | store_secure() | |  <- biometric-gated Keychain item
              |  | load_secure()  | |  <- Touch ID triggered by OS on access
              |  | is_available() | |  <- hardware + interactive check
              |  +----------------+ |
              |  #[cfg(target_os =  |
              |   "macos")]         |
              +----------+----------+
                         |
              +----------v----------+
              |   macOS Keychain    |
              | (Security.framework |
              |  via security-      |
              |  framework crate)   |
              +---------------------+
```

## Credential Storage

### Two Keychain items with different protection levels

| Item | Crate | Contents | Protection |
|------|-------|----------|------------|
| `magelab-cli/default` | `keyring` (existing) | `{ access_token, expires_at, user_id, email }` | Standard keychain (no biometric) |
| `magelab-cli/refresh-bio` | `security-framework` | Raw refresh token string | `kSecAccessControlBiometryCurrentSet` |

The access token is short-lived and read frequently (e.g., `auth token` for the Pi extension). Gating it behind biometrics every time would cause friction. The refresh token is the high-value secret -- it mints new JWTs. Biometric-gating the refresh token means:

- Reading a cached valid JWT: session cache check only (fast)
- Refreshing an expired JWT: Touch ID prompt -> unlock refresh token -> silent exchange
- Stolen credential file: attacker gets an expired JWT but cannot refresh without a fingerprint

## Tiered Verification

### Tier: Sensitive (always prompt)

Operations that are destructive or grant elevated access. Touch ID is prompted every time, regardless of session cache.

| Command |
|---------|
| `magelab keys create` |
| `magelab keys revoke` |
| `magelab logout` |
| First token access after expiry (refresh flow) |

### Tier: Cached (session cache, 5-minute window)

Read-only or frequent operations. After a successful Touch ID verification, subsequent commands within 5 minutes skip the prompt.

| Command |
|---------|
| `magelab auth token` |
| `magelab models` |
| `magelab usage` |
| `magelab balance` |
| `magelab connect` |
| `magelab status` |
| `magelab devices` |

### Session cache

A file at `~/.config/magelab/.touchid-session`:

- Contents: Unix timestamp of last successful verification
- Permissions: `0600`
- Valid when: timestamp is within 5 minutes AND file owner matches current UID
- Deleted on: `magelab logout`

No daemon or IPC. Each CLI invocation reads the file and checks the timestamp. The 5-minute window is hardcoded.

### Bypass: `--no-touchid`

A global CLI flag `magelab --no-touchid <command>` that skips all biometric checks. Falls back to current behavior (keyring without biometric gate, refresh token in regular item). For scripts, CI, and users who need to bypass temporarily.

`magelab login` does not require Touch ID since it is already browser-authenticated.

## Verification Flows

### Sensitive operation flow

```
Command invoked (e.g., keys create)
  -> touchid::is_available()?
    -> No:  proceed without prompt (graceful fallback)
    -> Yes: touchid::verify(Tier::Sensitive, "manage API keys")?
      -> Trigger standalone Touch ID prompt
      -> Success: proceed
      -> Fail/cancelled: abort with error
```

### Cached operation flow

```
Command invoked (e.g., auth token)
  -> touchid::is_available()?
    -> No:  proceed without prompt
    -> Yes: touchid::verify(Tier::Cached, "access auth token")?
      -> Check session cache file
        -> Valid (< 5 min, same UID): proceed, no prompt
        -> Expired/missing: trigger Touch ID
          -> Success: write session cache, proceed
          -> Fail/cancelled: abort with error
```

### Token refresh flow

```
JWT expired, refresh needed
  -> touchid::is_available()?
    -> No:  read refresh token from regular keychain (current behavior)
    -> Yes: touchid::load_secure()?
      -> OS triggers Touch ID to unlock biometric Keychain item
      -> Success: get refresh token, exchange for new JWT via gateway
      -> Fail: fall back to browser login
```

## Module API

### `src/auth/touchid.rs` -- public interface

```rust
/// Operation sensitivity tier
pub enum Tier {
    /// Always prompt Touch ID (keys, logout)
    Sensitive,
    /// Use 5-minute session cache (auth token, models, etc.)
    Cached,
}

/// Check if Touch ID is available (hardware present + interactive terminal)
pub fn is_available() -> bool;

/// Verify the user via Touch ID, respecting the tier and session cache
pub fn verify(tier: Tier, reason: &str) -> Result<()>;

/// Store refresh token in a biometric-protected Keychain item
pub fn store_secure(refresh_token: &str) -> Result<()>;

/// Load refresh token from biometric-protected item (triggers Touch ID)
pub fn load_secure() -> Result<Option<String>>;

/// Clear the biometric Keychain item and session cache
pub fn clear() -> Result<()>;
```

### `--no-touchid` flag handling

The `no_touchid` flag is set via a thread-local or `once_cell` static at CLI startup in `main()`, before any command dispatch. `is_available()` checks this static and returns `false` when the flag is set. This avoids threading the flag through every function signature.

```rust
static NO_TOUCHID: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn set_disabled(disabled: bool) {
    NO_TOUCHID.set(disabled).ok();
}

pub fn is_available() -> bool {
    if *NO_TOUCHID.get().unwrap_or(&false) {
        return false;
    }
    // ... hardware + interactive checks
}
```

### Non-macOS fallback

```rust
#[cfg(not(target_os = "macos"))]
mod fallback {
    // is_available() -> false
    // verify() -> Ok(())
    // store_secure() -> Ok(()) (stored in regular keychain instead)
    // load_secure() -> reads from regular keychain
    // clear() -> Ok(())
}
```

## Integration Points

### `credentials.rs`

- `save()`: after saving to `keyring`, call `touchid::store_secure(refresh_token)` if a refresh token is present. On success, remove the refresh token from the regular keychain item.
- `clear()`: also call `touchid::clear()`.

### `main.rs`

Add `--no-touchid` global flag:

```rust
#[derive(Parser)]
struct Cli {
    /// Skip Touch ID verification
    #[arg(long, global = true)]
    no_touchid: bool,

    #[command(subcommand)]
    command: Commands,
}
```

Gate calls at command dispatch:

- `cmd_auth_token()`: `verify(Tier::Cached, "access auth token")`
- `cmd_keys()` (create/revoke): `verify(Tier::Sensitive, "manage API keys")`
- `cmd_logout()`: `verify(Tier::Sensitive, "log out")`
- `cmd_account()`: `verify(Tier::Cached, "access account info")`
- `cmd_devices()`: `verify(Tier::Cached, "access devices")`
- `cmd_connect()`: `verify(Tier::Cached, "connect")`
- `cmd_status()`: `verify(Tier::Cached, "check status")`

### `oauth.rs`

In `ensure_valid_jwt()` / `get_token()`: when refresh is needed, use `touchid::load_secure()` instead of reading the refresh token from regular credentials.

## Migration

On first run after upgrade:

1. Load existing credentials via `keyring` (current path)
2. If Touch ID is available and a refresh token is present: store the refresh token in the biometric item, remove it from the regular item
3. If Touch ID is unavailable: no change, everything works as before

Migration is transparent and automatic. No user action required.

## Dependency Changes

```toml
# Cargo.toml
[target.'cfg(target_os = "macos")'.dependencies]
security-framework = "3"
```

The `keyring` crate stays for the non-biometric item. `security-framework` is only compiled on macOS.

## Testing Strategy (TDD)

Tests are written before implementation for each component.

### Unit tests -- `tests/touchid_test.rs`

The `security-framework` calls are wrapped behind a trait so cache and routing logic can be tested without Touch ID hardware.

**Tests to write first:**

- `is_available()` returns `false` when no biometric hardware (mocked)
- `is_available()` returns `false` when terminal is not interactive
- `is_available()` returns `false` when `--no-touchid` flag is set
- `verify(Tier::Sensitive, _)` always calls the biometric backend (never checks cache)
- `verify(Tier::Cached, _)` skips biometric when session cache is valid
- `verify(Tier::Cached, _)` prompts biometric when session cache is expired (> 5 min)
- `verify(Tier::Cached, _)` prompts biometric when session cache file is missing
- `verify(Tier::Cached, _)` prompts biometric when session cache file has wrong owner
- Session cache file is created with `0600` permissions after successful verification
- Session cache file is deleted on `clear()`
- `store_secure()` followed by `load_secure()` round-trips the refresh token (mocked backend)
- Migration: existing credentials with refresh token get split when Touch ID is available
- Migration: credentials without refresh token are unchanged
- `--no-touchid` flag causes `verify()` to return `Ok(())` without prompting

### Integration tests

- All commands work identically on Linux CI (no-op path, no new prompts)
- `--no-touchid` flag is accepted by every command
- Credential save/load round-trip with Touch ID compiled out

### Manual testing (requires macOS + enrolled fingerprint)

- Fresh login creates biometric Keychain item
- JWT expiry triggers Touch ID prompt, then silent refresh
- `keys create` always prompts Touch ID
- `auth token` within 5 min of previous verification: no prompt
- `auth token` after 5 min: Touch ID prompt
- Cancel Touch ID: command aborts with clear error message
- `logout` clears both Keychain items and session cache

## Future: Cross-Platform Biometrics

The `Tier`/`verify()` API is designed to be platform-agnostic. Future work could add:

- **Windows Hello** (`#[cfg(target_os = "windows")]`): use the Windows Hello biometric API via `windows` crate to protect credential access. Same two-item Keychain split, using Windows Credential Manager with `NCRYPT_PIN_CACHE_APPLICATION_TICKET_PROPERTY`.
- **Linux fprintd** (`#[cfg(target_os = "linux")]`): use `fprintd` D-Bus API via `zbus` crate for fingerprint verification. Session cache works the same way. Availability check includes whether `fprintd` is running and a fingerprint is enrolled.

Each platform gets its own `mod` block behind `#[cfg()]` with the same public API. The `Tier`, session cache, and `--no-touchid` flag are shared. This is out of scope for the current implementation.
