//! Compliance monitoring: detects deviations from the permissions issued to the aircraft.

use atc_geo::{LatLon, RunwayFrame, distance_m};
use atc_phraseology::CircuitLeg;

use crate::ifr::{Path, cross_track_m};
use crate::phase::FlightPhase;
use crate::state::AircraftState;

/// Margin before the hold line that already counts as an incursion.
const HOLD_LINE_MARGIN_M: f64 = 5.0;
/// Ground speed above which a roll on the runway counts as a takeoff.
const TAKEOFF_GS_KT: f64 = 40.0;
/// Altitude error tolerated before a deviation is counted.
const ALT_TOLERANCE_FT: f64 = 200.0;
/// An altitude error must persist this long before it is reported.
const ALT_PERSIST_S: f64 = 10.0;
/// A longer silence between samples is a telemetry gap.
const MAX_DT_S: f64 = 5.0;
/// Position slack added to the plausible travel distance between samples.
const JUMP_SLACK_M: f64 = 500.0;
/// Extra width beyond the runway half-width that still counts as on the runway.
const RUNWAY_MARGIN_M: f64 = 10.0;
const KT_TO_MS: f64 = 1852.0 / 3600.0;
/// Distance off the cleared procedure path that counts as off track ...
const OFF_TRACK_M: f64 = 1852.0;
/// ... once it has lasted this long.
const OFF_TRACK_PERSIST_S: f64 = 20.0;
/// Heading error tolerated on an assigned heading ...
const HEADING_TOL_DEG: f64 = 20.0;
/// ... before it has lasted this long.
const HEADING_PERSIST_S: f64 = 30.0;
/// The heading gap closing at least this fast (degrees per second): turning onto it.
const TURNING_DEG_S: f64 = 1.0;

/// What the aircraft is currently allowed to do.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Permissions {
    pub may_enter_runway: bool,
    pub may_take_off: bool,
    pub may_land: bool,
    pub assigned_alt_ft: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Deviation {
    RunwayIncursion {
        runway: String,
    },
    TakeoffWithoutClearance,
    LandingWithoutClearance,
    AltitudeDeviation {
        assigned: i32,
        actual: i32,
    },
    /// More than 1 NM off the path of the cleared procedure (ident) for 20 s.
    OffTrack {
        procedure: String,
        metres: u32,
    },
    /// More than 20 degrees off the assigned magnetic heading for 30 s.
    HeadingDeviation {
        assigned: u16,
        actual: u16,
    },
}

/// What an airborne aircraft off the circuit (`FlightPhase::Airborne`) has been cleared to
/// fly, as monitored by [`ComplianceMonitor`]. Each deviation from it is reported once per
/// instruction: a changed value re-arms its check.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IfrClearance {
    /// Cleared procedure (ident) and the path it is flown along.
    pub procedure: Option<(String, Path)>,
    /// Assigned heading, magnetic degrees.
    pub heading_deg: Option<u16>,
    /// Cleared level, ft.
    pub level_ft: Option<i32>,
}

/// One deviation check for one instruction (`key`): reported once when the deviation has
/// lasted long enough, and not again until the instruction changes.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Latch<K> {
    key: Option<K>,
    since: Option<f64>,
    reported: bool,
}

impl<K> Default for Latch<K> {
    fn default() -> Self {
        Latch {
            key: None,
            since: None,
            reported: false,
        }
    }
}

