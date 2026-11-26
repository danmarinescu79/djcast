//! Now playing for Serato DJ 5 and newer.
//!
//! Serato keeps its library in an SQLite database (`master.sqlite`). The moment a track is
//! loaded on a deck it adds a row to `history_entry` (deck, artist, title, start time) and
//! closes the row of the track that was on that deck before, so the open rows of the current
//! session are the tracks loaded on each deck right now.
//!
//! Playing or stopping a deck leaves no trace there, and Serato's window exposes no text, fader
//! or play state through UI Automation (checked with 5.1: the play buttons report the same
//! state playing or stopped). So which deck is heard comes only from the controller's channel
//! faders and crossfader over MIDI, with the same logic as the rekordbox source. Without a
//! controller (mixing with the mouse) djcast does not guess: guesses from the load order or
//! the track lengths were wrong too often in tests.
//!
//! The database is opened read-only; Serato's account file (`user_tokens.json`) is never read.

use crate::live::mixer::{Assign, MidiMap, MixerState};
use crate::live::onair::OnAir;
use crate::{DeckStatus, LiveStatus, Source, Track};
use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Shown when no controller MIDI input is connected.
const NEEDS_MIDI: &str = "Serato doesn't say which deck is playing: choose your controller's MIDI input \
                          in Settings → Controller (mixing with the mouse is not supported)";

#[derive(Debug, Clone, PartialEq)]
struct Loaded {
    deck: u8,
    artist: String,
    title: String,
}

pub struct SeratoSource {
    db: PathBuf,
    conn: Option<Connection>,
    loaded: Vec<Loaded>,
    mixer: Arc<Mutex<MixerState>>,
    _midi: Option<midir::MidiInputConnection<()>>,
    onair: OnAir,
    on_air: Option<u8>,
    error: Option<String>,
    audible: bool,
}

impl SeratoSource {
    pub fn new(midi_input: &str, map: MidiMap, assign: [Assign; 4], confirm: Duration) -> Result<Self> {
        let db = library_path().context("Serato DJ library folder not found")?;
        let mixer = Arc::new(Mutex::new(MixerState::new(assign)));
        let midi = crate::live::open_midi(midi_input, map, mixer.clone());
        Ok(Self {
            db,
            conn: None,
            loaded: Vec::new(),
            mixer,
            _midi: midi,
            onair: OnAir::new(confirm),
            on_air: None,
            error: None,
            audible: true,
        })
    }

    fn read_loaded(&mut self) -> Result<Vec<Loaded>> {
        if self.conn.is_none() {
            if !self.db.exists() {
                bail!("Serato DJ library not found at {} (Serato DJ 5 or newer is needed)", self.db.display());
            }
            let conn = Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
                .with_context(|| format!("opening {}", self.db.display()))?;
            conn.busy_timeout(Duration::from_millis(500))?;
            self.conn = Some(conn);
        }
        let conn = self.conn.as_ref().unwrap();
        let rows = open_entries(conn);
        if rows.is_err() {
            self.conn = None; // reopen next time (Serato restarted, database replaced)
        }
        rows
    }

    /// The deck heard now, once the controller's faders or crossfader have been seen.
    fn pick(&mut self, now: Instant) -> Option<u8> {
        let mixer = self.mixer.lock().unwrap();
        if !mixer.faders.iter().any(Option::is_some) && mixer.crossfader.is_none() {
            return None; // nothing moved yet: don't pick a deck at random
        }
        let levels: Vec<(u8, f32)> = self.loaded.iter().map(|l| (l.deck, mixer.gain(l.deck))).collect();
        drop(mixer);
        self.onair.update(&levels, now)
    }
}

/// Tracks loaded on the decks in the newest history session.
fn open_entries(conn: &Connection) -> Result<Vec<Loaded>> {
    let mut stmt = conn.prepare_cached(
        "SELECT deck, artist, name FROM history_entry
         WHERE session_id = (SELECT MAX(id) FROM history_session) AND end_time = -1
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
    let mut out: Vec<Loaded> = Vec::new();
    for row in rows {
        let (deck, artist, title) = row?;
        let Ok(deck) = deck.trim().parse::<u8>() else { continue };
        // One track per deck: a newer row for the same deck wins.
        out.retain(|l| l.deck != deck);
        out.push(Loaded { deck, artist: artist.trim().into(), title: title.trim().into() });
    }
    Ok(out)
}

/// `master.sqlite` of Serato DJ 5+.
fn library_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    // Not yet checked against a Mac install.
    #[cfg(not(windows))]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Application Support"));
    Some(base?.join("Serato").join("Library").join("master.sqlite"))
}

