//! Developer probe: stream to several (local test) servers at once for a few seconds.
//! Usage: cargo run -p djcast-core --example multi_probe -- <port1> <port2> <bad_port>
use djcast_core::config::ServerConfig;
use djcast_core::{Config, Engine, Output};

fn main() -> anyhow::Result<()> {
    env_logger_lite();
    let ports: Vec<u16> = std::env::args().skip(1).map(|p| p.parse().unwrap()).collect();
    let out = |name: &str, port: u16, bitrate: u32| Output {
        name: name.into(),
        server: ServerConfig { host: "127.0.0.1".into(), port, reconnect_delay_secs: 1, ..Default::default() },
        bitrate,
    };
    let mut cfg = Config::default();
    cfg.audio.device = "Realtek Digital".into();
    let engine = Engine::start_multi(cfg, vec![out("main", ports[0], 192), out("backup", ports[1], 128), out("dead", ports[2], 192)])?;
    engine.set_title("Multi Test - Song");
    std::thread::sleep(std::time::Duration::from_secs(6));
    for o in engine.status().outputs {
        println!("{:<7} connected={} bytes={} error={:?}", o.name, o.connected, o.bytes_sent, o.last_error);
    }
    engine.stop();
    Ok(())
}

fn env_logger_lite() {}
