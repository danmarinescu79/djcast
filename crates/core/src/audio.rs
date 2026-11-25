//! Audio capture via cpal. Produces interleaved stereo f32 PCM in [-1, 1].

use crate::config::{AudioConfig, CaptureMode};
use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use crossbeam_channel::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A device as shown to the user.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub is_input: bool,
    pub is_output: bool,
    pub is_default_input: bool,
    pub is_default_output: bool,
    /// Number of input channels (0 if not an input).
    pub input_channels: u16,
}

pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let def_in = host.default_input_device().map(|d| d.to_string());
    let def_out = host.default_output_device().map(|d| d.to_string());
    let mut out = Vec::new();
    for dev in host.devices().context("enumerating audio devices")? {
        let name = dev.to_string();
        let is_input = dev.supports_input();
        out.push(DeviceInfo {
            is_input,
            is_output: dev.supports_output(),
            is_default_input: def_in.as_deref() == Some(name.as_str()),
            is_default_output: def_out.as_deref() == Some(name.as_str()),
            input_channels: if is_input { dev.default_input_config().map(|c| c.channels()).unwrap_or(0) } else { 0 },
            name,
        });
    }
    Ok(out)
}

/// Controllers whose USB input carries the master mix on a known stereo pair
/// (name substring, first channel). Only add devices verified on real hardware.
const KNOWN_MASTER_PAIRS: &[(&str, u16)] = &[
    // Channels 1–8 are the deck/line inputs, 11–12 the master (verified on macOS).
    ("DDJ-1000", 11),
];

/// The first channel used when `first_channel = 0`: a known master pair, else 1.
pub fn auto_first_channel(device_name: &str, channels: u16) -> u16 {
    let name = device_name.to_lowercase();
    KNOWN_MASTER_PAIRS
        .iter()
        .find(|(n, first)| name.contains(&n.to_lowercase()) && *first < channels)
        .map_or(1, |(_, first)| *first)
}

/// Resolve the configured first channel against the device; returns a 0-based offset.
fn channel_offset(cfg: &AudioConfig, device_name: &str, channels: u16) -> Result<usize> {
    if cfg.mode != CaptureMode::Input {
        return Ok(0);
    }
    let first = if cfg.first_channel == 0 { auto_first_channel(device_name, channels) } else { cfg.first_channel };
    // A mono device has a single channel; otherwise the whole pair must exist.
    let last = if channels == 1 { 1 } else { first + 1 };
    if first == 0 || last > channels {
        bail!("'{device_name}' has {channels} input channel(s); channels {first}–{last} are not available");
    }
    Ok(first as usize - 1)
}

/// Stereo pairs tracked for the "wrong channels" hint (inputs with up to 32 channels).
const MAX_PAIRS: usize = 16;
/// A pair counts as carrying sound above this peak level (about -50 dBFS).
const SIGNAL_PEAK: f32 = 0.003;
/// The captured pair must be silent this long, while another one has sound, before we suggest it.
const HINT_AFTER: Duration = Duration::from_secs(10);
/// "Another pair has sound" means: within this window.
const RECENT: Duration = Duration::from_secs(2);
const NEVER: u64 = u64::MAX;

/// Peak levels of the last captured buffer, stored as f32 bits (lock-free for UI meters),
/// plus when each stereo pair of a multichannel input last carried sound.
pub struct Levels {
    left: AtomicU32,
    right: AtomicU32,
    start: Instant,
    /// Number of channels and the 0-based offset of the captured pair (0 = not multichannel).
    channels: AtomicU32,
    offset: AtomicU32,
    /// Milliseconds since `start` when each pair last had sound (`NEVER` if not yet).
    pair_signal_ms: [AtomicU64; MAX_PAIRS],
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            left: AtomicU32::new(0),
            right: AtomicU32::new(0),
            start: Instant::now(),
            channels: AtomicU32::new(0),
            offset: AtomicU32::new(0),
            pair_signal_ms: std::array::from_fn(|_| AtomicU64::new(NEVER)),
        }
    }
}

