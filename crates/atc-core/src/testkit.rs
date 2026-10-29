//! Synthetic flights for tests (feature `testkit`).

use atc_geo::{LatLon, M_PER_NM, RunwayFrame, bearing_deg, destination, distance_m};
use atc_navdata::fixes::ProcedureSet;
use atc_navdata::model::{Airport, Stand};
use atc_navdata::procedures::{CircuitSide, Procedures};
use atc_phraseology::Callsign;

use crate::state::{AircraftState, SimTime};
use crate::taxi::{TaxiGraph, TaxiTarget};

const TAXI_MS: f64 = 5.0;
const CIRCUIT_KT: f64 = 80.0;
const KT_PER_MS: f64 = 1.943_844;
const LIFTOFF_MS: f64 = 36.0;
const TAKEOFF_ACCEL: f64 = 2.0;
const ROLLOUT_DECEL: f64 = 3.0;
const CLIMB_FPM: f64 = 1000.0;
const IDLE_SAMPLES: usize = 5;
const UPWIND_NM: f64 = 1.0;
const DOWNWIND_OFFSET_NM: f64 = 0.8;
const TOUCHDOWN_ALONG_M: f64 = 300.0;

struct Builder<'a> {
    out: Vec<AircraftState>,
    t: f64,
    dt: f64,
    field_ft: f64,
    airport: &'a Airport,
    com1_khz: u32,
}

impl Builder<'_> {
    fn push(&mut self, pos: LatLon, heading: f64, speed_ms: f64, alt: f64, vs: f64, ground: bool) {
        let gs = speed_ms * KT_PER_MS;
        self.out.push(AircraftState {
            callsign: Callsign::new("SPTST"),
            t: SimTime(self.t),
            pos,
            alt_ft_msl: alt,
            heading_deg: heading.rem_euclid(360.0),
            ias_kt: gs,
            gs_kt: gs,
            vs_fpm: vs,
            on_ground: ground,
            com1_khz: self.com1_khz,
            xpdr_code: None,
            xpdr_mode: None,
            utc_s: None,
            day_of_year: None,
        });
        self.t += self.dt;
    }

    fn idle(&mut self, pos: LatLon, heading: f64) {
        for _ in 0..IDLE_SAMPLES {
            self.push(pos, heading, 0.0, self.field_ft, 0.0, true);
        }
    }

    /// Moves along `pts` at `speed_ms`, one sample per `dt`, ending exactly on the last point.
    /// `profile(distance)` gives `(alt_ft, vs_fpm, on_ground)`. Returns the final heading.
    fn walk(
        &mut self,
        pts: &[LatLon],
        speed_ms: f64,
        profile: impl Fn(f64) -> (f64, f64, bool),
    ) -> f64 {
        let segs: Vec<(LatLon, LatLon, f64)> = pts
            .windows(2)
            .map(|w| (w[0], w[1], distance_m(w[0], w[1])))
            .filter(|s| s.2 > 0.01)
            .collect();
        let total: f64 = segs.iter().map(|s| s.2).sum();
        let step = speed_ms * self.dt;
        let mut heading = segs.first().map_or(0.0, |s| bearing_deg(s.0, s.1));
        let mut k = 1.0;
        loop {
            let s = (k * step).min(total);
            let mut rem = s;
            for &(a, b, len) in &segs {
                heading = bearing_deg(a, b);
                if rem <= len {
                    let pos = destination(a, heading, rem);
                    let (alt, vs, ground) = profile(s);
                    self.push(pos, heading, speed_ms, alt, vs, ground);
                    break;
                }
                rem -= len;
            }
            if s >= total {
                return heading;
            }
            k += 1.0;
        }
    }

    fn node_pos(&self, id: u32) -> LatLon {
        self.airport
            .taxi
            .nodes
            .iter()
            .find(|n| n.id == id)
            .expect("node")
            .pos
    }
}

fn from_local(f: &RunwayFrame, along: f64, right: f64) -> LatLon {
    destination(
        destination(f.origin, f.course_deg, along),
        f.course_deg + 90.0,
        right,
    )
}

