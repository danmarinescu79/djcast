use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub audio: AudioConfig,
    pub encoder: EncoderConfig,
    pub server: ServerConfig,
    pub record: RecordConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Device name (substring match, case-insensitive). Empty = system default.
    pub device: String,
    /// `loopback` captures what an output device plays (WASAPI loopback on Windows);
    /// `input` captures a recording device (line-in, mixer USB input, virtual cable).
    pub mode: CaptureMode,
    /// Linear gain applied before encoding (1.0 = unchanged).
    pub gain: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self { device: String::new(), mode: CaptureMode::Loopback, gain: 1.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMode {
    Loopback,
    Input,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EncoderConfig {
    pub codec: Codec,
    /// Bitrate in kbps (MP3: 32..=320).
    pub bitrate: u32,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self { codec: Codec::Mp3, bitrate: 192 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    Mp3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub mount: String,
    pub user: String,
    pub password: String,
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
            password: "hackme".into(),
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordConfig {
    pub enabled: bool,
    /// Folder for recordings; a timestamped file is created per session.
    pub dir: PathBuf,
}
