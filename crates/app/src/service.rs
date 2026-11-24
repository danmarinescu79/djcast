//! `Station`: everything the desktop app drives — streaming engine, an always-on
//! input monitor (so meters work before going live), and now-playing detection.

use crate::AppConfig;
use anyhow::Result;
use djcast_core::audio::{self, Capture, Levels};
use djcast_core::dsp::{DspSnapshot, DspStats, Processor};
use djcast_core::{Engine, EngineStatus};
use djcast_nowplaying::output::NowPlayingFiles;
use djcast_nowplaying::{LiveStatus, Track};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Everything a UI shows, refreshed many times per second.
#[derive(Debug, Clone, Serialize)]
pub struct StationStatus {
    pub live: bool,
    pub connected: bool,
    pub connected_secs: u64,
    pub bytes_sent: u64,
    pub device: String,
    pub sample_rate: u32,
    pub recording: bool,
    pub level_l: f32,
    pub level_r: f32,
    pub loudness: LoudnessStatus,
    pub track: Option<TrackStatus>,
    pub tracklist: Vec<TracklistEntry>,
    pub decks: Option<LiveStatus>,
    pub outputs: Vec<OutputView>,
    pub nowplaying_source: String,
    pub nowplaying_error: Option<String>,
    pub error: Option<String>,
}

