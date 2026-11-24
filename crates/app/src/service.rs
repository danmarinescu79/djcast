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
    pub codec: djcast_core::config::Codec,
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
    /// "hh:mm:ss" since going live.
    pub time: String,
    /// "Artist - Title".
    pub title: String,
    #[serde(skip)]
    pub secs: u64,
    #[serde(skip)]
    pub artist: String,
    #[serde(skip)]
    pub track_title: String,
}

impl TracklistEntry {
    fn new(secs: u64, t: &Track) -> Self {
        Self {
            time: format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60),
            title: t.display(),
            secs,
            artist: t.artist.clone(),
            track_title: t.title.clone(),
        }
    }
}

/// State shared between the station and the now-playing poller.
#[derive(Clone)]
struct Ctx {
    config: Arc<Mutex<AppConfig>>,
    engine: Arc<Mutex<Option<Engine>>>,
    session_start: Arc<Mutex<Option<std::time::Instant>>>,
    files: Arc<Mutex<Option<NowPlayingFiles>>>,
    np: Arc<Mutex<NowPlayingShared>>,
    recording: Arc<Mutex<Option<std::path::PathBuf>>>,
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
    ctx: Ctx,
    monitor: Mutex<Option<Monitor>>,
    monitor_error: Mutex<Option<String>>,
    np_stop: Mutex<Arc<AtomicBool>>,
    np_source_name: Mutex<String>,
}

impl Station {
    pub fn new(config: AppConfig) -> Arc<Self> {
        let station = Arc::new(Self {
            ctx: Ctx {
                config: Arc::new(Mutex::new(config)),
                engine: Arc::new(Mutex::new(None)),
                session_start: Arc::new(Mutex::new(None)),
                files: Arc::new(Mutex::new(None)),
                np: Arc::new(Mutex::new(NowPlayingShared::default())),
                recording: Arc::new(Mutex::new(None)),
            },
            monitor: Mutex::new(None),
            monitor_error: Mutex::new(None),
            np_stop: Mutex::new(Arc::new(AtomicBool::new(false))),
            np_source_name: Mutex::new("none".into()),
        });
        station.restart_monitor();
        station.restart_nowplaying();
        station
    }

    pub fn config(&self) -> AppConfig {
        self.ctx.config.lock().unwrap().clone()
    }

    /// Apply new settings. Takes effect immediately for meters and now playing;
    /// streaming settings apply on the next Go Live.
    pub fn set_config(&self, cfg: AppConfig) {
        *self.ctx.config.lock().unwrap() = cfg;
        if !self.is_live() {
            self.restart_monitor();
        }
        self.restart_nowplaying();
    }

    pub fn is_live(&self) -> bool {
        self.ctx.engine.lock().unwrap().is_some()
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
        let current = self.ctx.np.lock().unwrap().track.clone();
        if let Some(t) = &current {
            engine.set_title(t.display());
            if let Some(f) = &files {
                let _ = f.update(t);
            }
        }
        {
            let mut np = self.ctx.np.lock().unwrap();
            np.tracklist.clear();
            if let Some(t) = &current {
                np.tracklist.push(TracklistEntry::new(0, t));
            }
        }
        *self.ctx.recording.lock().unwrap() = engine.status().recording;
        *self.ctx.files.lock().unwrap() = files;
        *self.ctx.session_start.lock().unwrap() = Some(std::time::Instant::now());
        *self.ctx.engine.lock().unwrap() = Some(engine);
        write_cue(&self.ctx);
        if cfg.share.on_live && cfg.share.discord_enabled() {
            let vars = message_vars(&self.ctx, &cfg);
            crate::share::post_discord_async(cfg.share.discord_webhook.clone(), crate::share::render(&cfg.share.live_message, &vars));
        }
        Ok(())
    }

    pub fn stop(&self) {
        let engine = self.ctx.engine.lock().unwrap().take();
        if let Some(e) = engine {
            e.stop();
        }
        write_cue(&self.ctx);
        let cfg = self.config();
        if cfg.share.on_stop && cfg.share.discord_enabled() {
            let vars = message_vars(&self.ctx, &cfg);
            crate::share::post_discord_async(cfg.share.discord_webhook.clone(), crate::share::render(&cfg.share.stop_message, &vars));
        }
        self.ctx.files.lock().unwrap().take();
        self.ctx.session_start.lock().unwrap().take();
        self.ctx.recording.lock().unwrap().take();
        self.restart_monitor();
    }

    /// Send a test message with the given settings (blocking).
    pub fn test_discord(&self, share: &crate::share::ShareConfig) -> Result<()> {
        let mut cfg = self.config();
        cfg.share = share.clone();
        let vars = message_vars(&self.ctx, &cfg);
        let text = format!("✅ djcast test message\n{}", crate::share::render(&share.live_message, &vars));
        crate::share::post_discord(&share.discord_webhook, &text)
    }

