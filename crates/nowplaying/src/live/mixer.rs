//! Mixer state from the controller's MIDI output (channel faders, crossfader, assigns).
//!
//! Built-in mapping = Pioneer DDJ (verified on a DDJ-1000):
//! - channel fader N: MIDI channel N, CC 0x13 (MSB) / 0x33 (LSB)
//! - crossfader: MIDI channel 7, CC 0x1F (MSB) / 0x3F (LSB)
//! - crossfader assign of channel N: notes 0x16 = A, 0x1D = THRU, 0x18 = B on MIDI channel N
//!
//! Other controllers use a custom mapping, usually filled in with "learn" (see
//! [`pick_control`]): move a fader and djcast records which CC it sends.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Assign {
    A,
    #[default]
    Thru,
    B,
}

/// A continuous control: a MIDI CC, 14-bit when it has a second (LSB) CC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MidiControl {
    /// MIDI channel, 1..=16; 0 = not mapped.
    pub channel: u8,
    pub cc: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lsb: Option<u8>,
}

impl MidiControl {
    pub fn is_mapped(&self) -> bool {
        (1..=16).contains(&self.channel)
    }

    /// The 0-based channel of the MIDI status byte, if mapped.
    fn status_channel(&self) -> Option<usize> {
        self.is_mapped().then(|| self.channel as usize - 1)
    }
}

/// Crossfader assign buttons: notes sent on each channel fader's MIDI channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignNotes {
    pub a: u8,
    pub thru: u8,
    pub b: u8,
}

/// Where each control lives in the MIDI stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MidiMap {
    /// Channel faders 1..4; unmapped ones count as fully open.
    pub faders: [MidiControl; 4],
    pub crossfader: MidiControl,
    /// `true` when the crossfader sends its lowest value on the B (right) side.
    pub crossfader_reversed: bool,
    /// Assign buttons, if the controller sends them; otherwise the configured assigns apply.
    pub assign_notes: Option<AssignNotes>,
}

impl MidiMap {
    pub fn pioneer_ddj() -> Self {
        let fader = |ch| MidiControl { channel: ch, cc: 0x13, lsb: Some(0x33) };
        Self {
            faders: [fader(1), fader(2), fader(3), fader(4)],
            crossfader: MidiControl { channel: 7, cc: 0x1F, lsb: Some(0x3F) },
            crossfader_reversed: false,
            assign_notes: Some(AssignNotes { a: 0x16, thru: 0x1D, b: 0x18 }),
        }
    }
}

/// Which mapping to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MappingPreset {
    #[default]
    PioneerDdj,
    Custom,
}

#[derive(Debug, Clone)]
pub struct MixerState {
    /// Fader positions 0..=1; `None` until the fader is first moved.
    pub faders: [Option<f32>; 4],
    /// 0 = full A (left), 1 = full B (right).
    pub crossfader: Option<f32>,
    pub assign: [Assign; 4],
    /// Last MSB per control (faders 0..4, crossfader 4), for 14-bit values.
    msb: [u8; 5],
}

impl MixerState {
    pub fn new(assign: [Assign; 4]) -> Self {
        Self { faders: [None; 4], crossfader: None, assign, msb: [0; 5] }
    }

