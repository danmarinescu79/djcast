//! Wires capture → processing → encoder(s) → servers (+ recording) with reconnect handling.
//!
//! One capture and one loudness processor feed one encoder per distinct format; every
//! output (server) has its own connection, reconnect timer, metadata and statistics,
//! so one server failing never affects the others.

use crate::audio::{self, Capture, Levels};
use crate::config::{Codec, Config, EncoderConfig, Protocol, ServerConfig};
use crate::dsp::{DspSnapshot, DspStats, Processor};
use crate::encoder;
use crate::icecast::{self, AudioInfo, SourceConnection};
use anyhow::{Result, bail};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// One destination server.
#[derive(Debug, Clone)]
pub struct Output {
    /// Shown in the UI and logs (e.g. the profile name).
    pub name: String,
    pub server: ServerConfig,
    pub codec: Codec,
    pub bitrate: u32,
}

/// State of one output.
#[derive(Debug, Clone, Default)]
pub struct OutputStatus {
    pub name: String,
    pub host: String,
    pub codec: Codec,
    pub bitrate: u32,
    pub connected: bool,
    pub bytes_sent: u64,
    pub connected_since: Option<Instant>,
    pub last_error: Option<String>,
}

/// Snapshot of the engine state, for CLI/GUI display. Top-level connection fields
/// summarize all outputs (connected = any connected, bytes = total).
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
    pub outputs: Vec<OutputStatus>,
}

#[derive(Clone)]
enum MetaMsg {
    Title(String),
    /// Re-send the current title (after a reconnect).
    Resend,
}

#[derive(Default)]
struct Shared {
    device: String,
    sample_rate: u32,
    title: Option<String>,
    recording: Option<PathBuf>,
}

/// A piece of encoded stream. `starts_stream` marks the first bytes of a new Ogg logical
/// stream (header pages), where a server that just connected may begin.
struct Chunk {
    bytes: Vec<u8>,
    starts_stream: bool,
}

pub struct Engine {
    capture: Option<Capture>,
    levels: Arc<Levels>,
    dsp_stats: Arc<DspStats>,
    shared: Arc<Mutex<Shared>>,
    /// Bumped on every title change, so the encoder thread sends in-band titles.
    title_gen: Arc<AtomicU64>,
    outputs: Vec<Arc<Mutex<OutputStatus>>>,
    stop: Arc<AtomicBool>,
    meta_txs: Vec<Sender<MetaMsg>>,
    threads: Vec<JoinHandle<()>>,
}

/// Encoder for one format (codec + bitrate) and the outputs it feeds.
struct EncoderGroup {
    enc: Box<dyn encoder::AudioEncoder>,
    sinks: Vec<Sender<Chunk>>,
    /// The recording is written from this group's stream.
    records: bool,
    /// Set by a network thread that just connected: begin a new Ogg logical stream.
    restart: Arc<AtomicBool>,
    /// The next emitted chunk begins a logical stream.
    starts_stream: bool,
}

impl Engine {
    /// Stream to the single server in `cfg.server` at `cfg.encoder.bitrate`.
    pub fn start(cfg: Config) -> Result<Self> {
        let output = Output {
            name: "main".into(),
            server: cfg.server.clone(),
            codec: cfg.encoder.codec,
            bitrate: cfg.encoder.bitrate,
        };
        Self::start_multi(cfg, vec![output])
    }

