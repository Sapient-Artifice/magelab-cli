# Touch ID Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add macOS Touch ID support to the magelab CLI for biometric-gated credential access, sensitive operation verification, and token refresh.

**Architecture:** New `src/auth/touchid.rs` module with platform-gated implementations. macOS uses `security-framework-sys` for biometric Keychain items and `objc2-local-authentication` for standalone Touch ID prompts. Non-macOS compiles to no-ops. All biometric logic is behind a `BiometricBackend` trait for testability.

**Tech Stack:** Rust, `security-framework-sys` 2.x, `objc2` 0.6, `objc2-local-authentication` 0.3, `objc2-foundation` 0.3, `core-foundation` 0.10

**Spec:** `docs/superpowers/specs/2026-04-24-touchid-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `Cargo.toml` | Modify | Add macOS-only dependencies |
| `src/auth/mod.rs` | Modify | Add `pub mod touchid;` |
| `src/auth/touchid.rs` | Create | Platform-gated module root — re-exports from platform impl |
| `src/auth/touchid/mod.rs` | Create | `Tier` enum, `BiometricBackend` trait, session cache, public API, `AtomicBool` flag |
| `src/auth/touchid/macos.rs` | Create | macOS `BiometricBackend` impl — LAContext + SecItem FFI |
| `src/auth/touchid/fallback.rs` | Create | Non-macOS no-op `BiometricBackend` impl |
| `src/auth/credentials.rs` | Modify | Hook `store_secure()`/`clear()` into save/clear |
| `src/main.rs` | Modify | Add `--no-touchid` flag, gate commands with `verify()` |
| `src/auth/oauth.rs` | Modify | Use `load_secure()` in token refresh path |
| `tests/touchid_test.rs` | Create | Unit tests for session cache, tier routing, flag |
| `tests/touchid_integration_test.rs` | Create | Integration tests for `--no-touchid` flag acceptance |

Note: `src/auth/touchid.rs` is replaced by a `src/auth/touchid/` directory module. The `mod.rs` inside it contains the shared logic (trait, cache, public API), while `macos.rs` and `fallback.rs` contain platform-specific implementations.

---

### Task 1: Add Dependencies

**Files:**
- Modify: `Cargo.toml:16-37`

- [ ] **Step 1: Add macOS-only dependencies to Cargo.toml**

Add the following section after the existing `[dependencies]` block and before `[dev-dependencies]`:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
security-framework-sys = "2"
core-foundation = "0.10"
core-foundation-sys = "0.8"
objc2 = "0.6"
objc2-local-authentication = "0.3"
objc2-foundation = "0.3"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles successfully with no errors.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: add macOS Touch ID dependencies (security-framework-sys, objc2)"
```

---

### Task 2: Create Touchid Module Skeleton with BiometricBackend Trait and Fallback

**Files:**
- Modify: `src/auth/mod.rs`
- Create: `src/auth/touchid/mod.rs`
- Create: `src/auth/touchid/fallback.rs`
- Test: `tests/touchid_test.rs`

- [ ] **Step 1: Write failing tests for the core API**

Create `tests/touchid_test.rs`:

```rust
use magelab_cli::auth::touchid::{self, Tier};

#[test]
fn is_available_returns_false_when_disabled() {
    touchid::set_disabled(true);
    assert!(!touchid::is_available());
    // Reset for other tests
    touchid::set_disabled(false);
}

#[test]
fn set_disabled_can_be_toggled() {
    touchid::set_disabled(true);
    assert!(!touchid::is_available());
    touchid::set_disabled(false);
    // On non-macOS CI, is_available() is still false (no hardware)
    // but the flag itself was toggled successfully
}

#[test]
fn verify_returns_ok_when_not_available() {
    // When Touch ID is not available (no hardware or disabled),
    // verify() should return Ok(()) — graceful fallback
    touchid::set_disabled(true);
    let result = touchid::verify(Tier::Sensitive, "test");
    assert!(result.is_ok());
    let result = touchid::verify(Tier::Cached, "test");
    assert!(result.is_ok());
    touchid::set_disabled(false);
}

#[test]
fn clear_returns_ok_when_not_available() {
    touchid::set_disabled(true);
    let result = touchid::clear();
    assert!(result.is_ok());
    touchid::set_disabled(false);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test touchid_test`
Expected: FAIL — `auth::touchid` module does not exist.

- [ ] **Step 3: Create the touchid module directory**

Convert `src/auth/mod.rs` from:

```rust
pub mod credentials;
pub mod oauth;
```

