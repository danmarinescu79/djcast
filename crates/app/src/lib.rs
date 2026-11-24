//! Shared application layer used by the CLI and the desktop app.

pub mod service;

use anyhow::{Context, Result};
use djcast_core::Config;
use djcast_nowplaying::NowPlayingConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The whole `djcast.toml`: streaming settings plus `[nowplaying]` and server profiles.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[serde(flatten)]
    pub stream: Config,
    pub nowplaying: NowPlayingConfig,
    /// Name of the profile copied into `[server]` / `[encoder]` when going live.
    pub active_profile: String,
    pub profiles: Vec<ServerProfile>,
}

/// A saved server destination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerProfile {
    pub name: String,
    pub server: djcast_core::config::ServerConfig,
    pub bitrate: u32,
}

impl Default for ServerProfile {
    fn default() -> Self {
        Self { name: "Default".into(), server: Default::default(), bitrate: 192 }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {} (create one with `djcast init`)", path.display()))?;
        let mut cfg: Self = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.normalize();
        Ok(cfg)
    }

    /// Make sure there is at least one profile and the active one exists, then copy the
    /// active profile into the streaming settings. Older configs without profiles get a
    /// "Default" profile made from their `[server]` section.
    pub fn normalize(&mut self) {
        if self.profiles.is_empty() {
            self.profiles.push(ServerProfile {
                name: "Default".into(),
                server: self.stream.server.clone(),
                bitrate: self.stream.encoder.bitrate,
            });
        }
        if !self.profiles.iter().any(|p| p.name == self.active_profile) {
            self.active_profile = self.profiles[0].name.clone();
        }
        let p = self.profiles.iter().find(|p| p.name == self.active_profile).unwrap();
        self.stream.server = p.server.clone();
        self.stream.encoder.bitrate = p.bitrate;
    }

    /// Load, or return defaults when the file does not exist yet.
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            let mut cfg = Self::default();
            cfg.normalize();
            Ok(cfg)
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_gets_default_profile_and_profiles_round_trip() {
        let old = "[server]\nhost = \"radio.example\"\nport = 9000\n[encoder]\nbitrate = 256\n";
        let mut cfg: AppConfig = toml::from_str(old).unwrap();
        cfg.normalize();
        assert_eq!(cfg.profiles.len(), 1);
        assert_eq!(cfg.profiles[0].server.host, "radio.example");
        assert_eq!(cfg.profiles[0].bitrate, 256);

        cfg.profiles.push(ServerProfile { name: "Backup".into(), bitrate: 128, ..Default::default() });
        cfg.active_profile = "Backup".into();
        cfg.normalize();
        assert_eq!(cfg.stream.server.host, "localhost");
        assert_eq!(cfg.stream.encoder.bitrate, 128);

        let back: AppConfig = toml::from_str(&toml::to_string_pretty(&cfg).unwrap()).unwrap();
        assert_eq!(back.profiles, cfg.profiles);
        assert_eq!(back.active_profile, "Backup");
    }
}