    /// Stream to several servers at once. `cfg.server` is ignored.
    pub fn start_multi(cfg: Config, outputs: Vec<Output>) -> Result<Self> {
        if outputs.is_empty() {
            bail!("no server to stream to");
        }
        if let Some(o) = outputs.iter().find(|o| o.server.protocol != Protocol::Icecast && !o.codec.works_with_shoutcast()) {
            bail!("{}: Shoutcast servers do not take Opus; choose MP3 or AAC, or an Icecast server", o.name);
        }
        let levels = Arc::new(Levels::default());
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));

        // ~2 s of 10 ms buffers between audio and encoder.
        let (pcm_tx, pcm_rx) = bounded::<Vec<f32>>(200);
        let capture = audio::start_capture(&cfg.audio, pcm_tx, levels.clone())?;
        let sample_rate = capture.sample_rate;
        {
            let mut sh = shared.lock().unwrap();
            sh.device = capture.device_name.clone();
            sh.sample_rate = sample_rate;
        }


        let dsp_stats = Arc::new(DspStats::default());
        let dsp = Processor::new(&cfg.processing, sample_rate, dsp_stats.clone());
        if let Some(t) = cfg.processing.target() {
            log::info!(
                "loudness processing: {:?}, target {t:.1} LUFS, ceiling {:.1} dBFS",
                cfg.processing.preset,
                cfg.processing.ceiling_db
            );
        }

        let title_gen = Arc::new(AtomicU64::new(0));
        let mut groups: Vec<((Codec, u32), EncoderGroup)> = Vec::new();
        let mut threads = Vec::new();
        let mut statuses = Vec::new();
        let mut meta_txs = Vec::new();

        for (i, out) in outputs.into_iter().enumerate() {
            // One encoder per distinct format.
            let key = (out.codec, out.bitrate);
            let gi = match groups.iter().position(|(k, _)| *k == key) {
                Some(gi) => gi,
                None => {
                    let enc = encoder::create(&EncoderConfig { codec: out.codec, bitrate: out.bitrate }, sample_rate)
                        .map_err(|e| anyhow::anyhow!("{}: {e:#}", out.name))?;
                    let group = EncoderGroup {
                        enc,
                        sinks: Vec::new(),
                        records: false,
                        restart: Arc::new(AtomicBool::new(false)),
                        starts_stream: true,
                    };
                    groups.push((key, group));
                    groups.len() - 1
                }
            };
            let group = &mut groups[gi].1;
            if i == 0 {
                group.records = true; // record what the first (main) server gets
            }
            let content_type = group.enc.content_type();
            let in_band = group.enc.in_band_metadata();
            let restart = in_band.then(|| group.restart.clone());

            // Encoded bytes → network. Small buffer: a live stream should drop, not lag.
            let (net_tx, net_rx) = bounded::<Chunk>(64);
            group.sinks.push(net_tx);

            let status = Arc::new(Mutex::new(OutputStatus {
                name: out.name.clone(),
                host: format!("{}:{}", out.server.host, out.server.port),
                codec: out.codec,
                bitrate: out.bitrate,
                ..Default::default()
            }));
            let (meta_tx, meta_rx) = bounded::<MetaMsg>(16);
            let info = AudioInfo { sample_rate, bitrate: out.bitrate, channels: 2 };

            let net = {
                let (server, status, stop, meta_tx) = (out.server.clone(), status.clone(), stop.clone(), meta_tx.clone());
                thread::Builder::new()
                    .name(format!("network-{}", out.name))
                    .spawn(move || network_loop(&server, content_type, info, restart, net_rx, &status, &stop, &meta_tx))?
            };
            let meta = {
                let (server, status, shared) = (out.server.clone(), status.clone(), shared.clone());
                thread::Builder::new()
                    .name(format!("metadata-{}", out.name))
                    .spawn(move || metadata_loop(&server, in_band, meta_rx, &status, &shared))?
            };
            threads.push(net);
            threads.push(meta);
            statuses.push(status);
            meta_txs.push(meta_tx);
        }

        let groups: Vec<EncoderGroup> = groups.into_iter().map(|(_, g)| g).collect();
        let recording = if cfg.record.enabled {
            let ext = groups.iter().find(|g| g.records).map_or("mp3", |g| g.enc.extension());
            let (path, w) = open_recording(&cfg, ext)?;
            shared.lock().unwrap().recording = Some(path);
            Some(w)
        } else {
            None
        };
        let encoder_thread = {
            let (shared, title_gen) = (shared.clone(), title_gen.clone());
            thread::Builder::new()
                .name("encoder".into())
                .spawn(move || encoder_loop(sample_rate, dsp, groups, pcm_rx, recording, &shared, &title_gen))?
        };
        threads.insert(0, encoder_thread);

        Ok(Self {
            capture: Some(capture),
            levels,
            dsp_stats,
            shared,
            title_gen,
            outputs: statuses,
            stop,
            meta_txs,
            threads,
        })
    }

    /// Set the "now playing" title (sent to every server, re-sent after reconnects).
    pub fn set_title(&self, title: impl Into<String>) {
        let title = title.into();
        self.shared.lock().unwrap().title = Some(title.clone());
        self.title_gen.fetch_add(1, Ordering::SeqCst);
        for tx in &self.meta_txs {
            let _ = tx.try_send(MetaMsg::Title(title.clone()));
        }
    }

    pub fn levels(&self) -> (f32, f32) {
        self.levels.get()
    }

    /// The captured input channels are silent while another pair has sound.
    pub fn channel_hint(&self) -> Option<audio::ChannelHint> {
        self.levels.channel_hint()
    }

    /// Loudness and processing meters (input/output LUFS, AGC gain, limiter reduction).
    pub fn processing(&self) -> DspSnapshot {
        self.dsp_stats.snapshot()
    }

    pub fn status(&self) -> EngineStatus {
        let sh = self.shared.lock().unwrap();
        let outputs: Vec<OutputStatus> = self.outputs.iter().map(|o| o.lock().unwrap().clone()).collect();
        EngineStatus {
            device: sh.device.clone(),
            sample_rate: sh.sample_rate,
            connected: outputs.iter().any(|o| o.connected),
            bytes_sent: outputs.iter().map(|o| o.bytes_sent).sum(),
            connected_since: outputs.iter().filter_map(|o| o.connected_since).min(),
            title: sh.title.clone(),
            recording: sh.recording.clone(),
            last_error: outputs
                .iter()
                .find_map(|o| o.last_error.as_ref().map(|e| if self.outputs.len() > 1 { format!("{}: {e}", o.name) } else { e.clone() })),
            outputs,
        }
    }

    /// Stop capture, flush the encoders and close connections.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Dropping the capture closes the PCM channel, which ends the encoder, which
        // closes the network channels, which end the network threads.
        self.capture.take();
        self.meta_txs.clear();
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
    mut dsp: Processor,
    mut groups: Vec<EncoderGroup>,
    pcm_rx: Receiver<Vec<f32>>,
    mut rec: Option<BufWriter<File>>,
    shared: &Mutex<Shared>,
    title_gen: &AtomicU64,
) {
    fn emit(bytes: Vec<u8>, group: &mut EncoderGroup, rec: &mut Option<BufWriter<File>>) {
        if bytes.is_empty() {
            return;
        }
        let starts_stream = std::mem::take(&mut group.starts_stream);
        if group.records
            && let Some(w) = rec.as_mut()
            && let Err(e) = w.write_all(&bytes)
        {
            log::error!("recording write failed, recording stopped: {e}");
            *rec = None;
        }
        // If a server is slow/down, drop its audio rather than building up delay.
        for sink in &group.sinks {
            let _ = sink.try_send(Chunk { bytes: bytes.clone(), starts_stream });
        }
    }
    /// End the current logical stream of an in-band-metadata encoder; the next chunk starts one.
    fn end_stream(result: Result<Vec<u8>>, group: &mut EncoderGroup, rec: &mut Option<BufWriter<File>>) {
        match result {
            Ok(bytes) => emit(bytes, group, rec),
            Err(e) => log::error!("{e:#}"),
        }
        group.starts_stream = true;
    }
    let mut seen_gen = 0;
    let mut encode_all = |pcm: &[f32], groups: &mut Vec<EncoderGroup>, rec: &mut Option<BufWriter<File>>| {
        let gen_now = title_gen.load(Ordering::SeqCst);
        let title = if gen_now != seen_gen {
            seen_gen = gen_now;
            shared.lock().unwrap().title.clone()
        } else {
            None
        };
        for g in groups.iter_mut() {
            if g.enc.in_band_metadata() {
                if let Some(t) = &title {
                    let r = g.enc.set_title(t);
                    end_stream(r, g, rec);
                    g.restart.store(false, Ordering::SeqCst); // the new stream serves that too
                } else if g.restart.swap(false, Ordering::SeqCst) {
                    let r = g.enc.restart();
                    end_stream(r, g, rec);
                }
            }
            match g.enc.encode(pcm) {
                Ok(bytes) => emit(bytes, g, rec),
                Err(e) => log::error!("{e:#}"),
            }
        }
    };

    // WASAPI loopback delivers nothing while the output is silent, and Icecast drops a
    // source that sends no data for ~10 s. Keep a sample clock and pad gaps with silence.
    let mut clock = SilenceClock::new(sample_rate);
    loop {
        match pcm_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(mut pcm) => {
                clock.on_audio();
                dsp.process(&mut pcm);
                encode_all(&pcm, &mut groups, &mut rec);
            }
            Err(RecvTimeoutError::Timeout) => {
                let missing = clock.take_silence();
                if missing > 0 {
                    encode_all(&vec![0.0; missing * 2], &mut groups, &mut rec);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    for g in &mut groups {
        match g.enc.flush() {
            Ok(bytes) => emit(bytes, g, &mut rec),
            Err(e) => log::warn!("flushing the encoder: {e:#}"),
        }
    }
    if let Some(mut w) = rec {
        let _ = w.flush();
    }
}

/// `restart` is set for Ogg streams: after connecting, ask the encoder for a new logical
/// stream and send nothing until it begins (servers need the header pages first).
#[allow(clippy::too_many_arguments)]
fn network_loop(
    server: &ServerConfig,
    content_type: &str,
    info: AudioInfo,
    restart: Option<Arc<AtomicBool>>,
    rx: Receiver<Chunk>,
    status: &Mutex<OutputStatus>,
    stop: &AtomicBool,
    meta_tx: &Sender<MetaMsg>,
) {
    let name = status.lock().unwrap().name.clone();
    let delay = Duration::from_secs(server.reconnect_delay_secs.max(1));
    let mut conn: Option<SourceConnection> = None;
    let mut next_attempt = Instant::now();
    let mut wait_for_stream_start = false;

    loop {
        if conn.is_none() && Instant::now() >= next_attempt && !stop.load(Ordering::SeqCst) {
            match SourceConnection::connect(server, content_type, info) {
                Ok(c) => {
                    conn = Some(c);
                    if let Some(r) = &restart {
                        r.store(true, Ordering::SeqCst);
                        wait_for_stream_start = true;
                    }
                    let mut st = status.lock().unwrap();
                    st.connected = true;
                    st.connected_since = Some(Instant::now());
                    st.last_error = None;
                    drop(st);
                    let _ = meta_tx.try_send(MetaMsg::Resend);
                }
                Err(e) => {
                    log::warn!("[{name}] connect failed: {e:#}");
                    status.lock().unwrap().last_error = Some(format!("{e:#}"));
                    next_attempt = Instant::now() + delay;
                }
            }
        }

        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Chunk { bytes, starts_stream }) => {
                if wait_for_stream_start {
                    if !starts_stream {
                        continue;
                    }
                    wait_for_stream_start = false;
                }
                if let Some(c) = conn.as_mut() {
                    match c.send(&bytes) {
                        Ok(()) => status.lock().unwrap().bytes_sent += bytes.len() as u64,
                        Err(e) => {
                            log::warn!("[{name}] connection lost: {e}");
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

/// Sends titles through the server's admin API. With `in_band` (Ogg) the encoder carries
/// the titles inside the stream, and this only logs them.
fn metadata_loop(server: &ServerConfig, in_band: bool, rx: Receiver<MetaMsg>, status: &Mutex<OutputStatus>, shared: &Mutex<Shared>) {
    let name = status.lock().unwrap().name.clone();
    for msg in rx.iter() {
        let title = match msg {
            MetaMsg::Title(t) => t,
            MetaMsg::Resend => match shared.lock().unwrap().title.clone() {
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
        if in_band {
            log::info!("[{name}] now playing: {title}");
            continue;
        }
        match icecast::update_metadata(server, &title) {
            Ok(()) => log::info!("[{name}] now playing: {title}"),
            Err(e) => log::warn!("[{name}] metadata: {e:#}"),
        }
    }
}

fn open_recording(cfg: &Config, ext: &str) -> Result<(PathBuf, BufWriter<File>)> {
    let dir = if cfg.record.dir.as_os_str().is_empty() { PathBuf::from("recordings") } else { cfg.record.dir.clone() };
    std::fs::create_dir_all(&dir)?;
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = dir.join(format!("djcast-{ts}.{ext}"));
    let file = File::create(&path)?;
    log::info!("recording to {}", path.display());
    Ok((path, BufWriter::new(file)))
}
