use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use djcast_core::{Config, Engine, audio, icecast};
use djcast_nowplaying::output::NowPlayingFiles;
use djcast_nowplaying::rekordbox::{self, MasterDb, history};
use djcast_nowplaying::{NowPlayingConfig, Track};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

#[derive(Parser)]
#[command(version, about = "Stream your DJ set to Icecast with automatic now-playing")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List audio devices.
    Devices,
    /// Show live input levels for a device without streaming (Ctrl+C to stop).
    Meter {
        /// Device name (substring). Empty = default for the mode.
        #[arg(short, long, default_value = "")]
        device: String,
        /// Capture an input device instead of loopback of an output device.
        #[arg(long)]
        input: bool,
        /// Stop after this many seconds and print the peak level.
        #[arg(long)]
        seconds: Option<f64>,
    },
    /// Write a default config file.
    Init {
        #[arg(default_value = "djcast.toml")]
        path: PathBuf,
    },
    /// Start streaming. Type a line + Enter to set the title, `q` + Enter to stop.
    Stream {
        #[arg(short, long, default_value = "djcast.toml")]
        config: PathBuf,
        /// Initial now-playing title.
        #[arg(short, long)]
        title: Option<String>,
    },
    /// Send a single metadata update to the server.
    Meta {
        #[arg(short, long, default_value = "djcast.toml")]
        config: PathBuf,
        title: String,
    },
    /// rekordbox tools.
    Rekordbox {
        #[command(subcommand)]
        cmd: RekordboxCmd,
    },
}

#[derive(Subcommand)]
enum RekordboxCmd {
    /// Store the master.db key in your user config folder (see docs/rekordbox-key.md).
    SetKey { key: String },
    /// Show where master.db and the key file are.
    Info,
    /// Print the most recent tracks from the play history.
    History {
        #[arg(short = 'n', long, default_value_t = 10)]
        count: usize,
    },
    /// Print now-playing changes without streaming.
    Watch {
        #[arg(short, long, default_value = "djcast.toml")]
        config: PathBuf,
        /// Override the configured source: db, live or auto.
        #[arg(short, long)]
        source: Option<String>,
        /// Print deck and mixer state every second.
        #[arg(short, long)]
        verbose: bool,
    },
}

/// The whole config file: streaming settings plus `[nowplaying]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct AppConfig {
    #[serde(flatten)]
    stream: Config,
    nowplaying: NowPlayingConfig,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match Cli::parse().cmd {
        Cmd::Devices => devices(),
        Cmd::Meter { device, input, seconds } => meter_cmd(device, input, seconds),
        Cmd::Init { path } => init(&path),
        Cmd::Stream { config, title } => stream(&config, title),
        Cmd::Meta { config, title } => icecast::update_metadata(&load(&config)?.stream.server, &title),
        Cmd::Rekordbox { cmd } => rekordbox_cmd(cmd),
    }
}

fn devices() -> Result<()> {
    println!("{:<6} {:<6} DEVICE", "IN", "OUT");
    for d in audio::list_devices()? {
        let flag = |yes: bool, def: bool| match (yes, def) {
            (true, true) => "yes*",
            (true, false) => "yes",
            _ => "-",
        };
        println!("{:<6} {:<6} {}", flag(d.is_input, d.is_default_input), flag(d.is_output, d.is_default_output), d.name);
    }
    println!("\n* = default. OUT devices can be captured with mode = \"loopback\", IN devices with mode = \"input\".");
    Ok(())
}

