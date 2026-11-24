//! 4-band compressor, like the multiband stage of broadcast processors.
//!
//! Split: Linkwitz-Riley 4th order (two cascaded Butterworth biquads) at 120 Hz, 600 Hz and
//! 4 kHz, as a tree with all-pass compensation on the lower bands, so the bands sum back to
//! a flat magnitude response when no compression happens. Each band has a stereo-linked
//! compressor with its own timing (slow on bass, fast on highs) and make-up gain.

/// Crossover frequencies in Hz.
pub const CROSSOVERS: [f32; 3] = [120.0, 600.0, 4000.0];

/// Per-band settings.
#[derive(Debug, Clone, Copy)]
pub struct BandSettings {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
}

/// Defaults tuned for a loudness-controlled DJ mix (after the AGC).
pub const DEFAULT_BANDS: [BandSettings; 4] = [
    BandSettings { threshold_db: -22.0, ratio: 3.0, attack_ms: 30.0, release_ms: 300.0, makeup_db: 2.5 },
    BandSettings { threshold_db: -22.0, ratio: 2.5, attack_ms: 20.0, release_ms: 220.0, makeup_db: 2.0 },
    BandSettings { threshold_db: -24.0, ratio: 2.5, attack_ms: 10.0, release_ms: 160.0, makeup_db: 2.0 },
    BandSettings { threshold_db: -28.0, ratio: 3.0, attack_ms: 4.0, release_ms: 110.0, makeup_db: 2.5 },
];

/// Stereo biquad (transposed direct form II).
#[derive(Clone, Copy, Default)]
pub(crate) struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: [f32; 2],
    z2: [f32; 2],
}

impl Biquad {
    /// RBJ cookbook Butterworth (Q = 1/√2) low- or high-pass.
    fn butterworth(freq: f32, rate: f32, high: bool) -> Self {
        let w0 = std::f32::consts::TAU * freq / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = if high {
            ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0)
        } else {
            ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0)
        };
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: -2.0 * cos / a0, a2: (1.0 - alpha) / a0, ..Default::default() }
    }

    /// RBJ cookbook low shelf (shelf slope 1).
    pub(crate) fn low_shelf(freq: f32, gain_db: f32, rate: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = std::f32::consts::TAU * freq / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / 2.0 * std::f32::consts::SQRT_2;
        let k = 2.0 * a.sqrt() * alpha;
        let a0 = (a + 1.0) + (a - 1.0) * cos + k;
        Self {
            b0: a * ((a + 1.0) - (a - 1.0) * cos + k) / a0,
            b1: 2.0 * a * ((a - 1.0) - (a + 1.0) * cos) / a0,
            b2: a * ((a + 1.0) - (a - 1.0) * cos - k) / a0,
            a1: -2.0 * ((a - 1.0) + (a + 1.0) * cos) / a0,
            a2: ((a + 1.0) + (a - 1.0) * cos - k) / a0,
            ..Default::default()
        }
    }

    /// RBJ cookbook peaking EQ.
    pub(crate) fn peaking(freq: f32, q: f32, gain_db: f32, rate: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = std::f32::consts::TAU * freq / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * cos / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha / a) / a0,
            ..Default::default()
        }
    }

    #[inline]
    pub(crate) fn run(&mut self, x: f32, ch: usize) -> f32 {
        let y = self.b0 * x + self.z1[ch];
        self.z1[ch] = self.b1 * x - self.a1 * y + self.z2[ch];
        self.z2[ch] = self.b2 * x - self.a2 * y;
        y
    }
}

/// Linkwitz-Riley 4th order low- or high-pass = two identical Butterworth stages.
#[derive(Clone, Copy)]
struct Lr4([Biquad; 2]);

impl Lr4 {
    fn new(freq: f32, rate: f32, high: bool) -> Self {
        let b = Biquad::butterworth(freq, rate, high);
        Self([b, b])
    }
    #[inline]
    fn run(&mut self, x: f32, ch: usize) -> f32 {
        let y = self.0[0].run(x, ch);
        self.0[1].run(y, ch)
    }
}

/// LR4 all-pass at a crossover (LP + HP), used to align the phase of lower bands.
#[derive(Clone, Copy)]
struct Lr4AllPass {
    lp: Lr4,
    hp: Lr4,
}

impl Lr4AllPass {
    fn new(freq: f32, rate: f32) -> Self {
        Self { lp: Lr4::new(freq, rate, false), hp: Lr4::new(freq, rate, true) }
    }
    #[inline]
    fn run(&mut self, x: f32, ch: usize) -> f32 {
        self.lp.run(x, ch) + self.hp.run(x, ch)
    }
}

struct Compressor {
    s: BandSettings,
    attack: f32,
    release: f32,
    env_db: f32,
    makeup: f32,
}

impl Compressor {
    fn new(s: BandSettings, rate: f32) -> Self {
        let coef = |ms: f32| (-1.0 / (ms.max(0.1) * 0.001 * rate)).exp();
        Self {
            s,
            attack: coef(s.attack_ms),
            release: coef(s.release_ms),
            env_db: -120.0,
            makeup: 10f32.powf(s.makeup_db / 20.0),
        }
    }

