//! Broadcast-style loudness processing: slow AGC towards a LUFS target, then a
//! look-ahead peak limiter. This is what makes a stream sound as loud and clear as
//! stations running Winamp DSP / Stereo Tool, without clipping the MP3 encoder.
//!
//! Signal: interleaved stereo f32. The limiter delays audio by its look-ahead (~5 ms).

use ebur128::{EbuR128, Mode};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    /// No processing (only a safety clamp).
    #[default]
    Off,
    /// -16 LUFS: podcast/streaming platform level, very transparent.
    Gentle,
    /// -13 LUFS: typical internet radio level.
    Radio,
    /// -10 LUFS: loud, like heavily processed stations; the limiter works harder.
    Loud,
    /// Use `target_lufs`.
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcessingConfig {
    pub preset: Preset,
    /// Loudness target for `preset = "custom"` (LUFS, e.g. -14.0).
    pub target_lufs: f64,
    /// Limiter ceiling in dBFS (keep ≤ -1 so MP3 encoding does not clip).
    pub ceiling_db: f64,
    /// Maximum boost the AGC may apply, in dB.
    pub max_gain_db: f64,
    /// 4-band compression between the AGC and the limiter (denser, more even sound).
    pub multiband: bool,
    /// Bass shelf below ~100 Hz, dB (-6..=+6; 0 = off). Applied before the AGC.
    pub eq_bass_db: f64,
    /// Presence bell around 3.5 kHz, dB (-6..=+6; 0 = off). Applied before the AGC.
    pub eq_presence_db: f64,
}

impl Default for ProcessingConfig {
    fn default() -> Self {
        Self {
            preset: Preset::Off,
            target_lufs: -14.0,
            ceiling_db: -1.0,
            max_gain_db: 15.0,
            multiband: false,
            eq_bass_db: 0.0,
            eq_presence_db: 0.0,
        }
    }
}

impl ProcessingConfig {
    pub fn target(&self) -> Option<f64> {
        match self.preset {
            Preset::Off => None,
            Preset::Gentle => Some(-16.0),
            Preset::Radio => Some(-13.0),
            Preset::Loud => Some(-10.0),
            Preset::Custom => Some(self.target_lufs),
        }
    }
}

/// Live processing values for meters (f32 bits in atomics; lock-free).
#[derive(Default)]
pub struct DspStats {
    in_lufs: AtomicU32,
    out_lufs: AtomicU32,
    agc_gain_db: AtomicU32,
    limiter_db: AtomicU32,
    band_db: [AtomicU32; 4],
    multiband_on: std::sync::atomic::AtomicBool,
}

#[derive(Debug, Clone, Copy)]
pub struct DspSnapshot {
    /// Short-term (3 s) loudness of the input, LUFS (−inf if silent / not enough data).
    pub in_lufs: f32,
    /// Short-term loudness of the processed output, LUFS.
    pub out_lufs: f32,
    pub agc_gain_db: f32,
    /// Deepest limiter gain reduction in the last block (≤ 0 dB).
    pub limiter_db: f32,
    /// Multiband gain reduction per band (low → high), dB ≤ 0; `None` when off.
    pub bands_db: Option<[f32; 4]>,
}