to:

```rust
pub mod credentials;
pub mod oauth;
pub mod touchid;
```

Create `src/auth/touchid/mod.rs`:

```rust
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod fallback;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(target_os = "macos"))]
use fallback as platform;

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};

/// Operation sensitivity tier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Always prompt Touch ID (keys, logout)
    Sensitive,
    /// Use 5-minute session cache (auth token, models, etc.)
    Cached,
}

static NO_TOUCHID: AtomicBool = AtomicBool::new(false);

/// Set the global disable flag (called from CLI --no-touchid)
pub fn set_disabled(disabled: bool) {
    NO_TOUCHID.store(disabled, Ordering::SeqCst);
}

/// Check if Touch ID is available (hardware + interactive terminal + not disabled)
pub fn is_available() -> bool {
    if NO_TOUCHID.load(Ordering::SeqCst) {
        return false;
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return false;
    }
    platform::is_hardware_available()
}

/// Verify the user via Touch ID, respecting tier and session cache.
/// Returns Ok(()) if Touch ID is not available (graceful fallback).
pub fn verify(tier: Tier, reason: &str) -> Result<()> {
    if !is_available() {
        return Ok(());
    }
    match tier {
        Tier::Sensitive => platform::prompt_biometric(reason),
        Tier::Cached => {
            if session_cache::is_valid() {
                return Ok(());
            }
            platform::prompt_biometric(reason)?;
            session_cache::touch()?;
            Ok(())
        }
    }
}

/// Store refresh token in a biometric-protected Keychain item.
/// No-op on non-macOS or when Touch ID is unavailable.
pub fn store_secure(refresh_token: &str) -> Result<()> {
    if !is_available() {
        return Ok(());
    }
    platform::store_biometric_item(refresh_token)
}

/// Load refresh token from biometric-protected Keychain item.
/// Returns None on non-macOS, when unavailable, or on biometric failure.
pub fn load_secure() -> Result<Option<String>> {
    if !is_available() {
        return Ok(None);
    }
    platform::load_biometric_item()
}

/// Clear biometric Keychain item and session cache.
pub fn clear() -> Result<()> {
    session_cache::delete();
    platform::delete_biometric_item()
}

mod session_cache;
```

Create `src/auth/touchid/fallback.rs`:

```rust
use anyhow::Result;

pub fn is_hardware_available() -> bool {
    false
}

pub fn prompt_biometric(_reason: &str) -> Result<()> {
    Ok(())
}

pub fn store_biometric_item(_token: &str) -> Result<()> {
    Ok(())
}

pub fn load_biometric_item() -> Result<Option<String>> {
    Ok(None)
}

pub fn delete_biometric_item() -> Result<()> {
    Ok(())
}
```

- [ ] **Step 4: Create session_cache stub**

Create `src/auth/touchid/session_cache.rs`:

```rust
pub fn is_valid() -> bool {
    false
}

pub fn touch() -> anyhow::Result<()> {
    Ok(())
}

pub fn delete() {
    // no-op stub
}
```

- [ ] **Step 5: Make the module public in lib.rs or adjust crate visibility**

The tests use `magelab_cli::auth::touchid`. Check if there's a `src/lib.rs`. If not, the binary crate needs the test to use internal module paths. Since this is a binary crate, add a `src/lib.rs` that re-exports the auth module:

Create `src/lib.rs`:

```rust
pub mod auth;
```

Then in `src/main.rs`, change:

```rust
mod auth;
```

to:

```rust
use magelab_cli::auth;
```