/// The runway end `runway` of `airport`: its frame (origin at the threshold) and length.
fn runway_frame(airport: &Airport, runway: &str) -> (RunwayFrame, f64) {
    let rw = airport
        .runways
        .iter()
        .find(|r| r.ends.iter().any(|e| e.ident == runway))
        .expect("runway");
    let i = rw.ends.iter().position(|e| e.ident == runway).expect("end");
    let (thr, opp) = (rw.ends[i].threshold, rw.ends[1 - i].threshold);
    (RunwayFrame::new(thr, opp), distance_m(thr, opp))
}

/// From `stand`: parked, taxi along `route` to the holding point, `hold_samples` there,
/// line up at right angles and roll (full length from the entry) until lift-off. Returns the
/// distance along the runway at lift-off.
fn taxi_and_take_off(
    b: &mut Builder<'_>,
    frame: &RunwayFrame,
    stand: &Stand,
    route: &[u32],
    hold_samples: usize,
) -> f64 {
    let field_ft = b.field_ft;
    b.idle(stand.pos, stand.heading_deg);
    let mut pts = vec![stand.pos];
    pts.extend(route.iter().map(|&n| b.node_pos(n)));
    let hold_pos = *pts.last().expect("route");
    let hdg = b.walk(&pts, TAXI_MS, |_| (field_ft, 0.0, true));
    for _ in 0..hold_samples {
        b.push(hold_pos, hdg, 0.0, field_ft, 0.0, true);
    }

    // Enter the runway at right angles and line up, then roll from there (full length).
    let (a0, _) = frame.to_local(hold_pos);
    let lineup = from_local(frame, a0, 0.0);
    b.walk(&[hold_pos, lineup], TAXI_MS, |_| (field_ft, 0.0, true));
    b.idle(lineup, frame.course_deg);
    let (mut v, mut s) = (0.0, 0.0);
    loop {
        v += TAKEOFF_ACCEL * b.dt;
        s += v * b.dt;
        if v >= LIFTOFF_MS {
            break;
        }
        b.push(
            from_local(frame, a0 + s, 0.0),
            frame.course_deg,
            v,
            field_ft,
            0.0,
            true,
        );
    }
    a0 + s
}

/// Rollout on `runway` from `touchdown_along` at `v` m/s to its farthest holding point, then
/// taxi to `stand` and stop there.
fn roll_out_and_taxi_in(
    b: &mut Builder<'_>,
    graph: &TaxiGraph,
    frame: &RunwayFrame,
    runway: &str,
    touchdown_along: f64,
    mut v: f64,
    stand: &Stand,
) {
    let (airport, field_ft, dt) = (b.airport, b.field_ft, b.dt);
    let exit = graph
        .holding_points(airport, runway)
        .into_iter()
        .map(|id| b.node_pos(id))
        .max_by(|a, c| frame.to_local(*a).0.total_cmp(&frame.to_local(*c).0))
        .expect("exit");
    let exit_along = frame.to_local(exit).0;
    let mut a = touchdown_along;
    loop {
        v = (v - ROLLOUT_DECEL * dt).max(TAXI_MS);
        a += v * dt;
        if a >= exit_along {
            break;
        }
        b.push(
            from_local(frame, a, 0.0),
            frame.course_deg,
            v,
            field_ft,
            0.0,
            true,
        );
    }
    let exit_centre = from_local(frame, exit_along, 0.0);
    b.push(exit_centre, frame.course_deg, TAXI_MS, field_ft, 0.0, true);
    let in_route = graph
        .route(
            airport,
            exit,
            &TaxiTarget::Stand {
                name: stand.name.clone(),
            },
        )
        .expect("route back to the stand");
    let mut pts = vec![exit_centre, exit];
    pts.extend(in_route.nodes.iter().map(|&n| b.node_pos(n)));
    pts.push(stand.pos);
    let hdg = b.walk(&pts, TAXI_MS, |_| (field_ft, 0.0, true));
    b.idle(stand.pos, hdg);
}

