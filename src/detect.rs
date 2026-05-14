use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

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

/// Find the mage-lab installation directory
pub fn find_magelab_home(config_override: Option<&str>) -> Option<PathBuf> {
    // 1. MAGELAB_HOME env var (highest priority)
    if let Ok(home) = std::env::var("MAGELAB_HOME") {
        return Some(PathBuf::from(home));
    }

    // 2. Config file override (user explicitly set magelab_home)
    if let Some(override_path) = config_override {
        if !override_path.is_empty() {
            return Some(PathBuf::from(override_path));
        }
    }

    // 3. Sibling directory relative to CLI binary
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

    // 4. Platform-specific default paths
    platform_default_paths()
        .into_iter()
        .find(|path| path.join("backend").join("main.py").exists())
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

/// Launch the Python backend in headless mode.
/// `port` is extracted from the configured local_url so it stays in sync.
pub fn launch_backend_headless(magelab_home: &Path, port: u16) -> Result<Child> {
    let backend_dir = magelab_home.join("backend");

    // Try to find Python in the mage-lab venv first, then system
    let python = find_python(&backend_dir);

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

    let child = Command::new(&python)
        .args([
            "-m",
            "uvicorn",
            "main:app",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--log-level",
            "warning",
        ])
        .current_dir(&backend_dir)
        .stdin(Stdio::null())
        .stdout(stdout_cfg)
        .stderr(stderr_cfg)
        .spawn()
        .with_context(|| format!("Failed to launch backend from {}", backend_dir.display()))?;

    Ok(child)
}

/// Extract the port from an http(s) URL, defaulting to 11115.
pub fn port_from_url(url: &str) -> u16 {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.port())
        .unwrap_or(11115)
}

pub fn find_python(backend_dir: &Path) -> String {
    // Check for venv — path differs by platform
    #[cfg(not(target_os = "windows"))]
    let venv_python = backend_dir.join(".venv").join("bin").join("python");
    #[cfg(target_os = "windows")]
    let venv_python = backend_dir.join(".venv").join("Scripts").join("python.exe");

    if venv_python.exists() {
        return venv_python.to_string_lossy().into();
    }

    // Fallback to system
    #[cfg(not(target_os = "windows"))]
    {
        "python3".to_string()
    }
    #[cfg(target_os = "windows")]
    {
        "python".to_string()
    }
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