/// One destination server while live.
#[derive(Debug, Clone, Serialize)]
pub struct OutputView {
    pub name: String,
    pub host: String,
    pub bitrate: u32,
    pub connected: bool,
    pub connected_secs: u64,
    pub bytes_sent: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoudnessStatus {
    pub preset: String,
    pub target_lufs: Option<f64>,
    pub in_lufs: Option<f32>,
    pub out_lufs: Option<f32>,
    pub agc_gain_db: f32,
    pub limiter_db: f32,
    /// Multiband gain reduction per band (low → high), when multiband is on.
    pub bands_db: Option<[f32; 4]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackStatus {
    pub artist: String,
    pub title: String,
    pub deck: Option<u8>,
    pub display: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TracklistEntry {
    pub time: String,
    pub title: String,
}

/// Captures and processes the input while offline, for meters only.
struct Monitor {
    _capture: Capture,
    levels: Arc<Levels>,
    stats: Arc<DspStats>,
}

impl Monitor {
    fn start(cfg: &AppConfig) -> Result<Self> {
        let (tx, rx) = djcast_core::crossbeam_channel::bounded::<Vec<f32>>(200);
        let levels = Arc::new(Levels::default());
        let capture = audio::start_capture(&cfg.stream.audio, tx, levels.clone())?;
        let stats = Arc::new(DspStats::default());
        let mut dsp = Processor::new(&cfg.stream.processing, capture.sample_rate, stats.clone());
        std::thread::Builder::new().name("monitor".into()).spawn(move || {
            for mut pcm in rx {
                dsp.process(&mut pcm);
            }
        })?;
        Ok(Self { _capture: capture, levels, stats })
    }
}

#[derive(Default)]
struct NowPlayingShared {
    track: Option<Track>,
    status: Option<LiveStatus>,
    error: Option<String>,
    tracklist: Vec<TracklistEntry>,
}

pub struct Station {
    config: Mutex<AppConfig>,
    engine: Arc<Mutex<Option<Engine>>>,
    session_start: Arc<Mutex<Option<std::time::Instant>>>,
    files: Arc<Mutex<Option<NowPlayingFiles>>>,
    monitor: Mutex<Option<Monitor>>,
    monitor_error: Mutex<Option<String>>,
    np: Arc<Mutex<NowPlayingShared>>,
    np_stop: Mutex<Arc<AtomicBool>>,
    np_source_name: Mutex<String>,
}

impl Station {
    pub fn new(config: AppConfig) -> Arc<Self> {
        let station = Arc::new(Self {
            config: Mutex::new(config),
            engine: Arc::new(Mutex::new(None)),
            session_start: Arc::new(Mutex::new(None)),
            files: Arc::new(Mutex::new(None)),
            monitor: Mutex::new(None),
            monitor_error: Mutex::new(None),
            np: Arc::new(Mutex::new(NowPlayingShared::default())),
            np_stop: Mutex::new(Arc::new(AtomicBool::new(false))),
            np_source_name: Mutex::new("none".into()),
        });
        station.restart_monitor();
        station.restart_nowplaying();
        station
    }

    pub fn config(&self) -> AppConfig {
        self.config.lock().unwrap().clone()
    }

    /// Apply new settings. Takes effect immediately for meters and now playing;
    /// streaming settings apply on the next Go Live.
    pub fn set_config(&self, cfg: AppConfig) {
        *self.config.lock().unwrap() = cfg;
        if !self.is_live() {
            self.restart_monitor();
        }
        self.restart_nowplaying();
    }

    pub fn is_live(&self) -> bool {
        self.engine.lock().unwrap().is_some()
    }

    pub fn go_live(&self) -> Result<()> {
        if self.is_live() {
            return Ok(());
        }
        let cfg = self.config();
        // Release the monitor's capture first; the engine opens its own.
        self.monitor.lock().unwrap().take();
        let engine = match Engine::start_multi(cfg.stream.clone(), cfg.live_outputs()) {
            Ok(e) => e,
            Err(e) => {
                self.restart_monitor();
                return Err(e);
            }
        };
        let files = match cfg.nowplaying.output_dir.trim() {
            "" => None,
            dir => NowPlayingFiles::new(dir).map_err(|e| log::warn!("now playing files: {e:#}")).ok(),
        };
        let current = self.np.lock().unwrap().track.clone();
        if let Some(t) = &current {
            engine.set_title(t.display());
            if let Some(f) = &files {
                let _ = f.update(t);
            }
        }
        {
            let mut np = self.np.lock().unwrap();
            np.tracklist.clear();
            if let Some(t) = current {
                np.tracklist.push(TracklistEntry { time: "00:00:00".into(), title: t.display() });
            }
        }
        *self.files.lock().unwrap() = files;
        *self.session_start.lock().unwrap() = Some(std::time::Instant::now());
        *self.engine.lock().unwrap() = Some(engine);
        Ok(())
    }

    pub fn stop(&self) {
        let engine = self.engine.lock().unwrap().take();
        if let Some(e) = engine {
            e.stop();
        }
        self.files.lock().unwrap().take();
        self.session_start.lock().unwrap().take();
        self.restart_monitor();
    }

    /// Send a title by hand (overrides detection until the next detected change).
    pub fn set_manual_title(&self, title: &str) {
        let t = Track { artist: String::new(), title: title.trim().into(), album: None, deck: None };
        let start = *self.session_start.lock().unwrap();
        on_track(&t, &self.engine, &self.files, &self.np, &start);
    }

    pub fn status(&self) -> StationStatus {
        let cfg = self.config.lock().unwrap().clone();
        let engine = self.engine.lock().unwrap();
        let (st, levels, dsp): (EngineStatus, (f32, f32), DspSnapshot) = match engine.as_ref() {
            Some(e) => (e.status(), e.levels(), e.processing()),
            None => {
                let mon = self.monitor.lock().unwrap();
                let (levels, dsp) = match mon.as_ref() {
                    Some(m) => (m.levels.get(), m.stats.snapshot()),
                    None => ((0.0, 0.0), DspStats::default().snapshot()),
                };
                (EngineStatus::default(), levels, dsp)
            }
        };
        let live = engine.is_some();
        drop(engine);
        let np = self.np.lock().unwrap();
        let finite = |v: f32| v.is_finite().then_some(v);
        StationStatus {
            live,
            connected: st.connected,
            connected_secs: st.connected_since.map(|t| t.elapsed().as_secs()).unwrap_or(0),
            bytes_sent: st.bytes_sent,
            device: if st.device.is_empty() { cfg.stream.audio.device.clone() } else { st.device.clone() },
            sample_rate: st.sample_rate,
            recording: st.recording.is_some(),
            level_l: levels.0,
            level_r: levels.1,
            loudness: LoudnessStatus {
                preset: format!("{:?}", cfg.stream.processing.preset).to_lowercase(),
                target_lufs: cfg.stream.processing.target(),
                in_lufs: finite(dsp.in_lufs),
                out_lufs: finite(dsp.out_lufs),
                agc_gain_db: dsp.agc_gain_db,
                limiter_db: dsp.limiter_db,
                bands_db: dsp.bands_db,
            },
            track: np.track.as_ref().map(|t| TrackStatus {
                artist: t.artist.clone(),
                title: t.title.clone(),
                deck: t.deck,
                display: t.display(),
            }),
            tracklist: np.tracklist.clone(),
            decks: np.status.clone(),
            outputs: st
                .outputs
                .iter()
                .map(|o| OutputView {
                    name: o.name.clone(),
                    host: o.host.clone(),
                    bitrate: o.bitrate,
                    connected: o.connected,
                    connected_secs: o.connected_since.map(|t| t.elapsed().as_secs()).unwrap_or(0),
                    bytes_sent: o.bytes_sent,
                    error: o.last_error.clone(),
                })
                .collect(),
            nowplaying_source: self.np_source_name.lock().unwrap().clone(),
            nowplaying_error: np.error.clone(),
            error: st.last_error.clone().or_else(|| self.monitor_error.lock().unwrap().clone()),
        }
    }

    fn restart_monitor(&self) {
        let cfg = self.config();
        let mut slot = self.monitor.lock().unwrap();
        slot.take();
        match Monitor::start(&cfg) {
            Ok(m) => {
                *slot = Some(m);
                *self.monitor_error.lock().unwrap() = None;
            }
            Err(e) => {
                log::warn!("input monitor: {e:#}");
                *self.monitor_error.lock().unwrap() = Some(format!("{e:#}"));
            }
        }
    }

    fn restart_nowplaying(&self) {
        // Stop the previous poller.
        let stop = Arc::new(AtomicBool::new(false));
        std::mem::replace(&mut *self.np_stop.lock().unwrap(), stop.clone()).store(true, Ordering::SeqCst);

        let cfg = self.config().nowplaying;
        let mut source = match djcast_nowplaying::create_source(&cfg) {
            Ok(Some(s)) => s,
            Ok(None) => {
                *self.np_source_name.lock().unwrap() = "none".into();
                return;
            }
            Err(e) => {
                self.np.lock().unwrap().error = Some(format!("{e:#}"));
                return;
            }
        };
        *self.np_source_name.lock().unwrap() = source.name().into();
        let interval = Duration::from_secs_f64(cfg.poll_interval_secs.max(0.5));
        let (engine, files, np, session) =
            (self.engine.clone(), self.files.clone(), self.np.clone(), self.session_start.clone());
        let _ = std::thread::Builder::new().name("nowplaying".into()).spawn(move || {
            let mut last: Option<Track> = None;
            while !stop.load(Ordering::SeqCst) {
                let result = source.poll();
                let status = source.live_status();
                {
                    let mut s = np.lock().unwrap();
                    s.status = status;
                    s.error = result.as_ref().err().map(|e| format!("{e:#}"));
                }
                if let Ok(Some(t)) = result
                    && Some(&t) != last.as_ref()
                {
                    last = Some(t.clone());
                    let start = *session.lock().unwrap();
                    on_track(&t, &engine, &files, &np, &start);
                }
                std::thread::sleep(interval);
            }
        });
    }

}

fn on_track(
    t: &Track,
    engine: &Mutex<Option<Engine>>,
    files: &Mutex<Option<NowPlayingFiles>>,
    np: &Mutex<NowPlayingShared>,
    session_start: &Option<std::time::Instant>,
) {
    if let Some(e) = engine.lock().unwrap().as_ref() {
        e.set_title(t.display());
    }
    if let Some(f) = files.lock().unwrap().as_ref()
        && let Err(e) = f.update(t)
    {
        log::warn!("now playing files: {e:#}");
    }
    let mut s = np.lock().unwrap();
    s.track = Some(t.clone());
    if let Some(start) = session_start {
        let secs = start.elapsed().as_secs();
        s.tracklist.push(TracklistEntry {
            time: format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60),
            title: t.display(),
        });
    }
}