/// The first stand of `airport` with a taxi route to the holding point of `runway`, and the
/// route.
fn stand_with_route<'a>(
    airport: &'a Airport,
    graph: &TaxiGraph,
    runway: &str,
) -> (&'a Stand, Vec<u32>) {
    let hold = TaxiTarget::HoldingPoint {
        runway: runway.into(),
    };
    airport
        .stands
        .iter()
        .find_map(|s| {
            graph
                .route(airport, s.pos, &hold)
                .ok()
                .map(|r| (s, r.nodes))
        })
        .expect("stand with a route to the holding point")
}

/// Synthetic flight on `runway`: stand, taxi to the holding point (5 m/s), full-length
/// takeoff after lining up, left/right rectangular circuit at the altitude from `procs` (80 kt,
/// 1 NM upwind past the runway end, downwind 0.8 NM abeam), landing, taxi back to the same
/// stand and stop there. Deterministic; `dt_s` is the sample interval. Panics if the data cannot support it.
pub fn circuit_track(
    airport: &Airport,
    procs: &Procedures,
    runway: &str,
    dt_s: f64,
) -> Vec<AircraftState> {
    let (frame, length) = runway_frame(airport, runway);
    let circuit = procs
        .circuits
        .iter()
        .find(|c| c.runway == runway)
        .expect("circuit");
    let side = match circuit.side {
        CircuitSide::Left => -1.0,
        CircuitSide::Right => 1.0,
    };
    let circuit_ft = f64::from(circuit.altitude_ft_msl);
    let field_ft = f64::from(airport.elevation_ft);
    let graph = TaxiGraph::build(&airport.taxi);
    let (stand, out_route) = stand_with_route(airport, &graph, runway);
    let mut b = Builder {
        out: Vec::new(),
        t: 0.0,
        dt: dt_s,
        field_ft,
        airport,
        com1_khz: procs.positions.first().map_or(0, |p| p.khz),
    };
    let liftoff = taxi_and_take_off(&mut b, &frame, stand, &out_route, IDLE_SAMPLES);

    // Circuit, as a polyline in the runway frame.
    let up_end = length + UPWIND_NM * M_PER_NM;
    let off = side * DOWNWIND_OFFSET_NM * M_PER_NM;
    let wps = [
        (liftoff, 0.0),
        (up_end, 0.0),
        (up_end, off),
        (-UPWIND_NM * M_PER_NM, off),
        (-UPWIND_NM * M_PER_NM, 0.0),
        (TOUCHDOWN_ALONG_M, 0.0),
    ];
    let pts: Vec<LatLon> = wps.iter().map(|&(a, r)| from_local(&frame, a, r)).collect();
    let d_to = |n: usize| {
        pts[..=n]
            .windows(2)
            .map(|w| distance_m(w[0], w[1]))
            .sum::<f64>()
    };
    let (s_top, s_end) = (d_to(3), d_to(5));
    let v_circ = CIRCUIT_KT / KT_PER_MS;
    let climb_ft = |d: f64| (field_ft + CLIMB_FPM / 60.0 * d / v_circ).min(circuit_ft);
    let top_ft = climb_ft(s_top);
    let descent_ft_per_m = (top_ft - field_ft) / (s_end - s_top);
    b.walk(&pts, v_circ, |d| {
        if d >= s_end {
            (field_ft, 0.0, true)
        } else if d <= s_top {
            let vs = if climb_ft(d) < circuit_ft {
                CLIMB_FPM
            } else {
                0.0
            };
            (climb_ft(d), vs, false)
        } else {
            let alt = top_ft - descent_ft_per_m * (d - s_top);
            (alt, -descent_ft_per_m * v_circ * 60.0, false)
        }
    });

    roll_out_and_taxi_in(
        &mut b,
        &graph,
        &frame,
        runway,
        TOUCHDOWN_ALONG_M,
        v_circ,
        stand,
    );
    b.out
}

