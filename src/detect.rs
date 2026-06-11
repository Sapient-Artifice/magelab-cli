use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendBundleKind {
    DevRepo,
    PackagedApp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendBundle {
    pub kind: BackendBundleKind,
    pub root: PathBuf,
    pub api_dir: Option<PathBuf>,
    pub backend_dir: PathBuf,
    pub python: PathBuf,
}

/// Discover online devices for the authenticated user
pub async fn discover_devices(gateway_url: &str, jwt: &str) -> Result<Vec<String>> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/realtime/devices", gateway_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", jwt))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Ok(Vec::new());
    }

    let body: serde_json::Value = resp.json().await?;
    let devices = body["devices"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(devices)
}

/// Get a short-lived ws-ticket for WebSocket relay connection
pub async fn get_ws_ticket(gateway_url: &str, jwt: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/v1/realtime/ws-ticket",
        gateway_url.trim_end_matches('/')
    );
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", jwt))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get ws-ticket ({}): {}", status, body);
    }

    let body: serde_json::Value = resp.json().await?;
    body["ws_ticket"]
        .as_str()
        .map(String::from)
        .context("No ws_ticket in response")
}

/// Shared HTTP client for health checks (avoids creating a new connection pool per call)
fn health_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_millis(200))
            .build()
            .unwrap_or_default()
    })
}

/// Check if the local backend is running by hitting /health
pub async fn check_backend_health(local_url: &str) -> bool {
    let url = format!("{}/health", local_url.trim_end_matches('/'));
    health_client()
        .get(&url)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Find a launchable backend bundle for either a dev repo or packaged app.
pub fn find_backend_bundle(config_override: Option<&str>) -> Result<Option<BackendBundle>> {
    if let Ok(api_dir) = std::env::var("MAGELAB_API_DIR") {
        return resolve_explicit_backend_bundle_path("MAGELAB_API_DIR", Path::new(&api_dir));
    }

    if let Ok(home) = std::env::var("MAGELAB_HOME") {
        return resolve_explicit_backend_bundle_path("MAGELAB_HOME", Path::new(&home));
    }

    if let Some(override_path) = config_override {
        if !override_path.is_empty() {
            return resolve_explicit_backend_bundle_path("magelab_home", Path::new(override_path));
        }
    }

    for candidate in dev_layout_candidates() {
        if let Some(bundle) = resolve_dev_repo_bundle(&candidate) {
            return Ok(Some(bundle));
        }
    }

    for candidate in platform_default_paths() {
        if let Some(bundle) = resolve_backend_bundle_path(&candidate)? {
            return Ok(Some(bundle));
        }
    }

    Ok(None)
}

fn resolve_explicit_backend_bundle_path(label: &str, path: &Path) -> Result<Option<BackendBundle>> {
    if !path.exists() {
        anyhow::bail!(
            "{} points to a path that does not exist: {}",
            label,
            path.display()
        );
    }

    resolve_backend_bundle_path(path)?.map(Some).ok_or_else(|| {
        anyhow::anyhow!(
            "{} does not contain a MageLab backend bundle: {}",
            label,
            path.display()
        )
    })
}

fn resolve_backend_bundle_path(path: &Path) -> Result<Option<BackendBundle>> {
    if path.file_name().and_then(|n| n.to_str()) == Some("main.py") {
        anyhow::bail!(
            "magelab_home should point to the MageLab install root or bundled API directory, not backend/main.py."
        );
    }

    if let Some(bundle) = resolve_packaged_api_bundle(path) {
        return Ok(Some(bundle));
    }

    for api_dir in packaged_api_candidates(path) {
        if let Some(bundle) = resolve_packaged_api_bundle(&api_dir) {
            return Ok(Some(bundle));
        }
    }

    if let Some(bundle) = resolve_dev_repo_bundle(path) {
        return Ok(Some(bundle));
    }

    Ok(None)
}

fn resolve_dev_repo_bundle(root: &Path) -> Option<BackendBundle> {
    let backend_dir = root.join("backend");
    if !is_dev_repo_root(root, &backend_dir) {
        return None;
    }

    Some(BackendBundle {
        kind: BackendBundleKind::DevRepo,
        root: root.to_path_buf(),
        api_dir: None,
        python: dev_python(&backend_dir),
        backend_dir,
    })
}

fn is_dev_repo_root(root: &Path, backend_dir: &Path) -> bool {
    backend_dir.join("main.py").exists()
        && (root.join(".git").exists()
            || root.join("pyproject.toml").exists()
            || backend_dir.join("pyproject.toml").exists()
            || root.join("package.json").exists())
}

fn resolve_packaged_api_bundle(api_dir: &Path) -> Option<BackendBundle> {
    let backend_dir = api_dir.join("backend");
    if !backend_dir.join("main.py").exists() {
        return None;
    }

    let python = packaged_python(api_dir)?;
    Some(BackendBundle {
        kind: BackendBundleKind::PackagedApp,
        root: api_dir.to_path_buf(),
        api_dir: Some(api_dir.to_path_buf()),
        backend_dir,
        python,
    })
}

fn packaged_api_candidates(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("bin").join("api"),
        root.join("Contents")
            .join("Resources")
            .join("bin")
            .join("api"),
    ]
}

