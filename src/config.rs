use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_model")]
    pub default_model: String,

    #[serde(default)]
    pub magelab_home: Option<String>,

    #[serde(default = "default_gateway_url")]
    pub gateway_url: String,

    #[serde(default = "default_local_url")]
    pub local_url: String,

    #[serde(default = "default_prefer")]
    pub prefer: String,

    #[serde(default = "default_auto_approve")]
    pub auto_approve: Vec<String>,

    #[serde(default = "default_theme")]
    pub theme: String,
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

    /// Get effective API key: env var takes precedence over config file
    pub fn api_key(&self) -> Option<String> {
        std::env::var("MAGELAB_API_KEY")
            .ok()
            .or_else(|| self.api_key.clone())
    }

    /// Returns true if remote mode requires setup (no API key configured)
    pub fn needs_remote_setup(&self) -> bool {
        self.api_key().is_none()
    }

    /// Interactive first-run setup: prompt for API key
    pub fn run_first_setup(&mut self) -> Result<()> {
        println!("Welcome to MageLab CLI!\n");
        println!("Enter your API key (get one at magelab.ai/dashboard):");
        print!("> ");
        std::io::Write::flush(&mut std::io::stdout())?;

        let mut key = String::new();
        std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut key)?;
        let key = key.trim().to_string();

        if key.is_empty() {
            anyhow::bail!("No API key provided");
        }

        self.api_key = Some(key);
        self.save()?;
        println!("\nSaved to {}\n", Self::path()?.display());
        Ok(())
    }
}
