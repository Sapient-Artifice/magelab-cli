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
