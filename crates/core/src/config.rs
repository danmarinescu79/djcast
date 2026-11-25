use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub audio: AudioConfig,
    /// Loudness processing (AGC + limiter) before encoding.
    pub processing: crate::dsp::ProcessingConfig,
    pub encoder: EncoderConfig,
    pub server: ServerConfig,
    pub record: RecordConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Device name (substring match, case-insensitive). Empty = system default.
    /// With `mode = "process"`: the program's executable name (empty = "rekordbox").
    pub device: String,
    /// `loopback` captures what an output device plays (WASAPI loopback on Windows);
    /// `input` captures a recording device (line-in, mixer USB input, virtual cable);
    /// `process` captures one program only (Windows 10 2004+; the program must play through
    /// Windows audio, not ASIO).
    pub mode: CaptureMode,
    /// Linear gain applied before encoding (1.0 = unchanged).
    pub gain: f32,
    /// With `mode = "input"`: first channel (1-based) of the stereo pair to capture, for
    /// devices with more than two inputs (the DDJ-1000 sends its master on 11–12).
    /// 0 = automatic: a known pair for known devices, else 1–2.
    pub first_channel: u16,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self { device: String::new(), mode: CaptureMode::Loopback, gain: 1.0, first_channel: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMode {
    Loopback,
    Input,
    Process,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EncoderConfig {
    pub codec: Codec,
    /// Bitrate in kbps (MP3: 32..=320, AAC: 96..=192, Opus: 6..=510).
    pub bitrate: u32,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self { codec: Codec::Mp3, bitrate: 192 }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    #[default]
    Mp3,
    /// AAC-LC in ADTS, from the operating system's encoder.
    Aac,
    /// Opus in Ogg.
    Opus,
}

impl Codec {
    /// MIME type of the stream.
    pub fn content_type(self) -> &'static str {
        match self {
            Codec::Mp3 => "audio/mpeg",
            Codec::Aac => "audio/aac",
            Codec::Opus => "audio/ogg",
        }
    }

    /// Shoutcast servers take MP3 and AAC only.
    pub fn works_with_shoutcast(self) -> bool {
        !matches!(self, Codec::Opus)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    /// Icecast mount point (not used by Shoutcast).
    pub mount: String,
    /// Icecast user (Shoutcast has none).
    pub user: String,
    /// Icecast over TLS (HTTPS). The certificate is checked against the system's trusted roots.
    pub tls: bool,
    pub password: String,
    /// Shoutcast v2 stream ID (sid), usually 1.
    pub stream_id: u32,
    /// Station info sent as ice-* / icy-* headers.
    pub name: String,
    pub description: String,
    pub genre: String,
    pub url: String,
    pub public: bool,
    /// Seconds to wait between reconnect attempts.
    pub reconnect_delay_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            protocol: Protocol::Icecast,
            host: "localhost".into(),
            port: 8000,
            mount: "/live".into(),
            user: "source".into(),
            tls: false,
            password: "hackme".into(),
            stream_id: 1,
            name: "My DJ Stream".into(),
            description: String::new(),
            genre: "Electronic".into(),
            url: String::new(),
            public: false,
            reconnect_delay_secs: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// Icecast 2.4+ (HTTP PUT, falls back to the legacy SOURCE method).
    Icecast,
    /// Shoutcast v1 (source on port + 1, password only).
    Shoutcast1,
    /// Shoutcast v2 / DNAS 2 in legacy source mode (password + stream ID).
    Shoutcast2,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordConfig {
    pub enabled: bool,
    /// Folder for recordings; a timestamped file is created per session.
    pub dir: PathBuf,
}
