use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use djcast_core::{Config, Engine, audio, icecast};
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
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match Cli::parse().cmd {
        Cmd::Devices => devices(),
        Cmd::Init { path } => init(&path),
        Cmd::Stream { config, title } => stream(&config, title),
        Cmd::Meta { config, title } => icecast::update_metadata(&load(&config)?.server, &title),
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
    std::fs::write(path, toml::to_string_pretty(&Config::default())?)?;
    println!("wrote {}", path.display());
    Ok(())
}

fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {} (create one with `djcast init`)", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn stream(config: &Path, title: Option<String>) -> Result<()> {
    let cfg = load(config)?;
    let engine = Engine::start(cfg)?;
    if let Some(t) = title {
        engine.set_title(t);
    }

    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || r.store(false, Ordering::SeqCst))?;
    }

    // stdin: each line is a new title; "q" quits.
    let (line_tx, line_rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });

    eprintln!("streaming — type a title + Enter to update now playing, `q` + Enter (or Ctrl+C) to stop");
    while running.load(Ordering::SeqCst) {
        match line_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(line) if line.trim().eq_ignore_ascii_case("q") => break,
            Ok(line) if !line.trim().is_empty() => engine.set_title(line.trim()),
            _ => {}
        }
        print_status(&engine);
    }
    eprintln!("\nstopping…");
    engine.stop();
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