impl<K: PartialEq> Latch<K> {
    /// Under instruction `key` (none: nothing to check), the deviation is present (`off`) at
    /// time `t`; `true` once it has lasted `persist_s`.
    fn update(&mut self, key: Option<K>, off: bool, t: f64, persist_s: f64) -> bool {
        if self.key != key {
            *self = Latch {
                key,
                since: None,
                reported: false,
            };
        }
        if self.key.is_none() || !off {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(t);
        if !self.reported && t - since >= persist_s {
            self.reported = true;
            return true;
        }
        false
    }
}

/// Cross distance (`min |right_m|`) of the nearest hold line of a runway: the minimum of
/// `|right|` over its holding-point positions; `None` without holding points.
pub fn hold_line_cross_m(rwy: &RunwayFrame, hold_points: &[LatLon]) -> Option<f64> {
    hold_points
        .iter()
        .map(|&p| rwy.to_local(p).1.abs())
        .min_by(f64::total_cmp)
}

/// Deviation detector for one aircraft and one active runway. Each deviation is reported once
/// and re-armed when its condition becomes false again.
///
/// Rules (all evaluated per sample):
/// - incursion: on the ground (raw flag, and the phase is not `Circuit`), with the limit replaced by the runway rectangle (half width + 10 m) in phases `Landed`/`TaxiIn` (vacating), `0 <= along <= length`, `|right| < hold_cross_m - 5 m`, and none
///   of `may_enter_runway` / `may_take_off` / `may_land` is set;
/// - takeoff: phase `OnRunway`/`TakeoffRoll`, on the ground on the runway (`|right| < half width`), gs > 40 kt, `!may_take_off`;
/// - landing: the phase goes from `Circuit` to `Landed` (the tracker's debounced touchdown,
///   not the raw `on_ground` flag, which flickers in the take-off roll) within the runway,
///   `!may_land`; re-armed once the phase is `Circuit` again;
/// - altitude: airborne (`Circuit` phase) with `assigned_alt_ft` set, `|alt - assigned| > 200 ft`
///   for at least 10 s; on `Downwind` the assigned altitude is replaced by the circuit
///   altitude;
/// - off the circuit (`Airborne` phase), against the [`IfrClearance`] set with
///   [`ComplianceMonitor::set_clearance`], each reported once per instruction: off track more
///   than 1 NM from the procedure path for 20 s; more than 20 degrees off the assigned heading
///   for 30 s (telemetry heading is true; it is compared in magnetic, see
///   [`ComplianceMonitor::with_magnetic_variation`]); more than 200 ft past the cleared level
///   on the far side from where the aircraft was when it was given (above it when climbing to
///   it, below it when descending to it), or more than 200 ft off it either way once it was
///   reached, for 10 s (climbing or descending to it is no deviation);
/// - telemetry gap (`dt > 5 s`, `dt <= 0`, or position jump `> 2 * gs * dt + 500 m`, gs being
///   the larger of the two samples): the altitude timer and the previous-sample memory are reset
///   and nothing is reported for that sample.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ComplianceMonitor {
    runway: String,
    rwy: RunwayFrame,
    length_m: f64,
    half_width_m: f64,
    incursion_limit_m: f64,
    circuit_alt_ft: f64,
    prev: Option<(f64, LatLon, f64, bool)>,
    prev_phase: Option<FlightPhase>,
    incursion: bool,
    takeoff: bool,
    landing: bool,
    alt_since: Option<f64>,
    alt_reported: bool,
    mag_var_deg: f64,
    clearance: IfrClearance,
    off_track: Latch<String>,
    heading: Latch<u16>,
    /// The heading gap of the previous sample and its time: an aircraft whose gap closes is
    /// turning onto the heading.
    heading_gap: Option<(f64, f64)>,
    level: Latch<i32>,
    /// The cleared level has been reached (within 200 ft) since it was given.
    level_reached: bool,
    /// The aircraft was above the cleared level at the first sample under it (descending to
    /// it); `None` until that sample.
    level_from_above: Option<bool>,
}

impl ComplianceMonitor {
    /// `rwy` is the frame of the active runway `runway` (ident, e.g. "25"), `length_m` and
    /// `half_width_m` its dimensions, `hold_cross_m` the cross distance of its nearest hold line
    /// (see [`hold_line_cross_m`]; the incursion threshold is that minus 5 m) and
    /// `circuit_alt_ft` the circuit altitude AMSL.
    pub fn new(
        runway: &str,
        rwy: RunwayFrame,
        length_m: f64,
        half_width_m: f64,
        hold_cross_m: f64,
        circuit_alt_ft: f64,
    ) -> Self {
        Self {
            runway: runway.to_owned(),
            rwy,
            length_m,
            half_width_m,
            incursion_limit_m: hold_cross_m - HOLD_LINE_MARGIN_M,
            circuit_alt_ft,
            prev: None,
            prev_phase: None,
            incursion: false,
            takeoff: false,
            landing: false,
            alt_since: None,
            alt_reported: false,
            mag_var_deg: 0.0,
            clearance: IfrClearance::default(),
            off_track: Latch::default(),
            heading: Latch::default(),
            heading_gap: None,
            level: Latch::default(),
            level_reached: false,
            level_from_above: None,
        }
    }

    /// Magnetic variation in degrees, east positive (`magnetic = true - variation`), for the
    /// heading check.
    pub fn with_magnetic_variation(mut self, deg: f64) -> Self {
        self.mag_var_deg = deg;
        self
    }

    /// What the aircraft is cleared to fly off the circuit, from this sample on.
    pub fn set_clearance(&mut self, c: IfrClearance) {
        if c.level_ft != self.clearance.level_ft {
            self.level_reached = false;
            self.level_from_above = None;
        }
        self.clearance = c;
    }

