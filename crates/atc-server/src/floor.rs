//! Pure-logic radio floor: one speaker per frequency. A pilot press is refused while the
//! controller's audio plays (plus a margin) or while another pilot holds or is being
//! processed on the frequency; the controller starts only a gap after any transmission
//! ends. No I/O and no clock reads: instants are passed in.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::hub::{ClientId, PttOutcome};

/// Radio timing knobs of the server.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadioTiming {
    /// Silence the controller leaves after any transmission before speaking.
    pub gap: Duration,
    /// Extra time the controller's audio is considered playing after its nominal length
    /// (network and playback latency).
    pub margin: Duration,
    /// Factor applied to the audio length to get its airtime.
    pub audio_scale: f64,
}

impl Default for RadioTiming {
    fn default() -> Self {
        RadioTiming {
            gap: Duration::from_secs(1),
            margin: Duration::from_millis(300),
            audio_scale: 1.0,
        }
    }
}

impl RadioTiming {
    /// No gap, no margin, no airtime: the floor never delays anything (tests).
    pub fn instant() -> Self {
        RadioTiming {
            gap: Duration::ZERO,
            margin: Duration::ZERO,
            audio_scale: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The pilot keys the radio.
    Held,
    /// The pilot let go; the transmission is being transcribed and answered.
    Processing,
}

#[derive(Debug, Default)]
struct FreqState {
    pilot: Option<(ClientId, Phase)>,
    /// When the controller's audio (with margin) stops occupying the frequency.
    atc_end: Option<Instant>,
    /// The controller may not start before this (the gap after the last transmission).
    quiet_until: Option<Instant>,
}

/// Who may speak on each frequency.
#[derive(Debug)]
pub struct Floor {
    gap: Duration,
    freqs: HashMap<u32, FreqState>,
}

impl Default for Floor {
    fn default() -> Self {
        Floor::new(RadioTiming::default().gap)
    }
}

impl Floor {
    /// A floor whose controller waits `gap` after a pilot's transmission is done.
    pub fn new(gap: Duration) -> Self {
        Floor {
            gap,
            freqs: HashMap::new(),
        }
    }

    /// A pilot keys `freq`. Granted on a free frequency, in the gap after a transmission,
    /// or when the same pilot re-keys during its own processing.
    pub fn pilot_press(&mut self, freq: u32, id: ClientId, now: Instant) -> PttOutcome {
        let st = self.freqs.entry(freq).or_default();
        if st.atc_end.is_some_and(|end| end > now) {
            return PttOutcome::Blocked;
        }
        match st.pilot {
            Some((holder, _)) if holder != id => PttOutcome::Blocked,
            _ => {
                st.pilot = Some((id, Phase::Held));
                PttOutcome::Granted
            }
        }
    }

    /// The pilot lets go: the floor stays theirs while the transmission is processed.
    pub fn pilot_release(&mut self, freq: u32, id: ClientId) {
        if let Some(st) = self.freqs.get_mut(&freq)
            && let Some((holder, phase)) = &mut st.pilot
            && *holder == id
        {
            *phase = Phase::Processing;
        }
    }

    /// The pilot's transmission is fully handled. True when this freed the floor.
    pub fn pilot_done(&mut self, freq: u32, id: ClientId, now: Instant) -> bool {
        let Some(st) = self.freqs.get_mut(&freq) else {
            return false;
        };
        match st.pilot {
            Some((holder, _)) if holder == id => {
                st.pilot = None;
                st.quiet_until = Some(now + self.gap);
                true
            }
            _ => false,
        }
    }

    /// When the controller may start on `freq`: `None` is now, `Some(t)` not before `t`.
    /// While a pilot holds the floor that is a far-future instant; the caller waits for a
    /// change notification instead.
    pub fn atc_may_start(&self, freq: u32, now: Instant) -> Option<Instant> {
        let st = self.freqs.get(&freq)?;
        if st.pilot.is_some() {
            return Some(now + Duration::from_secs(365 * 24 * 3600));
        }
        let not_before = st.atc_end.into_iter().chain(st.quiet_until).max()?;
        (not_before > now).then_some(not_before)
    }

    /// The controller starts `airtime` of audio on `freq` at `now`.
    pub fn atc_start(&mut self, freq: u32, now: Instant, airtime: Duration, timing: &RadioTiming) {
        let end = now + airtime.mul_f64(timing.audio_scale) + timing.margin;
        let st = self.freqs.entry(freq).or_default();
        st.atc_end = Some(end);
        st.quiet_until = Some(end + timing.gap);
    }

    /// When the controller's current audio (with margin) ends on `freq`.
    pub fn atc_end_at(&self, freq: u32) -> Option<Instant> {
        self.freqs.get(&freq)?.atc_end
    }

    /// The client is gone: frees everything it held or was processed on. Returns the
    /// frequencies freed.
    pub fn leave(&mut self, id: ClientId) -> Vec<u32> {
        let mut freed: Vec<u32> = self
            .freqs
            .iter_mut()
            .filter(|(_, st)| st.pilot.is_some_and(|(holder, _)| holder == id))
            .map(|(freq, st)| {
                st.pilot = None;
                *freq
            })
            .collect();
        freed.sort_unstable();
        freed
    }

    /// Whether anyone is speaking on `freq` at `now`.
    pub fn is_busy(&self, freq: u32, now: Instant) -> bool {
        self.freqs
            .get(&freq)
            .is_some_and(|st| st.pilot.is_some() || st.atc_end.is_some_and(|end| end > now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: u32 = 121_900;
    const SEC: Duration = Duration::from_secs(1);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn floor() -> Floor {
        Floor::new(SEC)
    }

    #[test]
    fn pilot_press_on_free_frequency_is_granted() {
        let mut f = floor();
        let t = Instant::now();
        assert_eq!(f.pilot_press(F, 1, t), PttOutcome::Granted);
        assert!(f.is_busy(F, t));
    }

    #[test]
    fn pilot_press_while_atc_talks_is_blocked() {
        let mut f = floor();
        let t0 = Instant::now();
        f.atc_start(F, t0, 2 * SEC, &RadioTiming::default());
        assert!(f.is_busy(F, t0 + SEC));
        assert_eq!(f.pilot_press(F, 1, t0 + SEC), PttOutcome::Blocked);
        assert_eq!(f.atc_end_at(F), Some(t0 + ms(2300)));
        assert_eq!(f.pilot_press(F, 1, t0 + ms(2400)), PttOutcome::Granted);
    }

    #[test]
    fn pilot_press_during_gap_is_granted() {
        let mut f = floor();
        let t0 = Instant::now();
        f.atc_start(F, t0, SEC, &RadioTiming::default());
        // Audio and margin ended at t0+1.3 s; the gap runs to t0+2.3 s.
        assert_eq!(f.atc_may_start(F, t0 + ms(1500)), Some(t0 + ms(2300)));
        assert_eq!(f.pilot_press(F, 1, t0 + ms(1500)), PttOutcome::Granted);
        assert!(f.atc_may_start(F, t0 + ms(1500)).is_some());
    }

    #[test]
    fn other_pilot_blocked_while_first_is_processing() {
        let mut f = floor();
        let t = Instant::now();
        assert_eq!(f.pilot_press(F, 1, t), PttOutcome::Granted);
        assert_eq!(f.pilot_press(F, 2, t), PttOutcome::Blocked);
        f.pilot_release(F, 1);
        assert_eq!(f.pilot_press(F, 2, t), PttOutcome::Blocked);
        assert!(f.is_busy(F, t));
        assert!(f.pilot_done(F, 1, t));
        assert_eq!(f.pilot_press(F, 2, t), PttOutcome::Granted);
    }

    #[test]
    fn same_pilot_rekeys_while_processing() {
        let mut f = floor();
        let t = Instant::now();
        f.pilot_press(F, 1, t);
        f.pilot_release(F, 1);
        assert_eq!(f.pilot_press(F, 1, t), PttOutcome::Granted);
        assert_eq!(f.pilot_press(F, 2, t), PttOutcome::Blocked);
    }

    #[test]
    fn atc_waits_for_pilot_then_gap() {
        let mut f = floor();
        let t = Instant::now();
        f.pilot_press(F, 1, t);
        f.pilot_release(F, 1);
        let far = f.atc_may_start(F, t).unwrap();
        assert!(far > t + Duration::from_secs(3600));
        let done = t + 5 * SEC;
        assert!(f.pilot_done(F, 1, done));
        assert_eq!(f.atc_may_start(F, done), Some(done + SEC));
        assert_eq!(f.atc_may_start(F, done + SEC), None);
    }

    #[test]
    fn pilot_done_by_a_non_holder_frees_nothing() {
        let mut f = floor();
        let t = Instant::now();
        f.pilot_press(F, 1, t);
        assert!(!f.pilot_done(F, 2, t));
        assert!(!f.pilot_done(F + 1, 1, t));
        assert_eq!(f.pilot_press(F, 2, t), PttOutcome::Blocked);
    }

    #[test]
    fn leave_frees_held_and_processing_floor() {
        let mut f = floor();
        let t = Instant::now();
        f.pilot_press(F, 1, t);
        f.pilot_press(F + 10, 1, t);
        f.pilot_release(F + 10, 1);
        f.pilot_press(F + 20, 2, t);
        assert_eq!(f.leave(1), vec![F, F + 10]);
        assert_eq!(f.pilot_press(F, 2, t), PttOutcome::Granted);
        assert_eq!(f.pilot_press(F + 10, 2, t), PttOutcome::Granted);
        assert_eq!(f.pilot_press(F + 20, 3, t), PttOutcome::Blocked);
        assert!(f.leave(1).is_empty());
    }

    #[test]
    fn floors_are_independent_per_frequency() {
        let mut f = floor();
        let t = Instant::now();
        assert_eq!(f.pilot_press(F, 1, t), PttOutcome::Granted);
        assert_eq!(f.pilot_press(F + 10, 2, t), PttOutcome::Granted);
        f.atc_start(F + 20, t, 2 * SEC, &RadioTiming::default());
        assert_eq!(f.atc_may_start(F + 30, t), None);
        assert_eq!(f.pilot_press(F + 30, 3, t), PttOutcome::Granted);
        assert!(!f.is_busy(F + 40, t));
        assert_eq!(f.atc_end_at(F), None);
    }

    #[test]
    fn instant_timing_never_delays() {
        let mut f = Floor::new(Duration::ZERO);
        let t = Instant::now();
        f.atc_start(F, t, 5 * SEC, &RadioTiming::instant());
        assert!(!f.is_busy(F, t));
        assert_eq!(f.atc_may_start(F, t), None);
        assert_eq!(f.pilot_press(F, 1, t), PttOutcome::Granted);
    }
}
