//! Now-playing detection.
//!
//! Each detection method is a [`Source`] (see `docs/research-now-playing.md`):
//! - `rekordbox-live` – deck contents from the rekordbox window + mixer state from MIDI (instant)
//! - `text-file`      – the last line of a file another program writes (VirtualDJ, …)
//! - planned: `prodjlink` – CDJs / DJM over the network
//!
//! rekordbox's own database is deliberately not read: it is encrypted, and the window
//! already shows everything we need.

pub mod live;
pub mod output;
pub mod textfile;

use live::mixer::{Assign, MappingPreset, MidiMap};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Now-playing settings (the `[nowplaying]` config section).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NowPlayingConfig {
    pub source: SourceKind,
    /// Seconds between polls.
    pub poll_interval_secs: f64,
    /// MIDI input of the DJ controller, used to read faders/crossfader.
    /// "auto" = first port containing "DDJ"; "" or "none" = don't use MIDI.
    pub midi_input: String,
    /// Seconds a deck must be clearly louder before the title switches to it.
    pub confirm_secs: f64,
    /// Crossfader assign per channel until the switches are moved (a, thru, b).
    pub crossfader_assign: [Assign; 4],
    /// Controller mapping: "pioneer-ddj" (built in) or "custom" (`midi_custom`).
    pub midi_mapping: MappingPreset,
    /// Faders and crossfader of another controller, usually learned in the app.
    pub midi_custom: MidiMap,
    /// Folder for OBS text files and the tracklist. Empty = disabled.
    pub output_dir: String,
    /// File followed by the `text-file` source (its last line is the current track).
    pub text_file: String,
}

impl Default for NowPlayingConfig {
    fn default() -> Self {
        Self {
            source: SourceKind::None,
            poll_interval_secs: 1.0,
            midi_input: "auto".into(),
            confirm_secs: 4.0,
            crossfader_assign: [Assign::Thru; 4],
            midi_mapping: MappingPreset::PioneerDdj,
            midi_custom: MidiMap::default(),
            output_dir: "nowplaying".into(),
            text_file: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// Titles only come from manual input.
    None,
    /// Deck contents from the rekordbox window + mixer state from MIDI. "auto" and
    /// "rekordbox-db" (the play history, removed in 0.4) are read as this source.
    #[serde(alias = "auto", alias = "rekordbox-db")]
    RekordboxLive,
    /// The last line of a text file written by another DJ program.
    TextFile,
}

/// Build the configured source (None if `source = "none"`).
pub fn create_source(cfg: &NowPlayingConfig) -> anyhow::Result<Option<Box<dyn Source>>> {
    Ok(match cfg.source {
        SourceKind::None => None,
        SourceKind::RekordboxLive => Some(Box::new(live_source(cfg)?)),
        SourceKind::TextFile => Some(Box::new(textfile::TextFileSource::new(cfg.text_file.trim())?)),
    })
}

fn live_source(cfg: &NowPlayingConfig) -> anyhow::Result<live::RekordboxLiveSource> {
    live::RekordboxLiveSource::new(&live::LiveConfig {
        midi_input: cfg.midi_input.clone(),
        confirm: Duration::from_secs_f64(cfg.confirm_secs.max(0.0)),
        assign: cfg.crossfader_assign,
        map: match cfg.midi_mapping {
            MappingPreset::PioneerDdj => MidiMap::pioneer_ddj(),
            MappingPreset::Custom => cfg.midi_custom.clone(),
        },
    })
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
    /// Deck and mixer state, for sources that know it.
    fn live_status(&self) -> Option<LiveStatus> {
        None
    }
}

/// One deck as shown in a UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeckStatus {
    pub deck: u8,
    pub title: String,
    pub artist: String,
    pub elapsed: Option<String>,
    /// Remaining time as shown by rekordbox ("-mm:ss").
    pub remaining: Option<String>,
    pub playing: bool,
    /// Shown in the rekordbox window right now (2-deck layouts hide two decks).
    pub visible: bool,
    /// Mixer gain 0..=1 (fader × crossfader); 1 when unknown.
    pub gain: f32,
    pub on_air: bool,
}

/// Deck and mixer state of a live source.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LiveStatus {
    pub decks: Vec<DeckStatus>,
    pub on_air: Option<u8>,
    pub faders: [Option<f32>; 4],
    pub crossfader: Option<f32>,
    /// The controller's MIDI input is connected.
    pub midi: bool,
    pub error: Option<String>,
    /// Reading the rekordbox window needs the OS permission (macOS: Accessibility).
    pub permission_needed: bool,
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

    #[test]
    fn removed_sources_read_as_live() {
        for old in ["auto", "rekordbox-db", "rekordbox-live"] {
            let cfg: NowPlayingConfig =
                serde_json::from_str(&format!(r#"{{"source":"{old}","rekordbox_key":"x","rekordbox_db":""}}"#)).unwrap();
            assert_eq!(cfg.source, SourceKind::RekordboxLive, "{old}");
        }
    }
}
