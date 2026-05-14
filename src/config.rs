use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,

    #[serde(default = "default_model")]
    pub default_model: String,

    #[serde(default)]
    pub magelab_home: Option<String>,

    #[serde(default = "default_gateway_url")]
    pub gateway_url: String,

    #[serde(default = "default_local_url")]
    pub local_url: String,

    /// Connection preference: "auto", "local", or "remote" (read by Pi extension)
    #[serde(default = "default_prefer")]
    pub prefer: String,

    /// Tools that skip user confirmation (read by backend/Pi extension)
    #[serde(default = "default_auto_approve")]
    pub auto_approve: Vec<String>,

    /// UI theme: "auto", "dark", or "light" (read by backend/Pi extension)
    #[serde(default = "default_theme")]
    pub theme: String,

    #[serde(default)]
    pub default_device: Option<String>,

    /// Enable relay — register headless backend as a device with the gateway
    #[serde(default)]
    pub relay_enabled: bool,

    #[serde(default = "default_true")]
    pub telemetry: Option<bool>,

    #[serde(default)]
    pub activated_user_id: Option<String>,
}

fn default_model() -> String {
    "qwen-3-235b-a22b-instruct-2507".into()
}
fn default_gateway_url() -> String {
    "https://api.magelab.ai".into()
}
fn default_local_url() -> String {
    "http://127.0.0.1:11115".into()
}
fn default_prefer() -> String {
    "auto".into()
}
fn default_theme() -> String {
    "auto".into()
}
fn default_true() -> Option<bool> {
    Some(true)
}
fn default_auto_approve() -> Vec<String> {
    vec![
        "read_file".into(),
        "search_files".into(),
        "BraveSearch".into(),
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: None,
            default_model: default_model(),
            magelab_home: None,
            gateway_url: default_gateway_url(),
            local_url: default_local_url(),
            prefer: default_prefer(),
            auto_approve: default_auto_approve(),
            theme: default_theme(),
            default_device: None,
            relay_enabled: false,
            telemetry: default_true(),
            activated_user_id: None,
        }
    }
}

impl Config {
    /// Standard config directory: ~/.config/magelab/
    pub fn dir() -> Result<PathBuf> {
        let base = dirs::config_dir().context("Could not determine config directory")?;
        Ok(base.join("magelab"))
    }

    /// Standard config file path
    pub fn path() -> Result<PathBuf> {
        Ok(Self::dir()?.join("cli.toml"))
    }

    /// Load config from default path, falling back to defaults
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if path.exists() {
            Self::load_from(path)
        } else {
            Ok(Self::default())
        }
    }

    /// Load config from a specific path
    pub fn load_from<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read {}", path.as_ref().display()))?;
        let config: Config =
            toml::from_str(&contents).with_context(|| "Failed to parse cli.toml")?;
        Ok(config)
    }

    /// Save config to default path
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        self.save_to(&path)
    }

    /// Save config to a specific path
    pub fn save_to<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let dir = path.as_ref().parent().context("Invalid config path")?;
        std::fs::create_dir_all(dir)?;
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }

    /// Whether telemetry is enabled (default: true)
    #[allow(dead_code)] // Used by analytics module (added in next commit)
    pub fn telemetry(&self) -> bool {
        self.telemetry.unwrap_or(true)
    }

    /// Get API key from MAGELAB_API_KEY env var.
    /// Plaintext api_key in cli.toml is deprecated — use the desktop app or env var.
    pub fn api_key(&self) -> Option<String> {
        std::env::var("MAGELAB_API_KEY").ok()
    }

    /// Set a config value by key name
    pub fn set_value(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "default_model" => self.default_model = value.to_string(),
            "magelab_home" => self.magelab_home = Some(value.to_string()),
            "gateway_url" => self.gateway_url = value.to_string(),
            "local_url" => self.local_url = value.to_string(),
            "prefer" => self.prefer = value.to_string(),
            "theme" => self.theme = value.to_string(),
            "default_device" => self.default_device = Some(value.to_string()),
            "relay_enabled" => self.relay_enabled = value.parse::<bool>().unwrap_or(false),
            "telemetry" => {
                self.telemetry = Some(match value {
                    "true" => true,
                    "false" => false,
                    _ => anyhow::bail!("telemetry must be 'true' or 'false'"),
                });
            }
            _ => anyhow::bail!("Unknown config key: {}", key),
        }
        Ok(())
    }
}
