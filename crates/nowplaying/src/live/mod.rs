//! Instant now-playing for rekordbox: deck contents from the UI, mixer state from MIDI.
//!
//! - A background thread reads rekordbox's window (UI Automation on Windows) twice a
//!   second and tracks, per deck, the loaded track and whether its time is running.
//! - An optional MIDI listener follows channel faders, crossfader and assign switches.
//! - [`onair::OnAir`] picks the deck the audience hears (hysteresis + confirmation).

pub mod decks;
pub mod mixer;
pub mod onair;
#[cfg(windows)]
mod uia;

use crate::{Source, Track};
use anyhow::{Result, anyhow};
use decks::DeckView;
use mixer::{Assign, MidiControl, MidiMap, MixerState};
use onair::OnAir;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SCAN_INTERVAL: Duration = Duration::from_millis(500);
/// A deck counts as playing if its time display changed this recently.
const PLAYING_WINDOW: Duration = Duration::from_millis(2500);
/// Report an error (so a fallback source can take over) after this long without a scan.
const STALE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct LiveConfig {
    /// MIDI input port name (substring). "auto" = first port containing "DDJ"; "" = no MIDI.
    pub midi_input: String,
    pub confirm: Duration,
    pub assign: [Assign; 4],
    pub map: MidiMap,
}

#[derive(Debug, Clone)]
struct DeckState {
    view: DeckView,
    last_tick: Option<Instant>,
    visible: bool,
    /// Playing state when the deck was last visible (2-deck layout hides decks 3/4 or 1/2).
    playing_when_hidden: bool,
}

#[derive(Default)]
struct Shared {
    decks: HashMap<u8, DeckState>,
    last_ok: Option<Instant>,
    error: Option<String>,
}

impl DeckState {
    fn playing(&self, now: Instant) -> bool {
        if self.visible {
            self.last_tick.is_some_and(|t| now.duration_since(t) < PLAYING_WINDOW)
        } else {
            self.playing_when_hidden
        }
    }
}

pub struct RekordboxLiveSource {
    shared: Arc<Mutex<Shared>>,
    mixer: Arc<Mutex<MixerState>>,
    _midi: Option<midir::MidiInputConnection<()>>,
    onair: OnAir,
}

impl RekordboxLiveSource {
    pub fn new(cfg: &LiveConfig) -> Result<Self> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        spawn_scanner(shared.clone())?;
        let mixer = Arc::new(Mutex::new(MixerState::new(cfg.assign)));
        let midi = open_midi(&cfg.midi_input, cfg.map.clone(), mixer.clone());
        Ok(Self { shared, mixer, _midi: midi, onair: OnAir::new(cfg.confirm) })
    }
}

impl RekordboxLiveSource {
    /// Structured deck/mixer state for a UI.
    pub fn status(&self) -> crate::LiveStatus {
        let now = Instant::now();
        let shared = self.shared.lock().unwrap();
        let mixer = self.mixer.lock().unwrap();
        let on_air = self.onair.current();
        let mut decks: Vec<crate::DeckStatus> = shared
            .decks
            .values()
            .map(|d| crate::DeckStatus {
                deck: d.view.deck,
                title: d.view.title.clone(),
                artist: d.view.artist.clone(),
                elapsed: d.view.elapsed.clone(),
                remaining: d.view.remaining.clone(),
                playing: d.playing(now),
                visible: d.visible,
                gain: mixer.gain(d.view.deck),
                on_air: on_air == Some(d.view.deck),
            })
            .collect();
        decks.sort_by_key(|d| d.deck);
        crate::LiveStatus {
            decks,
            on_air,
            faders: mixer.faders,
            crossfader: mixer.crossfader,
            midi: self._midi.is_some(),
            using_fallback: false,
            error: shared.error.clone(),
        }
    }