fn meter_cmd(device: String, input: bool, seconds: Option<f64>) -> Result<()> {
    use djcast_core::config::{AudioConfig, CaptureMode};
    let cfg = AudioConfig {
        device,
        mode: if input { CaptureMode::Input } else { CaptureMode::Loopback },
        gain: 1.0,
    };
    let (tx, rx) = crossbeam_channel::bounded(200);
    let levels = Arc::new(audio::Levels::default());
    let cap = audio::start_capture(&cfg, tx, levels.clone())?;
    eprintln!("metering '{}' ({} Hz), Ctrl+C to stop", cap.device_name, cap.sample_rate);
    // Drain audio so the capture never stalls; count buffers to detect "no data at all".
    let buffers = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    {
        let b = buffers.clone();
        std::thread::spawn(move || {
            for _ in rx {
                b.fetch_add(1, Ordering::Relaxed);
            }
        });
    }
    let start = std::time::Instant::now();
    let mut max_peak = 0f32;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let (l, r) = levels.get();
        max_peak = max_peak.max(l).max(r);
        eprint!("\rL {} R {}  buffers {}\x1b[K", meter(l), meter(r), buffers.load(Ordering::Relaxed));
        if seconds.is_some_and(|s| start.elapsed().as_secs_f64() >= s) {
            break;
        }
    }
    let db = 20.0 * max_peak.max(1e-6).log10();
    eprintln!();
    println!(
        "peak {:.1} dBFS, {} buffers received{}",
        db,
        buffers.load(Ordering::Relaxed),
        if max_peak < 1e-4 { " — SILENCE" } else { "" }
    );
    Ok(())
}

fn init(path: &Path) -> Result<()> {
    if path.exists() {
        anyhow::bail!("{} already exists", path.display());
    }
    std::fs::write(path, toml::to_string_pretty(&AppConfig::default())?)?;
    println!("wrote {}", path.display());
    Ok(())
}

fn load(path: &Path) -> Result<AppConfig> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {} (create one with `djcast init`)", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn rekordbox_cmd(cmd: RekordboxCmd) -> Result<()> {
    match cmd {
        RekordboxCmd::SetKey { key } => {
            let path = rekordbox::save_key(&key)?;
            // Verify it actually opens the database.
            let mut db = MasterDb::new(rekordbox::find_master_db()?, key.trim().to_string());
            db.open().context("the key was saved but could not unlock master.db")?;
            println!("key saved to {} and verified", path.display());
        }
        RekordboxCmd::Info => {
            match rekordbox::find_master_db() {
                Ok(p) => println!("master.db: {}", p.display()),
                Err(e) => println!("master.db: not found ({e})"),
            }
            if let Some(p) = rekordbox::default_key_path() {
                println!("key file:  {} ({})", p.display(), if p.exists() { "present" } else { "missing" });
            }
        }
        RekordboxCmd::History { count } => {
            let mut db = MasterDb::new(rekordbox::find_master_db()?, rekordbox::load_key(None)?);
            let conn = db.open()?;
            for e in history::recent(&conn, count)?.into_iter().rev() {
                let t = Track { artist: e.artist, title: e.title, album: e.album, deck: None };
                println!("{}  {}", e.created_at, t.display());
            }
        }
        RekordboxCmd::Watch { config, source, verbose } => {
            use djcast_nowplaying::SourceKind;
            let mut np = if config.exists() { load(&config)?.nowplaying } else { NowPlayingConfig::default() };
            np.source = match source.as_deref() {
                Some("db") => SourceKind::RekordboxDb,
                Some("live") => SourceKind::RekordboxLive,
                Some("auto") => SourceKind::Auto,
                Some(other) => anyhow::bail!("unknown source '{other}' (use db, live or auto)"),
                None if np.source == SourceKind::None => SourceKind::Auto,
                None => np.source,
            };
            let (tx, rx) = mpsc::channel();
            spawn_nowplaying(&np, tx, verbose)?;
            eprintln!("watching now playing ({:?}), Ctrl+C to stop", np.source);
            for ev in rx {
                if let Event::Track(t) = ev {
                    let deck = t.deck.map(|d| format!("[deck {d}] ")).unwrap_or_default();
                    println!("{} now playing: {deck}{}", clock(), t.display());
                }
            }
        }
    }
    Ok(())
}

enum Event {
    Line(String),
    Track(Track),
}

/// Poll the configured source on a background thread; send a `Track` event on changes.
/// Remove `[...]` parts (elapsed times) so a snapshot compares equal while clocks run.
fn strip_brackets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Local-ish wall clock "HH:MM:SS" (UTC offset not applied; for relative timing).
fn clock() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{:02}:{:02}:{:02}", s / 3600 % 24, s / 60 % 60, s % 60)
}

