//! djcast-core: capture audio, encode it and push it to a streaming server.
//!
//! Data flow:
//! ```text
//! cpal capture thread ──PCM f32 stereo──> encoder thread ──bytes──┬─> server sink (reconnecting)
//!                                                                  └─> local recording (optional)
//! ```

mod aac;
pub mod audio;
pub mod config;
pub mod dsp;
pub mod encoder;
pub mod engine;
pub mod icecast;
pub mod multiband;
mod ogg_opus;
#[cfg(windows)]
mod process_capture;

pub use config::Config;
pub use crossbeam_channel;
pub use engine::{Engine, EngineStatus, Output, OutputStatus};