    /// One line per deck plus the mixer, for troubleshooting.
    pub fn debug_snapshot(&self) -> String {
        let now = Instant::now();
        let shared = self.shared.lock().unwrap();
        let mixer = self.mixer.lock().unwrap();
        let mut decks: Vec<_> = shared.decks.values().collect();
        decks.sort_by_key(|d| d.view.deck);
        let mut out = String::new();
        for d in decks {
            out.push_str(&format!(
                "  deck {} {:<7} {:<7} gain {:.2} | {} - {} [{}]\n",
                d.view.deck,
                if d.playing(now) { "PLAYING" } else { "stopped" },
                if d.visible { "visible" } else { "hidden" },
                mixer.gain(d.view.deck),
                d.view.artist,
                d.view.title,
                d.view.elapsed.as_deref().unwrap_or("--:--"),
            ));
        }
        let fmt = |v: Option<f32>| v.map_or("  ?".to_string(), |v| format!("{v:.2}"));
        out.push_str(&format!(
            "  mixer: faders {} {} {} {} | xfader {} | assign {:?} | on air: {:?}{}",
            fmt(mixer.faders[0]),
            fmt(mixer.faders[1]),
            fmt(mixer.faders[2]),
            fmt(mixer.faders[3]),
            fmt(mixer.crossfader),
            mixer.assign,
            self.onair.current(),
            shared.error.as_ref().map(|e| format!(" | error: {e}")).unwrap_or_default(),
        ));
        out
    }
}

impl Source for RekordboxLiveSource {
    fn name(&self) -> &'static str {
        "rekordbox-live"
    }

    fn debug_snapshot(&self) -> Option<String> {
        Some(RekordboxLiveSource::debug_snapshot(self))
    }

    fn live_status(&self) -> Option<crate::LiveStatus> {
        Some(self.status())
    }

    fn poll(&mut self) -> Result<Option<Track>> {
        let now = Instant::now();
        let shared = self.shared.lock().unwrap();
        let fresh = shared.last_ok.is_some_and(|t| now.duration_since(t) < STALE);
        if !fresh {
            return Err(anyhow!(shared.error.clone().unwrap_or_else(|| "waiting for rekordbox".into())));
        }
        let mixer = self.mixer.lock().unwrap();
        let levels: Vec<(u8, f32)> = shared
            .decks
            .values()
            .map(|d| (d.view.deck, if d.playing(now) { mixer.gain(d.view.deck) } else { 0.0 }))
            .collect();
        drop(mixer);
        let deck = self.onair.update(&levels, now);
        Ok(deck.and_then(|d| shared.decks.get(&d)).map(|d| Track {
            artist: d.view.artist.clone(),
            title: d.view.title.clone(),
            album: None,
            deck: Some(d.view.deck),
        }))
    }
}

/// Merge a fresh scan into the per-deck state.
fn merge(shared: &mut Shared, views: Vec<DeckView>, now: Instant) {
    for d in shared.decks.values_mut() {
        if d.visible && views.iter().all(|v| v.deck != d.view.deck) {
            d.playing_when_hidden = d.playing(now);
            d.visible = false;
        }
    }
    for v in views {
        match shared.decks.get_mut(&v.deck) {
            Some(d) => {
                if d.visible && d.view.elapsed.is_some() && d.view.elapsed != v.elapsed && d.view.title == v.title {
                    d.last_tick = Some(now);
                }
                d.visible = true;
                d.view = v;
            }
            None => {
                shared.decks.insert(
                    v.deck,
                    DeckState { view: v, last_tick: None, visible: true, playing_when_hidden: false },
                );
            }
        }
    }
}

#[cfg(windows)]
fn spawn_scanner(shared: Arc<Mutex<Shared>>) -> Result<()> {
    std::thread::Builder::new().name("rekordbox-ui".into()).spawn(move || {
        let mut scanner = match uia::Scanner::new() {
            Ok(s) => s,
            Err(e) => {
                shared.lock().unwrap().error = Some(format!("{e:#}"));
                return;
            }
        };
        loop {
            let result = scanner.scan();
            let now = Instant::now();
            {
                let mut s = shared.lock().unwrap();
                match result {
                    Ok(items) => {
                        let views = decks::parse_decks(&items);
                        if views.is_empty() {
                            s.error = Some("no decks found in the rekordbox window".into());
                        } else {
                            merge(&mut s, views, now);
                            s.last_ok = Some(now);
                            s.error = None;
                        }
                    }
                    Err(e) => s.error = Some(format!("{e:#}")),
                }
            }
            std::thread::sleep(SCAN_INTERVAL);
        }
    })?;
    Ok(())
}

#[cfg(not(windows))]
fn spawn_scanner(shared: Arc<Mutex<Shared>>) -> Result<()> {
    shared.lock().unwrap().error = Some("reading the rekordbox window is not supported on this OS yet".into());
    Ok(())
}