impl DspStats {
    pub fn snapshot(&self) -> DspSnapshot {
        let g = |a: &AtomicU32| f32::from_bits(a.load(Ordering::Relaxed));
        DspSnapshot {
            in_lufs: g(&self.in_lufs),
            out_lufs: g(&self.out_lufs),
            agc_gain_db: g(&self.agc_gain_db),
            limiter_db: g(&self.limiter_db),
            bands_db: self
                .multiband_on
                .load(Ordering::Relaxed)
                .then(|| [0, 1, 2, 3].map(|i| g(&self.band_db[i]))),
        }
    }
    fn set(a: &AtomicU32, v: f32) {
        a.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// Tone controls: bass shelf + presence bell.
struct Eq {
    filters: Vec<crate::multiband::Biquad>,
}

impl Eq {
    const MAX_DB: f64 = 6.0;

    fn new(cfg: &ProcessingConfig, rate: u32) -> Option<Self> {
        use crate::multiband::Biquad;
        let rate = rate as f32;
        let clamp = |v: f64| v.clamp(-Self::MAX_DB, Self::MAX_DB) as f32;
        let mut filters = Vec::new();
        if cfg.eq_bass_db.abs() >= 0.1 {
            filters.push(Biquad::low_shelf(100.0, clamp(cfg.eq_bass_db), rate));
        }
        if cfg.eq_presence_db.abs() >= 0.1 {
            filters.push(Biquad::peaking(3500.0, 0.8, clamp(cfg.eq_presence_db), rate));
        }
        (!filters.is_empty()).then_some(Self { filters })
    }

    fn process(&mut self, buf: &mut [f32]) {
        for frame in buf.as_chunks_mut::<2>().0 {
            for f in &mut self.filters {
                frame[0] = f.run(frame[0], 0);
                frame[1] = f.run(frame[1], 1);
            }
        }
    }
}

pub struct Processor {
    eq: Option<Eq>,
    agc: Option<Agc>,
    multiband: Option<crate::multiband::Multiband>,
    limiter: Option<Limiter>,
    in_meter: EbuR128,
    out_meter: EbuR128,
    ceiling: f32,
    stats: std::sync::Arc<DspStats>,
}

impl Processor {
    pub fn new(cfg: &ProcessingConfig, sample_rate: u32, stats: std::sync::Arc<DspStats>) -> Self {
        let ceiling = db_to_lin(cfg.ceiling_db.min(0.0)) as f32;
        let target = cfg.target();
        Self {
            eq: Eq::new(cfg, sample_rate),
            agc: target.map(|t| Agc::new(t, cfg.max_gain_db, sample_rate)),
            multiband: (target.is_some() && cfg.multiband)
                .then(|| crate::multiband::Multiband::new(sample_rate, crate::multiband::DEFAULT_BANDS)),
            limiter: target.map(|_| Limiter::new(ceiling, sample_rate)),
            in_meter: EbuR128::new(2, sample_rate, Mode::S).expect("valid meter params"),
            out_meter: EbuR128::new(2, sample_rate, Mode::S).expect("valid meter params"),
            ceiling,
            stats: {
                stats.multiband_on.store(target.is_some() && cfg.multiband, Ordering::Relaxed);
                stats
            },
        }
    }

    /// Process one block of interleaved stereo samples in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        let _ = self.in_meter.add_frames_f32(buf);
        let in_st = self.in_meter.loudness_shortterm().unwrap_or(f64::NEG_INFINITY);

        if let Some(eq) = &mut self.eq {
            eq.process(buf);
        }

        if let Some(agc) = &mut self.agc {
            agc.process(buf, in_st);
        }
        if let Some(mb) = &mut self.multiband {
            let gr = mb.process(buf);
            for (a, v) in self.stats.band_db.iter().zip(gr) {
                DspStats::set(a, v);
            }
        }
        let mut lim_db = 0.0;
        if let Some(lim) = &mut self.limiter {
            lim_db = lim.process(buf);
        } else {
            for s in buf.iter_mut() {
                *s = s.clamp(-1.0, 1.0);
            }
        }
        // Final safety clamp (float rounding after the limiter).
        for s in buf.iter_mut() {
            *s = s.clamp(-self.ceiling, self.ceiling).clamp(-1.0, 1.0);
        }

        let _ = self.out_meter.add_frames_f32(buf);
        let out_st = self.out_meter.loudness_shortterm().unwrap_or(f64::NEG_INFINITY);
        DspStats::set(&self.stats.in_lufs, in_st as f32);
        DspStats::set(&self.stats.out_lufs, out_st as f32);
        DspStats::set(&self.stats.agc_gain_db, self.agc.as_ref().map_or(0.0, |a| a.gain_db as f32));
        DspStats::set(&self.stats.limiter_db, lim_db);
    }
}

/// Slow automatic gain control driven by short-term loudness.
struct Agc {
    target: f64,
    max_gain_db: f64,
    rate: f64,
    gain_db: f64,
    initialized: bool,
}

impl Agc {
    /// Below this the input is treated as a pause: hold the gain, don't pump up noise.
    const GATE_LUFS: f64 = -40.0;
    const MIN_GAIN_DB: f64 = -12.0;
    /// Gain changes, dB per second.
    const UP_DB_PER_S: f64 = 1.0;
    const DOWN_DB_PER_S: f64 = 3.0;

    fn new(target: f64, max_gain_db: f64, rate: u32) -> Self {
        Self { target, max_gain_db: max_gain_db.max(0.0), rate: rate as f64, gain_db: 0.0, initialized: false }
    }

