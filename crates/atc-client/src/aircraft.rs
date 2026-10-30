//! The aircraft the sim reports (type and tail number) and a debounce for changes of it.
use std::time::{Duration, Instant};

/// What X-Plane says about the loaded aircraft; `None` is unknown or empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SimAircraft {
    pub icao: Option<String>,
    pub tail: Option<String>,
}

/// Passes a value on once it has been offered unchanged for `stable` and differs from the
/// last one passed on. The sim briefly reports odd values while an aircraft loads.
#[derive(Debug)]
pub struct ChangeFilter {
    stable: Duration,
    /// The value being offered and since when.
    candidate: Option<(SimAircraft, Instant)>,
    returned: Option<SimAircraft>,
}

impl ChangeFilter {
    pub fn new(stable: Duration) -> Self {
        Self {
            stable,
            candidate: None,
            returned: None,
        }
    }

    pub fn offer(&mut self, now: Instant, a: SimAircraft) -> Option<SimAircraft> {
        let since = match &self.candidate {
            Some((c, since)) if *c == a => *since,
            _ => {
                self.candidate = Some((a.clone(), now));
                now
            }
        };
        if now.duration_since(since) >= self.stable && self.returned.as_ref() != Some(&a) {
            self.returned = Some(a.clone());
            return Some(a);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ac(icao: &str) -> SimAircraft {
        SimAircraft {
            icao: Some(icao.into()),
            tail: None,
        }
    }

    const STABLE: Duration = Duration::from_secs(2);

    #[test]
    fn filter_returns_first_stable_value() {
        let t0 = Instant::now();
        let mut f = ChangeFilter::new(STABLE);
        assert_eq!(f.offer(t0, ac("C172")), None);
        assert_eq!(f.offer(t0 + Duration::from_secs(1), ac("C172")), None);
        assert_eq!(
            f.offer(t0 + Duration::from_secs(2), ac("C172")),
            Some(ac("C172"))
        );
        // Already returned: not again.
        assert_eq!(f.offer(t0 + Duration::from_secs(3), ac("C172")), None);
    }

    #[test]
    fn flicker_shorter_than_2s_is_ignored() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut f = ChangeFilter::new(STABLE);
        f.offer(t0, ac("C172"));
        assert!(f.offer(t0 + s(2), ac("C172")).is_some());
        assert_eq!(f.offer(t0 + s(3), ac("B738")), None);
        assert_eq!(f.offer(t0 + s(4), ac("B738")), None);
        assert_eq!(f.offer(t0 + s(4), ac("C172")), None);
        assert_eq!(f.offer(t0 + s(9), ac("C172")), None);
    }

    #[test]
    fn change_after_2s_is_returned() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut f = ChangeFilter::new(STABLE);
        f.offer(t0, ac("C172"));
        assert!(f.offer(t0 + s(2), ac("C172")).is_some());
        assert_eq!(f.offer(t0 + s(5), ac("B738")), None);
        assert_eq!(f.offer(t0 + s(7), ac("B738")), Some(ac("B738")));
    }
}