/// The captured channels are silent while another pair carries sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ChannelHint {
    /// First channel (1-based) being captured.
    pub current: u16,
    /// First channel of a pair that has sound.
    pub suggested: u16,
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

    fn set_layout(&self, ch: Channels) {
        self.channels.store(ch.count as u32, Ordering::Relaxed);
        self.offset.store(ch.offset as u32, Ordering::Relaxed);
    }

    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// Remember which pairs of a multichannel buffer carry sound.
    fn track_pairs<T>(&self, data: &[T], count: usize)
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        if count <= 2 || self.channels.load(Ordering::Relaxed) == 0 {
            return;
        }
        let pairs = (count / 2).min(MAX_PAIRS);
        let mut peaks = [0f32; MAX_PAIRS];
        for frame in data.chunks_exact(count) {
            for (p, peak) in peaks.iter_mut().enumerate().take(pairs) {
                let s = frame[2 * p].to_sample::<f32>().abs().max(frame[2 * p + 1].to_sample::<f32>().abs());
                *peak = peak.max(s);
            }
        }
        let now = self.now_ms();
        for (p, peak) in peaks.iter().enumerate().take(pairs) {
            if *peak > SIGNAL_PEAK {
                self.pair_signal_ms[p].store(now, Ordering::Relaxed);
            }
        }
    }

    /// A pair to suggest when the captured one has been silent for a while and another has sound.
    pub fn channel_hint(&self) -> Option<ChannelHint> {
        let count = self.channels.load(Ordering::Relaxed) as usize;
        if count <= 2 {
            return None;
        }
        let now = self.now_ms();
        let current = self.offset.load(Ordering::Relaxed) as usize / 2;
        let age = |p: usize| match self.pair_signal_ms[p].load(Ordering::Relaxed) {
            NEVER => now,
            t => now.saturating_sub(t),
        };
        if age(current) < HINT_AFTER.as_millis() as u64 {
            return None;
        }
        (0..(count / 2).min(MAX_PAIRS))
            .filter(|&p| p != current && age(p) < RECENT.as_millis() as u64)
            .min_by_key(|&p| age(p))
            .map(|p| ChannelHint { current: (2 * current + 1) as u16, suggested: (2 * p + 1) as u16 })
    }
}

/// A running capture. Dropping it stops the stream.
pub struct Capture {
    _stream: Option<cpal::Stream>,
    #[cfg(windows)]
    _process: Option<crate::process_capture::ProcessCapture>,
    pub sample_rate: u32,
    pub device_name: String,
}

/// Program captured by `mode = "process"` when no name is given.
pub const DEFAULT_PROCESS: &str = "rekordbox";

/// Open the configured device and start pushing stereo f32 chunks into `tx`.
pub fn start_capture(cfg: &AudioConfig, tx: Sender<Vec<f32>>, levels: Arc<Levels>) -> Result<Capture> {
    if cfg.mode == CaptureMode::Process {
        return start_process_capture(cfg, tx, levels);
    }
    let host = cpal::default_host();
    let device = find_device(&host, cfg)?;
    let device_name = device.to_string();

    // In loopback mode we capture with the *output* format of the device.
    let supported = match cfg.mode {
        CaptureMode::Loopback | CaptureMode::Process => device.default_output_config(),
        CaptureMode::Input => device.default_input_config(),
    }
    .with_context(|| format!("no usable config for device '{device_name}'"))?;

    let sample_rate = supported.sample_rate();
    let channels = supported.channels() as usize;
    let stream_cfg = supported.config();
    let gain = cfg.gain;
    let offset = channel_offset(cfg, &device_name, channels as u16)?;

    log::info!(
        "capture: '{device_name}' {:?} {} Hz, {} ch (using {}–{}), {:?}",
        cfg.mode,
        sample_rate,
        channels,
        offset + 1,
        (offset + 2).min(channels),
        supported.sample_format()
    );

    let ch = Channels { count: channels, offset };
    // Only an input's pairs can be chosen, so only inputs get the "wrong channels" hint.
    if cfg.mode == CaptureMode::Input {
        levels.set_layout(ch);
    }
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, stream_cfg, ch, gain, tx, levels)?,
        SampleFormat::F64 => build::<f64>(&device, stream_cfg, ch, gain, tx, levels)?,
        SampleFormat::I16 => build::<i16>(&device, stream_cfg, ch, gain, tx, levels)?,
        SampleFormat::I32 => build::<i32>(&device, stream_cfg, ch, gain, tx, levels)?,
        SampleFormat::U16 => build::<u16>(&device, stream_cfg, ch, gain, tx, levels)?,
        SampleFormat::U8 => build::<u8>(&device, stream_cfg, ch, gain, tx, levels)?,
        other => bail!("unsupported sample format {other:?}"),
    };
    stream.play().context("starting audio stream")?;

    Ok(Capture {
        _stream: Some(stream),
        #[cfg(windows)]
        _process: None,
        sample_rate,
        device_name,
    })
}

