//! Now-playing source backed by the rekordbox play history in `master.db`.
//!
//! rekordbox adds a track to the history roughly a minute after it starts playing,
//! so this source lags behind; it is the stable fallback for faster sources.

use super::{MasterDb, history};
use crate::{Source, Track};
use anyhow::Result;

pub struct RekordboxDbSource {
    db: MasterDb,
    /// History id that was newest when we started; older entries are never reported.
    baseline: Option<Option<String>>,
    current: Option<(String, Track)>,
}

impl RekordboxDbSource {
    pub fn new(db: MasterDb) -> Self {
        Self { db, baseline: None, current: None }
    }
}

impl Source for RekordboxDbSource {
    fn name(&self) -> &'static str {
        "rekordbox-db"
    }

    fn poll(&mut self) -> Result<Option<Track>> {
        if !self.db.changed() {
            return Ok(self.current.as_ref().map(|(_, t)| t.clone()));
        }
        let conn = self.db.open()?;
        let latest = history::recent(&conn, 1)?.into_iter().next();

        let Some(baseline) = &self.baseline else {
            // First read: remember where history was, don't announce an old track.
            self.baseline = Some(latest.map(|e| e.id));
            return Ok(None);
        };
        if let Some(e) = latest {
            let is_new = baseline.as_deref() != Some(e.id.as_str())
                && self.current.as_ref().map(|(id, _)| id.as_str()) != Some(e.id.as_str());
            if is_new {
                let track = Track { artist: e.artist, title: e.title, album: e.album, deck: None };
                self.current = Some((e.id, track));
            }
        }
        Ok(self.current.as_ref().map(|(_, t)| t.clone()))
    }
}
