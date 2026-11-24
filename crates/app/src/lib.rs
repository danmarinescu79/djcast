//! Shared application layer used by the CLI and the desktop app.

pub mod service;

use anyhow::{Context, Result};
use djcast_core::Config;
use djcast_nowplaying::NowPlayingConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The whole `djcast.toml`: streaming settings plus `[nowplaying]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[serde(flatten)]
    pub stream: Config,
    pub nowplaying: NowPlayingConfig,
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {} (create one with `djcast init`)", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Load, or return defaults when the file does not exist yet.
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if path.exists() { Self::load(path) } else { Ok(Self::default()) }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?).with_context(|| format!("writing {}", path.display()))
    }
}

/// Per-user config location: `%APPDATA%\djcast\djcast.toml` (Windows),
/// `~/Library/Application Support/djcast/djcast.toml` (macOS).
/// `DJCAST_CONFIG` overrides it.
pub fn user_config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("DJCAST_CONFIG") {
        return PathBuf::from(p);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("djcast").join("djcast.toml")
}
