//! Flight phase and circuit-leg tracking from telemetry samples.

use atc_geo::{LatLon, RunwayFrame, distance_m};
use atc_navdata::procedures::CircuitSide;
use atc_phraseology::CircuitLeg;

use crate::state::AircraftState;

/// A stop at a holding point stays `HoldingShort` while the aircraft moves off it towards the
/// runway, until it enters the runway rectangle or is farther than this from every holding point.
const HOLD_LATCH_M: f64 = 250.0;
/// A ground/air transition is accepted only after the new state has lasted this long.
const DEBOUNCE_S: f64 = 2.0;
/// A landing counts only after this long airborne ...
const MIN_AIRBORNE_S: f64 = 10.0;
/// ... or after having climbed this far above the field.
const MIN_HEIGHT_FT: f64 = 300.0;
/// Radius around a holding point / stand within which a stopped aircraft counts as "at" it.
const NEAR_POINT_M: f64 = 30.0;
/// Extra width beyond the runway half-width that still counts as "on the runway".
const RUNWAY_MARGIN_M: f64 = 10.0;
/// Below this ground speed the aircraft is stationary.
pub(crate) const STOPPED_KT: f64 = 1.0;
/// At or above this ground speed on the runway the aircraft is in its takeoff roll.
const TAKEOFF_ROLL_KT: f64 = 30.0;
/// Half-width of the strip around the extended centreline (upwind / final legs).
const CENTRAL_STRIP_M: f64 = 300.0;
/// Heading tolerance when matching a leg direction.
const HEADING_TOL_DEG: f64 = 30.0;
/// Consecutive samples required before the circuit leg changes.
const CONFIRM_SAMPLES: u8 = 3;
/// Off the circuit, the extended centreline counts as final up to this far out ...
const FINAL_MAX_M: f64 = 10.0 * 1852.0;
/// ... and this far to either side of it.
const FINAL_HALF_WIDTH_M: f64 = 0.5 * 1852.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FlightPhase {
    Parked,
    TaxiOut,
    HoldingShort,
    OnRunway,
    TakeoffRoll,
    Circuit(CircuitLeg),
    /// Airborne and not flying the circuit: an IFR flight, or a VFR flight leaving or joining
    /// via points.
    Airborne,
    Landed,
    TaxiIn,
}

impl FlightPhase {
    /// In the air: in the circuit or not.
    pub fn is_airborne(self) -> bool {
        matches!(self, FlightPhase::Circuit(_) | FlightPhase::Airborne)
    }
}

/// Signed difference `a - b` in degrees, in `(-180, 180]`.
fn angle_diff(a: f64, b: f64) -> f64 {
    180.0 - (b - a + 180.0).rem_euclid(360.0)
}

