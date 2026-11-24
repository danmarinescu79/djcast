//! Sharing a session: Discord webhook messages and a CUE sheet next to the recording.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// `[share]` settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShareConfig {
    /// Discord channel webhook URL. Kept in the OS credential store when available.
    pub discord_webhook: String,
    /// Public address listeners use; empty = derived from the main server.
    pub listen_url: String,
    pub on_live: bool,
    pub on_track: bool,
    pub on_stop: bool,
    /// Message templates: {station}, {url}, {track}, {tracklist}.
    pub live_message: String,
    pub track_message: String,
    pub stop_message: String,
}

impl Default for ShareConfig {
    fn default() -> Self {
        Self {
            discord_webhook: String::new(),
            listen_url: String::new(),
            on_live: true,
            on_track: false,
            on_stop: true,
            live_message: "🔴 **{station}** is live! {track}\n{url}".into(),
            track_message: "🎵 {track}".into(),
            stop_message: "⏹ **{station}** is off air. Tracklist:\n{tracklist}".into(),
        }
    }
}

impl ShareConfig {
    pub fn discord_enabled(&self) -> bool {
        self.discord_webhook.trim().starts_with("https://")
    }
}

/// Values for the message placeholders.
#[derive(Debug, Clone, Default)]
pub struct MessageVars {
    pub station: String,
    pub url: String,
    pub track: String,
    pub tracklist: String,
}

pub fn render(template: &str, v: &MessageVars) -> String {
    template
        .replace("{station}", &v.station)
        .replace("{url}", &v.url)
        .replace("{track}", &v.track)
        .replace("{tracklist}", &v.tracklist)
        .trim()
        .to_string()
}

/// Discord allows 2000 characters per message: split on line boundaries.
pub fn split_message(text: &str, limit: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        // A single over-long line is cut hard.
        let line: String = line.chars().take(limit).collect();
        if !cur.is_empty() && cur.chars().count() + 1 + line.chars().count() > limit {
            parts.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(&line);
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

/// Post a message to a Discord webhook (blocking; call from a background thread).
pub fn post_discord(webhook: &str, content: &str) -> anyhow::Result<()> {
    for part in split_message(content, 1900) {
        let body = serde_json::json!({ "content": part, "allowed_mentions": { "parse": [] } });
        ureq::post(webhook.trim())
            .header("User-Agent", djcast_core::icecast::USER_AGENT)
            .send_json(&body)
            .map_err(|e| anyhow::anyhow!("Discord webhook: {e}"))?;
    }
    Ok(())
}

/// Fire-and-forget post; errors go to the log.
pub fn post_discord_async(webhook: String, content: String) {
    if content.is_empty() {
        return;
    }
    let _ = std::thread::Builder::new().name("discord".into()).spawn(move || {
        if let Err(e) = post_discord(&webhook, &content) {
            log::warn!("{e:#}");
        }
    });
}

/// One track in a session, for CUE sheets.
#[derive(Debug, Clone)]
pub struct CueTrack {
    pub secs: u64,
    pub artist: String,
    pub title: String,
}

/// CUE sheet text for a recording, so players show the tracks of the set.
pub fn cue_sheet(recording: &Path, session_title: &str, tracks: &[CueTrack]) -> String {
    let esc = |s: &str| s.replace('"', "'");
    let file = recording.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = format!("TITLE \"{}\"\nFILE \"{}\" MP3\n", esc(session_title), esc(&file));
    for (i, t) in tracks.iter().enumerate() {
        out.push_str(&format!("  TRACK {:02} AUDIO\n", i + 1));
        out.push_str(&format!("    TITLE \"{}\"\n", esc(&t.title)));
        if !t.artist.is_empty() {
            out.push_str(&format!("    PERFORMER \"{}\"\n", esc(&t.artist)));
        }
        // mm:ss:ff (75 frames per second); minutes may exceed 99 for long sets.
        out.push_str(&format!("    INDEX 01 {:02}:{:02}:00\n", t.secs / 60, t.secs % 60));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_and_splitting() {
        let v = MessageVars { station: "Radio".into(), url: "http://x".into(), track: "A - B".into(), tracklist: String::new() };
        assert_eq!(render("🔴 **{station}** is live! {track}\n{url}", &v), "🔴 **Radio** is live! A - B\nhttp://x");
        let long: String = (0..300).map(|i| format!("00:00:{i:02} Artist - Track {i}\n")).collect();
        let parts = split_message(&long, 1900);
        assert!(parts.len() > 1);
        assert!(parts.iter().all(|p| p.chars().count() <= 1900));
        assert_eq!(parts.join("\n").lines().count(), 300);
    }

    #[test]
    fn cue_sheet_format() {
        let cue = cue_sheet(
            Path::new("recordings/djcast-1.mp3"),
            "My \"set\"",
            &[
                CueTrack { secs: 0, artist: "A".into(), title: "One".into() },
                CueTrack { secs: 6125, artist: String::new(), title: "Two".into() },
            ],
        );
        assert!(cue.starts_with("TITLE \"My 'set'\"\nFILE \"djcast-1.mp3\" MP3\n"));
        assert!(cue.contains("  TRACK 01 AUDIO\n    TITLE \"One\"\n    PERFORMER \"A\"\n    INDEX 01 00:00:00\n"));
        assert!(cue.contains("  TRACK 02 AUDIO\n    TITLE \"Two\"\n    INDEX 01 102:05:00\n"));
    }
}