#[cfg(windows)]
fn start_process_capture(cfg: &AudioConfig, tx: Sender<Vec<f32>>, levels: Arc<Levels>) -> Result<Capture> {
    let wanted = if cfg.device.trim().is_empty() { DEFAULT_PROCESS } else { cfg.device.trim() };
    let gain = cfg.gain;
    let ch = Channels { count: 2, offset: 0 };
    let pc = crate::process_capture::start(wanted, move |data| deliver(to_stereo(data, ch, gain), &levels, &tx))?;
    Ok(Capture {
        _stream: None,
        device_name: format!("{} (program audio)", pc.process_name),
        _process: Some(pc),
        sample_rate: crate::process_capture::RATE,
    })
}

#[cfg(not(windows))]
fn start_process_capture(_: &AudioConfig, _: Sender<Vec<f32>>, _: Arc<Levels>) -> Result<Capture> {
    bail!("capturing a single program is only available on Windows for now")
}

/// Update the meters and hand a stereo chunk to the encoder.
fn deliver(stereo: Vec<f32>, levels: &Levels, tx: &Sender<Vec<f32>>) {
    let (mut pl, mut pr) = (0f32, 0f32);
    for f in stereo.as_chunks::<2>().0 {
        pl = pl.max(f[0].abs());
        pr = pr.max(f[1].abs());
    }
    levels.set(pl, pr);
    // Never block the audio thread: drop the chunk if the encoder is stalled.
    let _ = tx.try_send(stereo);
}

fn find_device(host: &cpal::Host, cfg: &AudioConfig) -> Result<cpal::Device> {
    if cfg.device.trim().is_empty() {
        return match cfg.mode {
            CaptureMode::Loopback | CaptureMode::Process => host.default_output_device(),
            CaptureMode::Input => host.default_input_device(),
        }
        .ok_or_else(|| anyhow!("no default {:?} device", cfg.mode));
    }
    let needle = cfg.device.to_lowercase();
    let candidates = match cfg.mode {
        CaptureMode::Loopback | CaptureMode::Process => host.output_devices()?.collect::<Vec<_>>(),
        CaptureMode::Input => host.input_devices()?.collect::<Vec<_>>(),
    };
    candidates
        .into_iter()
        .find(|d| d.to_string().to_lowercase().contains(&needle))
        .ok_or_else(|| anyhow!("device matching '{}' not found (run `djcast devices`)", cfg.device))
}

/// Peak level of one stereo pair of an input, from [`measure_input_pairs`].
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct PairLevel {
    /// First channel (1-based).
    pub first: u16,
    /// Peak in dBFS (-inf for digital silence).
    pub peak_db: f32,
}