**Important:** Also update the other `mod` declarations in `main.rs`. The modules that are only used internally by the binary (`connect`, `detect`, `account`, `config`, `settings`, `ui`, `client`) stay as `mod` declarations in `main.rs`. Only `auth` moves to `lib.rs` since tests need access to it.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --test touchid_test`
Expected: All 4 tests PASS.

Run: `cargo test`
Expected: All existing tests still pass.

- [ ] **Step 7: Commit**

```bash
git add src/auth/touchid/ src/auth/mod.rs src/lib.rs src/main.rs tests/touchid_test.rs
git commit -m "feat(touchid): add module skeleton with BiometricBackend trait and fallback"
```

---

### Task 3: Implement Session Cache

**Files:**
- Modify: `src/auth/touchid/session_cache.rs`
- Modify: `tests/touchid_test.rs`

- [ ] **Step 1: Write failing tests for session cache**

Append to `tests/touchid_test.rs`:

```rust
mod session_cache_tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;

    // We test session cache logic directly by calling the internal functions
    // with a custom cache directory. To do this, we use the _with_dir variants.

    use magelab_cli::auth::touchid::session_cache;

    #[test]
    fn cache_is_invalid_when_file_missing() {
        let dir = TempDir::new().unwrap();
        assert!(!session_cache::is_valid_in(dir.path()));
    }

    #[test]
    fn cache_is_valid_after_touch() {
        let dir = TempDir::new().unwrap();
        session_cache::touch_in(dir.path()).unwrap();
        assert!(session_cache::is_valid_in(dir.path()));
    }

    #[test]
    fn cache_is_invalid_after_expiry() {
        let dir = TempDir::new().unwrap();
        let cache_path = dir.path().join("touchid-session");
        // Write a timestamp 6 minutes in the past
        let old_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 360;
        fs::write(&cache_path, old_ts.to_string()).unwrap();
        assert!(!session_cache::is_valid_in(dir.path()));
    }

    #[test]
    fn cache_is_invalid_with_garbage_content() {
        let dir = TempDir::new().unwrap();
        let cache_path = dir.path().join("touchid-session");
        fs::write(&cache_path, "not-a-number").unwrap();
        assert!(!session_cache::is_valid_in(dir.path()));
    }

    #[test]
    fn cache_is_deleted_by_delete() {
        let dir = TempDir::new().unwrap();
        session_cache::touch_in(dir.path()).unwrap();
        assert!(dir.path().join("touchid-session").exists());
        session_cache::delete_in(dir.path());
        assert!(!dir.path().join("touchid-session").exists());
    }

    #[test]
    fn cache_file_has_restrictive_permissions() {
        let dir = TempDir::new().unwrap();
        session_cache::touch_in(dir.path()).unwrap();
        let cache_path = dir.path().join("touchid-session");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::metadata(&cache_path).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn cache_respects_custom_ttl_env() {
        let dir = TempDir::new().unwrap();
        let cache_path = dir.path().join("touchid-session");
        // Write a timestamp 2 seconds in the past
        let recent_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 2;
        fs::write(&cache_path, recent_ts.to_string()).unwrap();

        // With default TTL (300s), this should be valid
        assert!(session_cache::is_valid_in(dir.path()));

        // With TTL of 1 second, this should be invalid
        std::env::set_var("MAGELAB_TOUCHID_TTL", "1");
        assert!(!session_cache::is_valid_in(dir.path()));
        std::env::remove_var("MAGELAB_TOUCHID_TTL");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test touchid_test session_cache`
Expected: FAIL — `session_cache` functions with `_in` suffix don't exist yet.

- [ ] **Step 3: Implement session cache**

Replace `src/auth/touchid/session_cache.rs` with:

```rust
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_FILENAME: &str = "touchid-session";
const DEFAULT_TTL_SECS: u64 = 300; // 5 minutes

fn ttl_secs() -> u64 {
    std::env::var("MAGELAB_TOUCHID_TTL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TTL_SECS)
}

fn default_cache_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("magelab"))
}

/// Check if session cache is valid (default directory)
pub fn is_valid() -> bool {
    match default_cache_dir() {
        Some(dir) => is_valid_in(&dir),
        None => false,
    }
}

/// Check if session cache is valid in a given directory
pub fn is_valid_in(dir: &Path) -> bool {
    let path = dir.join(CACHE_FILENAME);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let timestamp: u64 = match content.trim().parse() {
        Ok(t) => t,
        Err(_) => return false,
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    now.saturating_sub(timestamp) < ttl_secs()
}

/// Write current timestamp to session cache (default directory)
pub fn touch() -> Result<()> {
    match default_cache_dir() {
        Some(dir) => touch_in(&dir),
        None => Ok(()),
    }
}

/// Write current timestamp to session cache in a given directory
pub fn touch_in(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(CACHE_FILENAME);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("Failed to get system time")?
        .as_secs();
    std::fs::write(&path, now.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Delete session cache file (default directory)
pub fn delete() {
    if let Some(dir) = default_cache_dir() {
        delete_in(&dir);
    }
}

/// Delete session cache file in a given directory
pub fn delete_in(dir: &Path) {
    let path = dir.join(CACHE_FILENAME);
    std::fs::remove_file(&path).ok();
}
```

- [ ] **Step 4: Make session_cache module public**

In `src/auth/touchid/mod.rs`, change:

```rust
mod session_cache;
```

to:

```rust
pub mod session_cache;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test touchid_test session_cache`
Expected: All 7 session cache tests PASS.

Run: `cargo test`
Expected: All tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/auth/touchid/session_cache.rs src/auth/touchid/mod.rs tests/touchid_test.rs
git commit -m "feat(touchid): implement session cache with TTL and file permissions"
```

---

### Task 4: Implement macOS Biometric Backend

**Files:**
- Create: `src/auth/touchid/macos.rs`

This task implements the actual macOS Touch ID integration using `LAContext` for standalone prompts and `SecItemAdd`/`SecItemCopyMatching` for biometric-gated Keychain items.

**Note:** This code only compiles on macOS (`#[cfg(target_os = "macos")]`). CI on Linux skips this file entirely.

- [ ] **Step 1: Implement the macOS backend**

Create `src/auth/touchid/macos.rs`:

```rust
use anyhow::{Context, Result};
use core_foundation::base::{CFType, TCFType, kCFAllocatorDefault};
use core_foundation::boolean::CFBoolean;
use core_foundation::data::CFData;
use core_foundation::dictionary::CFMutableDictionary;
use core_foundation::string::CFString;
use security_framework_sys::access_control::*;
use security_framework_sys::base::{errSecItemNotFound, errSecSuccess, errSecAuthFailed};
use security_framework_sys::item::*;
use security_framework_sys::keychain_item::*;
use std::ptr;

const KEYCHAIN_SERVICE: &str = "magelab-cli";
const KEYCHAIN_ACCOUNT: &str = "refresh-bio";

/// Check if Touch ID hardware is available via LAContext
pub fn is_hardware_available() -> bool {
    use objc2::rc::Retained;
    use objc2_local_authentication::LAContext;

    let context = unsafe { LAContext::new() };
    let mut error: *mut objc2_foundation::NSError = ptr::null_mut();
    let available = unsafe {
        context.canEvaluatePolicy_error_(
            objc2_local_authentication::LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
            &mut error,
        )
    };
    available
}

/// Prompt the user with a standalone Touch ID dialog via LAContext
pub fn prompt_biometric(reason: &str) -> Result<()> {
    use objc2::rc::Retained;
    use objc2_foundation::NSString;
    use objc2_local_authentication::{LAContext, LAPolicy};

    let context = unsafe { LAContext::new() };
    let reason_ns = NSString::from_str(reason);

    // evaluatePolicy is async with a callback — we block on it with a channel
    let (tx, rx) = std::sync::mpsc::channel();

    let block = block2::ConcreteBlock::new(move |success: bool, error: *mut objc2_foundation::NSError| {
        if success {
            tx.send(Ok(())).ok();
        } else {
            let msg = if error.is_null() {
                "Touch ID verification failed.".to_string()
            } else {
                let err = unsafe { &*error };
                let code = unsafe { objc2_foundation::NSError::code(err) };
                match code {
                    -2 => "Touch ID verification cancelled.".to_string(),
                    -8 => "Touch ID is locked. Use your device passcode to unlock, then try again.".to_string(),
                    _ => format!("Touch ID verification failed (code {}).", code),
                }
            };
            tx.send(Err(anyhow::anyhow!("{}", msg))).ok();
        }
    });

    unsafe {
        context.evaluatePolicy_localizedReason_reply_(
            LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
            &reason_ns,
            &block,
        );
    }

    rx.recv().context("Touch ID callback not received")?
}

/// Store a refresh token in a biometric-protected Keychain item
pub fn store_biometric_item(token: &str) -> Result<()> {
    // Delete any existing item first (ignore errors)
    delete_biometric_item().ok();

    unsafe {
        // Create access control with biometry
        let mut error: *mut core_foundation_sys::error::CFErrorRef = ptr::null_mut();
        let access_control = SecAccessControlCreateWithFlags(
            kCFAllocatorDefault,
            kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly,
            kSecAccessControlBiometryCurrentSet,
            &mut error as *mut _ as *mut _,
        );

        if access_control.is_null() {
            anyhow::bail!("Failed to create access control for biometric Keychain item");
        }

        let mut query = CFMutableDictionary::new();
        query.add(&CFString::new(kSecClass as &str), &CFString::new(kSecClassGenericPassword as &str));
        query.add(&CFString::new(kSecAttrService as &str), &CFString::from_static_string(KEYCHAIN_SERVICE));
        query.add(&CFString::new(kSecAttrAccount as &str), &CFString::from_static_string(KEYCHAIN_ACCOUNT));
        query.add(&CFString::new(kSecValueData as &str), &CFData::from_buffer(token.as_bytes()));
        query.add(&CFString::new(kSecAttrAccessControl as &str), &CFType::wrap_under_create_rule(access_control));
        query.add(&CFString::new(kSecUseAuthenticationUI as &str), &CFString::new(kSecUseAuthenticationUIAllow as &str));

        let status = SecItemAdd(query.as_concrete_TypeRef() as _, ptr::null_mut());

        if status != errSecSuccess {
            anyhow::bail!("Failed to store biometric Keychain item (OSStatus {})", status);
        }
    }

    Ok(())
}

/// Load refresh token from biometric-protected Keychain item (triggers Touch ID)
pub fn load_biometric_item() -> Result<Option<String>> {
    unsafe {
        let mut query = CFMutableDictionary::new();
        query.add(&CFString::new(kSecClass as &str), &CFString::new(kSecClassGenericPassword as &str));
        query.add(&CFString::new(kSecAttrService as &str), &CFString::from_static_string(KEYCHAIN_SERVICE));
        query.add(&CFString::new(kSecAttrAccount as &str), &CFString::from_static_string(KEYCHAIN_ACCOUNT));
        query.add(&CFString::new(kSecReturnData as &str), &CFBoolean::true_value());
        query.add(&CFString::new(kSecMatchLimit as &str), &CFString::new(kSecMatchLimitOne as &str));

        let prompt = CFString::from_static_string("Authenticate to access MageLab credentials");
        query.add(&CFString::new(kSecUseOperationPrompt as &str), &prompt);

        let mut result: core_foundation_sys::base::CFTypeRef = ptr::null_mut();
        let status = SecItemCopyMatching(
            query.as_concrete_TypeRef() as _,
            &mut result,
        );

        match status {
            s if s == errSecSuccess => {
                if result.is_null() {
                    return Ok(None);
                }
                let data = CFData::wrap_under_create_rule(result as _);
                let token = String::from_utf8(data.bytes().to_vec())
                    .context("Biometric Keychain item is not valid UTF-8")?;
                Ok(Some(token))
            }
            s if s == errSecItemNotFound => Ok(None),
            s if s == errSecAuthFailed => {
                // Biometric enrollment changed or auth failed
                eprintln!("Touch ID enrollment changed. Please log in again: magelab login");
                Ok(None)
            }
            _ => {
                anyhow::bail!("Failed to read biometric Keychain item (OSStatus {})", status);
            }
        }
    }
}

/// Delete biometric Keychain item
pub fn delete_biometric_item() -> Result<()> {
    unsafe {
        let mut query = CFMutableDictionary::new();
        query.add(&CFString::new(kSecClass as &str), &CFString::new(kSecClassGenericPassword as &str));
        query.add(&CFString::new(kSecAttrService as &str), &CFString::from_static_string(KEYCHAIN_SERVICE));
        query.add(&CFString::new(kSecAttrAccount as &str), &CFString::from_static_string(KEYCHAIN_ACCOUNT));

        let status = SecItemDelete(query.as_concrete_TypeRef() as _);

        if status != errSecSuccess && status != errSecItemNotFound {
            anyhow::bail!("Failed to delete biometric Keychain item (OSStatus {})", status);
        }
    }

    Ok(())
}
```

**Important:** The exact `CFString` key construction above uses symbolic constants from `security-framework-sys`. The actual API may require passing raw `CFStringRef` values rather than constructing new `CFString`s from the constant names. During implementation, you may need to adjust to use the actual `CFStringRef` statics exported by the crate (e.g., `kSecClass` is a `CFStringRef`, not a `&str`). The intent and structure above is correct — the exact FFI bridging may need adjustment during compilation.

- [ ] **Step 2: Verify it compiles on macOS**

Run: `cargo check`
Expected: Compiles successfully. (On Linux CI, the file is skipped via `#[cfg]`.)

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: All existing tests plus touchid tests pass. The macOS backend is not directly unit-tested (requires hardware), but the fallback path runs in tests via the `set_disabled(true)` flag.

- [ ] **Step 4: Commit**

```bash
git add src/auth/touchid/macos.rs
git commit -m "feat(touchid): implement macOS biometric backend (LAContext + SecItem)"
```

---

### Task 5: Integrate Touch ID into Credential Save/Clear

**Files:**
- Modify: `src/auth/credentials.rs:55-96`
- Test: `tests/touchid_test.rs`

- [ ] **Step 1: Write failing test for credential save integration**

Append to `tests/touchid_test.rs`:

```rust
mod credential_integration_tests {
    use magelab_cli::auth::credentials::Credentials;
    use magelab_cli::auth::touchid;

    #[test]
    fn save_with_touchid_disabled_preserves_refresh_token_in_regular_store() {
        // When Touch ID is disabled, save() should keep the refresh token
        // in the regular keychain (existing behavior, no biometric split)
        touchid::set_disabled(true);

        let creds = Credentials {
            access_token: Some("test-access".to_string()),
            refresh_token: Some("test-refresh".to_string()),
            expires_at: Some(9999999999),
            user_id: Some("user-1".to_string()),
            email: Some("test@example.com".to_string()),
        };

        // Save should succeed
        // Note: this writes to the real keychain/file — acceptable in tests
        // because we clean up after
        let result = creds.save();
        assert!(result.is_ok());

        // Reload and verify refresh token is still there
        let loaded = Credentials::load().unwrap();
        assert_eq!(loaded.refresh_token.as_deref(), Some("test-refresh"));

        // Clean up
        Credentials::clear().ok();
        touchid::set_disabled(false);
    }
}
```

- [ ] **Step 2: Run test to verify it passes (baseline)**

Run: `cargo test --test touchid_test credential_integration`
Expected: PASS — this test verifies current behavior before we modify `save()`.

- [ ] **Step 3: Modify credentials.rs to hook Touch ID**

In `src/auth/credentials.rs`, add the import at the top:

```rust
use super::touchid;
```

Modify the `save()` method to call `store_secure()` after successful keychain save:

```rust
    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string(self)?;

        // Try keychain
        if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            if entry.set_password(&json).is_ok() {
                // If Touch ID is available and we have a refresh token,
                // store it in the biometric-protected keychain item
                if let Some(ref rt) = self.refresh_token {
                    if touchid::is_available() {
                        if let Err(e) = touchid::store_secure(rt) {
                            eprintln!("Warning: Could not store credentials in biometric keychain. Touch ID refresh will not be available. ({})", e);
                        }
                        // Note: we do NOT remove refresh_token from the regular item here.
                        // The regular item keeps the full credential set as a fallback.
                        // The biometric item is an additional layer, not a replacement.
                    }
                }
                return Ok(());
            }
        }

        // File fallback
        eprintln!("Warning: No system keychain available — storing tokens in credentials file");
        let path = Self::path()?;
        let dir = path.parent().context("Invalid credentials path")?;
        std::fs::create_dir_all(dir)?;
        std::fs::write(&path, &json)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }
```

Modify the `clear()` method to also clear Touch ID state:

```rust
    pub fn clear() -> Result<()> {
        // Clear Touch ID biometric item and session cache
        touchid::clear()?;

        // Try keychain
        if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            entry.delete_credential().ok();
        }

        // File
        let path = Self::path()?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
```

- [ ] **Step 4: Run tests to verify everything passes**

Run: `cargo test`
Expected: All tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/auth/credentials.rs tests/touchid_test.rs
git commit -m "feat(touchid): integrate biometric store into credential save/clear"
```

---

### Task 6: Add --no-touchid Flag and Gate Commands in main.rs

**Files:**
- Modify: `src/main.rs:17-26` (Cli struct)
- Modify: `src/main.rs:138-178` (main function)
- Test: `tests/touchid_integration_test.rs`

- [ ] **Step 1: Write failing integration test**

Create `tests/touchid_integration_test.rs`:

```rust
use assert_cmd::Command;

#[test]
fn no_touchid_flag_accepted_by_version() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "version"])
        .assert()
        .success()
        .stdout(predicates::str::contains("magelab"));
}

#[test]
fn no_touchid_flag_accepted_by_status() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "status"])
        .assert()
        .success();
}