/// Turn rate of a path track, degrees per second (standard rate).
const TURN_DEG_S: f64 = 3.0;
/// Seconds a path track stands at the holding point before lining up.
const PATH_HOLD_S: f64 = 20.0;
/// Approach speed of a path track from its last waypoint to the touchdown, kt.
const LANDING_KT: f64 = 65.0;

/// Where a path track starts.
#[derive(Clone, Debug, PartialEq)]
pub enum PathStart {
    /// Parked on the stand of this name.
    Stand(String),
    /// In the air: position, altitude (ft MSL) and true heading.
    Air {
        /// Position.
        pos: LatLon,
        /// Altitude, ft MSL.
        alt_ft: f64,
        /// True heading, degrees.
        hdg: f64,
    },
}

/// What a waypoint of a path track is.
#[derive(Clone, Debug, PartialEq)]
pub enum WaypointPos {
    /// A fix of the airport's procedures (`KK599`).
    Fix(String),
    /// A VFR point of `procedures.toml` (`ZULU`).
    Point(String),
    /// A position.
    LatLon(LatLon),
    /// Not a place: fly this magnetic heading (as a controller gives it) for `nm`.
    Heading {
        /// Magnetic heading, degrees.
        deg: f64,
        /// Distance flown on it, NM.
        nm: f64,
    },
}

/// One leg of a path track: flown to `at`, reaching `alt_ft` (MSL) there, at `kt` ground
/// speed.
#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    /// Where the leg goes.
    pub at: WaypointPos,
    /// Altitude reached at the end of the leg, ft MSL.
    pub alt_ft: f64,
    /// Ground speed on the leg, kt.
    pub kt: f64,
}

/// A flight of [`path_track`].
#[derive(Clone, Debug, PartialEq)]
pub struct PathSpec {
    /// Where the flight starts.
    pub start: PathStart,
    /// Departure runway: from the stand, taxi to its holding point, line up and take off.
    pub runway: Option<String>,
    /// The legs flown, in order.
    pub waypoints: Vec<Waypoint>,
    /// Runway to land on after the last waypoint, then taxi to a stand (the start stand, or
    /// the first one with a taxi route).
    pub land: Option<String>,
}

/// The airborne state of a path track.
struct Air {
    pos: LatLon,
    hdg: f64,
    alt: f64,
}

impl Builder<'_> {
    /// Flies one leg of a path track from `a`: towards `to` (or on `heading` true for
    /// `heading_m`), turning at the standard rate, at `kt`, with a constant gradient to
    /// `alt_ft` at the end of the leg.
    fn fly_leg(
        &mut self,
        a: &mut Air,
        to: Option<LatLon>,
        heading: Option<(f64, f64)>,
        alt_ft: f64,
        kt: f64,
    ) {
        let v = kt / KT_PER_MS;
        let step = v * self.dt;
        // Radius of a standard-rate turn.
        let radius = v * 360.0 / TURN_DEG_S / (2.0 * std::f64::consts::PI);
        let mut flown = 0.0;
        loop {
            let (want, left) = match (to, heading) {
                (Some(p), _) => (bearing_deg(a.pos, p), distance_m(a.pos, p)),
                (None, Some((hdg, len))) => (hdg, len - flown),
                (None, None) => return,
            };
            let off = ((want - a.hdg + 540.0).rem_euclid(360.0)) - 180.0;
            if left <= step || (to.is_some() && left < 2.0 * radius && off.abs() > 90.0) {
                return;
            }
            let max_turn = TURN_DEG_S * self.dt;
            a.hdg = (a.hdg + off.clamp(-max_turn, max_turn)).rem_euclid(360.0);
            a.pos = destination(a.pos, a.hdg, step);
            flown += step;
            let alt = a.alt + (alt_ft - a.alt) * (step / left).min(1.0);
            let vs = (alt - a.alt) / self.dt * 60.0;
            a.alt = alt;
            self.push(a.pos, a.hdg, v, alt, vs, false);
        }
    }
}

