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

pub mod session_cache;