#[test]
fn no_touchid_flag_accepted_by_config() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "config"])
        .assert()
        .success();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test touchid_integration_test`
Expected: FAIL — `--no-touchid` is not a recognized flag.

- [ ] **Step 3: Add --no-touchid flag to CLI struct**

In `src/main.rs`, modify the `Cli` struct:

```rust
#[derive(Parser)]
#[command(
    name = "magelab",
    version,
    about = "MageLab CLI — infrastructure management for MageLab"
)]
struct Cli {
    /// Skip Touch ID verification
    #[arg(long, global = true)]
    no_touchid: bool,

    #[command(subcommand)]
    command: Commands,
}
```

- [ ] **Step 4: Set the disable flag and gate commands in main()**

In the `main()` function, after parsing CLI args and before the match:

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::load().unwrap_or_default();

    // Set Touch ID disable flag before any command dispatch
    auth::touchid::set_disabled(cli.no_touchid);

    match cli.command {
```

Then add `verify()` calls at the top of gated command handlers. Modify the match arms:

For `Commands::Auth`:
```rust
        Commands::Auth { action } => match action {
            AuthAction::Token => {
                auth::touchid::verify(auth::touchid::Tier::Cached, "access auth token")?;
                cmd_auth_token(&config).await
            }
        },
```

