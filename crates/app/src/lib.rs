//! Shared application layer used by the CLI and the desktop app.

pub mod secrets;
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
    /// The file this was loaded from still contains passwords in plain text.
    #[serde(skip)]
    pub plaintext_passwords_on_disk: bool,
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
        cfg.plaintext_passwords_on_disk = !cfg.stream.server.password.is_empty()
            || cfg.profiles.iter().any(|p| !p.server.password.is_empty());
        // Passwords not in the file come from the OS credential store.
        for p in &mut cfg.profiles {
            if p.server.password.is_empty()
                && let Some(pw) = secrets::get(&p.name)
            {
                p.server.password = pw;
            }
        }
        cfg.normalize();
        Ok(cfg)
    }

    /// Remove stored passwords of profiles that no longer exist (deleted or renamed).
    pub fn forget_removed_profiles(old: &AppConfig, new: &AppConfig) {
        for p in &old.profiles {
            if !new.profiles.iter().any(|n| n.name == p.name) {
                secrets::delete(&p.name);
            }
        }
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

    /// Write the config. Where available, passwords go to the OS credential store and
    /// are left out of the file; if storing one fails it stays in the file (with a warning).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut out = self.clone();
        if secrets::available() {
            for p in &mut out.profiles {
                if p.server.password.is_empty() {
                    secrets::delete(&p.name);
                    continue;
                }
                match secrets::set(&p.name, &p.server.password) {
                    Ok(()) => p.server.password.clear(),
                    Err(e) => log::warn!(
                        "could not store the password of \"{}\" in the credential store ({e}); keeping it in the config file",
                        p.name
                    ),
                }
            }
            // `[server]` mirrors the active profile.
            if let Some(active) = out.profiles.iter().find(|p| p.name == out.active_profile) {
                out.stream.server.password = active.server.password.clone();
            }
        }
        std::fs::write(path, toml::to_string_pretty(&out)?).with_context(|| format!("writing {}", path.display()))
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