    /// Gain (linear) for a stereo-linked input sample pair; also returns the reduction in dB.
    #[inline]
    fn gain(&mut self, l: f32, r: f32) -> (f32, f32) {
        let level_db = 20.0 * l.abs().max(r.abs()).max(1e-6).log10();
        let coef = if level_db > self.env_db { self.attack } else { self.release };
        self.env_db = level_db + coef * (self.env_db - level_db);
        let over = self.env_db - self.s.threshold_db;
        let gr_db = if over > 0.0 { -over * (1.0 - 1.0 / self.s.ratio.max(1.0)) } else { 0.0 };
        (10f32.powf(gr_db / 20.0) * self.makeup, gr_db)
    }
}

pub struct Multiband {
    lp1: Lr4,
    hp1: Lr4,
    lp2: Lr4,
    hp2: Lr4,
    lp3: Lr4,
    hp3: Lr4,
    ap2_b1: Lr4AllPass,
    ap3_b1: Lr4AllPass,
    ap3_b2: Lr4AllPass,
    comps: [Compressor; 4],
}

impl Multiband {
    pub fn new(rate: u32, bands: [BandSettings; 4]) -> Self {
        let r = rate as f32;
        let [f1, f2, f3] = CROSSOVERS;
        Self {
            lp1: Lr4::new(f1, r, false),
            hp1: Lr4::new(f1, r, true),
            lp2: Lr4::new(f2, r, false),
            hp2: Lr4::new(f2, r, true),
            lp3: Lr4::new(f3, r, false),
            hp3: Lr4::new(f3, r, true),
            ap2_b1: Lr4AllPass::new(f2, r),
            ap3_b1: Lr4AllPass::new(f3, r),
            ap3_b2: Lr4AllPass::new(f3, r),
            comps: bands.map(|b| Compressor::new(b, r)),
        }
    }

    /// Split one stereo frame into 4 phase-aligned bands: `[band][channel]`.
    #[inline]
    fn split(&mut self, x: [f32; 2]) -> [[f32; 2]; 4] {
        let mut out = [[0.0; 2]; 4];
        for (ch, &s) in x.iter().enumerate() {
            let low = self.lp1.run(s, ch);
            let rest = self.hp1.run(s, ch);
            let b1 = self.ap3_b1.run(self.ap2_b1.run(low, ch), ch);
            let mid = self.lp2.run(rest, ch);
            let upper = self.hp2.run(rest, ch);
            let b2 = self.ap3_b2.run(mid, ch);
            let b3 = self.lp3.run(upper, ch);
            let b4 = self.hp3.run(upper, ch);
            out[0][ch] = b1;
            out[1][ch] = b2;
            out[2][ch] = b3;
            out[3][ch] = b4;
        }
        out
    }

    /// Process interleaved stereo in place. Returns the deepest gain reduction per band (dB, ≤ 0).
    pub fn process(&mut self, buf: &mut [f32]) -> [f32; 4] {
        let mut deepest = [0f32; 4];
        for f in buf.as_chunks_mut::<2>().0 {
            let bands = self.split(*f);
            let mut sum = [0f32; 2];
            for (i, b) in bands.iter().enumerate() {
                let (g, gr) = self.comps[i].gain(b[0], b[1]);
                deepest[i] = deepest[i].min(gr);
                sum[0] += b[0] * g;
                sum[1] += b[1] * g;
            }
            *f = sum;
        }
        deepest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(freq: f32, amp: f32, secs: f32) -> Vec<f32> {
        (0..(RATE as f32 * secs) as usize)
            .flat_map(|i| {
                let s = (i as f32 * freq * std::f32::consts::TAU / RATE as f32).sin() * amp;
                [s, s]
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// With compression off, the 4 bands must sum back to the input level at every frequency.
    #[test]
    fn bands_sum_flat_without_compression() {
        let flat = BandSettings { threshold_db: 0.0, ratio: 1.0, attack_ms: 10.0, release_ms: 100.0, makeup_db: 0.0 };
        for freq in [40.0, 120.0, 300.0, 600.0, 1500.0, 4000.0, 9000.0] {
            let mut mb = Multiband::new(RATE, [flat; 4]);
            let input = sine(freq, 0.25, 1.0);
            let mut out = input.clone();
            mb.process(&mut out);
            let half = out.len() / 2; // skip the filters' settling time
            let diff_db = 20.0 * (rms(&out[half..]) / rms(&input[half..])).log10();
            assert!(diff_db.abs() < 0.1, "{freq} Hz: {diff_db:.3} dB");
        }
    }

    #[test]
    fn loud_bass_is_compressed_more_than_quiet_highs() {
        let mut mb = Multiband::new(RATE, DEFAULT_BANDS);
        let mut buf: Vec<f32> = sine(60.0, 0.7, 1.0).iter().zip(sine(8000.0, 0.01, 1.0)).map(|(a, b)| a + b).collect();
        let mut gr = [0f32; 4];
        for chunk in buf.chunks_mut(960) {
            let g = mb.process(chunk);
            for i in 0..4 {
                gr[i] = gr[i].min(g[i]);
            }
        }
        assert!(gr[0] < -6.0, "bass reduction {:.1} dB", gr[0]);
        assert!(gr[3] > -1.0, "highs reduction {:.1} dB", gr[3]);
    }
}