fn dev_layout_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.clone());
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join("mage-lab"));
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..4 {
            dir = dir.and_then(|d| d.parent().map(|p| p.to_path_buf()));
            if let Some(ref d) = dir {
                candidates.push(d.join("mage-lab"));
            }
        }
    }

    candidates
}

fn platform_default_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = dirs::home_dir();

    #[cfg(target_os = "macos")]
    {
        if let Some(h) = &home {
            paths.push(h.join("Applications").join("Mage Lab"));
            paths.push(h.join("Applications").join("magelab.app"));
        }
        paths.push(PathBuf::from("/Applications/Mage Lab"));
        paths.push(PathBuf::from("/Applications/magelab.app"));
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(h) = &home {
            paths.push(h.join(".local/share/magelab"));
        }
        paths.push(PathBuf::from("/opt/magelab"));
        paths.push(PathBuf::from("/usr/lib/magelab"));
        paths.push(PathBuf::from("/usr/lib/magelab/bin/api"));
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            paths.push(PathBuf::from(&local).join("magelab"));
            paths.push(PathBuf::from(local).join("MageLab"));
        }
        if let Ok(pf) = std::env::var("PROGRAMFILES") {
            paths.push(PathBuf::from(pf).join("MageLab"));
        }
    }

    paths
}

/// Random per-launch secret shared with the backend so only the launcher
/// (or whoever it hands the secret to) can drive its runtime auth endpoints.
pub fn generate_backend_control_secret() -> String {
    use rand::Rng;
    (0..32)
        .map(|_| format!("{:02x}", rand::rngs::OsRng.gen::<u8>()))
        .collect()
}

/// Launch the Python backend in headless mode.
/// If `relay_enabled` is true, sets REALTIME_DESKTOP_BROKER_ENABLED=1 so
/// the backend registers as a relay device with the gateway.
///
/// Sets MAGELAB_BACKEND_AUTH_SOURCE=cli so the backend may pull auth tokens
/// and vault secrets via `mage internal` helper commands — that pull path
/// is only enabled for backends mage launched intentionally. The control
/// secret guards the backend's runtime auth endpoints.
pub fn launch_backend_headless(
    bundle: &BackendBundle,
    host: &str,
    port: u16,
    relay_enabled: bool,
    control_secret: &str,
) -> Result<Child> {
    // Log backend output to ~/.config/magelab/backend.log
    let log_dir = dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
        .join("magelab");
    std::fs::create_dir_all(&log_dir).ok();
    let log_path = log_dir.join("backend.log");
    let (stdout_cfg, stderr_cfg): (Stdio, Stdio) =
        match std::fs::File::create(&log_path).and_then(|f| f.try_clone().map(|f2| (f, f2))) {
            Ok((f, f2)) => (Stdio::from(f), Stdio::from(f2)),
            Err(_) => (Stdio::null(), Stdio::null()),
        };

    let mut cmd = build_backend_command(bundle, host, port, relay_enabled, control_secret);
    cmd.stdin(Stdio::null())
        .stdout(stdout_cfg)
        .stderr(stderr_cfg);

    let child = cmd.spawn().with_context(|| {
        format!(
            "Failed to launch backend from {}",
            bundle.backend_dir.display()
        )
    })?;

    Ok(child)
}