/// Synthetic flight along waypoints (see [`PathSpec`]): from a stand (taxi at 5 m/s to the
/// holding point of `spec.runway`, 20 s there, line up, full-length take-off) or from a
/// position in the air, then straight legs to each waypoint with standard-rate (3°/s) turns,
/// at its ground speed and with a constant climb or descent to its altitude; then, with
/// `spec.land`, on to the touchdown point (300 m past the threshold) at 65 kt descending to
/// the field, the rollout to the farthest holding point and the taxi to a stand. Fixes come
/// from `set`, VFR points from `procs`. Deterministic; `dt_s` is the sample interval. Panics
/// if the data cannot support it (unknown stand, fix, point or runway).
pub fn path_track(
    airport: &Airport,
    procs: &Procedures,
    set: Option<&ProcedureSet>,
    spec: &PathSpec,
    dt_s: f64,
) -> Vec<AircraftState> {
    let field_ft = f64::from(airport.elevation_ft);
    let graph = TaxiGraph::build(&airport.taxi);
    let mut b = Builder {
        out: Vec::new(),
        t: 0.0,
        dt: dt_s,
        field_ft,
        airport,
        com1_khz: procs.positions.first().map_or(0, |p| p.khz),
    };
    let stand_named = |name: &str| {
        airport
            .stands
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("no stand {name}"))
    };
    let (mut air, home) = match &spec.start {
        PathStart::Stand(name) => {
            let stand = stand_named(name);
            let runway = spec.runway.as_deref().expect("a departure runway");
            let (frame, _) = runway_frame(airport, runway);
            let hold = TaxiTarget::HoldingPoint {
                runway: runway.into(),
            };
            let route = graph
                .route(airport, stand.pos, &hold)
                .unwrap_or_else(|e| panic!("no route from stand {name} to {runway}: {e:?}"));
            let hold_samples = (PATH_HOLD_S / dt_s).round() as usize;
            let liftoff = taxi_and_take_off(&mut b, &frame, stand, &route.nodes, hold_samples);
            let air = Air {
                pos: from_local(&frame, liftoff, 0.0),
                hdg: frame.course_deg,
                alt: field_ft,
            };
            (air, Some(stand))
        }
        PathStart::Air { pos, alt_ft, hdg } => (
            Air {
                pos: *pos,
                hdg: *hdg,
                alt: *alt_ft,
            },
            None,
        ),
    };
    let var = procs.magnetic_variation_deg;
    for wp in &spec.waypoints {
        let (to, heading) = match &wp.at {
            WaypointPos::Fix(name) => (
                Some(
                    set.and_then(|s| s.fix(name))
                        .unwrap_or_else(|| panic!("no fix {name}")),
                ),
                None,
            ),
            WaypointPos::Point(name) => {
                let p = procs
                    .vfr_points
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(name))
                    .unwrap_or_else(|| panic!("no VFR point {name}"));
                (
                    Some(LatLon {
                        lat: p.lat,
                        lon: p.lon,
                    }),
                    None,
                )
            }
            WaypointPos::LatLon(p) => (Some(*p), None),
            WaypointPos::Heading { deg, nm } => {
                (None, Some(((deg + var).rem_euclid(360.0), nm * M_PER_NM)))
            }
        };
        b.fly_leg(&mut air, to, heading, wp.alt_ft, wp.kt);
    }
    if let Some(runway) = &spec.land {
        let (frame, _) = runway_frame(airport, runway);
        let touchdown = from_local(&frame, TOUCHDOWN_ALONG_M, 0.0);
        b.fly_leg(&mut air, Some(touchdown), None, field_ft, LANDING_KT);
        let stand = home.unwrap_or_else(|| stand_with_route(airport, &graph, runway).0);
        let (along, _) = frame.to_local(air.pos);
        roll_out_and_taxi_in(
            &mut b,
            &graph,
            &frame,
            runway,
            along,
            LANDING_KT / KT_PER_MS,
            stand,
        );
    }
    b.out
}
