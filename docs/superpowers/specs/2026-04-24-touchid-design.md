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

Two Apple frameworks are used, each for a distinct purpose:

1. **`LocalAuthentication.framework`** (`LAContext.evaluatePolicy`) -- standalone biometric prompts for `verify()`. Used by Sensitive-tier operations (keys, logout) that need a "prove you're the user" check without accessing any Keychain item.
2. **`Security.framework`** (`SecItemAdd`/`SecItemCopyMatching` with `kSecAccessControlBiometryCurrentSet`) -- biometric-gated Keychain storage for `store_secure()`/`load_secure()`. The OS triggers Touch ID automatically when the protected item is accessed.

Both require the `security-framework-sys` crate for raw FFI calls. The high-level `security-framework` crate does not expose `SecAccessControl` on generic password items, so `store_secure()` and `load_secure()` call `SecItemAdd`/`SecItemCopyMatching` directly with `CFDictionary` containing `kSecAttrAccessControl`. The `verify()` standalone prompt uses `LAContext` via Objective-C FFI (`objc2` crate).

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
              |   macOS Frameworks  |
              | Security.framework  |
              |  (Keychain + ACL)   |
              | LocalAuthentication |
              |  (standalone prompt)|
              | via security-       |
              | framework-sys +     |
              | objc2 crates        |
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
| `magelab devices` |

`magelab status` does not require Touch ID -- it only performs a local health check and reads cached credential state without contacting the gateway.

### Session cache

A file at `~/.config/magelab/touchid-session`:

- Contents: Unix timestamp of last successful verification
- Permissions: `0600`
- Valid when: timestamp is within 5 minutes AND file owner matches current UID
- Deleted on: `magelab logout`

No daemon or IPC. Each CLI invocation reads the file and checks the timestamp. The 5-minute window is hardcoded. An optional `MAGELAB_TOUCHID_TTL` environment variable can override the default (in seconds).

**Limitation:** The session cache file is writable by any process running as the same user. A same-UID attacker (e.g., malicious script in the same shell session) could forge the file to bypass Touch ID for Cached-tier operations. This is an accepted limitation: a same-UID attacker already has access to process memory and could read credentials directly. Touch ID primarily protects against a different user, a remote attacker with the credential file, or an unauthorized process on a locked machine.

### Bypass: `--no-touchid`

A global CLI flag `magelab --no-touchid <command>` that skips all biometric checks. Falls back to current behavior (keyring without biometric gate, refresh token in regular item). For scripts, CI, and users who need to bypass temporarily.

`magelab login` does not require Touch ID since it is already browser-authenticated.

## Verification Flows

### Sensitive operation flow

Uses `LAContext.evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics)` for a standalone biometric prompt. This is independent of Keychain access.

```
Command invoked (e.g., keys create)
  -> touchid::is_available()?
    -> No:  proceed without prompt (graceful fallback)
    -> Yes: touchid::verify(Tier::Sensitive, "manage API keys")?
      -> LAContext.evaluatePolicy() triggers Touch ID
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

The `no_touchid` flag is set via an `AtomicBool` static at CLI startup in `main()`, before any command dispatch. `is_available()` checks this static and returns `false` when the flag is set. This avoids threading the flag through every function signature.

`AtomicBool` is used instead of `OnceLock<bool>` because it always succeeds on write (no "first-write-wins" semantic that could silently fail in tests or double-init scenarios).

```rust
use std::sync::atomic::{AtomicBool, Ordering};

static NO_TOUCHID: AtomicBool = AtomicBool::new(false);

pub fn set_disabled(disabled: bool) {
    NO_TOUCHID.store(disabled, Ordering::SeqCst);
}