/// Build the backend launch command (separated from spawn for testability).
fn build_backend_command(
    bundle: &BackendBundle,
    host: &str,
    port: u16,
    relay_enabled: bool,
    control_secret: &str,
) -> Command {
    let backend_arg = bundle.backend_dir.to_string_lossy().to_string();
    let port_arg = port.to_string();
    let mut cmd = Command::new(&bundle.python);
    cmd.args([
        "-m",
        "uvicorn",
        "main:app",
        "--app-dir",
        &backend_arg,
        "--host",
        host,
        "--port",
        &port_arg,
        "--log-level",
        "warning",
    ])
    .current_dir(&bundle.backend_dir);

    cmd.env("MAGELAB_BACKEND_AUTH_SOURCE", "cli");
    cmd.env("MAGELAB_BACKEND_CONTROL_SECRET", control_secret);

    if relay_enabled {
        cmd.env("REALTIME_DESKTOP_BROKER_ENABLED", "1");
    }

    cmd
}

/// Extract the port from an http(s) URL, defaulting to 11115.
pub fn port_from_url(url: &str) -> u16 {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.port())
        .unwrap_or(11115)
}

fn dev_python(backend_dir: &Path) -> PathBuf {
    // Check for venv — path differs by platform
    #[cfg(not(target_os = "windows"))]
    let venv_python = backend_dir.join(".venv").join("bin").join("python");
    #[cfg(target_os = "windows")]
    let venv_python = backend_dir.join(".venv").join("Scripts").join("python.exe");

    if venv_python.exists() {
        return venv_python;
    }

    // Fallback to system
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from("python3")
    }
    #[cfg(target_os = "windows")]
    {
        PathBuf::from("python")
    }
}

fn packaged_python(api_dir: &Path) -> Option<PathBuf> {
    #[cfg(not(target_os = "windows"))]
    let candidates = [api_dir.join("python").join("bin").join("python3")];

    #[cfg(target_os = "windows")]
    let candidates = [
        api_dir.join("python").join("python.exe"),
        api_dir.join("python").join("Scripts").join("python.exe"),
    ];

    candidates.into_iter().find(|path| path.exists())
}

/// Poll the backend health endpoint until it's ready
pub async fn wait_for_backend(local_url: &str, timeout: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    let interval = Duration::from_millis(100);

    while start.elapsed() < timeout {
        if check_backend_health(local_url).await {
            return Ok(());
        }
        tokio::time::sleep(interval).await;
    }

    anyhow::bail!("Backend did not become healthy within {:?}", timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bundle() -> BackendBundle {
        BackendBundle {
            kind: BackendBundleKind::DevRepo,
            root: PathBuf::from("/tmp/magelab-test"),
            api_dir: None,
            backend_dir: PathBuf::from("/tmp/magelab-test/backend"),
            python: PathBuf::from("python3"),
        }
    }

    fn env_of(cmd: &Command, key: &str) -> Option<String> {
        cmd.get_envs()
            .find(|(k, _)| *k == std::ffi::OsStr::new(key))
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().to_string()))
    }

    #[test]
    fn launch_sets_cli_auth_source_and_control_secret() {
        let cmd = build_backend_command(&test_bundle(), "127.0.0.1", 11115, false, "feedc0de");
        assert_eq!(
            env_of(&cmd, "MAGELAB_BACKEND_AUTH_SOURCE").as_deref(),
            Some("cli")
        );
        assert_eq!(
            env_of(&cmd, "MAGELAB_BACKEND_CONTROL_SECRET").as_deref(),
            Some("feedc0de")
        );
        assert_eq!(env_of(&cmd, "REALTIME_DESKTOP_BROKER_ENABLED"), None);
    }

    #[test]
    fn launch_sets_relay_env_when_enabled() {
        let cmd = build_backend_command(&test_bundle(), "127.0.0.1", 11115, true, "feedc0de");
        assert_eq!(
            env_of(&cmd, "REALTIME_DESKTOP_BROKER_ENABLED").as_deref(),
            Some("1")
        );
    }

    #[test]
    fn control_secret_is_random_hex() {
        let a = generate_backend_control_secret();
        let b = generate_backend_control_secret();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
