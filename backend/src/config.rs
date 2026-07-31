use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default)]
    pub forgejo: ForgejoConfig,
    #[serde(default)]
    pub google: GoogleConfig,
    #[serde(default)]
    pub email: EmailConfig,
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("data")
}

/// Reusable OAuth2 client credentials, shared across providers (Forgejo, Google, …).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OAuth2Credentials {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
}

impl OAuth2Credentials {
    pub fn is_configured(&self) -> bool {
        !self.client_id.is_empty() && !self.client_secret.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoogleConfig {
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ForgejoConfig {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub repo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmailConfig {
    #[serde(default)]
    pub from: String,
    /// Kindle (or other recipient) email address.
    #[serde(default)]
    pub to: String,
}

pub type SharedConfig = Arc<RwLock<Config>>;

impl Default for Config {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            forgejo: ForgejoConfig::default(),
            google: GoogleConfig::default(),
            email: EmailConfig::default(),
        }
    }
}

/// Load config from `path`, or create and write a default file if missing.
pub fn load_or_create(path: &Path) -> Config {
    if path.exists() {
        let contents = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read config {}: {e}", path.display()));
        serde_json::from_str(&contents)
            .unwrap_or_else(|e| panic!("Failed to parse config {}: {e}", path.display()))
    } else {
        tracing::info!("No config file at {}, creating default", path.display());
        let cfg = Config::default();
        if let Err(e) = save(&cfg, path) {
            tracing::warn!("Could not write default config: {e}");
        }
        cfg
    }
}

/// Serialise `config` to pretty JSON and write it to `path`.
pub fn save(config: &Config, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {e}"))?;
    }
    let contents = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialise config: {e}"))?;
    std::fs::write(path, contents)
        .map_err(|e| format!("Failed to write config to {}: {e}", path.display()))
}