    /// Feed one MIDI message.
    pub fn apply(&mut self, map: &MidiMap, msg: &[u8]) {
        if msg.len() != 3 {
            return;
        }
        let (kind, ch, num, val) = (msg[0] & 0xF0, (msg[0] & 0x0F) as usize, msg[1], msg[2]);
        match kind {
            0xB0 => {
                let controls = map.faders.iter().chain(std::iter::once(&map.crossfader)).enumerate();
                for (slot, c) in controls {
                    if c.status_channel() != Some(ch) {
                        continue;
                    }
                    let value = if num == c.cc {
                        self.msb[slot] = val;
                        val as f32 / 127.0
                    } else if Some(num) == c.lsb {
                        fourteen_bit(self.msb[slot], val)
                    } else {
                        continue;
                    };
                    if slot < 4 {
                        self.faders[slot] = Some(value);
                    } else {
                        self.crossfader = Some(if map.crossfader_reversed { 1.0 - value } else { value });
                    }
                }
            }
            0x90 if val > 0 => {
                let Some(notes) = map.assign_notes else { return };
                for (i, f) in map.faders.iter().enumerate() {
                    if f.status_channel() == Some(ch) {
                        if num == notes.a {
                            self.assign[i] = Assign::A;
                        } else if num == notes.thru {
                            self.assign[i] = Assign::Thru;
                        } else if num == notes.b {
                            self.assign[i] = Assign::B;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// How audible channel `deck` (1-based) is through the mixer, 0..=1.
    /// Unknown controls (never moved) count as fully open.
    pub fn gain(&self, deck: u8) -> f32 {
        let i = (deck as usize).saturating_sub(1).min(3);
        let fader = self.faders[i].unwrap_or(1.0);
        let x = self.crossfader.unwrap_or(0.5);
        let xf = match self.assign[i] {
            Assign::Thru => 1.0,
            // Equal-power curve: both sides ≈0.71 in the middle.
            Assign::A => (x * std::f32::consts::FRAC_PI_2).cos(),
            Assign::B => (x * std::f32::consts::FRAC_PI_2).sin(),
        };
        if self.crossfader.is_none() && self.assign[i] != Assign::Thru {
            return fader; // crossfader position unknown: don't guess
        }
        fader * xf
    }
}

fn fourteen_bit(msb: u8, lsb: u8) -> f32 {
    (((msb as u32) << 7) | lsb as u32) as f32 / 16383.0
}

/// Distinct values a CC must take before it counts as a moved fader (jog wheels, buttons
/// and knob touches send only a few).
const LEARN_MIN_VALUES: usize = 8;

/// "Learn": from the MIDI messages seen while the user moved one control, find that control.
/// A fader sends one CC through many values; a 14-bit fader also sends its LSB on CC + 32.
pub fn pick_control(msgs: &[[u8; 3]]) -> Option<MidiControl> {
    let mut values: HashMap<(u8, u8), HashSet<u8>> = HashMap::new();
    let mut order: Vec<(u8, u8)> = Vec::new();
    for m in msgs {
        if m[0] & 0xF0 != 0xB0 {
            continue;
        }
        let key = (m[0] & 0x0F, m[1]);
        if !values.contains_key(&key) {
            order.push(key);
        }
        values.entry(key).or_default().insert(m[2]);
    }
    let moved = |k: &(u8, u8)| values.get(k).is_some_and(|v| v.len() >= LEARN_MIN_VALUES);
    let &(ch, cc) = order.iter().find(|k| moved(k))?;
    // An LSB (CC 32..63) moves more than its MSB; prefer the MSB when both were sent.
    let (msb, lsb) = if (32..64).contains(&cc) && values.contains_key(&(ch, cc - 32)) {
        (cc - 32, Some(cc))
    } else if cc < 32 && values.contains_key(&(ch, cc + 32)) {
        (cc, Some(cc + 32))
    } else {
        (cc, None)
    };
    Some(MidiControl { channel: ch + 1, cc: msb, lsb })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fader_and_crossfader_messages() {
        let map = MidiMap::pioneer_ddj();
        let mut m = MixerState::new([Assign::Thru; 4]);
        assert_eq!(m.gain(1), 1.0); // unknown = open
        m.apply(&map, &[0xB0, 0x13, 0]); // ch1 fader MSB 0
        m.apply(&map, &[0xB0, 0x33, 0]);
        assert_eq!(m.gain(1), 0.0);
        m.apply(&map, &[0xB1, 0x13, 127]);
        m.apply(&map, &[0xB1, 0x33, 127]);
        assert!((m.gain(2) - 1.0).abs() < 1e-6);

        // ch3 on A, ch4 on B, crossfader fully left → only A side audible.
        m.apply(&map, &[0x92, 0x16, 127]);
        m.apply(&map, &[0x93, 0x18, 127]);
        m.apply(&map, &[0xB6, 0x1F, 0]);
        m.apply(&map, &[0xB6, 0x3F, 0]);
        assert!((m.gain(3) - 1.0).abs() < 1e-6);
        assert!(m.gain(4) < 1e-6);
        // Note-off of an assign button changes nothing.
        m.apply(&map, &[0x92, 0x16, 0]);
        assert_eq!(m.assign[2], Assign::A);
    }

    #[test]
    fn custom_map_on_one_channel_with_reversed_crossfader() {
        // A controller with all faders on MIDI channel 1 (CC 1-4), 7-bit, crossfader CC 8.
        let c = |cc| MidiControl { channel: 1, cc, lsb: None };
        let map = MidiMap { faders: [c(1), c(2), c(3), c(4)], crossfader: c(8), crossfader_reversed: true, assign_notes: None };
        let mut m = MixerState::new([Assign::A, Assign::B, Assign::Thru, Assign::Thru]);
        m.apply(&map, &[0xB0, 2, 64]);
        assert!((m.faders[1].unwrap() - 64.0 / 127.0).abs() < 1e-6);
        assert_eq!(m.faders[0], None);
        // Reversed: value 0 means fully B.
        m.apply(&map, &[0xB0, 8, 0]);
        assert_eq!(m.crossfader, Some(1.0));
        assert!(m.gain(1) < 1e-6);
        // Unmapped controls are ignored.
        let mut partial = map.clone();
        partial.faders[0] = MidiControl::default();
        let mut p = MixerState::new([Assign::Thru; 4]);
        p.apply(&partial, &[0xB0, 1, 0]);
        assert_eq!(p.gain(1), 1.0);
        // Notes are ignored without assign notes.
        m.apply(&map, &[0x90, 0x16, 127]);
        assert_eq!(m.assign[0], Assign::A);
    }

    #[test]
    fn learn_finds_the_moved_fader() {
        let sweep = |ch: u8, cc: u8, lsb: bool| -> Vec<[u8; 3]> {
            (0..40u8)
                .flat_map(|v| {
                    let mut m = vec![[0xB0 | ch, cc, v * 3]];
                    if lsb {
                        m.push([0xB0 | ch, cc + 32, ((v as u32 * 37) % 128) as u8]);
                    }
                    m
                })
                .collect()
        };
        // Jog wheel noise first (relative values 1/127), then a 14-bit fader on channel 2.
        let mut msgs: Vec<[u8; 3]> = (0..30).map(|i| [0xB0, 0x22, if i % 2 == 0 { 1 } else { 127 }]).collect();
        msgs.extend(sweep(1, 0x13, true));
        assert_eq!(pick_control(&msgs), Some(MidiControl { channel: 2, cc: 0x13, lsb: Some(0x33) }));
        // 7-bit fader.
        assert_eq!(pick_control(&sweep(0, 7, false)), Some(MidiControl { channel: 1, cc: 7, lsb: None }));
        // A button press is not a fader.
        assert_eq!(pick_control(&[[0x90, 1, 127], [0x80, 1, 0]]), None);
    }
}