impl Source for SeratoSource {
    fn name(&self) -> &'static str {
        "serato"
    }

    fn poll(&mut self) -> Result<Option<Track>> {
        match self.read_loaded() {
            Ok(loaded) => self.loaded = loaded,
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                return Err(e);
            }
        }
        // The decks stay visible in the app; only the title needs the controller.
        if self._midi.is_none() {
            self.error = Some(NEEDS_MIDI.into());
            return Err(anyhow!(NEEDS_MIDI));
        }
        self.error = None;
        if self.audible {
            self.on_air = self.pick(Instant::now()).or(self.on_air);
        }
        Ok(self.on_air.and_then(|d| self.loaded.iter().find(|l| l.deck == d)).map(|l| Track {
            artist: l.artist.clone(),
            title: l.title.clone(),
            album: None,
            deck: Some(l.deck),
        }))
    }

    fn set_audible(&mut self, audible: bool) {
        self.audible = audible;
    }

    fn debug_snapshot(&self) -> Option<String> {
        let mixer = self.mixer.lock().unwrap();
        let mut out = String::new();
        for l in &self.loaded {
            out.push_str(&format!("  deck {} gain {:.2} | {} - {}\n", l.deck, mixer.gain(l.deck), l.artist, l.title));
        }
        out.push_str(&format!("  on air: {:?}{}", self.on_air, self.error.as_ref().map(|e| format!(" | error: {e}")).unwrap_or_default()));
        Some(out)
    }

    fn live_status(&self) -> Option<LiveStatus> {
        let mixer = self.mixer.lock().unwrap();
        let mut decks: Vec<DeckStatus> = self
            .loaded
            .iter()
            .map(|l| DeckStatus {
                deck: l.deck,
                title: l.title.clone(),
                artist: l.artist.clone(),
                elapsed: None,
                remaining: None,
                // Serato does not say which decks play (see play_state_known).
                playing: false,
                visible: true,
                gain: mixer.gain(l.deck),
                on_air: self.on_air == Some(l.deck),
            })
            .collect();
        decks.sort_by_key(|d| d.deck);
        Some(LiveStatus {
            decks,
            on_air: self.on_air,
            faders: mixer.faders,
            crossfader: mixer.crossfader,
            midi: self._midi.is_some(),
            error: self.error.clone(),
            permission_needed: false,
            play_state_known: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The part of Serato 5.1's schema that we read, with rows as Serato wrote them in a test.
    fn library(rows: &[(&str, &str, &str, i64, i64)]) -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE history_session (id INTEGER PRIMARY KEY AUTOINCREMENT, start_time INTEGER, end_time INTEGER);
             CREATE TABLE history_entry (id INTEGER PRIMARY KEY AUTOINCREMENT, session_id INTEGER NOT NULL DEFAULT -1,
               deck TEXT NOT NULL DEFAULT '', artist TEXT NOT NULL DEFAULT '', name TEXT NOT NULL DEFAULT '',
               start_time INTEGER NOT NULL, end_time INTEGER DEFAULT -1);
             INSERT INTO history_session (start_time, end_time) VALUES (100, -1);",
        )
        .unwrap();
        for (deck, artist, name, start, end) in rows {
            c.execute(
                "INSERT INTO history_entry (session_id, deck, artist, name, start_time, end_time) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![deck, artist, name, start, end],
            )
            .unwrap();
        }
        c
    }

    #[test]
    fn loaded_tracks_are_the_open_rows() {
        let c = library(&[
            ("1", "Early 90S", "Boogie Singer", 313, 559),
            ("2", "PEZNT ", "Burn", 315, 523),
            ("2", "Alex O'Neill", "Body Sax", 523, 592),
            ("1", "Stan Kolev ", "Ajna", 559, -1),
            ("2", "Early 90S", "Boogie Singer", 592, -1),
        ]);
        let loaded = open_entries(&c).unwrap();
        assert_eq!(loaded.iter().map(|l| (l.deck, l.title.as_str())).collect::<Vec<_>>(), [(1, "Ajna"), (2, "Boogie Singer")]);
        assert_eq!(loaded[0].artist, "Stan Kolev");
    }

    #[test]
    fn a_new_load_replaces_the_deck_track() {
        let c = library(&[("1", "", "A", 10, -1), ("2", "", "B", 20, -1)]);
        c.execute("UPDATE history_entry SET end_time = 30 WHERE name = 'A'", []).unwrap();
        c.execute("INSERT INTO history_entry (session_id, deck, name, start_time) VALUES (1, '1', 'C', 30)", []).unwrap();
        let loaded = open_entries(&c).unwrap();
        assert_eq!(loaded.iter().map(|l| (l.deck, l.title.as_str())).collect::<Vec<_>>(), [(2, "B"), (1, "C")]);
    }

    #[test]
    fn only_the_newest_session_counts() {
        let c = library(&[("1", "", "Old", 10, -1)]);
        c.execute("INSERT INTO history_session (start_time, end_time) VALUES (200, -1)", []).unwrap();
        assert!(open_entries(&c).unwrap().is_empty());
    }
}
