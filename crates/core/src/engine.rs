//! Wires capture → encoder → (server, recording) together with reconnect handling.

use crate::audio::{self, Capture, Levels};
use crate::config::Config;
use crate::encoder;
use crate::icecast::{self, AudioInfo, SourceConnection};
use anyhow::Result;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Snapshot of the engine state, for CLI/GUI display.
#[derive(Debug, Clone, Default)]
pub struct EngineStatus {
    pub device: String,
    pub sample_rate: u32,
    pub connected: bool,
    pub bytes_sent: u64,
    pub connected_since: Option<Instant>,
    pub title: Option<String>,
    pub recording: Option<PathBuf>,
    pub last_error: Option<String>,
}

enum MetaMsg {
    Title(String),
    /// Re-send the current title (after a reconnect).
    Resend,
}

pub struct Engine {
    capture: Option<Capture>,
    levels: Arc<Levels>,
    status: Arc<Mutex<EngineStatus>>,
    stop: Arc<AtomicBool>,
    meta_tx: Sender<MetaMsg>,
    threads: Vec<JoinHandle<()>>,
}

impl Engine {
    pub fn start(cfg: Config) -> Result<Self> {
        let levels = Arc::new(Levels::default());
        let status = Arc::new(Mutex::new(EngineStatus::default()));
        let stop = Arc::new(AtomicBool::new(false));

        // ~2 s of 10 ms buffers between audio and encoder.
        let (pcm_tx, pcm_rx) = bounded::<Vec<f32>>(200);
        let capture = audio::start_capture(&cfg.audio, pcm_tx, levels.clone())?;
        {
            let mut st = status.lock().unwrap();
            st.device = capture.device_name.clone();
            st.sample_rate = capture.sample_rate;
        }

        let enc = encoder::create(&cfg.encoder, capture.sample_rate)?;
        let info = AudioInfo { sample_rate: capture.sample_rate, bitrate: cfg.encoder.bitrate, channels: 2 };
        let content_type = enc.content_type();

        let recording = if cfg.record.enabled { Some(open_recording(&cfg)?) } else { None };
        if let Some((path, _)) = &recording {
            status.lock().unwrap().recording = Some(path.clone());
        }

        // Encoded bytes → network. Small buffer: a live stream should drop, not lag.
        let (net_tx, net_rx) = bounded::<Vec<u8>>(64);
        let (meta_tx, meta_rx) = bounded::<MetaMsg>(16);

        let encoder_thread = thread::Builder::new()
            .name("encoder".into())
            .spawn(move || encoder_loop(info.sample_rate, enc, pcm_rx, net_tx, recording.map(|(_, w)| w)))?;

        let net_thread = {
            let (cfg, status, stop, meta_tx) = (cfg.clone(), status.clone(), stop.clone(), meta_tx.clone());
            thread::Builder::new()
                .name("network".into())
                .spawn(move || network_loop(&cfg, content_type, info, net_rx, &status, &stop, &meta_tx))?
        };

        let meta_thread = {
            let (cfg, status) = (cfg.clone(), status.clone());
            thread::Builder::new().name("metadata".into()).spawn(move || metadata_loop(&cfg, meta_rx, &status))?
        };

        Ok(Self {
            capture: Some(capture),
            levels,
            status,
            stop,
            meta_tx,
            threads: vec![encoder_thread, net_thread, meta_thread],
        })
    }

    /// Set the "now playing" title (sent to the server, re-sent after reconnects).
    pub fn set_title(&self, title: impl Into<String>) {
        let _ = self.meta_tx.try_send(MetaMsg::Title(title.into()));
    }

    pub fn levels(&self) -> (f32, f32) {
        self.levels.get()
    }

    pub fn status(&self) -> EngineStatus {
        self.status.lock().unwrap().clone()
    }

    /// Stop capture, flush the encoder and close connections.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Dropping the capture closes the PCM channel, which ends the encoder,
        // which closes the network channel, which ends the network thread.
        self.capture.take();
        let (dummy_tx, _) = bounded(1);
        drop(std::mem::replace(&mut self.meta_tx, dummy_tx));
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if !self.threads.is_empty() {
            self.shutdown();
        }
    }
}

/// Detects gaps in capture (no buffers at all) and says how much silence to insert.
/// Only kicks in after a real gap, so device/wall clock drift never inserts silence mid-audio.
struct SilenceClock {
    last: Instant,
    rate: u64,
}

impl SilenceClock {
    const GAP: Duration = Duration::from_millis(200);

    fn new(rate: u32) -> Self {
        Self { last: Instant::now(), rate: rate as u64 }
    }

    /// Real audio arrived.
    fn on_audio(&mut self) {
        self.last = Instant::now();
    }