/// Listen to every stereo pair of an input for `duration` (music should be playing) and
/// return each pair's peak level. Opens its own stream, so it works while live.
pub fn measure_input_pairs(device: &str, duration: Duration) -> Result<Vec<PairLevel>> {
    let host = cpal::default_host();
    let cfg = AudioConfig { device: device.into(), mode: CaptureMode::Input, ..Default::default() };
    let dev = find_device(&host, &cfg)?;
    let supported = dev.default_input_config().context("no usable input config")?;
    let count = supported.channels() as usize;
    let peaks = Arc::new(std::sync::Mutex::new(vec![0f32; count]));
    let stream = match supported.sample_format() {
        SampleFormat::F32 => measure::<f32>(&dev, supported.config(), peaks.clone())?,
        SampleFormat::F64 => measure::<f64>(&dev, supported.config(), peaks.clone())?,
        SampleFormat::I16 => measure::<i16>(&dev, supported.config(), peaks.clone())?,
        SampleFormat::I32 => measure::<i32>(&dev, supported.config(), peaks.clone())?,
        SampleFormat::U16 => measure::<u16>(&dev, supported.config(), peaks.clone())?,
        SampleFormat::U8 => measure::<u8>(&dev, supported.config(), peaks.clone())?,
        other => bail!("unsupported sample format {other:?}"),
    };
    stream.play().context("starting audio stream")?;
    std::thread::sleep(duration);
    drop(stream);
    let peaks = peaks.lock().unwrap();
    let db = |p: f32| 20.0 * p.log10();
    Ok(if count == 1 {
        vec![PairLevel { first: 1, peak_db: db(peaks[0]) }]
    } else {
        (0..count / 2)
            .map(|p| PairLevel { first: (2 * p + 1) as u16, peak_db: db(peaks[2 * p].max(peaks[2 * p + 1])) })
            .collect()
    })
}

/// The pair to capture after [`measure_input_pairs`]: the loudest one with sound.
pub fn best_pair(levels: &[PairLevel]) -> Option<u16> {
    let threshold = 20.0 * SIGNAL_PEAK.log10();
    levels
        .iter()
        .filter(|l| l.peak_db > threshold)
        .max_by(|a, b| a.peak_db.total_cmp(&b.peak_db))
        .map(|l| l.first)
}

fn measure<T>(device: &cpal::Device, config: cpal::StreamConfig, peaks: Arc<std::sync::Mutex<Vec<f32>>>) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let count = config.channels as usize;
    Ok(device.build_input_stream::<T, _, _>(
        config,
        move |data: &[T], _| {
            let mut p = peaks.lock().unwrap();
            for frame in data.chunks_exact(count) {
                for (c, s) in frame.iter().enumerate() {
                    p[c] = p[c].max(s.to_sample::<f32>().abs());
                }
            }
        },
        |err| log::warn!("channel detection: {err}"),
        None,
    )?)
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: Channels,
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
            levels.track_pairs(data, channels.count);
            deliver(to_stereo(data, channels, gain), &levels, &tx)
        },
        |err| match err.kind() {
            cpal::ErrorKind::Xrun => log::debug!("audio xrun"),
            _ => log::error!("audio stream error: {err}"),
        },
        None,
    )?;
    Ok(stream)
}

/// Channel layout of the captured buffers: `count` interleaved channels, of which the
/// stereo pair starting at `offset` (0-based) is kept.
#[derive(Debug, Clone, Copy)]
struct Channels {
    count: usize,
    offset: usize,
}

