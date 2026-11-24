//! Audio capture via cpal. Produces interleaved stereo f32 PCM in [-1, 1].

use crate::config::{AudioConfig, CaptureMode};
use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use crossbeam_channel::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// A device as shown to the user.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub is_input: bool,
    pub is_output: bool,
    pub is_default_input: bool,
    pub is_default_output: bool,
}

pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let def_in = host.default_input_device().map(|d| d.to_string());
    let def_out = host.default_output_device().map(|d| d.to_string());
    let mut out = Vec::new();
    for dev in host.devices().context("enumerating audio devices")? {
        let name = dev.to_string();
        out.push(DeviceInfo {
            is_input: dev.supports_input(),
            is_output: dev.supports_output(),
            is_default_input: def_in.as_deref() == Some(name.as_str()),
            is_default_output: def_out.as_deref() == Some(name.as_str()),
            name,
        });
    }
    Ok(out)
}

/// Peak levels of the last captured buffer, stored as f32 bits (lock-free for UI meters).
#[derive(Default)]
pub struct Levels {
    left: AtomicU32,
    right: AtomicU32,
}

impl Levels {
    pub fn get(&self) -> (f32, f32) {
        (
            f32::from_bits(self.left.load(Ordering::Relaxed)),
            f32::from_bits(self.right.load(Ordering::Relaxed)),
        )
    }
    fn set(&self, l: f32, r: f32) {
        self.left.store(l.to_bits(), Ordering::Relaxed);
        self.right.store(r.to_bits(), Ordering::Relaxed);
    }
}

/// A running capture. Dropping it stops the stream.
pub struct Capture {
    _stream: cpal::Stream,
    pub sample_rate: u32,
    pub device_name: String,
}

/// Open the configured device and start pushing stereo f32 chunks into `tx`.
pub fn start_capture(cfg: &AudioConfig, tx: Sender<Vec<f32>>, levels: Arc<Levels>) -> Result<Capture> {
    let host = cpal::default_host();
    let device = find_device(&host, cfg)?;
    let device_name = device.to_string();

    // In loopback mode we capture with the *output* format of the device.
    let supported = match cfg.mode {
        CaptureMode::Loopback => device.default_output_config(),
        CaptureMode::Input => device.default_input_config(),
    }
    .with_context(|| format!("no usable config for device '{device_name}'"))?;

    let sample_rate = supported.sample_rate();
    let channels = supported.channels() as usize;
    let stream_cfg = supported.config();
    let gain = cfg.gain;

    log::info!(
        "capture: '{device_name}' {:?} {} Hz, {} ch, {:?}",
        cfg.mode,
        sample_rate,
        channels,
        supported.sample_format()
    );

    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, stream_cfg, channels, gain, tx, levels)?,
        SampleFormat::F64 => build::<f64>(&device, stream_cfg, channels, gain, tx, levels)?,
        SampleFormat::I16 => build::<i16>(&device, stream_cfg, channels, gain, tx, levels)?,
        SampleFormat::I32 => build::<i32>(&device, stream_cfg, channels, gain, tx, levels)?,
        SampleFormat::U16 => build::<u16>(&device, stream_cfg, channels, gain, tx, levels)?,
        SampleFormat::U8 => build::<u8>(&device, stream_cfg, channels, gain, tx, levels)?,
        other => bail!("unsupported sample format {other:?}"),
    };
    stream.play().context("starting audio stream")?;

    Ok(Capture { _stream: stream, sample_rate, device_name })
}

fn find_device(host: &cpal::Host, cfg: &AudioConfig) -> Result<cpal::Device> {
    if cfg.device.trim().is_empty() {
        return match cfg.mode {
            CaptureMode::Loopback => host.default_output_device(),
            CaptureMode::Input => host.default_input_device(),
        }
        .ok_or_else(|| anyhow!("no default {:?} device", cfg.mode));
    }
    let needle = cfg.device.to_lowercase();
    let candidates = match cfg.mode {
        CaptureMode::Loopback => host.output_devices()?.collect::<Vec<_>>(),
        CaptureMode::Input => host.input_devices()?.collect::<Vec<_>>(),
    };
    candidates
        .into_iter()
        .find(|d| d.to_string().to_lowercase().contains(&needle))
        .ok_or_else(|| anyhow!("device matching '{}' not found (run `djcast devices`)", cfg.device))
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    gain: f32,
    tx: Sender<Vec<f32>>,
    levels: Arc<Levels>,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let stream = device.build_input_stream::<T, _, _>(
        config,
        move |data: &[T], _| {
            let stereo = to_stereo(data, channels, gain);
            let (mut pl, mut pr) = (0f32, 0f32);
            for f in stereo.as_chunks::<2>().0 {
                pl = pl.max(f[0].abs());
                pr = pr.max(f[1].abs());
            }
            levels.set(pl, pr);
            // Never block the audio thread: drop the chunk if the encoder is stalled.
            let _ = tx.try_send(stereo);
        },
        |err| match err.kind() {
            cpal::ErrorKind::Xrun => log::debug!("audio xrun"),
            _ => log::error!("audio stream error: {err}"),
        },
        None,
    )?;
    Ok(stream)
}

/// Convert an interleaved buffer with N channels into interleaved stereo f32.
fn to_stereo<T>(data: &[T], channels: usize, gain: f32) -> Vec<f32>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let frames = data.len() / channels.max(1);
    let mut out = Vec::with_capacity(frames * 2);
    for frame in data.chunks_exact(channels.max(1)) {
        let l = frame[0].to_sample::<f32>() * gain;
        let r = if channels > 1 { frame[1].to_sample::<f32>() * gain } else { l };
        out.push(l.clamp(-1.0, 1.0));
        out.push(r.clamp(-1.0, 1.0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_is_duplicated_and_gain_clamped() {
        let s = to_stereo(&[0.25f32, -0.8], 1, 2.0);
        assert_eq!(s, vec![0.5, 0.5, -1.0, -1.0]);
    }

    #[test]
    fn extra_channels_are_dropped() {
        let s = to_stereo(&[0.1f32, 0.2, 0.9, 0.9], 4, 1.0);
        assert_eq!(s, vec![0.1, 0.2]);
    }
}
