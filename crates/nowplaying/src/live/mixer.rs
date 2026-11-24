//! Mixer state from the controller's MIDI output (channel faders, crossfader, assigns).
//!
//! Default mapping = Pioneer DDJ (verified on a DDJ-1000):
//! - channel fader N: MIDI channel N, CC 0x13 (MSB) / 0x33 (LSB)
//! - crossfader: MIDI channel 7, CC 0x1F (MSB) / 0x3F (LSB)
//! - crossfader assign of channel N: notes 0x16 = A, 0x1D = THRU, 0x18 = B on MIDI channel N

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Assign {
    A,
    #[default]
    Thru,
    B,
}

/// Where each control lives in the MIDI stream.
#[derive(Debug, Clone)]
pub struct MidiMap {
    pub fader_msb: u8,
    pub fader_lsb: u8,
    pub xfader_channel: u8,
    pub xfader_msb: u8,
    pub xfader_lsb: u8,
    pub assign_a: u8,
    pub assign_thru: u8,
    pub assign_b: u8,
}

impl MidiMap {
    pub fn pioneer_ddj() -> Self {
        Self {
            fader_msb: 0x13,
            fader_lsb: 0x33,
            xfader_channel: 6, // 0-based (MIDI channel 7)
            xfader_msb: 0x1F,
            xfader_lsb: 0x3F,
            assign_a: 0x16,
            assign_thru: 0x1D,
            assign_b: 0x18,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MixerState {
    /// Fader positions 0..=1; `None` until the fader is first moved.
    pub faders: [Option<f32>; 4],
    /// 0 = full A (left), 1 = full B (right).
    pub crossfader: Option<f32>,
    pub assign: [Assign; 4],
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
            0xB0 if ch < 4 && num == map.fader_msb => {
                self.msb[ch] = val;
                self.faders[ch] = Some(val as f32 / 127.0);
            }
            0xB0 if ch < 4 && num == map.fader_lsb => {
                self.faders[ch] = Some(fourteen_bit(self.msb[ch], val));
            }
            0xB0 if ch == map.xfader_channel as usize && num == map.xfader_msb => {
                self.msb[4] = val;
                self.crossfader = Some(val as f32 / 127.0);
            }
            0xB0 if ch == map.xfader_channel as usize && num == map.xfader_lsb => {
                self.crossfader = Some(fourteen_bit(self.msb[4], val));
            }
            0x90 if ch < 4 && val > 0 => {
                if num == map.assign_a {
                    self.assign[ch] = Assign::A;
                } else if num == map.assign_thru {
                    self.assign[ch] = Assign::Thru;
                } else if num == map.assign_b {
                    self.assign[ch] = Assign::B;
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
}
