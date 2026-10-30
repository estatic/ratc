//! The aircraft the sim reports (type and tail number), a debounce for changes of it, and
//! the identity (callsign and type) the client presents to the server.
use std::time::{Duration, Instant};

use atc_proto::CallsignSource;
use tokio::sync::watch;

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

/// Type sent while X-Plane has not reported one.
pub const UNKNOWN_TYPE: &str = "ZZZZ";

/// Printed (once) when there is neither a config callsign nor a tail number.
pub const MISSING_CALLSIGN: &str =
    "X-Plane has no tail number for this aircraft; set callsign in config/client.toml";

/// Appended to [`MISSING_CALLSIGN`] when a connection under another identity exists.
pub fn still_flying(callsign: &str, aircraft_type: &str) -> String {
    format!("{MISSING_CALLSIGN}; still flying as {callsign} ({aircraft_type})")
}

/// Who the client tells the server it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub callsign: String,
    pub aircraft_type: String,
    pub source: CallsignSource,
}

/// No callsign in the config and no tail number from the sim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingCallsign;

/// The config callsign wins; otherwise the sim tail number. Unknown type is `ZZZZ`.
pub fn resolve(
    config_callsign: Option<&str>,
    sim: &SimAircraft,
) -> Result<Identity, MissingCallsign> {
    let (callsign, source) = match (config_callsign, sim.tail.as_deref()) {
        (Some(c), _) => (c, CallsignSource::Config),
        (None, Some(t)) => (t, CallsignSource::Sim),
        (None, None) => return Err(MissingCallsign),
    };
    Ok(Identity {
        callsign: callsign.to_string(),
        aircraft_type: sim.icao.clone().unwrap_or_else(|| UNKNOWN_TYPE.into()),
        source,
    })
}

/// Debounces what the sim reports, resolves it and publishes the identity for the connection.
pub struct IdentityTracker {
    config_callsign: Option<String>,
    filter: ChangeFilter,
    /// The latest sim report, re-offered on every [`IdentityTracker::tick`].
    last: Option<SimAircraft>,
    tx: watch::Sender<Option<Identity>>,
    /// The sim aircraft the missing-callsign line was last printed for.
    missing_for: Option<SimAircraft>,
}

impl IdentityTracker {
    pub fn new(config_callsign: Option<String>, tx: watch::Sender<Option<Identity>>) -> Self {
        Self {
            config_callsign,
            filter: ChangeFilter::new(Duration::from_secs(2)),
            last: None,
            tx,
            missing_for: None,
        }
    }

    /// The connection is held back for want of the sim's aircraft: nothing reported yet and
    /// no identity to start with.
    pub fn waiting_for_sim(&self) -> bool {
        self.last.is_none() && self.tx.borrow().is_none()
    }

    /// A new report from the sim. Returns a line to show, if any.
    pub fn observe(&mut self, now: Instant, mut sim: SimAircraft) -> Option<String> {
        if self.config_callsign.is_some() {
            // Only the type matters; the tail number must not restart the debounce.
            sim.tail = None;
        }
        self.last = Some(sim);
        self.tick(now)
    }

    /// Re-offers the latest report so a value that has become stable is released although
    /// the sim sent nothing new (it updates at 1 Hz).
    pub fn tick(&mut self, now: Instant) -> Option<String> {
        let sim = self.filter.offer(now, self.last.clone()?)?;
        match resolve(self.config_callsign.as_deref(), &sim) {
            Ok(id) => {
                self.missing_for = None;
                self.tx.send_replace(Some(id));
                None
            }
            Err(MissingCallsign) if self.missing_for.as_ref() != Some(&sim) => {
                self.missing_for = Some(sim);
                Some(match &*self.tx.borrow() {
                    Some(id) => still_flying(&id.callsign, &id.aircraft_type),
                    None => MISSING_CALLSIGN.to_string(),
                })
            }
            Err(MissingCallsign) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim(icao: Option<&str>, tail: Option<&str>) -> SimAircraft {
        SimAircraft {
            icao: icao.map(Into::into),
            tail: tail.map(Into::into),
        }
    }

    #[test]
    fn resolve_prefers_config_callsign() {
        let id = resolve(Some("SP-ABC"), &sim(Some("C172"), Some("N123"))).unwrap();
        assert_eq!(id.callsign, "SP-ABC");
        assert_eq!(id.aircraft_type, "C172");
        assert_eq!(id.source, CallsignSource::Config);
    }

    #[test]
    fn resolve_uses_tail_number() {
        let id = resolve(None, &sim(Some("B738"), Some("SP-LWA"))).unwrap();
        assert_eq!(id.callsign, "SP-LWA");
        assert_eq!(id.source, CallsignSource::Sim);
    }

    #[test]
    fn resolve_without_any_callsign_is_missing() {
        assert_eq!(
            resolve(None, &sim(Some("C172"), None)),
            Err(MissingCallsign)
        );
    }

    #[test]
    fn unknown_type_is_zzzz() {
        let id = resolve(Some("SP-ABC"), &sim(None, None)).unwrap();
        assert_eq!(id.aircraft_type, "ZZZZ");
    }

    #[test]
    fn tracker_releases_a_stable_value_on_ticks_alone() {
        let (tx, rx) = watch::channel(None);
        let mut t = IdentityTracker::new(None, tx);
        let t0 = Instant::now();
        let s = Duration::from_secs;
        assert_eq!(t.observe(t0, sim(Some("C1"), Some("SP-A"))), None);
        // The prefix is replaced before it was stable.
        assert_eq!(
            t.observe(t0 + s(1), sim(Some("C172"), Some("SP-ABC"))),
            None
        );
        assert_eq!(*rx.borrow(), None);
        t.tick(t0 + s(2));
        assert_eq!(*rx.borrow(), None);
        t.tick(t0 + s(3));
        assert_eq!(rx.borrow().as_ref().unwrap().callsign, "SP-ABC");
    }

    #[test]
    fn tracker_reports_a_missing_callsign_once() {
        let (tx, rx) = watch::channel(None);
        let mut t = IdentityTracker::new(None, tx);
        let t0 = Instant::now();
        t.observe(t0, sim(Some("C172"), None));
        assert_eq!(
            t.tick(t0 + Duration::from_secs(2)),
            Some(MISSING_CALLSIGN.to_string())
        );
        assert_eq!(t.tick(t0 + Duration::from_secs(3)), None);
        assert_eq!(*rx.borrow(), None);
    }

    #[test]
    fn missing_callsign_mid_session_says_still_flying() {
        let (tx, rx) = watch::channel(None);
        let mut t = IdentityTracker::new(None, tx);
        let t0 = Instant::now();
        let s = Duration::from_secs;
        t.observe(t0, sim(Some("C172"), Some("SP-ABC")));
        t.tick(t0 + s(2));
        assert!(rx.borrow().is_some());
        t.observe(t0 + s(3), sim(Some("B738"), None));
        assert_eq!(
            t.tick(t0 + s(5)).as_deref(),
            Some(
                "X-Plane has no tail number for this aircraft; set callsign in config/client.toml; still flying as SP-ABC (C172)"
            )
        );
        assert_eq!(t.tick(t0 + s(6)), None);
        // Another aircraft without a tail number is reported again.
        t.observe(t0 + s(7), sim(Some("A320"), None));
        assert!(t.tick(t0 + s(9)).is_some());
    }

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