For `Commands::Logout`:
```rust
        Commands::Logout => {
            auth::touchid::verify(auth::touchid::Tier::Sensitive, "log out")?;
            cmd_logout()
        }
```

For `Commands::Connect`:
```rust
        Commands::Connect { json, no_launch, local, relay, remote } => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "connect")?;
            cmd_connect(&config, json, no_launch, local, relay, remote).await
        }
```

For `Commands::Devices`:
```rust
        Commands::Devices { action, json } => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "access devices")?;
            cmd_devices(&config, action, json).await
        }
```

For `Commands::Models`, `Commands::Usage`, `Commands::Balance`:
```rust
        Commands::Models => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "access account info")?;
            cmd_account(&config, "models").await
        }
        Commands::Usage => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "access account info")?;
            cmd_account(&config, "usage").await
        }
        Commands::Balance => {
            auth::touchid::verify(auth::touchid::Tier::Cached, "access account info")?;
            cmd_account(&config, "balance").await
        }
```

For `Commands::Keys` — only gate create/revoke, not list:
```rust
        Commands::Keys { action } => {
            match &action {
                KeysAction::Create | KeysAction::Revoke { .. } => {
                    auth::touchid::verify(auth::touchid::Tier::Sensitive, "manage API keys")?;
                }
                KeysAction::List => {
                    auth::touchid::verify(auth::touchid::Tier::Cached, "access API keys")?;
                }
            }
            cmd_keys(&config, action).await
        }
```

