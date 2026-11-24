//! Now-playing detection.
//!
//! Each detection method is a [`Source`] (see `docs/research-now-playing.md`):
//! - `rekordbox-db`   – play history in rekordbox `master.db` (stable, ~1 min delay)
//! - `rekordbox-live` – deck contents from the rekordbox window + mixer state from MIDI (instant)
//! - `auto`           – `rekordbox-live`, falling back to `rekordbox-db` when it is unavailable
//! - planned: `prodjlink` – CDJs / DJM over the network

pub mod live;
pub mod output;
pub mod rekordbox;

use live::mixer::Assign;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Now-playing settings (the `[nowplaying]` config section).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NowPlayingConfig {
    pub source: SourceKind,
    /// Seconds between polls.
    pub poll_interval_secs: f64,
    /// rekordbox database key. Empty = read from the user key file
    /// (see `djcast rekordbox set-key`).
    pub rekordbox_key: String,
    /// rekordbox `master.db` path. Empty = auto-detect.
    pub rekordbox_db: String,
    /// MIDI input of the DJ controller, used to read faders/crossfader.
    /// "auto" = first port containing "DDJ"; "" or "none" = don't use MIDI.
    pub midi_input: String,
    /// Seconds a deck must be clearly louder before the title switches to it.
    pub confirm_secs: f64,
    /// Crossfader assign per channel until the switches are moved (a, thru, b).
    pub crossfader_assign: [Assign; 4],
    /// Folder for OBS text files and the tracklist. Empty = disabled.
    pub output_dir: String,
}

impl Default for NowPlayingConfig {
    fn default() -> Self {
        Self {
            source: SourceKind::None,
            poll_interval_secs: 1.0,
            rekordbox_key: String::new(),
            rekordbox_db: String::new(),
            midi_input: "auto".into(),
            confirm_secs: 4.0,
            crossfader_assign: [Assign::Thru; 4],
            output_dir: "nowplaying".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// Titles only come from manual input.
    None,
    RekordboxDb,
    RekordboxLive,
    Auto,
}

/// Build the configured source (None if `source = "none"`).
pub fn create_source(cfg: &NowPlayingConfig) -> anyhow::Result<Option<Box<dyn Source>>> {
    Ok(match cfg.source {
        SourceKind::None => None,
        SourceKind::RekordboxDb => Some(Box::new(db_source(cfg)?)),
        SourceKind::RekordboxLive => Some(Box::new(live_source(cfg)?)),
        SourceKind::Auto => {
            let db = match db_source(cfg) {
                Ok(db) => Some(db),
                Err(e) => {
                    log::warn!("history fallback unavailable: {e:#}");
                    None
                }
            };
            Some(Box::new(AutoSource { live: live_source(cfg)?, db, using_db: false }))
        }
    })
}

fn db_source(cfg: &NowPlayingConfig) -> anyhow::Result<rekordbox::RekordboxDbSource> {
    let path = if cfg.rekordbox_db.trim().is_empty() {
        rekordbox::find_master_db()?
    } else {
        cfg.rekordbox_db.trim().into()
    };
    let key = rekordbox::load_key(Some(&cfg.rekordbox_key))?;
    Ok(rekordbox::RekordboxDbSource::new(rekordbox::MasterDb::new(path, key)))
}

fn live_source(cfg: &NowPlayingConfig) -> anyhow::Result<live::RekordboxLiveSource> {
    live::RekordboxLiveSource::new(&live::LiveConfig {
        midi_input: cfg.midi_input.clone(),
        confirm: Duration::from_secs_f64(cfg.confirm_secs.max(0.0)),
        assign: cfg.crossfader_assign,
    })
}

/// Live detection, with the play history as fallback while live is unavailable.
struct AutoSource {
    live: live::RekordboxLiveSource,
    db: Option<rekordbox::RekordboxDbSource>,
    using_db: bool,
}

impl Source for AutoSource {
    fn name(&self) -> &'static str {
        "auto"
    }

    fn debug_snapshot(&self) -> Option<String> {
        Some(format!(
            "  using: {}\n{}",
            if self.using_db { "rekordbox history (fallback)" } else { "live" },
            self.live.debug_snapshot()
        ))
    }

    fn poll(&mut self) -> anyhow::Result<Option<Track>> {
        // Keep the history source's baseline current so switching to it never
        // announces an old track.
        let db_result = self.db.as_mut().map(|db| db.poll());
        match self.live.poll() {
            Ok(t) => {
                if self.using_db {
                    log::info!("now playing: live detection available again");
                    self.using_db = false;
                }
                Ok(t)
            }
            Err(e) => match db_result {
                Some(r) => {
                    if !self.using_db {
                        log::info!("now playing: live unavailable ({e:#}); using rekordbox history");
                        self.using_db = true;
                    }
                    r
                }
                None => Err(e),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    /// Deck number (1-based) when known.
    pub deck: Option<u8>,
}

impl Track {
    /// Text used for Icecast `song=` metadata.
    pub fn display(&self) -> String {
        match (self.artist.trim(), self.title.trim()) {
            ("", t) => t.to_string(),
            (a, "") => a.to_string(),
            (a, t) => format!("{a} - {t}"),
        }
    }
}

/// A now-playing detection method.
pub trait Source: Send {
    fn name(&self) -> &'static str;
    /// Poll for the track currently on air. `Ok(None)` = nothing playing / unknown.
    fn poll(&mut self) -> anyhow::Result<Option<Track>>;
    /// Internal state for troubleshooting (`--verbose`).
    fn debug_snapshot(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_formats() {
        let t = |a: &str, ti: &str| Track { artist: a.into(), title: ti.into(), album: None, deck: None };
        assert_eq!(t("Artist", "Song").display(), "Artist - Song");
        assert_eq!(t("", "Song").display(), "Song");
        assert_eq!(t("Artist", " ").display(), "Artist");
    }
}