    pub fn update(
        &mut self,
        s: &AircraftState,
        phase: FlightPhase,
        perm: &Permissions,
    ) -> Vec<Deviation> {
        let prev = self.prev.replace((s.t.0, s.pos, s.gs_kt, s.on_ground));
        let prev_phase = self.prev_phase.replace(phase);
        if let Some((t, pos, gs, _)) = prev {
            let dt = s.t.0 - t;
            let reach = 2.0 * gs.max(s.gs_kt) * KT_TO_MS * dt + JUMP_SLACK_M;
            if dt <= 0.0 || dt > MAX_DT_S || distance_m(pos, s.pos) > reach {
                self.alt_since = None;
                self.alt_reported = false;
                for since in [
                    &mut self.off_track.since,
                    &mut self.heading.since,
                    &mut self.level.since,
                ] {
                    *since = None;
                }
                self.prev = Some((s.t.0, s.pos, s.gs_kt, s.on_ground));
                return Vec::new();
            }
        }
        let (along, right) = self.rwy.to_local(s.pos);
        let in_length = (0.0..=self.length_m).contains(&along);
        let mut out = Vec::new();

        // After landing the aircraft vacates through the hold-line zone: only the runway
        // rectangle itself counts there.
        let limit = if matches!(phase, FlightPhase::Landed | FlightPhase::TaxiIn) {
            self.half_width_m + RUNWAY_MARGIN_M
        } else {
            self.incursion_limit_m
        };
        // Ground per the tracker: a raw touchdown is not on the runway before the phase says so.
        let incursion = s.on_ground
            && !phase.is_airborne()
            && in_length
            && right.abs() < limit
            && !(perm.may_enter_runway || perm.may_take_off || perm.may_land);
        if incursion && !self.incursion {
            out.push(Deviation::RunwayIncursion {
                runway: self.runway.clone(),
            });
        }
        self.incursion = incursion;

        let takeoff = s.on_ground
            && matches!(phase, FlightPhase::OnRunway | FlightPhase::TakeoffRoll)
            && in_length
            && right.abs() < self.half_width_m
            && s.gs_kt > TAKEOFF_GS_KT
            && !perm.may_take_off;
        if takeoff && !self.takeoff {
            out.push(Deviation::TakeoffWithoutClearance);
        }
        self.takeoff = takeoff;

        if phase.is_airborne() {
            self.landing = false;
        }
        let touchdown =
            phase == FlightPhase::Landed && prev_phase.is_some_and(FlightPhase::is_airborne);
        if touchdown && in_length && right.abs() < self.half_width_m && !perm.may_land {
            if !self.landing {
                out.push(Deviation::LandingWithoutClearance);
            }
            self.landing = true;
        }

        self.check_altitude(s, phase, perm, &mut out);
        self.check_clearance(s, phase, &mut out);
        out
    }

    /// The [`IfrClearance`] checks (see the type docs); nothing outside `Airborne`.
    fn check_clearance(&mut self, s: &AircraftState, phase: FlightPhase, out: &mut Vec<Deviation>) {
        let t = s.t.0;
        let airborne = phase == FlightPhase::Airborne && !s.on_ground;
        let c = &self.clearance;

        let procedure = c.procedure.as_ref().filter(|_| airborne);
        let off_m = procedure.map_or(0.0, |(_, path)| cross_track_m(path, s.pos));
        let key = procedure.map(|(ident, _)| ident.clone());
        if self
            .off_track
            .update(key, off_m > OFF_TRACK_M, t, OFF_TRACK_PERSIST_S)
            && let Some((ident, _)) = procedure
        {
            out.push(Deviation::OffTrack {
                procedure: ident.clone(),
                metres: off_m.round() as u32,
            });
        }

        let assigned = c.heading_deg.filter(|_| airborne);
        let actual = (s.heading_deg - self.mag_var_deg).rem_euclid(360.0);
        let gap = assigned.map_or(0.0, |a| {
            ((actual - f64::from(a) + 540.0).rem_euclid(360.0) - 180.0).abs()
        });
        // Turning towards the assigned heading (the gap closing at a rate turn's pace or
        // more) is not a deviation: a large turn takes longer than the persistence time.
        let turning = self
            .heading_gap
            .is_some_and(|(prev, pt)| t > pt && (prev - gap) / (t - pt) >= TURNING_DEG_S);
        self.heading_gap = assigned.map(|_| (gap, t));
        if self.heading.update(
            assigned,
            gap > HEADING_TOL_DEG && !turning,
            t,
            HEADING_PERSIST_S,
        ) && let Some(a) = assigned
        {
            let actual = actual.round() as u16;
            out.push(Deviation::HeadingDeviation {
                assigned: a,
                actual: if actual == 0 { 360 } else { actual },
            });
        }

        let level = c.level_ft.filter(|_| airborne);
        let error = level.map_or(0.0, |l| s.alt_ft_msl - f64::from(l));
        if level.is_some() && error.abs() <= ALT_TOLERANCE_FT {
            self.level_reached = true;
        }
        if level.is_some() && self.level_from_above.is_none() {
            self.level_from_above = Some(error > ALT_TOLERANCE_FT);
        }
        let from_above = self.level_from_above == Some(true);
        let busted = if self.level_reached {
            error.abs() > ALT_TOLERANCE_FT
        } else if from_above {
            error < -ALT_TOLERANCE_FT
        } else {
            error > ALT_TOLERANCE_FT
        };
        if self.level.update(level, busted, t, ALT_PERSIST_S)
            && let Some(l) = level
        {
            out.push(Deviation::AltitudeDeviation {
                assigned: l,
                actual: s.alt_ft_msl.round() as i32,
            });
        }
    }

