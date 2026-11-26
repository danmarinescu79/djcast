//! Now playing for Serato DJ 5 and newer.
//!
//! Serato keeps its library in an SQLite database (`master.sqlite`). The moment a track is
//! loaded on a deck it adds a row to `history_entry` (deck, artist, title, start time) and
//! closes the row of the track that was on that deck before, so the open rows of the current
//! session are the tracks loaded on each deck right now. Playing or stopping a deck leaves no
//! trace there, so which deck is heard comes from:
//! - the controller's faders and crossfader over MIDI, when they are known (same logic as
//!   the rekordbox source); or else
//! - the usual two-deck flow: the next track is loaded on the other deck while the current one
//!   plays, and the deck that played is reloaded only after the mix. So of the loaded tracks,
//!   the one loaded **earliest** is the one playing; the title follows when the next track is
//!   loaded after the mix.
//!
//! The database is opened read-only; Serato's account file (`user_tokens.json`) is never read.

use crate::live::mixer::{Assign, MidiMap, MixerState};
use crate::live::onair::OnAir;
use crate::{DeckStatus, LiveStatus, Source, Track};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
struct Loaded {
    deck: u8,
    artist: String,
    title: String,
    /// When Serato loaded it (Unix seconds); ties are broken by the row id.
    loaded_at: i64,
    id: i64,
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

    /// The deck heard now, from the mixer when it is known, else the earliest loaded track.
    fn pick(&mut self, now: Instant) -> Option<u8> {
        let mixer = self.mixer.lock().unwrap();
        let mixer_known = self._midi.is_some() && (mixer.faders.iter().any(Option::is_some) || mixer.crossfader.is_some());
        if mixer_known {
            let levels: Vec<(u8, f32)> = self.loaded.iter().map(|l| (l.deck, mixer.gain(l.deck))).collect();
            drop(mixer);
            return self.onair.update(&levels, now);
        }
        drop(mixer);
        earliest(&self.loaded)
    }
}

/// Tracks loaded on the decks in the newest history session.
fn open_entries(conn: &Connection) -> Result<Vec<Loaded>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, deck, artist, name, start_time FROM history_entry
         WHERE session_id = (SELECT MAX(id) FROM history_session) AND end_time = -1
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?))
    })?;
    let mut out: Vec<Loaded> = Vec::new();
    for row in rows {
        let (id, deck, artist, title, loaded_at) = row?;
        let Ok(deck) = deck.trim().parse::<u8>() else { continue };
        // One track per deck: a newer row for the same deck wins.
        out.retain(|l| l.deck != deck);
        out.push(Loaded { deck, artist: artist.trim().into(), title: title.trim().into(), loaded_at, id });
    }
    Ok(out)
}

fn earliest(loaded: &[Loaded]) -> Option<u8> {
    loaded.iter().min_by_key(|l| (l.loaded_at, l.id)).map(|l| l.deck)
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
            Ok(loaded) => {
                self.loaded = loaded;
                self.error = None;
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                return Err(e);
            }
        }
        self.on_air = self.pick(Instant::now()).or(self.on_air);
        Ok(self.on_air.and_then(|d| self.loaded.iter().find(|l| l.deck == d)).map(|l| Track {
            artist: l.artist.clone(),
            title: l.title.clone(),
            album: None,
            deck: Some(l.deck),
        }))
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
                // Serato does not say which decks play; the one on air is the best guess.
                playing: self.on_air == Some(l.deck),
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
        // Without the mixer, the track loaded earliest is the one playing.
        assert_eq!(earliest(&loaded), Some(1));
    }

    #[test]
    fn two_deck_flow_without_mixer() {
        // Deck 1 plays A; B is cued on deck 2 → A. After the mix, C goes on deck 1 → B.
        let c = library(&[("1", "", "A", 10, -1)]);
        assert_eq!(earliest(&open_entries(&c).unwrap()), Some(1));
        c.execute("INSERT INTO history_entry (session_id, deck, name, start_time) VALUES (1, '2', 'B', 20)", []).unwrap();
        assert_eq!(earliest(&open_entries(&c).unwrap()), Some(1));
        c.execute("UPDATE history_entry SET end_time = 30 WHERE name = 'A'", []).unwrap();
        c.execute("INSERT INTO history_entry (session_id, deck, name, start_time) VALUES (1, '1', 'C', 30)", []).unwrap();
        assert_eq!(earliest(&open_entries(&c).unwrap()), Some(2));
    }

    #[test]
    fn only_the_newest_session_counts() {
        let c = library(&[("1", "", "Old", 10, -1)]);
        c.execute("INSERT INTO history_session (start_time, end_time) VALUES (200, -1)", []).unwrap();
        assert!(open_entries(&c).unwrap().is_empty());
    }
}