`Commands::Status`, `Commands::Login`, `Commands::Config`, `Commands::Version`, `Commands::Completions`, `Commands::SetupPi` — no Touch ID gate (unchanged).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test touchid_integration_test`
Expected: All 3 tests PASS.

Run: `cargo test`
Expected: All tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/touchid_integration_test.rs
git commit -m "feat(touchid): add --no-touchid flag and gate commands with verify()"
```

---

### Task 7: Integrate Touch ID into Token Refresh Path

**Files:**
- Modify: `src/main.rs` (the `get_token` function, lines ~440-458)
- Modify: `src/auth/oauth.rs` (the `ensure_valid_jwt` function, lines ~539-560)

- [ ] **Step 1: Write a test for the refresh path behavior**

Append to `tests/touchid_test.rs`:

```rust
mod refresh_path_tests {
    use magelab_cli::auth::touchid;

    #[test]
    fn load_secure_returns_none_when_disabled() {
        touchid::set_disabled(true);
        let result = touchid::load_secure().unwrap();
        assert!(result.is_none());
        touchid::set_disabled(false);
    }

    #[test]
    fn store_secure_is_noop_when_disabled() {
        touchid::set_disabled(true);
        let result = touchid::store_secure("test-token");
        assert!(result.is_ok());
        touchid::set_disabled(false);
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test touchid_test refresh_path`
Expected: PASS — these test the no-op fallback path.