    fn check_altitude(
        &mut self,
        s: &AircraftState,
        phase: FlightPhase,
        perm: &Permissions,
        out: &mut Vec<Deviation>,
    ) {
        let assigned = match (phase, perm.assigned_alt_ft) {
            (FlightPhase::Circuit(_), Some(a)) if !s.on_ground => {
                if phase == FlightPhase::Circuit(CircuitLeg::Downwind) {
                    Some(self.circuit_alt_ft)
                } else {
                    Some(f64::from(a))
                }
            }
            _ => None,
        };
        let off = assigned.filter(|a| (s.alt_ft_msl - a).abs() > ALT_TOLERANCE_FT);
        let Some(assigned) = off else {
            self.alt_since = None;
            self.alt_reported = false;
            return;
        };
        let since = *self.alt_since.get_or_insert(s.t.0);
        if s.t.0 - since >= ALT_PERSIST_S && !self.alt_reported {
            self.alt_reported = true;
            out.push(Deviation::AltitudeDeviation {
                assigned: assigned.round() as i32,
                actual: s.alt_ft_msl.round() as i32,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SimTime;
    use crate::taxi::TaxiGraph;
    use atc_geo::destination;
    use atc_navdata::model::Airport;
    use atc_navdata::parse_apt_dat;
    use atc_phraseology::Callsign;

    const APT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/airports/EPKK/apt.dat"
    );

    struct Fixture {
        frame: RunwayFrame,
        cross: f64,
        mon: ComplianceMonitor,
    }

    fn fixture() -> Fixture {
        let ap: Airport = parse_apt_dat(&std::fs::read_to_string(APT).unwrap(), "EPKK").unwrap();
        let rw = ap
            .runways
            .iter()
            .find(|r| r.ends.iter().any(|e| e.ident == "25"))
            .unwrap();
        let i = rw.ends.iter().position(|e| e.ident == "25").unwrap();
        let (thr, opp) = (rw.ends[i].threshold, rw.ends[1 - i].threshold);
        let frame = RunwayFrame::new(thr, opp);
        let g = TaxiGraph::build(&ap.taxi);
        let holds: Vec<LatLon> = g
            .holding_points(&ap, "25")
            .into_iter()
            .map(|id| ap.taxi.nodes.iter().find(|n| n.id == id).unwrap().pos)
            .collect();
        let cross = hold_line_cross_m(&frame, &holds).unwrap();
        let mon = ComplianceMonitor::new(
            "25",
            frame,
            distance_m(thr, opp),
            rw.width_m / 2.0,
            cross,
            1800.0,
        );
        Fixture { frame, cross, mon }
    }

    fn at(f: &Fixture, along: f64, right: f64) -> LatLon {
        at_frame(f.frame, along, right)
    }

    fn at_frame(frame: RunwayFrame, along: f64, right: f64) -> LatLon {
        let p = destination(frame.origin, frame.course_deg, along);
        destination(p, frame.course_deg + 90.0, right)
    }

    fn st(t: f64, pos: LatLon, alt: f64, gs: f64, on_ground: bool) -> AircraftState {
        AircraftState {
            callsign: Callsign::new("SPABC"),
            t: SimTime(t),
            pos,
            alt_ft_msl: alt,
            heading_deg: 0.0,
            ias_kt: gs,
            gs_kt: gs,
            vs_fpm: 0.0,
            on_ground,
            com1_khz: 0,
            xpdr_code: None,
            xpdr_mode: None,
            utc_s: None,
            day_of_year: None,
        }
    }

    fn none() -> Permissions {
        Permissions {
            may_enter_runway: false,
            may_take_off: false,
            may_land: false,
            assigned_alt_ft: None,
        }
    }

    const TAXI: FlightPhase = FlightPhase::TaxiOut;

    #[test]
    fn incursion_detected_past_hold_line() {
        let mut f = fixture();
        assert!(f.cross > 20.0, "cross = {}", f.cross);
        // Short of the threshold line: nothing.
        let p = at(&f, 500.0, f.cross - 3.0);
        assert!(
            f.mon
                .update(&st(0.0, p, 800.0, 5.0, true), TAXI, &none())
                .is_empty()
        );
        // Past it: once, then silent while it persists.
        let p = at(&f, 500.0, f.cross - 10.0);
        let dev = Deviation::RunwayIncursion {
            runway: "25".into(),
        };
        assert_eq!(
            f.mon.update(&st(1.0, p, 800.0, 5.0, true), TAXI, &none()),
            vec![dev.clone()]
        );
        assert!(
            f.mon
                .update(&st(2.0, p, 800.0, 5.0, true), TAXI, &none())
                .is_empty()
        );
        // Resolved by leaving, then it fires again on re-entry.
        let out = at(&f, 500.0, f.cross + 20.0);
        assert!(
            f.mon
                .update(&st(3.0, out, 800.0, 5.0, true), TAXI, &none())
                .is_empty()
        );
        assert_eq!(
            f.mon.update(&st(4.0, p, 800.0, 5.0, true), TAXI, &none()),
            vec![dev]
        );
    }

    #[test]
    fn no_incursion_when_cleared() {
        let mut f = fixture();
        let p = at(&f, 500.0, 0.0);
        let perm = Permissions {
            may_enter_runway: true,
            ..none()
        };
        assert!(
            f.mon
                .update(&st(0.0, p, 800.0, 5.0, true), TAXI, &perm)
                .is_empty()
        );
        assert!(
            f.mon
                .update(&st(1.0, p, 800.0, 5.0, true), TAXI, &perm)
                .is_empty()
        );
    }

    #[test]
    fn takeoff_without_clearance() {
        let mut f = fixture();
        let perm = Permissions {
            may_enter_runway: true,
            ..none()
        };
        let fr = f.frame;
        let p = |a: f64| at_frame(fr, a, 0.0);
        assert!(
            f.mon
                .update(
                    &st(0.0, p(100.0), 791.0, 20.0, true),
                    FlightPhase::OnRunway,
                    &perm
                )
                .is_empty()
        );
        // Below 40 kt: not yet.
        assert!(
            f.mon
                .update(
                    &st(1.0, p(150.0), 791.0, 39.0, true),
                    FlightPhase::TakeoffRoll,
                    &perm
                )
                .is_empty()
        );
        let got = f.mon.update(
            &st(2.0, p(200.0), 791.0, 45.0, true),
            FlightPhase::TakeoffRoll,
            &perm,
        );
        assert_eq!(got, vec![Deviation::TakeoffWithoutClearance]);
        assert!(
            f.mon
                .update(
                    &st(3.0, p(260.0), 791.0, 60.0, true),
                    FlightPhase::TakeoffRoll,
                    &perm
                )
                .is_empty()
        );
        // With clearance: nothing.
        let mut f = fixture();
        let perm = Permissions {
            may_enter_runway: true,
            may_take_off: true,
            ..none()
        };
        assert!(
            f.mon
                .update(
                    &st(0.0, at(&f, 200.0, 0.0), 791.0, 60.0, true),
                    FlightPhase::TakeoffRoll,
                    &perm
                )
                .is_empty()
        );
    }

    /// A wheel leaving the ground for a moment in the take-off roll is no landing: the
    /// phase stays `TakeoffRoll`.
    #[test]
    fn takeoff_roll_bounce_is_not_a_landing() {
        let mut f = fixture();
        let perm = Permissions {
            may_enter_runway: true,
            may_take_off: true,
            ..none()
        };
        let roll = FlightPhase::TakeoffRoll;
        for (t, ground) in [
            (0.0, true),
            (0.5, false),
            (1.0, true),
            (1.5, false),
            (2.0, true),
        ] {
            let s = st(t, at(&f, 300.0 + 20.0 * t, 0.0), 791.0, 45.0, ground);
            assert!(f.mon.update(&s, roll, &perm).is_empty(), "t = {t}");
        }
    }

    #[test]
    fn landing_without_clearance() {
        let mut f = fixture();
        let up = FlightPhase::Circuit(CircuitLeg::Final);
        let perm = none();
        assert!(
            f.mon
                .update(&st(0.0, at(&f, -200.0, 0.0), 900.0, 70.0, false), up, &perm)
                .is_empty()
        );
        let got = f.mon.update(
            &st(1.0, at(&f, 300.0, 0.0), 791.0, 65.0, true),
            FlightPhase::Landed,
            &perm,
        );
        // Per ruling an uncleared landing also sits on the runway rectangle: both are reported.
        assert_eq!(
            got,
            vec![
                Deviation::RunwayIncursion {
                    runway: "25".into()
                },
                Deviation::LandingWithoutClearance
            ]
        );
        assert!(
            f.mon
                .update(
                    &st(2.0, at(&f, 400.0, 0.0), 791.0, 60.0, true),
                    FlightPhase::Landed,
                    &perm
                )
                .is_empty()
        );
        // Cleared landing: nothing.
        let mut f = fixture();
        let perm = Permissions {
            may_land: true,
            ..none()
        };
        f.mon
            .update(&st(0.0, at(&f, -200.0, 0.0), 900.0, 70.0, false), up, &perm);
        assert!(
            f.mon
                .update(
                    &st(1.0, at(&f, 300.0, 0.0), 791.0, 65.0, true),
                    FlightPhase::Landed,
                    &perm
                )
                .is_empty()
        );
    }

    #[test]
    fn altitude_deviation_needs_10s() {
        let mut f = fixture();
        let ph = FlightPhase::Circuit(CircuitLeg::Upwind);
        let perm = Permissions {
            assigned_alt_ft: Some(1800),
            ..none()
        };
        let p = at(&f, 3000.0, 0.0);
        for t in 0..10 {
            let got = f
                .mon
                .update(&st(f64::from(t), p, 2100.0, 80.0, false), ph, &perm);
            assert!(got.is_empty(), "t = {t}");
        }
        let got = f.mon.update(&st(10.0, p, 2100.4, 80.0, false), ph, &perm);
        assert_eq!(
            got,
            vec![Deviation::AltitudeDeviation {
                assigned: 1800,
                actual: 2100
            }]
        );
        assert!(
            f.mon
                .update(&st(11.0, p, 2100.0, 80.0, false), ph, &perm)
                .is_empty()
        );
        // Back within 200 ft resets the timer; a short excursion stays silent.
        assert!(
            f.mon
                .update(&st(12.0, p, 1900.0, 80.0, false), ph, &perm)
                .is_empty()
        );
        assert!(
            f.mon
                .update(&st(13.0, p, 2100.0, 80.0, false), ph, &perm)
                .is_empty()
        );
        assert!(
            f.mon
                .update(&st(17.0, p, 2100.0, 80.0, false), ph, &perm)
                .is_empty()
        );
        assert!(
            f.mon
                .update(&st(20.0, p, 2100.0, 80.0, false), ph, &perm)
                .is_empty()
        );
        assert_eq!(
            f.mon
                .update(&st(23.0, p, 2100.0, 80.0, false), ph, &perm)
                .len(),
            1
        );
    }

    #[test]
    fn altitude_uses_circuit_altitude_on_downwind_and_needs_assignment() {
        let mut f = fixture();
        let down = FlightPhase::Circuit(CircuitLeg::Downwind);
        let p = at(&f, 500.0, -1500.0);
        let perm = Permissions {
            assigned_alt_ft: Some(3000),
            ..none()
        };
        for t in 0..10 {
            f.mon
                .update(&st(f64::from(t), p, 2100.0, 80.0, false), down, &perm);
        }
        let got = f.mon.update(&st(10.0, p, 2100.0, 80.0, false), down, &perm);
        assert_eq!(
            got,
            vec![Deviation::AltitudeDeviation {
                assigned: 1800,
                actual: 2100
            }]
        );
        // No assignment, or on the ground: never.
        let mut f = fixture();
        for t in 0..20 {
            assert!(
                f.mon
                    .update(&st(f64::from(t), p, 2500.0, 80.0, false), down, &none())
                    .is_empty()
            );
        }
        let perm = Permissions {
            assigned_alt_ft: Some(3000),
            ..none()
        };
        for t in 0..20 {
            let s = st(f64::from(t), at(&f, 500.0, 500.0), 800.0, 5.0, true);
            assert!(f.mon.update(&s, TAXI, &perm).is_empty());
        }
    }

    #[test]
    fn teleport_resets_phase_without_deviation() {
        let mut f = fixture();
        let far = at(&f, 20_000.0, 0.0);
        let rwy = at(&f, 500.0, 0.0);
        assert!(
            f.mon
                .update(&st(0.0, far, 800.0, 5.0, true), TAXI, &none())
                .is_empty()
        );
        // 20 km jump in 1 s.
        assert!(
            f.mon
                .update(&st(1.0, rwy, 800.0, 5.0, true), TAXI, &none())
                .is_empty()
        );
        // Pause: identical t.
        assert!(
            f.mon
                .update(&st(1.0, rwy, 800.0, 5.0, true), TAXI, &none())
                .is_empty()
        );
        // Time going backwards.
        assert!(
            f.mon
                .update(&st(0.5, rwy, 800.0, 5.0, true), TAXI, &none())
                .is_empty()
        );
        // Long silence.
        assert!(
            f.mon
                .update(&st(60.0, rwy, 800.0, 5.0, true), TAXI, &none())
                .is_empty()
        );
        // Normal samples resume monitoring.
        assert_eq!(
            f.mon
                .update(&st(61.0, rwy, 800.0, 5.0, true), TAXI, &none())
                .len(),
            1
        );
    }

    #[test]
    fn gap_resets_altitude_timer() {
        let mut f = fixture();
        let ph = FlightPhase::Circuit(CircuitLeg::Upwind);
        let perm = Permissions {
            assigned_alt_ft: Some(1800),
            ..none()
        };
        let p = at(&f, 3000.0, 0.0);
        for t in 0..8 {
            f.mon
                .update(&st(f64::from(t), p, 2100.0, 80.0, false), ph, &perm);
        }
        // 30 s silence, then samples: the timer restarts at the post-gap sample.
        assert!(
            f.mon
                .update(&st(40.0, p, 2100.0, 80.0, false), ph, &perm)
                .is_empty()
        );
        assert!(
            f.mon
                .update(&st(44.0, p, 2100.0, 80.0, false), ph, &perm)
                .is_empty()
        );
        for t in [48.0, 52.0] {
            let got = f.mon.update(&st(t, p, 2100.0, 80.0, false), ph, &perm);
            assert!(got.is_empty(), "t = {t}");
        }
        let got = f.mon.update(&st(54.0, p, 2100.0, 80.0, false), ph, &perm);
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn touchdown_with_landing_clearance_is_clean() {
        let mut f = fixture();
        let fin = FlightPhase::Circuit(CircuitLeg::Final);
        let perm = Permissions {
            may_land: true,
            ..none()
        };
        f.mon.update(
            &st(0.0, at(&f, -200.0, 0.0), 900.0, 70.0, false),
            fin,
            &perm,
        );
        // Phase is still debounced Circuit(Final) at touchdown, gs ~60 kt.
        let got = f
            .mon
            .update(&st(1.0, at(&f, 300.0, 0.0), 791.0, 60.0, true), fin, &perm);
        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn vacating_after_landing_is_not_an_incursion() {
        let mut f = fixture();
        let perm = none();
        // Between the runway rectangle and the hold line.
        let between = at(&f, 800.0, f.cross - 10.0);
        assert!(
            f.mon
                .update(
                    &st(0.0, between, 791.0, 10.0, true),
                    FlightPhase::TaxiIn,
                    &perm
                )
                .is_empty()
        );
        // Back onto the rectangle: incursion.
        let on = at(&f, 800.0, 0.0);
        let got = f
            .mon
            .update(&st(1.0, on, 791.0, 10.0, true), FlightPhase::TaxiIn, &perm);
        assert_eq!(
            got,
            vec![Deviation::RunwayIncursion {
                runway: "25".into()
            }]
        );
    }

    /// Off the circuit at time `t` on the [`east`] track `north_m` to its north, at `alt`
    /// heading `hdg` (true).
    fn airborne(f: &mut Fixture, t: f64, north_m: f64, alt: f64, hdg: f64) -> Vec<Deviation> {
        let pos = east(f, t, north_m);
        let mut s = st(t, pos, alt, 100.0, false);
        s.heading_deg = hdg;
        f.mon.update(&s, FlightPhase::Airborne, &none())
    }

    /// A track 20 km north of the threshold, flown east at 100 kt (51.4 m/s).
    fn east(f: &Fixture, t: f64, north_m: f64) -> LatLon {
        let base = destination(f.frame.origin, 0.0, 20_000.0 + north_m);
        destination(base, 90.0, t * 51.4)
    }

    fn path_east(f: &Fixture) -> Path {
        let a = destination(f.frame.origin, 0.0, 20_000.0);
        Path {
            points: vec![
                ("A".into(), a),
                ("B".into(), destination(a, 90.0, 50_000.0)),
            ],
            constraints: Vec::new(),
        }
    }

    #[test]
    fn off_track_reported_once_per_procedure() {
        let mut f = fixture();
        f.mon.set_clearance(IfrClearance {
            procedure: Some(("ALFA1A".into(), path_east(&f))),
            ..IfrClearance::default()
        });
        let mut got = Vec::new();
        // 1.5 NM off for 40 s, back on track, off again: one call.
        for k in 0..60 {
            let t = f64::from(k) * 2.0;
            let off = if (20..30).contains(&k) { 0.0 } else { 2800.0 };
            got.extend(airborne(&mut f, t, off, 3000.0, 90.0));
        }
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(matches!(
            &got[0],
            Deviation::OffTrack { procedure, metres } if procedure == "ALFA1A" && (2700..2900).contains(metres)
        ));
        // A new procedure re-arms it.
        f.mon.set_clearance(IfrClearance {
            procedure: Some(("BRAV1A".into(), path_east(&f))),
            ..IfrClearance::default()
        });
        let mut again = Vec::new();
        for k in 60..80 {
            let t = f64::from(k) * 2.0;
            again.extend(airborne(&mut f, t, 2800.0, 3000.0, 90.0));
        }
        assert_eq!(again.len(), 1, "{again:?}");
    }

    #[test]
    fn heading_deviation_after_thirty_seconds_in_magnetic() {
        let mut f = fixture();
        f.mon = f.mon.clone().with_magnetic_variation(6.0);
        f.mon.set_clearance(IfrClearance {
            heading_deg: Some(90),
            ..IfrClearance::default()
        });
        // True 96 is magnetic 090: on heading.
        let mut got = Vec::new();
        for k in 0..20 {
            let t = f64::from(k) * 2.0;
            got.extend(airborne(&mut f, t, 0.0, 3000.0, 96.0));
        }
        assert!(got.is_empty(), "{got:?}");
        // True 126 (magnetic 120) for 28 s: nothing; at 30 s: one deviation; then silent.
        for k in 20..60 {
            let t = f64::from(k) * 2.0;
            let d = airborne(&mut f, t, 0.0, 3000.0, 126.0);
            if k < 35 {
                assert!(d.is_empty(), "k = {k}: {d:?}");
            }
            got.extend(d);
        }
        assert_eq!(
            got,
            vec![Deviation::HeadingDeviation {
                assigned: 90,
                actual: 120
            }]
        );
    }

    /// A turn of 180 degrees at the standard rate takes a minute: no deviation while the
    /// aircraft is turning towards the assigned heading.
    #[test]
    fn turning_onto_the_heading_is_no_deviation() {
        let mut f = fixture();
        f.mon.set_clearance(IfrClearance {
            heading_deg: Some(270),
            ..IfrClearance::default()
        });
        let mut got = Vec::new();
        // From 090, 3 degrees per second to the left: 60 s, then on 270.
        for k in 0..45 {
            let t = f64::from(k) * 2.0;
            let hdg = (90.0 - 6.0 * f64::from(k)).max(-90.0).rem_euclid(360.0);
            got.extend(airborne(&mut f, t, 0.0, 3000.0, hdg));
        }
        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn cleared_level_climb_is_no_deviation_bust_and_descent_are() {
        let mut f = fixture();
        f.mon.set_clearance(IfrClearance {
            level_ft: Some(5000),
            ..IfrClearance::default()
        });
        let mut t = 0.0;
        let mut fly = |f: &mut Fixture, alt: f64, n: usize| {
            let mut out = Vec::new();
            for _ in 0..n {
                out.extend(airborne(f, t, 0.0, alt, 90.0));
                t += 2.0;
            }
            out
        };
        // Climbing below the level: nothing.
        assert!(fly(&mut f, 3000.0, 20).is_empty());
        // Level, then 400 ft low for 20 s: one call, not repeated.
        assert!(fly(&mut f, 5000.0, 5).is_empty());
        let low = fly(&mut f, 4600.0, 20);
        assert_eq!(
            low,
            vec![Deviation::AltitudeDeviation {
                assigned: 5000,
                actual: 4600
            }]
        );
        // Back, and low again: still the same instruction, no second call.
        assert!(fly(&mut f, 5000.0, 5).is_empty());
        assert!(fly(&mut f, 4600.0, 20).is_empty());
        // A new level: climbing to it is fine, overshooting it is not.
        f.mon.set_clearance(IfrClearance {
            level_ft: Some(7000),
            ..IfrClearance::default()
        });
        assert!(fly(&mut f, 6000.0, 10).is_empty());
        assert_eq!(fly(&mut f, 7400.0, 10).len(), 1);
    }

    /// A descent to a new level: above it is fine until it is reached, going through it is
    /// a bust, and so is leaving it either way once reached.
    #[test]
    fn descent_to_cleared_level_is_no_deviation() {
        let mut f = fixture();
        f.mon.set_clearance(IfrClearance {
            level_ft: Some(3000),
            ..IfrClearance::default()
        });
        let mut t = 0.0;
        let mut fly = |f: &mut Fixture, alt: f64, n: usize| {
            let mut out = Vec::new();
            for _ in 0..n {
                out.extend(airborne(f, t, 0.0, alt, 90.0));
                t += 2.0;
            }
            out
        };
        assert!(fly(&mut f, 5000.0, 20).is_empty());
        assert!(fly(&mut f, 4000.0, 20).is_empty());
        assert_eq!(fly(&mut f, 2600.0, 10).len(), 1);
        // Reached: above it is a bust now.
        f.mon.set_clearance(IfrClearance {
            level_ft: Some(2500),
            ..IfrClearance::default()
        });
        assert!(fly(&mut f, 2600.0, 10).is_empty());
        assert_eq!(fly(&mut f, 3000.0, 10).len(), 1);
    }
}
