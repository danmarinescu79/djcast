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
    /// Print new tracks as rekordbox adds them to the history (no streaming).
    Watch {
        #[arg(short, long, default_value = "djcast.toml")]
        config: PathBuf,
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
        RekordboxCmd::Watch { config } => {
            let np = if config.exists() { load(&config)?.nowplaying } else { NowPlayingConfig::default() };
            let np = NowPlayingConfig { source: djcast_nowplaying::SourceKind::RekordboxDb, ..np };
            let (tx, rx) = mpsc::channel();
            spawn_nowplaying(&np, tx)?;
            eprintln!("watching rekordbox history (tracks appear ~1 min after they start), Ctrl+C to stop");
            for ev in rx {
                if let Event::Track(t) = ev {
                    println!("now playing: {}", t.display());
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
fn spawn_nowplaying(cfg: &NowPlayingConfig, tx: mpsc::Sender<Event>) -> Result<bool> {
    let Some(mut source) = djcast_nowplaying::create_source(cfg)? else {
        return Ok(false);
    };
    let interval = Duration::from_secs_f64(cfg.poll_interval_secs.max(0.5));
    log::info!("now playing source: {}", source.name());
    std::thread::Builder::new().name("nowplaying".into()).spawn(move || {
        let mut last: Option<Track> = None;
        let mut failing = false;
        loop {
            match source.poll() {
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
    spawn_nowplaying(&cfg.nowplaying, tx.clone())?;
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

    eprintln!("streaming — type a title + Enter to update now playing, `q` + Enter (or Ctrl+C) to stop");
    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Event::Line(line)) if line.trim().eq_ignore_ascii_case("q") => break,
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