    /// The session tracklist as text ("hh:mm:ss Artist - Title" per line).
    pub fn tracklist_text(&self) -> String {
        tracklist_text(&self.ctx)
    }

    /// Send a title by hand (overrides detection until the next detected change).
    pub fn set_manual_title(&self, title: &str) {
        let t = Track { artist: String::new(), title: title.trim().into(), album: None, deck: None };
        on_track(&self.ctx, &t);
    }

    pub fn status(&self) -> StationStatus {
        let cfg = self.ctx.config.lock().unwrap().clone();
        let engine = self.ctx.engine.lock().unwrap();
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
        let np = self.ctx.np.lock().unwrap();
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
                    codec: o.codec,
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
                self.ctx.np.lock().unwrap().error = Some(format!("{e:#}"));
                return;
            }
        };
        *self.np_source_name.lock().unwrap() = source.name().into();
        let interval = Duration::from_secs_f64(cfg.poll_interval_secs.max(0.5));
        let ctx = self.ctx.clone();
        let _ = std::thread::Builder::new().name("nowplaying".into()).spawn(move || {
            let mut last: Option<Track> = None;
            while !stop.load(Ordering::SeqCst) {
                let result = source.poll();
                let status = source.live_status();
                {
                    let mut s = ctx.np.lock().unwrap();
                    s.status = status;
                    s.error = result.as_ref().err().map(|e| format!("{e:#}"));
                }
                if let Ok(Some(t)) = result
                    && Some(&t) != last.as_ref()
                {
                    last = Some(t.clone());
                    on_track(&ctx, &t);
                }
                std::thread::sleep(interval);
            }
        });
    }

}

fn on_track(ctx: &Ctx, t: &Track) {
    let live = match ctx.engine.lock().unwrap().as_ref() {
        Some(e) => {
            e.set_title(t.display());
            true
        }
        None => false,
    };
    if let Some(f) = ctx.files.lock().unwrap().as_ref()
        && let Err(e) = f.update(t)
    {
        log::warn!("now playing files: {e:#}");
    }
    {
        let mut s = ctx.np.lock().unwrap();
        s.track = Some(t.clone());
        if let Some(start) = *ctx.session_start.lock().unwrap() {
            s.tracklist.push(TracklistEntry::new(start.elapsed().as_secs(), t));
        }
    }
    if live {
        write_cue(ctx);
        let cfg = ctx.config.lock().unwrap().clone();
        if cfg.share.on_track && cfg.share.discord_enabled() {
            let vars = message_vars(ctx, &cfg);
            crate::share::post_discord_async(cfg.share.discord_webhook.clone(), crate::share::render(&cfg.share.track_message, &vars));
        }
    }
}

fn tracklist_text(ctx: &Ctx) -> String {
    ctx.np.lock().unwrap().tracklist.iter().map(|e| format!("{} {}", e.time, e.title)).collect::<Vec<_>>().join("\n")
}

/// Public listen address: the configured one, else derived from the main server.
fn listen_url(cfg: &AppConfig) -> String {
    if !cfg.share.listen_url.trim().is_empty() {
        return cfg.share.listen_url.trim().to_string();
    }
    let Some(out) = cfg.live_outputs().into_iter().next() else { return String::new() };
    let s = out.server;
    match s.protocol {
        djcast_core::config::Protocol::Icecast => {
            let mount = if s.mount.starts_with('/') { s.mount.clone() } else { format!("/{}", s.mount) };
            format!("http://{}:{}{}", s.host, s.port, mount)
        }
        _ => format!("http://{}:{}/", s.host, s.port),
    }
}

fn message_vars(ctx: &Ctx, cfg: &AppConfig) -> crate::share::MessageVars {
    let main = cfg.live_outputs().into_iter().next();
    crate::share::MessageVars {
        station: main.map(|o| o.server.name).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "djcast".into()),
        url: listen_url(cfg),
        track: ctx.np.lock().unwrap().track.as_ref().map(|t| t.display()).unwrap_or_default(),
        tracklist: tracklist_text(ctx),
    }
}

/// Keep a CUE sheet next to the session recording, updated on every track change.
fn write_cue(ctx: &Ctx) {
    let Some(rec) = ctx.recording.lock().unwrap().clone() else { return };
    let tracks: Vec<crate::share::CueTrack> = ctx
        .np
        .lock()
        .unwrap()
        .tracklist
        .iter()
        .map(|e| crate::share::CueTrack { secs: e.secs, artist: e.artist.clone(), title: e.track_title.clone() })
        .collect();
    let title = rec.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "djcast session".into());
    if let Err(e) = std::fs::write(rec.with_extension("cue"), crate::share::cue_sheet(&rec, &title, &tracks)) {
        log::warn!("CUE sheet: {e}");
    }
}