    fn process(&mut self, buf: &mut [f32], in_lufs: f64) {
        let start = self.gain_db;
        if in_lufs.is_finite() && in_lufs > Self::GATE_LUFS {
            let desired = (self.target - in_lufs).clamp(Self::MIN_GAIN_DB, self.max_gain_db);
            if !self.initialized {
                // First real measurement (3 s of audio): jump straight to the target.
                self.gain_db = desired;
                self.initialized = true;
            } else {
                let secs = (buf.len() / 2) as f64 / self.rate;
                let step = if desired > self.gain_db { Self::UP_DB_PER_S } else { Self::DOWN_DB_PER_S } * secs;
                self.gain_db += (desired - self.gain_db).clamp(-step, step);
            }
        }
        // Ramp linearly across the block to avoid zipper noise.
        let frames = (buf.len() / 2).max(1);
        let (g0, g1) = (db_to_lin(start), db_to_lin(self.gain_db));
        for (i, f) in buf.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let g = (g0 + (g1 - g0) * (i as f64 / frames as f64)) as f32;
            f[0] *= g;
            f[1] *= g;
        }
    }
}

/// Stereo-linked look-ahead peak limiter.
///
/// gain(t) = moving average over L of (sliding minimum over L of the required gain),
/// applied to audio delayed by L-1 frames. Every term of the average is ≤ the gain the
/// delayed sample needs, so peaks never exceed the ceiling; the average makes the attack
/// a smooth ramp. Release is exponential.
struct Limiter {
    ceiling: f32,
    lookahead: usize,
    delay: VecDeque<[f32; 2]>,
    min_q: VecDeque<(u64, f32)>,
    avg_ring: VecDeque<f32>,
    avg_sum: f64,
    gain: f32,
    release_coef: f32,
    n: u64,
}

impl Limiter {
    const LOOKAHEAD_S: f64 = 0.005;
    const RELEASE_S: f64 = 0.120;

    fn new(ceiling: f32, rate: u32) -> Self {
        let lookahead = ((rate as f64 * Self::LOOKAHEAD_S) as usize).max(2);
        let mut delay = VecDeque::with_capacity(lookahead);
        delay.extend(std::iter::repeat_n([0.0f32; 2], lookahead - 1));
        let mut avg_ring = VecDeque::with_capacity(lookahead);
        avg_ring.extend(std::iter::repeat_n(1.0f32, lookahead));
        Self {
            ceiling,
            lookahead,
            delay,
            min_q: VecDeque::new(),
            avg_ring,
            avg_sum: lookahead as f64,
            gain: 1.0,
            release_coef: (1.0 - (-1.0 / (Self::RELEASE_S * rate as f64)).exp()) as f32,
            n: 0,
        }
    }

    /// Returns the deepest gain reduction in this block, in dB (≤ 0).
    fn process(&mut self, buf: &mut [f32]) -> f32 {
        let mut min_gain = 1.0f32;
        for f in buf.as_chunks_mut::<2>().0 {
            let peak = f[0].abs().max(f[1].abs());
            let required = if peak > self.ceiling { self.ceiling / peak } else { 1.0 };

            // Sliding minimum over the last `lookahead` frames.
            while self.min_q.back().is_some_and(|&(_, v)| v >= required) {
                self.min_q.pop_back();
            }
            self.min_q.push_back((self.n, required));
            while self.min_q.front().is_some_and(|&(i, _)| i + self.lookahead as u64 <= self.n) {
                self.min_q.pop_front();
            }
            let window_min = self.min_q.front().map_or(1.0, |&(_, v)| v);

            // Moving average of the minimum → smooth attack.
            self.avg_sum += window_min as f64 - self.avg_ring.pop_front().unwrap() as f64;
            self.avg_ring.push_back(window_min);
            let target = (self.avg_sum / self.lookahead as f64) as f32;

            // Instant down (already smoothed), exponential release up.
            self.gain = if target < self.gain { target } else { self.gain + (target - self.gain) * self.release_coef };
            min_gain = min_gain.min(self.gain);

            self.delay.push_back([f[0], f[1]]);
            let out = self.delay.pop_front().unwrap();
            f[0] = out[0] * self.gain;
            f[1] = out[1] * self.gain;
            self.n += 1;
        }
        20.0 * min_gain.log10()
    }
}