    /// Frames of silence to emit now (0 if no gap); advances the clock when > 0.
    fn take_silence(&mut self) -> usize {
        let gap = self.last.elapsed();
        if gap < Self::GAP {
            return 0;
        }
        self.last = Instant::now();
        (gap.as_millis() as u64 * self.rate / 1000) as usize
    }
}

fn encoder_loop(
    sample_rate: u32,
    mut enc: Box<dyn encoder::AudioEncoder>,
    pcm_rx: Receiver<Vec<f32>>,
    net_tx: Sender<Vec<u8>>,
    mut rec: Option<BufWriter<File>>,
) {
    let emit = |bytes: Vec<u8>, rec: &mut Option<BufWriter<File>>| {
        if bytes.is_empty() {
            return;
        }
        if let Some(w) = rec.as_mut()
            && let Err(e) = w.write_all(&bytes)
        {
            log::error!("recording write failed, recording stopped: {e}");
            *rec = None;
        }
        // If the network is slow/down, drop audio rather than building up delay.
        let _ = net_tx.try_send(bytes);
    };

    // WASAPI loopback delivers nothing while the output is silent, and Icecast drops a
    // source that sends no data for ~10 s. Keep a sample clock and pad gaps with silence.
    let mut clock = SilenceClock::new(sample_rate);
    loop {
        match pcm_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(pcm) => {
                clock.on_audio();
                match enc.encode(&pcm) {
                    Ok(bytes) => emit(bytes, &mut rec),
                    Err(e) => log::error!("{e:#}"),
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let missing = clock.take_silence();
                if missing > 0
                    && let Ok(bytes) = enc.encode(&vec![0.0; missing * 2])
                {
                    emit(bytes, &mut rec);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    if let Ok(bytes) = enc.flush() {
        emit(bytes, &mut rec);
    }
    if let Some(mut w) = rec {
        let _ = w.flush();
    }
}

fn network_loop(
    cfg: &Config,
    content_type: &str,
    info: AudioInfo,
    rx: Receiver<Vec<u8>>,
    status: &Mutex<EngineStatus>,
    stop: &AtomicBool,
    meta_tx: &Sender<MetaMsg>,
) {
    let delay = Duration::from_secs(cfg.server.reconnect_delay_secs.max(1));
    let mut conn: Option<SourceConnection> = None;
    let mut next_attempt = Instant::now();

    loop {
        if conn.is_none() && Instant::now() >= next_attempt && !stop.load(Ordering::SeqCst) {
            match SourceConnection::connect(&cfg.server, content_type, info) {
                Ok(c) => {
                    conn = Some(c);
                    let mut st = status.lock().unwrap();
                    st.connected = true;
                    st.connected_since = Some(Instant::now());
                    st.last_error = None;
                    drop(st);
                    let _ = meta_tx.try_send(MetaMsg::Resend);
                }
                Err(e) => {
                    log::warn!("connect failed: {e:#}");
                    status.lock().unwrap().last_error = Some(format!("{e:#}"));
                    next_attempt = Instant::now() + delay;
                }
            }
        }

        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(bytes) => {
                if let Some(c) = conn.as_mut() {
                    match c.send(&bytes) {
                        Ok(()) => status.lock().unwrap().bytes_sent += bytes.len() as u64,
                        Err(e) => {
                            log::warn!("connection lost: {e}");
                            conn = None;
                            let mut st = status.lock().unwrap();
                            st.connected = false;
                            st.connected_since = None;
                            st.last_error = Some(format!("connection lost: {e}"));
                            next_attempt = Instant::now() + delay;
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    status.lock().unwrap().connected = false;
}

fn metadata_loop(cfg: &Config, rx: Receiver<MetaMsg>, status: &Mutex<EngineStatus>) {
    for msg in rx.iter() {
        let title = match msg {
            MetaMsg::Title(t) => {
                status.lock().unwrap().title = Some(t.clone());
                t
            }
            MetaMsg::Resend => match status.lock().unwrap().title.clone() {
                Some(t) => {
                    // Give the server a moment to register the new source.
                    thread::sleep(Duration::from_millis(500));
                    t
                }
                None => continue,
            },
        };
        if !status.lock().unwrap().connected {
            continue; // will be re-sent on connect
        }
        match icecast::update_metadata(&cfg.server, &title) {
            Ok(()) => log::info!("now playing: {title}"),
            Err(e) => log::warn!("metadata: {e:#}"),
        }
    }
}

fn open_recording(cfg: &Config) -> Result<(PathBuf, BufWriter<File>)> {
    let dir = if cfg.record.dir.as_os_str().is_empty() { PathBuf::from("recordings") } else { cfg.record.dir.clone() };
    std::fs::create_dir_all(&dir)?;
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = dir.join(format!("djcast-{ts}.mp3"));
    let file = File::create(&path)?;
    log::info!("recording to {}", path.display());
    Ok((path, BufWriter::new(file)))
}