- [ ] **Step 3: Modify get_token() in main.rs to use biometric refresh token**

In `src/main.rs`, modify the `get_token` function:

```rust
/// Get the best available token (JWT preferred, API key fallback)
async fn get_token(config: &Config) -> Result<String> {
    let creds = auth::credentials::Credentials::load().unwrap_or_default();
    if let Some(token) = &creds.access_token {
        if creds.is_token_valid() {
            return Ok(token.clone());
        }

        // Try biometric-protected refresh token first
        if let Ok(Some(bio_refresh)) = auth::touchid::load_secure() {
            if let Ok(new_creds) = auth::oauth::refresh_token(&config.gateway_url, &bio_refresh).await {
                let _ = new_creds.save();
                if let Some(t) = new_creds.access_token {
                    return Ok(t);
                }
            }
        }

        // Fall back to regular refresh token
        if let Some(refresh) = &creds.refresh_token {
            if let Ok(new_creds) = auth::oauth::refresh_token(&config.gateway_url, refresh).await {
                let _ = new_creds.save();
                if let Some(t) = new_creds.access_token {
                    return Ok(t);
                }
            }
        }
    }
    config
        .api_key()
        .ok_or_else(|| anyhow::anyhow!("Not authenticated. Run: magelab login"))
}
```

- [ ] **Step 4: Modify ensure_valid_jwt() in oauth.rs similarly**