/// Deterministic phase classifier for one aircraft and one active runway.
///
/// Ground rules (evaluated per sample, `rect` = runway rectangle: `0 <= along <= L` and
/// `|right| < half width + 10 m`; "stopped" = gs < 1 kt):
/// - Before the first airborne sample (departure cycle):
///   - stopped and within 30 m of a holding point of the active runway: `HoldingShort`
///     (latched while moving on from it towards the runway: it ends on entering `rect` or when
///     more than 250 m from every holding point);
///   - in `rect`: `TakeoffRoll` if gs >= 30 kt; else `OnRunway`;
///   - stopped and never moved yet in this cycle: `Parked`;
///   - otherwise: `TaxiOut` (moving, or stopped after having moved).
/// - After an airborne sample, on the ground (arrival):
///   - in `rect` and not yet left it: `Landed`;
///   - stopped within 30 m of a stand: `Parked` (the cycle then restarts);
///   - otherwise: `TaxiIn`.
///
/// Airborne rules (`Circuit(leg)`), in the runway frame with `d` = heading minus runway course,
/// and the circuit side at `right_m` of sign `-1` (Left) or `+1` (Right):
/// - `|right| < 300 m`, `|d| < 30`: `Upwind` for `along > L`, `Final` for `along < 0`, and for
///   `0 <= along <= L` `Final` if the previous leg was Base/Final else `Upwind`; other headings
///   in the strip keep the current leg (turns);
/// - `|right| >= 300 m` on the circuit side: `d` within 30 of `side * 90` is `Crosswind`,
///   within 30 of `-side * 90` is `Base`, within 30 of 180 is `Downwind`; otherwise unchanged;
///   the non-circuit side never changes the leg.
///
/// `on_ground` is debounced by time (`SimTime`): a change is accepted once it has persisted
/// 2 s, and a landing (air to ground) additionally needs 10 s airborne or 300 ft above the
/// field elevation since the last liftoff; the first sample is taken as is. Bounces and
/// flicker therefore never leave `Landed`/`TakeoffRoll` or restart the circuit.
///
/// The first airborne sample sets the leg at once; afterwards a different leg is adopted only
/// after 3 consecutive samples agreeing on it.
///
/// Off the circuit ([`PhaseTracker::set_circuit`] `false`) no circuit legs are tracked: an
/// airborne sample is `Airborne`, except `Circuit(Final)` on the extended centreline before
/// the threshold (`-10 NM < along < 0`, `|right| < 0.5 NM`, `|d| < 30`), and on over the
/// runway (`along < L`) when the sample before was on the final, so the landing logic works
/// for arrivals and the flare over the runway is not taken for a missed approach.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PhaseTracker {
    rwy: RunwayFrame,
    length_m: f64,
    half_width_m: f64,
    side: CircuitSide,
    hold_points: Vec<LatLon>,
    stands: Vec<LatLon>,
    has_moved: bool,
    held: bool,
    field_elev_ft: f64,
    on_ground: Option<bool>,
    pending_change: Option<f64>,
    airborne_since: f64,
    max_alt_ft: f64,
    airborne: bool,
    left_runway: bool,
    leg: Option<CircuitLeg>,
    pending: Option<(CircuitLeg, u8)>,
    circuit: bool,
    /// Off the circuit: the last airborne sample was on the final.
    on_final: bool,
}

impl PhaseTracker {
    /// `rwy` is the frame of the active runway (origin at its threshold), `length_m` the
    /// runway length, `hold_points` the holding-point positions of that runway and `stands`
    /// the stand positions; `field_elev_ft` gates landing detection.
    pub fn new(
        rwy: RunwayFrame,
        length_m: f64,
        rwy_half_width_m: f64,
        side: CircuitSide,
        field_elev_ft: f64,
        hold_points: Vec<LatLon>,
        stands: Vec<LatLon>,
    ) -> Self {
        Self {
            rwy,
            length_m,
            half_width_m: rwy_half_width_m,
            side,
            hold_points,
            stands,
            has_moved: false,
            held: false,
            field_elev_ft,
            on_ground: None,
            pending_change: None,
            airborne_since: 0.0,
            max_alt_ft: f64::MIN,
            airborne: false,
            left_runway: false,
            leg: None,
            pending: None,
            circuit: true,
            on_final: false,
        }
    }

    /// The aircraft stopped after a flight away from a stand and its pilot starts the next
    /// flight there: a new departure cycle begins, as when it parks at a stand.
    pub fn begin_new_cycle(&mut self) {
        self.airborne = false;
        self.left_runway = false;
        self.has_moved = false;
        self.held = false;
        self.leg = None;
        self.pending = None;
        self.on_final = false;
    }

    /// Whether the aircraft flies the circuit (the default): circuit legs are produced only
    /// when on.
    pub fn set_circuit(&mut self, on: bool) {
        self.circuit = on;
    }