pub fn is_available() -> bool {
    if NO_TOUCHID.load(Ordering::SeqCst) {
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

- `save()`: after saving to `keyring`, call `touchid::store_secure(refresh_token)` if a refresh token is present. Verify the biometric item can be stored successfully before removing the refresh token from the regular keychain item. If `store_secure()` fails, keep the refresh token in the regular item and log a warning.
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

`cmd_status()` does not require Touch ID verification.

### `oauth.rs`

In `ensure_valid_jwt()` / `get_token()`: when refresh is needed, use `touchid::load_secure()` instead of reading the refresh token from regular credentials.

## Migration

On first run after upgrade:

1. Load existing credentials via `keyring` (current path)
2. If Touch ID is available and a refresh token is present:
   a. Store the refresh token in the biometric Keychain item via `store_secure()`
   b. If store succeeds, remove the refresh token from the regular keychain item
   c. If store fails, keep the refresh token in the regular item and log a warning -- the CLI continues to work without biometric-gated refresh
3. If Touch ID is unavailable: no change, everything works as before

Migration is transparent and automatic. No user action required. No credential data is lost even if the biometric store fails.

## Dependency Changes

```toml
# Cargo.toml
[target.'cfg(target_os = "macos")'.dependencies]
security-framework-sys = "3"  # Raw FFI for SecItemAdd/SecItemCopyMatching with ACL
objc2 = "0.6"                 # LAContext for standalone biometric prompts
objc2-local-authentication = "0.4"  # LocalAuthentication.framework bindings
objc2-foundation = "0.3"      # NSError, NSString
```

The `keyring` crate stays for the non-biometric item. The macOS-only dependencies are only compiled on macOS. The `objc2` family provides safe-ish Objective-C interop for `LAContext` without manual FFI.

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
- `load_secure()` returns `None` when biometric enrollment has changed (fingerprints removed after setup) -- refresh flow falls back to browser login
- `store_secure()` failure does not remove refresh token from regular keychain (no data loss)
- `verify()` returns appropriate error when Touch ID is locked out after too many failures

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
- Biometric enrollment changed after storing refresh token: graceful fallback to browser login
- Touch ID locked out (too many failures): clear error message with recovery instructions

## Error Handling

| Scenario | Behavior | User-facing message |
|----------|----------|---------------------|
| Touch ID cancelled by user | Abort command | "Touch ID verification cancelled." |
| Touch ID locked out (too many failures) | Abort command | "Touch ID is locked. Use your device passcode to unlock, then try again." |
| Biometric enrollment changed (fingerprints removed/re-enrolled) | `load_secure()` returns `None`, fall back to browser login | "Touch ID enrollment changed. Please log in again: magelab login" |
| Biometric Keychain item corrupted | `load_secure()` returns `None`, fall back to browser login | (silent fallback, no user-facing error) |
| `store_secure()` fails during save | Keep refresh token in regular keychain, log warning | "Warning: Could not store credentials in biometric keychain. Touch ID refresh will not be available." |
| No Touch ID hardware | All biometric functions no-op | (no message, silent fallback) |
| Non-interactive terminal (piped stdin) | `is_available()` returns `false` | (no message, silent fallback) |

## Limitations

1. **Session cache same-UID trust boundary.** The session cache file can be forged by any process running as the same user. This is an accepted limitation -- a same-UID attacker already has access to process memory and the regular keychain. Touch ID primarily protects against different users, remote attackers with the credential file, or unauthorized access on a locked machine.

2. **Web login does not return a refresh token.** The default login method (`magelab login` / `login_web()`) currently returns credentials with `refresh_token: None`. This means use case #3 (biometric-gated token refresh) only works for users who logged in via Google OAuth or Magic Auth. Web login users still get use cases #1 (credential access protection) and #2 (sensitive operation gating), but will fall back to browser re-login when the JWT expires. To enable use case #3 for Web login, the web app backend would need to issue refresh tokens during the CLI code exchange flow -- this is a backend change outside the scope of this spec.

3. **`kSecAccessControlBiometryCurrentSet` invalidation.** If a user removes all enrolled fingerprints or adds new ones after the biometric Keychain item was created, the item becomes inaccessible (`errSecAuthFailed`). The CLI handles this gracefully by falling back to browser login, but the user must re-authenticate.

## Future: Cross-Platform Biometrics

The `Tier`/`verify()` API is designed to be platform-agnostic. Future work could add:

- **Windows Hello** (`#[cfg(target_os = "windows")]`): use the Windows Hello biometric API via `windows` crate to protect credential access. Same two-item Keychain split, using Windows Credential Manager with `NCRYPT_PIN_CACHE_APPLICATION_TICKET_PROPERTY`.
- **Linux fprintd** (`#[cfg(target_os = "linux")]`): use `fprintd` D-Bus API via `zbus` crate for fingerprint verification. Session cache works the same way. Availability check includes whether `fprintd` is running and a fingerprint is enrolled.

The `robius-authentication` crate is a potential unifying option that wraps macOS Touch ID, Windows Hello, and Linux polkit behind a single API. Worth evaluating when cross-platform biometrics become a priority.

Each platform gets its own `mod` block behind `#[cfg()]` with the same public API. The `Tier`, session cache, and `--no-touchid` flag are shared. This is out of scope for the current implementation.