fn spawn_nowplaying(cfg: &NowPlayingConfig, tx: mpsc::Sender<Event>, verbose: bool) -> Result<bool> {
    let Some(mut source) = djcast_nowplaying::create_source(cfg)? else {
        return Ok(false);
    };
    let interval = Duration::from_secs_f64(cfg.poll_interval_secs.max(0.5));
    log::info!("now playing source: {}", source.name());
    std::thread::Builder::new().name("nowplaying".into()).spawn(move || {
        let mut last: Option<Track> = None;
        let mut failing = false;
        let mut last_snapshot = String::new();
        loop {
            let result = source.poll();
            // Print the state only when something other than running clocks changed.
            if verbose && let Some(s) = source.debug_snapshot() {
                let key = strip_brackets(&s);
                if key != last_snapshot {
                    eprintln!("--- {}\n{s}", clock());
                    last_snapshot = key;
                }
            }
            match result {
                Ok(Some(t)) if Some(&t) != last.as_ref() => {
                    last = Some(t.clone());
                    if tx.send(Event::Track(t)).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    if !failing {
                        log::warn!("now playing ({}): {e:#}", source.name());
                    }
                    failing = true;
                    std::thread::sleep(interval);
                    continue;
                }
            }
            failing = false;
            std::thread::sleep(interval);
        }
    })?;
    Ok(true)
}

fn stream(config: &Path, title: Option<String>) -> Result<()> {
    let cfg = load(config)?;
    let files = match cfg.nowplaying.output_dir.trim() {
        "" => None,
        dir => Some(NowPlayingFiles::new(dir)?),
    };
    let engine = Engine::start(cfg.stream.clone())?;

    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || r.store(false, Ordering::SeqCst))?;
    }

    let (tx, rx) = mpsc::channel::<Event>();
    // stdin: each line is a new title; "q" quits.
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                if tx.send(Event::Line(line)).is_err() {
                    break;
                }
            }
        });
    }
    spawn_nowplaying(&cfg.nowplaying, tx.clone(), false)?;
    if let Some(t) = title {
        tx.send(Event::Line(t))?;
    }

    let set_track = |t: &Track| {
        engine.set_title(t.display());
        if let Some(f) = &files
            && let Err(e) = f.update(t)
        {
            log::warn!("now playing files: {e:#}");
        }
    };

    eprintln!("streaming — type a title + Enter to update now playing, `q` or `quit` + Enter (or Ctrl+C) to stop");
    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Event::Line(line)) if is_quit(&line) => break,
            Ok(Event::Line(line)) if !line.trim().is_empty() => {
                set_track(&Track { artist: String::new(), title: line.trim().into(), album: None, deck: None });
            }
            Ok(Event::Track(t)) => set_track(&t),
            _ => {}
        }
        print_status(&engine);
    }
    eprintln!("\nstopping…");
    engine.stop();
    if let Some(f) = &files {
        eprintln!("tracklist: {}", f.tracklist_path().display());
    }
    Ok(())
}

/// `q`, `qq`, `quit`, `exit` (any case) stop the stream instead of becoming a title.
fn is_quit(line: &str) -> bool {
    let l = line.trim().to_ascii_lowercase();
    matches!(l.as_str(), "quit" | "exit") || (!l.is_empty() && l.chars().all(|c| c == 'q'))
}

fn print_status(engine: &Engine) {
    let st = engine.status();
    let (l, r) = engine.levels();
    let state = if st.connected {
        let secs = st.connected_since.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        format!("LIVE {:02}:{:02}:{:02} {:>7} KB", secs / 3600, secs / 60 % 60, secs % 60, st.bytes_sent / 1024)
    } else {
        "OFFLINE (reconnecting)".to_string()
    };
    eprint!(
        "\r{state} | L {} R {} | {}\x1b[K",
        meter(l),
        meter(r),
        st.title.as_deref().unwrap_or("—")
    );
    let _ = std::io::stderr().flush();
}

fn meter(peak: f32) -> String {
    const W: usize = 12;
    // dBFS scale from -48 to 0.
    let db = 20.0 * peak.max(1e-6).log10();
    let n = (((db + 48.0) / 48.0).clamp(0.0, 1.0) * W as f32).round() as usize;
    let clip = if peak >= 0.999 { '!' } else { ' ' };
    format!("[{}{}]{clip}", "#".repeat(n), ".".repeat(W - n))
}