    /// Debounced `on_ground` (see the type docs).
    fn ground_state(&mut self, s: &AircraftState) -> bool {
        let t = s.t.0;
        let Some(current) = self.on_ground else {
            self.on_ground = Some(s.on_ground);
            self.airborne_since = t;
            self.max_alt_ft = s.alt_ft_msl;
            return s.on_ground;
        };
        if !current {
            self.max_alt_ft = self.max_alt_ft.max(s.alt_ft_msl);
        }
        if s.on_ground == current {
            self.pending_change = None;
            return current;
        }
        let since = *self.pending_change.get_or_insert(t);
        let landing_ok = !s.on_ground
            || (t - self.airborne_since >= MIN_AIRBORNE_S
                || self.max_alt_ft - self.field_elev_ft >= MIN_HEIGHT_FT);
        if t - since >= DEBOUNCE_S && landing_ok {
            self.on_ground = Some(s.on_ground);
            self.pending_change = None;
            if !s.on_ground {
                self.airborne_since = t;
                self.max_alt_ft = s.alt_ft_msl;
            }
            return s.on_ground;
        }
        current
    }

    pub fn update(&mut self, s: &AircraftState) -> FlightPhase {
        let (along, right) = self.rwy.to_local(s.pos);
        let on_ground = self.ground_state(s);
        if !on_ground {
            if !self.airborne {
                self.airborne = true;
                self.left_runway = false;
            }
            if !self.circuit {
                let d = angle_diff(s.heading_deg, self.rwy.course_deg);
                // Over the runway the final goes on (flare before touchdown) when it was final.
                let ahead = if self.on_final { self.length_m } else { 0.0 };
                let on_final = along < ahead
                    && along > -FINAL_MAX_M
                    && right.abs() < FINAL_HALF_WIDTH_M
                    && d.abs() < HEADING_TOL_DEG;
                self.on_final = on_final;
                return if on_final {
                    FlightPhase::Circuit(CircuitLeg::Final)
                } else {
                    FlightPhase::Airborne
                };
            }
            return FlightPhase::Circuit(self.track_leg(along, right, s.heading_deg));
        }
        let stopped = s.gs_kt < STOPPED_KT;
        let in_rect = (0.0..=self.length_m).contains(&along)
            && right.abs() < self.half_width_m + RUNWAY_MARGIN_M;
        if self.airborne {
            self.leg = None;
            self.pending = None;
            if in_rect && !self.left_runway {
                return FlightPhase::Landed;
            }
            self.left_runway = true;
            if stopped && near_any(&self.stands, s.pos, NEAR_POINT_M) {
                // Full stop at a stand: a new departure cycle starts.
                self.airborne = false;
                self.has_moved = false;
                self.held = false;
                return FlightPhase::Parked;
            }
            return FlightPhase::TaxiIn;
        }
        if !stopped {
            self.has_moved = true;
        }
        if stopped && near_any(&self.hold_points, s.pos, NEAR_POINT_M) {
            self.held = true;
            return FlightPhase::HoldingShort;
        }
        if in_rect || !near_any(&self.hold_points, s.pos, HOLD_LATCH_M) {
            self.held = false;
        }
        if self.held {
            FlightPhase::HoldingShort
        } else if in_rect {
            if s.gs_kt >= TAKEOFF_ROLL_KT {
                FlightPhase::TakeoffRoll
            } else {
                FlightPhase::OnRunway
            }
        } else if stopped && !self.has_moved {
            FlightPhase::Parked
        } else {
            FlightPhase::TaxiOut
        }
    }

    fn track_leg(&mut self, along: f64, right: f64, heading_deg: f64) -> CircuitLeg {
        let raw = self.raw_leg(along, right, heading_deg);
        let Some(current) = self.leg else {
            let first = raw.unwrap_or(CircuitLeg::Upwind);
            self.leg = Some(first);
            return first;
        };
        match raw {
            Some(leg) if leg != current => {
                let n = match self.pending {
                    Some((p, n)) if p == leg => n + 1,
                    _ => 1,
                };
                if n >= CONFIRM_SAMPLES {
                    self.leg = Some(leg);
                    self.pending = None;
                    leg
                } else {
                    self.pending = Some((leg, n));
                    current
                }
            }
            _ => {
                self.pending = None;
                current
            }
        }
    }

