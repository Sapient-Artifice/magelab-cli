use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionMode {
    Local,
    Remote,
    Auto,
}

#[allow(dead_code)]
impl ConnectionMode {
    pub fn from_flags(local: bool, remote: bool) -> Self {
        match (local, remote) {
            (true, _) => Self::Local,
            (_, true) => Self::Remote,
            _ => Self::Auto,
        }
    }
}

/// The resolved connection after auto-detection
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ResolvedConnection {
    /// Direct WebSocket to local backend (full tools)
    Local,
    /// REST/SSE to gateway API (chat only, API key auth)
    RemoteRest,
    /// WebSocket relay through gateway (full tools, JWT auth)
    RemoteRelay { jwt: String },
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

/// Check if the local backend is running by hitting /health
pub async fn check_backend_health(local_url: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap_or_default();

    let url = format!("{}/health", local_url.trim_end_matches('/'));
    client
        .get(&url)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Find the mage-lab installation directory
pub fn find_magelab_home(config_override: Option<&str>) -> Option<PathBuf> {
    // 1. MAGELAB_HOME env var
    if let Ok(home) = std::env::var("MAGELAB_HOME") {
        return Some(PathBuf::from(home));
    }

    // 2. Sibling directory relative to CLI binary
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("mage-lab"));
        if let Some(path) = sibling {
            if path.join("backend").join("main.py").exists() {
                return Some(path);
            }
        }
    }

    // 3. Platform-specific default paths
    for path in platform_default_paths() {
        if path.join("backend").join("main.py").exists() {
            return Some(path);
        }
    }

    // 4. Config file override
    if let Some(override_path) = config_override {
        if !override_path.is_empty() {
            return Some(PathBuf::from(override_path));
        }
    }

    None
}

fn platform_default_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = dirs::home_dir();

    #[cfg(target_os = "macos")]
    {
        if let Some(h) = &home {
            paths.push(h.join("Applications").join("Mage Lab"));
        }
        paths.push(PathBuf::from("/Applications/Mage Lab"));
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(h) = &home {
            paths.push(h.join(".local/share/magelab"));
        }
        paths.push(PathBuf::from("/opt/magelab"));
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            paths.push(PathBuf::from(local).join("MageLab"));
        }
        if let Ok(pf) = std::env::var("PROGRAMFILES") {
            paths.push(PathBuf::from(pf).join("MageLab"));
        }
    }

    paths
}

/// Launch the Python backend in headless mode
pub fn launch_backend_headless(magelab_home: &Path) -> Result<Child> {
    let backend_dir = magelab_home.join("backend");

    // Try to find Python in the mage-lab venv first, then system
    let python = find_python(&backend_dir);

    let child = Command::new(&python)
        .args([
            "-m",
            "uvicorn",
            "main:app",
            "--host",
            "127.0.0.1",
            "--port",
            "11115",
            "--log-level",
            "warning",
        ])
        .current_dir(&backend_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to launch backend from {}", backend_dir.display()))?;

    Ok(child)
}

fn find_python(backend_dir: &Path) -> String {
    // Check for venv
    let venv_python = backend_dir.join(".venv").join("bin").join("python");
    if venv_python.exists() {
        return venv_python.to_string_lossy().into();
    }
    // Fallback to system
    "python3".to_string()
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

/// Send shutdown command to a backend child process via stdin
#[allow(dead_code)]
pub fn shutdown_backend(child: &mut Child) -> Result<()> {
    use std::io::Write;
    if let Some(stdin) = child.stdin.as_mut() {
        writeln!(stdin, "sidecar shutdown")?;
    }
    Ok(())
}
