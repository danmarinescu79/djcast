//! Decide which deck the audience hears, without flickering during transitions.

use std::time::{Duration, Instant};

/// Below this a deck counts as inaudible.
pub const AUDIBLE: f32 = 0.05;
/// A challenger must be this much louder than the current deck.
const HYSTERESIS: f32 = 1.25;
/// When the current deck went silent, switch after this (shorter) delay.
const SILENT_CONFIRM: Duration = Duration::from_secs(1);

pub struct OnAir {
    current: Option<u8>,
    pending: Option<(u8, Instant)>,
    confirm: Duration,
}

impl OnAir {
    pub fn new(confirm: Duration) -> Self {
        Self { current: None, pending: None, confirm }
    }

    pub fn current(&self) -> Option<u8> {
        self.current
    }

    /// `levels`: effective loudness per deck (playing × mixer gain).
    pub fn update(&mut self, levels: &[(u8, f32)], now: Instant) -> Option<u8> {
        let level = |d: Option<u8>| d.and_then(|d| levels.iter().find(|(x, _)| *x == d)).map_or(0.0, |(_, l)| *l);
        let best = levels
            .iter()
            .filter(|(_, l)| *l >= AUDIBLE)
            // Loudest; on a tie the current deck, then the lowest deck number, wins.
            .max_by(|a, b| {
                a.1.total_cmp(&b.1)
                    .then_with(|| (Some(a.0) == self.current).cmp(&(Some(b.0) == self.current)))
                    .then_with(|| b.0.cmp(&a.0))
            })
            .map(|(d, _)| *d);

        let Some(best) = best else {
            self.pending = None;
            return self.current; // silence: keep showing the last track
        };
        if self.current.is_none() {
            self.current = Some(best);
            return self.current;
        }
        if Some(best) == self.current {
            self.pending = None;
            return self.current;
        }

        let cur = level(self.current);
        let silent = cur < AUDIBLE;
        if !silent && level(Some(best)) < cur * HYSTERESIS {
            self.pending = None; // not clearly louder
            return self.current;
        }
        let wait = if silent { SILENT_CONFIRM.min(self.confirm) } else { self.confirm };
        match self.pending {
            Some((d, since)) if d == best => {
                if now.duration_since(since) >= wait {
                    self.current = Some(best);
                    self.pending = None;
                }
            }
            _ => self.pending = Some((best, now)),
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_needs_confirmation_and_hysteresis() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let mut o = OnAir::new(Duration::from_secs(4));
        assert_eq!(o.update(&[(1, 1.0), (2, 0.0)], s(0)), Some(1));
        // Deck 2 comes in at the same level: no switch (tie, no hysteresis margin).
        assert_eq!(o.update(&[(1, 1.0), (2, 1.0)], s(1)), Some(1));
        // Deck 1 fader goes down: deck 2 clearly louder, but needs 4 s.
        assert_eq!(o.update(&[(1, 0.5), (2, 1.0)], s(2)), Some(1));
        assert_eq!(o.update(&[(1, 0.5), (2, 1.0)], s(5)), Some(1));
        assert_eq!(o.update(&[(1, 0.5), (2, 1.0)], s(6)), Some(2));
    }

    #[test]
    fn short_cut_does_not_switch() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let mut o = OnAir::new(Duration::from_secs(4));
        o.update(&[(1, 1.0), (2, 0.0)], s(0));
        o.update(&[(1, 0.2), (2, 1.0)], s(1)); // quick cut to deck 2...
        assert_eq!(o.update(&[(1, 1.0), (2, 0.0)], s(2)), Some(1)); // ...and back
        assert_eq!(o.update(&[(1, 1.0), (2, 0.0)], s(9)), Some(1));
    }

    #[test]
    fn current_deck_stops_switches_fast() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut o = OnAir::new(Duration::from_secs(4));
        o.update(&[(1, 1.0), (2, 1.0)], ms(0));
        assert_eq!(o.update(&[(1, 0.0), (2, 1.0)], ms(100)), Some(1));
        assert_eq!(o.update(&[(1, 0.0), (2, 1.0)], ms(1200)), Some(2));
    }

    #[test]
    fn silence_keeps_last() {
        let mut o = OnAir::new(Duration::from_secs(4));
        let t = Instant::now();
        o.update(&[(1, 1.0)], t);
        assert_eq!(o.update(&[(1, 0.0), (2, 0.0)], t + Duration::from_secs(30)), Some(1));
    }
}