/// Names of the MIDI inputs currently available (for a settings picker).
pub fn midi_inputs() -> Vec<String> {
    let Ok(input) = midir::MidiInput::new("djcast-list") else {
        return Vec::new();
    };
    let mut names: Vec<String> = input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect();
    names.dedup();
    names
}

/// The MIDI input matching `wanted` ("auto" = first port with "DDJ" in its name).
fn find_port(input: &midir::MidiInput, wanted: &str) -> Option<midir::MidiInputPort> {
    let needle = if wanted.eq_ignore_ascii_case("auto") { "ddj".to_string() } else { wanted.to_lowercase() };
    input.ports().into_iter().find(|p| input.port_name(p).is_ok_and(|n| n.to_lowercase().contains(&needle)))
}

fn open_midi(wanted: &str, map: MidiMap, mixer: Arc<Mutex<MixerState>>) -> Option<midir::MidiInputConnection<()>> {
    let wanted = wanted.trim();
    if wanted.is_empty() || wanted.eq_ignore_ascii_case("none") {
        return None;
    }
    let input = midir::MidiInput::new("djcast").ok()?;
    let Some(port) = find_port(&input, wanted) else {
        log::info!("no MIDI input matching '{wanted}'; on-air detection uses deck play state only");
        return None;
    };
    let name = input.port_name(&port).unwrap_or_default();
    match input.connect(&port, "djcast-mixer", move |_, msg, _| mixer.lock().unwrap().apply(&map, msg), ()) {
        Ok(conn) => {
            log::info!("MIDI mixer input: {name}");
            Some(conn)
        }
        Err(e) => {
            log::warn!("could not open MIDI input '{name}': {e}");
            None
        }
    }
}

/// Listen on the MIDI input `wanted` until the user moves a fader (or `timeout` passes) and
/// return that control. Works while rekordbox has the port open (multi-client MIDI).
pub fn midi_learn(wanted: &str, timeout: Duration) -> Result<MidiControl> {
    let wanted = wanted.trim();
    if wanted.is_empty() || wanted.eq_ignore_ascii_case("none") {
        anyhow::bail!("choose a MIDI input first");
    }
    let input = midir::MidiInput::new("djcast-learn")?;
    let port = find_port(&input, wanted).ok_or_else(|| anyhow::anyhow!("no MIDI input matching \"{wanted}\""))?;
    let msgs: Arc<Mutex<Vec<[u8; 3]>>> = Arc::default();
    let sink = msgs.clone();
    let _conn = input
        .connect(&port, "djcast-learn", move |_, m, _| {
            if let [a, b, c] = *m {
                sink.lock().unwrap().push([a, b, c]);
            }
        }, ())
        .map_err(|e| anyhow::anyhow!("could not open the MIDI input: {e}"))?;
    let start = Instant::now();
    while start.elapsed() < timeout {
        std::thread::sleep(Duration::from_millis(100));
        if mixer::pick_control(&msgs.lock().unwrap()).is_some() {
            // Let the move finish so a 14-bit LSB is seen too.
            std::thread::sleep(Duration::from_millis(400));
            return Ok(mixer::pick_control(&msgs.lock().unwrap()).expect("still there"));
        }
    }
    anyhow::bail!("no fader movement seen — move the control from one end to the other")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(deck: u8, title: &str, elapsed: &str) -> DeckView {
        DeckView { deck, title: title.into(), artist: "A".into(), elapsed: Some(elapsed.into()), remaining: None }
    }

    #[test]
    fn ticking_time_means_playing_and_hidden_decks_keep_state() {
        let mut s = Shared::default();
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        merge(&mut s, vec![view(1, "One", "00:10"), view(2, "Two", "00:00")], at(0));
        merge(&mut s, vec![view(1, "One", "00:11"), view(2, "Two", "00:00")], at(1000));
        assert!(s.decks[&1].playing(at(1000)));
        assert!(!s.decks[&2].playing(at(1000)));
        // Left side switched to deck 3: deck 1 hidden but still playing.
        merge(&mut s, vec![view(3, "Three", "00:00"), view(2, "Two", "00:00")], at(1500));
        assert!(s.decks[&1].playing(at(9000)));
        // A newly loaded track on a deck is not "playing" just because the time changed.
        merge(&mut s, vec![view(3, "Other", "03:00"), view(2, "Two", "00:00")], at(2000));
        assert!(!s.decks[&3].playing(at(2000)));
    }
}
