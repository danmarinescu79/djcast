//! "Setup report": what a tester needs to tell us about their DJ setup, collected in one go.
//! The text is shown to the user before anything is sent; it holds no passwords, server
//! addresses or track names.

use djcast_app::service::Station;
use djcast_core::audio::{self, PairLevel};
use djcast_core::config::CaptureMode;
use std::fmt::Write;
use std::time::Duration;

/// How long to listen to each pair of a multichannel input.
const LISTEN: Duration = Duration::from_secs(3);

/// Build the report. Takes a few seconds: it listens to the input while music plays.
pub fn build(station: &Station) -> String {
    let cfg = station.config();
    let audio_cfg = &cfg.stream.audio;
    let np = &cfg.nowplaying;
    let mut r = String::new();

    let _ = writeln!(r, "djcast {} · {} ({})", env!("CARGO_PKG_VERSION"), os_version(), std::env::consts::ARCH);

    // Audio
    let devices = audio::list_devices().unwrap_or_default();
    let device = if audio_cfg.device.trim().is_empty() { "(system default)" } else { audio_cfg.device.trim() };
    let mode = match audio_cfg.mode {
        CaptureMode::Loopback => "loopback",
        CaptureMode::Input => "input",
        CaptureMode::Process => "one program",
    };
    let _ = write!(r, "\nAudio capture: {mode} · \"{device}\"");
    let chosen = devices.iter().find(|d| d.is_input && d.name == audio_cfg.device);
    if audio_cfg.mode == CaptureMode::Input
        && let Some(d) = chosen.filter(|d| d.input_channels > 2)
    {
        let first = match audio_cfg.first_channel {
            0 => audio::auto_first_channel(&d.name, d.input_channels),
            n => n,
        };
        let how = if audio_cfg.first_channel == 0 { "automatic" } else { "chosen" };
        let _ = write!(r, " · channels {first}–{} ({how})", first + 1);
    }
    let _ = writeln!(r);
    let _ = writeln!(r, "Devices:");
    for d in &devices {
        let mut kinds = Vec::new();
        if d.is_input {
            let auto = audio::auto_first_channel(&d.name, d.input_channels);
            kinds.push(if auto > 1 {
                format!("input {} ch (known master {auto}–{})", d.input_channels, auto + 1)
            } else {
                format!("input {} ch", d.input_channels)
            });
        }
        if d.is_output {
            kinds.push("output".into());
        }
        let _ = writeln!(r, "  - {}: {}", d.name, kinds.join(", "));
    }
    let status = station.status();
    let db = |v: f32| if v > 0.0 { format!("{:.0} dBFS", 20.0 * v.log10()) } else { "silent".into() };
    let _ = writeln!(r, "Level now: L {} · R {}", db(status.level_l), db(status.level_r));
    if audio_cfg.mode == CaptureMode::Input
        && let Some(d) = chosen.filter(|d| d.input_channels > 2)
    {
        match audio::measure_input_pairs(&d.name, LISTEN) {
            Ok(levels) => {
                let _ = writeln!(r, "Levels per channel pair ({} s): {}", LISTEN.as_secs(), pairs(&levels));
            }
            Err(e) => {
                let _ = writeln!(r, "Levels per channel pair: could not measure ({e:#})");
            }
        }
    }
    if let Some(h) = status.channel_hint {
        let _ = writeln!(r, "Hint shown: channels {}–{} silent, {}–{} have sound", h.current, h.current + 1, h.suggested, h.suggested + 1);
    }

    // Now playing
    let _ = writeln!(r, "\nNow playing source: {}", status.nowplaying_source);
    match &status.decks {
        Some(live) => {
            let decks: Vec<String> = live
                .decks
                .iter()
                .map(|d| {
                    let state = if d.playing { "playing" } else { "stopped" };
                    let title = if d.title.trim().is_empty() { "no title" } else { "title read" };
                    format!("{} {state}, {title}", d.deck)
                })
                .collect();
            let window = if live.permission_needed {
                "permission missing (Accessibility)".to_string()
            } else if decks.is_empty() {
                "no decks found".to_string()
            } else {
                format!("decks {}", decks.join(" · "))
            };
            let _ = writeln!(r, "rekordbox window: {window}");
            let fader = |v: Option<f32>| v.map_or("–".to_string(), |v| format!("{v:.2}"));
            let _ = writeln!(
                r,
                "Controller MIDI: {} (input \"{}\", mapping {:?}) · faders {} · crossfader {} · on air: {}",
                if live.midi { "connected" } else { "not connected" },
                np.midi_input,
                np.midi_mapping,
                live.faders.iter().map(|f| fader(*f)).collect::<Vec<_>>().join(" "),
                fader(live.crossfader),
                live.on_air.map_or("–".to_string(), |d| format!("deck {d}")),
            );
        }
        None => {
            let _ = writeln!(r, "rekordbox window: not used by this source");
        }
    }
    let midi = djcast_nowplaying::live::midi_inputs();
    let _ = writeln!(r, "MIDI inputs: {}", if midi.is_empty() { "none".to_string() } else { midi.join(", ") });
    if let Some(e) = &status.nowplaying_error {
        let _ = writeln!(r, "Now playing error: {e}");
    }
    if let Some(e) = &status.error {
        let _ = writeln!(r, "Error: {e}");
    }
    r
}

fn pairs(levels: &[PairLevel]) -> String {
    let mut parts: Vec<String> = levels
        .iter()
        .map(|l| {
            let v = if l.peak_db.is_finite() { format!("{:.0}", l.peak_db) } else { "silent".into() };
            format!("{}–{} {v}", l.first, l.first + 1)
        })
        .collect();
    if let Some(best) = audio::best_pair(levels) {
        parts.push(format!("→ sound on {best}–{}", best + 1));
    } else {
        parts.push("→ no sound on any pair (was a track playing?)".into());
    }
    parts.join(" · ")
}

fn os_version() -> String {
    #[cfg(target_os = "macos")]
    let out = std::process::Command::new("sw_vers").arg("-productVersion").output();
    #[cfg(windows)]
    let out = std::process::Command::new("cmd").args(["/C", "ver"]).output();
    #[cfg(not(any(target_os = "macos", windows)))]
    let out: std::io::Result<std::process::Output> = Err(std::io::ErrorKind::Unsupported.into());
    let version = out.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    match std::env::consts::OS {
        "macos" => format!("macOS {version}"),
        _ if !version.is_empty() => version,
        os => os.to_string(),
    }
}

/// The GitHub "Setup report" form with the report and version filled in.
pub fn form_url(repo_url: &str, report: &str) -> String {
    let notes = format!("Setup report from djcast:\n\n```\n{}\n```\n", report.trim());
    format!(
        "{repo_url}/issues/new?template=setup_report.yml&version={}&notes={}",
        encode(env!("CARGO_PKG_VERSION")),
        encode(&notes)
    )
}

/// Percent-encode for a URL query value.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(encode("a b&c=d\n–"), "a%20b%26c%3Dd%0A%E2%80%93");
    }

    #[test]
    fn form_url_points_at_the_setup_report_form() {
        let url = form_url("https://github.com/o/r", "line");
        assert!(url.starts_with("https://github.com/o/r/issues/new?template=setup_report.yml&version="));
        assert!(url.contains("&notes=Setup%20report"));
    }
}