fn db_to_lin(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const RATE: u32 = 48_000;

    /// Stereo sine at `amp`, `secs` long.
    fn sine(amp: f32, secs: f32) -> Vec<f32> {
        (0..(RATE as f32 * secs) as usize)
            .flat_map(|i| {
                let s = (i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * amp;
                [s, s]
            })
            .collect()
    }

    fn run(cfg: &ProcessingConfig, mut audio: Vec<f32>) -> (Vec<f32>, DspSnapshot) {
        let stats = Arc::new(DspStats::default());
        let mut p = Processor::new(cfg, RATE, stats.clone());
        for block in audio.chunks_mut(960) {
            p.process(block);
        }
        (audio, stats.snapshot())
    }

    #[test]
    fn quiet_input_is_raised_to_target_without_exceeding_ceiling() {
        let cfg = ProcessingConfig { preset: Preset::Radio, ..Default::default() };
        // A quiet mix: sine at -20 dBFS peak (≈ -23 LUFS).
        let (out, snap) = run(&cfg, sine(0.1, 12.0));
        assert!(snap.in_lufs < -20.0, "input {:.1} LUFS", snap.in_lufs);
        assert!((snap.out_lufs - -13.0).abs() < 1.0, "output {:.1} LUFS", snap.out_lufs);
        let peak = out.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak <= db_to_lin(-1.0) as f32 + 1e-6, "peak {peak}");
    }

    #[test]
    fn limiter_catches_sudden_peaks() {
        let cfg = ProcessingConfig { preset: Preset::Loud, ..Default::default() };
        let mut audio = sine(0.1, 6.0);
        // A full-scale burst after the AGC has settled on the quiet signal.
        let start = audio.len() / 2;
        for s in &mut audio[start..start + 4800] {
            *s *= 10.0;
        }
        let (out, snap) = run(&cfg, audio);
        let peak = out.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak <= db_to_lin(-1.0) as f32 + 1e-6, "peak {peak}");
        assert!(snap.agc_gain_db > 0.0);
    }

    #[test]
    fn silence_is_not_pumped_up() {
        let cfg = ProcessingConfig { preset: Preset::Loud, ..Default::default() };
        let mut audio = sine(0.0005, 6.0); // ≈ -70 dBFS hiss
        audio.extend(vec![0.0; RATE as usize * 2]);
        let (_, snap) = run(&cfg, audio);
        assert_eq!(snap.agc_gain_db, 0.0);
    }

    #[test]
    fn multiband_stays_under_the_ceiling_and_reports_bands() {
        let cfg = ProcessingConfig { preset: Preset::Loud, multiband: true, ..Default::default() };
        let mut audio = sine(0.1, 6.0);
        let start = audio.len() / 2;
        for s in &mut audio[start..start + 4800] {
            *s *= 10.0;
        }
        let (out, snap) = run(&cfg, audio);
        let peak = out.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak <= db_to_lin(-1.0) as f32 + 1e-6, "peak {peak}");
        assert!(snap.bands_db.is_some());
    }

    #[test]
    fn off_only_clamps() {
        let cfg = ProcessingConfig::default();
        let (out, snap) = run(&cfg, vec![0.5, -0.5, 1.5, -1.5]);
        assert_eq!(out, vec![0.5, -0.5, db_to_lin(-1.0) as f32, -db_to_lin(-1.0) as f32]);
        assert_eq!(snap.agc_gain_db, 0.0);
    }

    fn tone_rms_db(freq: f32, cfg: &ProcessingConfig) -> f32 {
        let mut buf: Vec<f32> = (0..RATE)
            .flat_map(|i| {
                let s = (i as f32 * freq * std::f32::consts::TAU / RATE as f32).sin() * 0.25;
                [s, s]
            })
            .collect();
        let mut p = Processor::new(cfg, RATE, Arc::new(DspStats::default()));
        p.process(&mut buf);
        let tail = &buf[buf.len() / 2..]; // after the filters settle
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        20.0 * rms.log10()
    }

    #[test]
    fn eq_boosts_bass_and_presence_only_where_asked() {
        let flat = ProcessingConfig::default();
        let eq = ProcessingConfig { eq_bass_db: 6.0, eq_presence_db: 4.0, ..Default::default() };
        let diff = |f: f32| tone_rms_db(f, &eq) - tone_rms_db(f, &flat);
        assert!((diff(40.0) - 6.0).abs() < 0.8, "40 Hz: {}", diff(40.0));
        assert!((diff(3500.0) - 4.0).abs() < 0.5, "3.5 kHz: {}", diff(3500.0));
        assert!(diff(800.0).abs() < 1.0, "800 Hz: {}", diff(800.0));
        assert!(diff(15_000.0).abs() < 1.0, "15 kHz: {}", diff(15_000.0));
    }
}
