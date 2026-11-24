//! Now-playing detection.
//!
//! Each detection method is a [`Source`]. Planned sources (see `docs/research-now-playing.md`):
//! - `rekordbox_db`  – history table of rekordbox `master.db` (stable, ~1 min delay)
//! - `rekordbox_mem` – deck state read from rekordbox process memory (instant, version-specific)
//! - `midi`          – controller faders/crossfader to decide which deck is on air
//! - `prodjlink`     – CDJs / DJM over the network

use serde::{Deserialize, Serialize};

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