In `src/auth/oauth.rs`, modify `ensure_valid_jwt`:

```rust
pub async fn ensure_valid_jwt(gateway_url: &str) -> Result<String> {
    let creds = Credentials::load()?;

    if creds.is_token_valid() {
        return creds
            .access_token
            .ok_or_else(|| anyhow::anyhow!("Token marked valid but access_token is missing"));
    }

    // Try biometric-protected refresh token first
    if let Ok(Some(bio_refresh)) = super::touchid::load_secure() {
        if let Ok(new_creds) = refresh_token(gateway_url, &bio_refresh).await {
            return new_creds
                .access_token
                .ok_or_else(|| anyhow::anyhow!("Refresh succeeded but no access_token returned"));
        }
    }

    // Fall back to regular refresh token
    if let Some(ref rt) = creds.refresh_token {
        if let Ok(new_creds) = refresh_token(gateway_url, rt).await {
            return new_creds
                .access_token
                .ok_or_else(|| anyhow::anyhow!("Refresh succeeded but no access_token returned"));
        }
    }

    let new_creds = login(gateway_url).await?;
    new_creds
        .access_token
        .ok_or_else(|| anyhow::anyhow!("Login succeeded but no access_token returned"))
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: All tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/auth/oauth.rs tests/touchid_test.rs
git commit -m "feat(touchid): use biometric-protected refresh token in token refresh path"
```

---

### Task 8: Add --no-touchid Flag to Existing Integration Tests

**Files:**
- Modify: `tests/touchid_integration_test.rs`

- [ ] **Step 1: Add tests verifying --no-touchid works with all gated commands**

Append to `tests/touchid_integration_test.rs`:

```rust
#[test]
fn no_touchid_flag_accepted_by_login_status() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "login", "--status"])
        .assert()
        .success();
}

#[test]
fn no_touchid_flag_accepted_by_completions() {
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "completions", "bash"])
        .assert()
        .success();
}

// Test that --no-touchid is a global flag (works before any subcommand)
#[test]
fn no_touchid_flag_is_global() {
    // Should be accepted even for commands that don't gate on Touch ID
    Command::cargo_bin("magelab")
        .unwrap()
        .args(["--no-touchid", "version"])
        .assert()
        .success();
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test touchid_integration_test`
Expected: All tests PASS.

- [ ] **Step 3: Run full test suite and clippy**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: All tests PASS, no clippy warnings.

- [ ] **Step 4: Commit**

```bash
git add tests/touchid_integration_test.rs
git commit -m "test(touchid): add integration tests for --no-touchid flag acceptance"
```

---

### Task 9: Final Verification and Cleanup

**Files:**
- All modified files

- [ ] **Step 1: Run full quality gates**

```bash
cargo check
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Expected: All pass with no errors or warnings.

- [ ] **Step 2: Fix any formatting issues**

Run: `cargo fmt`

- [ ] **Step 3: Verify the crate builds as a binary**

Run: `cargo build --release`
Expected: Builds successfully.

- [ ] **Step 4: Verify --no-touchid flag works end-to-end**

Run: `cargo run -- --no-touchid version`
Expected: Prints version.

Run: `cargo run -- --no-touchid status`
Expected: Prints status.

- [ ] **Step 5: Commit any fmt changes**

```bash
git add -A
git commit -m "chore: format touchid code"
```

(Skip if no changes.)

---

## Summary

| Task | What it builds | Key files |
|------|---------------|-----------|
| 1 | Dependencies | `Cargo.toml` |
| 2 | Module skeleton + fallback + public API | `src/auth/touchid/{mod,fallback}.rs`, `src/lib.rs` |
| 3 | Session cache with TTL | `src/auth/touchid/session_cache.rs` |
| 4 | macOS biometric backend (LAContext + SecItem) | `src/auth/touchid/macos.rs` |
| 5 | Credential save/clear integration | `src/auth/credentials.rs` |
| 6 | `--no-touchid` flag + command gating | `src/main.rs` |
| 7 | Token refresh path integration | `src/main.rs`, `src/auth/oauth.rs` |
| 8 | Integration test coverage | `tests/touchid_integration_test.rs` |
| 9 | Final verification + cleanup | All |