    /// Leg suggested by one sample alone; `None` when the sample says nothing (turns).
    fn raw_leg(&self, along: f64, right: f64, heading_deg: f64) -> Option<CircuitLeg> {
        let d = angle_diff(heading_deg, self.rwy.course_deg);
        let side = match self.side {
            CircuitSide::Left => -1.0,
            CircuitSide::Right => 1.0,
        };
        if right.abs() < CENTRAL_STRIP_M {
            if d.abs() >= HEADING_TOL_DEG {
                return None;
            }
            let inbound = matches!(self.leg, Some(CircuitLeg::Base | CircuitLeg::Final));
            return Some(if along > self.length_m {
                CircuitLeg::Upwind
            } else if along < 0.0 || inbound {
                CircuitLeg::Final
            } else {
                CircuitLeg::Upwind
            });
        }
        if right.signum() != side {
            return None;
        }
        if (d - side * 90.0).abs() < HEADING_TOL_DEG {
            Some(CircuitLeg::Crosswind)
        } else if (d + side * 90.0).abs() < HEADING_TOL_DEG {
            Some(CircuitLeg::Base)
        } else if d.abs() > 180.0 - HEADING_TOL_DEG {
            Some(CircuitLeg::Downwind)
        } else {
            None
        }
    }
}

fn near_any(points: &[LatLon], p: LatLon, radius_m: f64) -> bool {
    points.iter().any(|&q| distance_m(q, p) <= radius_m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SimTime;
    use crate::taxi::TaxiGraph;
    use crate::testkit::circuit_track;
    use atc_navdata::model::Airport;
    use atc_navdata::parse_apt_dat;
    use atc_navdata::procedures::{Procedures, parse_procedures};
    use atc_phraseology::Callsign;

    const APT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/airports/EPKK/apt.dat"
    );
    const PROCS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/airports/EPKK/procedures.toml"
    );

    fn load() -> (Airport, Procedures) {
        (
            parse_apt_dat(&std::fs::read_to_string(APT).unwrap(), "EPKK").unwrap(),
            parse_procedures(&std::fs::read_to_string(PROCS).unwrap()).unwrap(),
        )
    }

    fn tracker(ap: &Airport, procs: &Procedures, runway: &str) -> (PhaseTracker, RunwayFrame) {
        let rw = ap
            .runways
            .iter()
            .find(|r| r.ends.iter().any(|e| e.ident == runway))
            .unwrap();
        let i = rw.ends.iter().position(|e| e.ident == runway).unwrap();
        let (thr, opp) = (rw.ends[i].threshold, rw.ends[1 - i].threshold);
        let frame = RunwayFrame::new(thr, opp);
        let g = TaxiGraph::build(&ap.taxi);
        let holds = g
            .holding_points(ap, runway)
            .into_iter()
            .map(|id| ap.taxi.nodes.iter().find(|n| n.id == id).unwrap().pos)
            .collect();
        let side = procs
            .circuits
            .iter()
            .find(|c| c.runway == runway)
            .unwrap()
            .side;
        let t = PhaseTracker::new(
            frame,
            distance_m(thr, opp),
            rw.width_m / 2.0,
            side,
            f64::from(ap.elevation_ft),
            holds,
            ap.stands.iter().map(|s| s.pos).collect(),
        );
        (t, frame)
    }

    fn phases(t: &mut PhaseTracker, track: &[AircraftState]) -> Vec<FlightPhase> {
        let mut out: Vec<FlightPhase> = Vec::new();
        for s in track {
            let p = t.update(s);
            if out.last() != Some(&p) {
                out.push(p);
            }
        }
        out
    }

    fn expected() -> Vec<FlightPhase> {
        use CircuitLeg::*;
        use FlightPhase::*;
        vec![
            Parked,
            TaxiOut,
            HoldingShort,
            OnRunway,
            TakeoffRoll,
            Circuit(Upwind),
            Circuit(Crosswind),
            Circuit(Downwind),
            Circuit(Base),
            Circuit(Final),
            Landed,
            TaxiIn,
            Parked,
        ]
    }

    #[test]
    fn synthetic_circuit_visits_legs_in_order() {
        let (ap, procs) = load();
        for dt in [0.5, 1.0, 2.0] {
            let track = circuit_track(&ap, &procs, "25", dt);
            let (mut t, _) = tracker(&ap, &procs, "25");
            assert_eq!(phases(&mut t, &track), expected(), "dt = {dt}");
        }
    }

    /// A path track: from the stand, take-off on 25, out to ZULU, a heading leg, back onto
    /// the final of 25, landing and taxi back to the stand; turns at most 3 degrees per
    /// second, the heading leg flown on its magnetic heading.
    #[test]
    fn path_track_departs_flies_and_lands() {
        use crate::testkit::{PathSpec, PathStart, Waypoint, WaypointPos, path_track};
        use FlightPhase::*;
        let (ap, procs) = load();
        let thr25 = ap.runways[0].ends[1].threshold;
        let final_2nm = atc_geo::destination(thr25, 77.8, 2.0 * 1852.0);
        let spec = PathSpec {
            start: PathStart::Stand("35".into()),
            runway: Some("25".into()),
            waypoints: vec![
                Waypoint {
                    at: WaypointPos::Point("ZULU".into()),
                    alt_ft: 2000.0,
                    kt: 90.0,
                },
                Waypoint {
                    at: WaypointPos::Heading { deg: 30.0, nm: 3.0 },
                    alt_ft: 2000.0,
                    kt: 90.0,
                },
                Waypoint {
                    at: WaypointPos::LatLon(final_2nm),
                    alt_ft: 1400.0,
                    kt: 80.0,
                },
            ],
            land: Some("25".into()),
        };
        let track = path_track(&ap, &procs, None, &spec, 1.0);
        assert_eq!(track, path_track(&ap, &procs, None, &spec, 1.0));
        let (mut t, _) = tracker(&ap, &procs, "25");
        t.set_circuit(false);
        assert_eq!(
            phases(&mut t, &track),
            vec![
                Parked,
                TaxiOut,
                HoldingShort,
                OnRunway,
                TakeoffRoll,
                Airborne,
                Circuit(CircuitLeg::Final),
                Landed,
                TaxiIn,
                Parked
            ]
        );
        for w in track
            .windows(2)
            .filter(|w| !w[0].on_ground && !w[1].on_ground)
        {
            let turn = (w[1].heading_deg - w[0].heading_deg + 540.0).rem_euclid(360.0) - 180.0;
            assert!(turn.abs() <= 3.0 + 1e-9, "{turn} at {:?}", w[1].t);
        }
        // The heading leg: magnetic 030 is true 036 with the 6 E variation.
        assert!(
            track
                .iter()
                .any(|s| !s.on_ground && (s.heading_deg - 36.0).abs() < 1e-6),
            "no sample on true 036"
        );
    }

    #[test]
    fn circuit_track_is_deterministic() {
        let (ap, procs) = load();
        assert_eq!(
            circuit_track(&ap, &procs, "25", 1.0),
            circuit_track(&ap, &procs, "25", 1.0)
        );
    }

    #[test]
    fn runway_07_circuit_also_works() {
        let (ap, procs) = load();
        let track = circuit_track(&ap, &procs, "07", 1.0);
        let (mut t, _) = tracker(&ap, &procs, "07");
        assert_eq!(phases(&mut t, &track), expected());
    }

    #[test]
    fn left_vs_right_circuit() {
        let (ap, procs) = load();
        let mut right = procs.clone();
        for c in &mut right.circuits {
            c.side = CircuitSide::Right;
        }
        for (p, sign) in [(&procs, -1.0), (&right, 1.0)] {
            let track = circuit_track(&ap, p, "25", 1.0);
            let (mut t, frame) = tracker(&ap, p, "25");
            let mut downwind = 0;
            for s in &track {
                if t.update(s) == FlightPhase::Circuit(CircuitLeg::Downwind) {
                    downwind += 1;
                    assert!(frame.to_local(s.pos).1 * sign > 1000.0);
                }
            }
            assert!(downwind > 10);
            let (mut t, _) = tracker(&ap, p, "25");
            assert_eq!(phases(&mut t, &track), expected());
        }
    }

    fn sample(pos: LatLon, heading: f64, gs: f64, on_ground: bool) -> AircraftState {
        sample_at(0.0, pos, heading, gs, on_ground)
    }

    fn sample_at(t: f64, pos: LatLon, heading: f64, gs: f64, on_ground: bool) -> AircraftState {
        AircraftState {
            callsign: Callsign::new("SPABC"),
            t: SimTime(t),
            alt_ft_msl: if on_ground { 791.0 } else { 1800.0 },
            pos,
            heading_deg: heading,
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

    #[test]
    fn leg_changes_only_after_three_samples() {
        let (ap, procs) = load();
        let (mut t, frame) = tracker(&ap, &procs, "25");
        let down = atc_geo::destination(
            atc_geo::destination(frame.origin, frame.course_deg, 1000.0),
            frame.course_deg - 90.0,
            1481.0,
        );
        let hdg = (frame.course_deg + 180.0).rem_euclid(360.0);
        let upwind = |t: &mut PhaseTracker| {
            let p = atc_geo::destination(frame.origin, frame.course_deg, 3000.0);
            t.update(&sample(p, frame.course_deg, 80.0, false))
        };
        assert_eq!(upwind(&mut t), FlightPhase::Circuit(CircuitLeg::Upwind));
        let d = FlightPhase::Circuit(CircuitLeg::Downwind);
        let u = FlightPhase::Circuit(CircuitLeg::Upwind);
        assert_eq!(t.update(&sample(down, hdg, 80.0, false)), u);
        assert_eq!(t.update(&sample(down, hdg, 80.0, false)), u);
        assert_eq!(t.update(&sample(down, hdg, 80.0, false)), d);
    }

    #[test]
    fn parked_after_taxi_in_stop_at_stand() {
        let (ap, procs) = load();
        let (mut t, frame) = tracker(&ap, &procs, "25");
        let off = atc_geo::destination(frame.origin, frame.course_deg, 3000.0);
        t.update(&sample_at(0.0, off, frame.course_deg, 80.0, false));
        let stand = ap.stands[0].pos;
        let mut at = |time: f64, gs: f64| t.update(&sample_at(time, stand, 0.0, gs, true));
        assert_eq!(at(100.0, 5.0), FlightPhase::Circuit(CircuitLeg::Upwind));
        assert_eq!(at(102.0, 5.0), FlightPhase::TaxiIn);
        assert_eq!(at(103.0, 0.0), FlightPhase::Parked);
        assert_eq!(at(104.0, 5.0), FlightPhase::TaxiOut);
    }

    #[test]
    fn landing_bounce_stays_landed() {
        let (ap, procs) = load();
        let (mut t, frame) = tracker(&ap, &procs, "25");
        let at = |a: f64| atc_geo::destination(frame.origin, frame.course_deg, a);
        let h = frame.course_deg;
        t.update(&sample_at(0.0, at(-3000.0), h, 70.0, false));
        t.update(&sample_at(20.0, at(-500.0), h, 70.0, false));
        let landed = FlightPhase::Landed;
        let mut s = |time: f64, a: f64, g: bool| t.update(&sample_at(time, at(a), h, 65.0, g));
        s(30.0, 300.0, true);
        assert_eq!(s(32.0, 400.0, true), landed);
        // Bounce: air for 1 s, back on the ground.
        assert_eq!(s(33.0, 450.0, false), landed);
        assert_eq!(s(34.0, 500.0, true), landed);
        assert_eq!(s(36.0, 600.0, true), landed);
    }

    /// Not flying the circuit: `Airborne` everywhere except on the extended centreline within
    /// 10 NM of the threshold, which is `Final`.
    #[test]
    fn off_circuit_is_airborne_except_on_final() {
        let (ap, procs) = load();
        let (mut t, frame) = tracker(&ap, &procs, "25");
        t.set_circuit(false);
        let at = |along: f64, right: f64| {
            atc_geo::destination(
                atc_geo::destination(frame.origin, frame.course_deg, along),
                frame.course_deg + 90.0,
                right,
            )
        };
        let h = frame.course_deg;
        let mut s =
            |time: f64, p: LatLon, hdg: f64| t.update(&sample_at(time, p, hdg, 90.0, false));
        // Upwind and downwind positions of the circuit.
        assert_eq!(s(0.0, at(3000.0, 0.0), h), FlightPhase::Airborne);
        assert_eq!(
            s(2.0, at(1000.0, -1500.0), h + 180.0),
            FlightPhase::Airborne
        );
        // Final: on the centreline 5 NM out, heading along the course.
        let fin = FlightPhase::Circuit(CircuitLeg::Final);
        assert_eq!(s(4.0, at(-5.0 * 1852.0, 300.0), h), fin);
        // 12 NM out, or 1 NM to the side, or heading across: not final.
        assert_eq!(s(6.0, at(-12.0 * 1852.0, 0.0), h), FlightPhase::Airborne);
        assert_eq!(s(8.0, at(-5.0 * 1852.0, 1852.0), h), FlightPhase::Airborne);
        assert_eq!(
            s(10.0, at(-5.0 * 1852.0, 0.0), h + 90.0),
            FlightPhase::Airborne
        );
        // From the final over the threshold (flaring before touchdown): still final; past
        // the runway end (gone around): airborne.
        assert_eq!(s(12.0, at(-500.0, 0.0), h), fin);
        assert_eq!(s(14.0, at(200.0, 0.0), h), fin);
        assert_eq!(s(16.0, at(2400.0, 0.0), h), fin);
        assert_eq!(s(18.0, at(3200.0, 0.0), h), FlightPhase::Airborne);
        // Over the runway without coming from the final: airborne.
        assert_eq!(s(20.0, at(200.0, 0.0), h), FlightPhase::Airborne);
    }

    #[test]
    fn takeoff_roll_flicker_not_landed() {
        let (ap, procs) = load();
        let (mut t, frame) = tracker(&ap, &procs, "25");
        let at = |a: f64| atc_geo::destination(frame.origin, frame.course_deg, a);
        let h = frame.course_deg;
        let mut s = |time: f64, a: f64, g: bool| {
            let mut st = sample_at(time, at(a), h, 50.0, g);
            st.alt_ft_msl = 850.0; // low: a bounce here is not a landing
            t.update(&st)
        };
        let roll = FlightPhase::TakeoffRoll;
        assert_eq!(s(0.0, 100.0, true), roll);
        // One airborne sample, then ground again: still rolling, never Circuit.
        assert_eq!(s(1.0, 150.0, false), roll);
        assert_eq!(s(2.0, 200.0, true), roll);
        // Real liftoff: airborne for 2 s, then a 1 s bounce right after is not a landing.
        assert_eq!(s(3.0, 300.0, false), roll);
        assert_eq!(
            s(5.0, 400.0, false),
            FlightPhase::Circuit(CircuitLeg::Upwind)
        );
        assert_eq!(
            s(6.0, 450.0, true),
            FlightPhase::Circuit(CircuitLeg::Upwind)
        );
        assert_eq!(
            s(8.0, 500.0, true),
            FlightPhase::Circuit(CircuitLeg::Upwind)
        );
    }
}