/// Convert an interleaved buffer into interleaved stereo f32 (a lone channel is doubled).
fn to_stereo<T>(data: &[T], ch: Channels, gain: f32) -> Vec<f32>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let count = ch.count.max(1);
    let left = ch.offset.min(count - 1);
    let right = (left + 1).min(count - 1);
    let mut out = Vec::with_capacity(data.len() / count * 2);
    for frame in data.chunks_exact(count) {
        out.push((frame[left].to_sample::<f32>() * gain).clamp(-1.0, 1.0));
        out.push((frame[right].to_sample::<f32>() * gain).clamp(-1.0, 1.0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ch(count: usize, offset: usize) -> Channels {
        Channels { count, offset }
    }

    #[test]
    fn mono_is_duplicated_and_gain_clamped() {
        let s = to_stereo(&[0.25f32, -0.8], ch(1, 0), 2.0);
        assert_eq!(s, vec![0.5, 0.5, -1.0, -1.0]);
    }

    #[test]
    fn extra_channels_are_dropped() {
        let s = to_stereo(&[0.1f32, 0.2, 0.9, 0.9], ch(4, 0), 1.0);
        assert_eq!(s, vec![0.1, 0.2]);
    }

    #[test]
    fn a_later_pair_can_be_picked() {
        let s = to_stereo(&[0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], ch(4, 2), 1.0);
        assert_eq!(s, vec![0.3, 0.4, 0.7, 0.8]);
    }

    #[test]
    fn known_controllers_use_their_master_pair() {
        assert_eq!(auto_first_channel("DDJ-1000", 12), 11);
        assert_eq!(auto_first_channel("PIONEER DDJ-1000 (2)", 12), 11);
        // Fewer channels than expected (another driver): fall back to 1–2.
        assert_eq!(auto_first_channel("DDJ-1000", 2), 1);
        assert_eq!(auto_first_channel("Scarlett 4i4", 4), 1);
    }

    /// Levels for a 6-channel input capturing `offset`, created `ago` in the past.
    fn levels_at(offset: usize, ago: Duration) -> Levels {
        let l = Levels { start: Instant::now() - ago, ..Default::default() };
        l.set_layout(ch(6, offset));
        l
    }

    /// One 6-channel frame with sound on the given pair.
    fn frame_with_sound(pair: usize) -> Vec<f32> {
        let mut f = vec![0.0; 6];
        f[2 * pair] = 0.5;
        f
    }

    #[test]
    fn hint_when_captured_pair_is_silent_and_another_has_sound() {
        let l = levels_at(0, Duration::from_secs(11));
        l.track_pairs(&frame_with_sound(2), 6);
        assert_eq!(l.channel_hint(), Some(ChannelHint { current: 1, suggested: 5 }));
    }

    #[test]
    fn no_hint_too_early_or_when_the_captured_pair_has_sound() {
        let early = levels_at(0, Duration::from_secs(3));
        early.track_pairs(&frame_with_sound(2), 6);
        assert_eq!(early.channel_hint(), None);

        let fine = levels_at(4, Duration::from_secs(60));
        fine.track_pairs(&frame_with_sound(2), 6);
        assert_eq!(fine.channel_hint(), None);
    }

    #[test]
    fn no_hint_for_stereo_or_loopback() {
        let l = Levels { start: Instant::now() - Duration::from_secs(60), ..Default::default() };
        l.track_pairs(&frame_with_sound(2), 6);
        assert_eq!(l.channel_hint(), None);
    }

    #[test]
    fn best_pair_is_the_loudest_with_sound() {
        let pl = |first, peak_db| PairLevel { first, peak_db };
        assert_eq!(best_pair(&[pl(1, -95.0), pl(3, f32::NEG_INFINITY), pl(11, -6.8)]), Some(11));
        assert_eq!(best_pair(&[pl(1, -20.0), pl(11, -6.8)]), Some(11));
        assert_eq!(best_pair(&[pl(1, -95.0), pl(11, -96.0)]), None);
    }

    #[test]
    fn channel_pair_must_exist() {
        let input = |first| AudioConfig { mode: CaptureMode::Input, first_channel: first, ..Default::default() };
        assert_eq!(channel_offset(&input(0), "DDJ-1000", 12).unwrap(), 10);
        assert_eq!(channel_offset(&input(3), "Card", 4).unwrap(), 2);
        assert!(channel_offset(&input(4), "Card", 4).is_err());
        assert_eq!(channel_offset(&input(0), "Mic", 1).unwrap(), 0);
        // Loopback always uses the first pair.
        let lb = AudioConfig { first_channel: 11, ..Default::default() };
        assert_eq!(channel_offset(&lb, "Speakers", 2).unwrap(), 0);
    }
}
