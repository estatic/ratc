//! Radar vectoring geometry: the planned path to the final approach course and how the
//! radar picture is judged against it.
//!
//! **The planned path.** When Approach takes an arrival on radar vectors it plans, in
//! advance, the ground tracks to fly ([`plan_path`]): an aircraft whose turn onto the
//! intercept is not more than 90 degrees joins directly (an intercept leg from where it is,
//! or via a lead point 3 NM back along the intercept); otherwise it is taken around a
//! pattern on its side of the final (the circuit side on the centreline): downwind 4 NM
//! abeam out to 3 NM beyond the intercept point, base towards the centreline, intercept.
//! The intercept meets the final at 30 degrees (up to 45 to re-intercept close to the
//! course) at least 2 NM outside the FAF; the final leg runs from the join to the FAF.
//! Every leg has an altitude: the descent at 300 ft per NM reaches the intercept altitude
//! before the intercept leg.
//!
//! **Judging.** On each radar update [`judge`] compares the aircraft with the current leg:
//! the next leg is due one turn lead before its turn point ([`turn_lead_m`]: standard-rate
//! radius at the radar ground speed, half rate above 250 kt, plus 10 s); off the leg by more
//! than 0.5 NM (1 NM on downwind) or off the planned altitude by more than 300 ft is a
//! deviation only when the aircraft is not converging. Legs are ground tracks: the heading
//! to give is the track corrected for the drift the radar shows ([`RadarTrack`]). After the
//! approach clearance [`established`] and [`through_course`] judge the final course itself.
//!
//! All bearings are true degrees; distances are metres unless named `_nm` / `_ft`.

use std::collections::VecDeque;

use atc_geo::{LatLon, M_PER_NM, RunwayFrame, bearing_deg, destination, distance_m};
use serde::{Deserialize, Serialize};

/// Radar vectors aim at the join this far out on the final, NM ...
pub const INTERCEPT_NM: f64 = 8.0;
/// ... and never nearer than this outside the FAF, NM.
pub const JOIN_OUTSIDE_FAF_NM: f64 = 2.0;
/// A direct 30-degree join is taken up to this far beyond the nominal join point, NM.
pub const JOIN_BEYOND_NM: f64 = 6.0;
/// Nominal intercept angle onto the final, degrees.
pub const INTERCEPT_ANGLE_DEG: f64 = 30.0;
/// Steepest intercept angle when re-intercepting close to the course, degrees.
pub const REINTERCEPT_MAX_DEG: f64 = 45.0;
/// Within this of the course (NM) an aircraft is re-intercepted from where it is.
pub const NEAR_COURSE_NM: f64 = 1.0;
/// The lead point of a direct join lies this far back along the intercept, NM.
pub const LEAD_NM: f64 = 3.0;
/// Pattern: downwind this far abeam the final, NM ...
pub const DOWNWIND_Y_NM: f64 = 4.0;
/// ... out to this far beyond the join point, where the base starts, NM.
pub const BASE_BEYOND_NM: f64 = 3.0;
/// The turn onto the intercept more than this: a pattern instead of a direct join, degrees.
pub const PATTERN_GAP_DEG: f64 = 90.0;
/// Within this of the course (NM) the pattern side is the circuit side.
pub const CENTRELINE_NM: f64 = 0.5;
/// Standard-rate turn radius in NM is the ground speed in knots divided by this.
pub const STANDARD_RATE_KT_PER_NM: f64 = 188.5;
/// Above this ground speed the turn is planned at half rate (double radius), kt.
pub const HALF_RATE_ABOVE_KT: f64 = 250.0;
/// Reaction time added to the turn lead, s.
pub const REACTION_S: f64 = 10.0;
/// Planned descent gradient, ft per NM.
pub const DESCENT_FT_PER_NM: f64 = 300.0;
/// Cross-track tolerance off a leg, NM (on downwind: [`DOWNWIND_TOLERANCE_NM`]).
pub const LEG_TOLERANCE_NM: f64 = 0.5;
pub const DOWNWIND_TOLERANCE_NM: f64 = 1.0;
/// Heading tolerance off the assigned heading, degrees.
pub const HEADING_TOLERANCE_DEG: f64 = 15.0;
/// Altitude tolerance off the planned altitude, ft.
pub const ALTITUDE_TOLERANCE_FT: f64 = 300.0;
/// A correction is transmitted when the heading changes by at least this, degrees ...
pub const MIN_HEADING_CHANGE_DEG: f64 = 10.0;
/// ... at most once per this per aircraft, s ...
pub const CORRECTION_INTERVAL_S: f64 = 20.0;
/// ... and not for this long after a heading or level readback, s.
pub const READBACK_HOLD_S: f64 = 10.0;
/// Established: within this of the course (NM) ...
pub const ESTABLISHED_NM: f64 = 0.3;
/// ... tracking within this of it, degrees.
pub const ESTABLISHED_DEG: f64 = 15.0;
/// Through the course: past it by more than this, NM ...
pub const THROUGH_NM: f64 = 0.3;
/// ... or converging at more than this (degrees) within [`NEAR_COURSE_NM`] of it.
pub const THROUGH_CONVERGE_DEG: f64 = 45.0;
/// The radar track and drift are taken over this window, s.
pub const DRIFT_WINDOW_S: f64 = 30.0;
/// Converging: closing the leg at least this many degrees ...
const CONVERGING_DEG: f64 = 5.0;
/// ... or climbing / descending towards the planned altitude at least this fast, fpm.
const CONVERGING_FPM: f64 = 200.0;
/// Below this distance between the first and last radar position there is no track, m.
const MIN_TRACK_M: f64 = 50.0;
/// Planned leg altitudes are rounded down to this, ft.
const ALT_STEP_FT: f64 = 100.0;

/// The final approach course vectors lead to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FinalCourse {
    /// Landing threshold.
    pub thr: LatLon,
    /// True course of the final (landing direction).
    pub course_deg: f64,
    /// Distance of the FAF from the threshold.
    pub faf_m: f64,
    /// Altitude to be established on the final at (FAF altitude, below the glide path).
    pub intercept_alt_ft: i32,
    /// Circuit side of the final: +1 right, -1 left (of the landing direction).
    pub circuit_side: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LegKind {
    Downwind,
    Base,
    Intercept,
    Final,
}

/// One leg of the planned path: a ground track from `from` to `to` at `alt_ft` (the
/// altitude to be at by the end of the leg).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorLeg {
    pub kind: LegKind,
    pub track_deg: f64,
    pub from: LatLon,
    pub to: LatLon,
    pub alt_ft: i32,
}

/// The planned path to the final; `current` indexes the leg being flown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorPlan {
    pub legs: Vec<VectorLeg>,
    pub current: usize,
}

impl VectorPlan {
    /// The leg being flown (the last one once past the end).
    pub fn leg(&self) -> &VectorLeg {
        &self.legs[self.current.min(self.legs.len() - 1)]
    }

    /// Whether the path joins the final of `f` at least [`JOIN_OUTSIDE_FAF_NM`] outside the
    /// FAF (false without a final leg).
    pub fn joins_in_time(&self, f: &FinalCourse) -> bool {
        self.legs
            .iter()
            .find(|l| l.kind == LegKind::Final)
            .is_some_and(|l| {
                Frame(f).local(l.from).0 >= f.faf_m / M_PER_NM + JOIN_OUTSIDE_FAF_NM - 0.01
            })
    }
}

/// What the radar shows of an aircraft.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radar {
    pub pos: LatLon,
    /// Ground track, true.
    pub track_deg: f64,
    pub gs_kt: f64,
    pub alt_ft: f64,
    pub vs_fpm: f64,
}

/// The radar picture against the current leg of a plan.
#[derive(Debug, PartialEq)]
pub enum Verdict {
    /// Following the plan (in tolerance, or converging on it).
    OnPlan,
    /// One turn lead from the end of the leg: the next leg's heading is due.
    LegDue,
    /// Off the leg and not converging; positive right of the leg.
    OffTrack { right_nm: f64 },
    /// Off the planned altitude and not converging; positive above.
    OffAltitude { ft: f64 },
}

/// Recent radar positions and headings of one aircraft, for its track and drift.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RadarTrack {
    /// `(time s, position, true heading)`, oldest first, at most [`DRIFT_WINDOW_S`] old.
    samples: VecDeque<(f64, LatLon, f64)>,
}

impl RadarTrack {
    /// Adds a radar sample; samples older than [`DRIFT_WINDOW_S`] are dropped (all of them
    /// when time went back).
    pub fn push(&mut self, t: f64, pos: LatLon, heading_deg: f64) {
        if self.samples.back().is_some_and(|s| s.0 > t) {
            self.samples.clear();
        }
        self.samples.push_back((t, pos, heading_deg));
        while self
            .samples
            .front()
            .is_some_and(|s| s.0 < t - DRIFT_WINDOW_S)
        {
            self.samples.pop_front();
        }
    }

    /// Ground track from the first to the last kept position; `None` while too short.
    pub fn track_deg(&self) -> Option<f64> {
        let (first, last) = (self.samples.front()?, self.samples.back()?);
        (distance_m(first.1, last.1) >= MIN_TRACK_M).then(|| bearing_deg(first.1, last.1))
    }

    /// Radar track minus the mean heading over the window, degrees in [-180, 180); 0 without
    /// a track.
    pub fn drift_deg(&self) -> f64 {
        let Some(track) = self.track_deg() else {
            return 0.0;
        };
        let (s, c) = self.samples.iter().fold((0.0, 0.0), |(s, c), x| {
            let h = x.2.to_radians();
            (s + h.sin(), c + h.cos())
        });
        signed(track - s.atan2(c).to_degrees())
    }
}

/// The heading (true) that makes good `track_deg` with `drift_deg` of drift.
pub fn heading_for_track(track_deg: f64, drift_deg: f64) -> f64 {
    (track_deg - drift_deg).rem_euclid(360.0)
}

/// Distance before a turn point at which the turn of `turn_deg` must start at `gs_kt`:
/// the standard-rate radius (double above [`HALF_RATE_ABOVE_KT`]) times `tan(turn / 2)`,
/// plus [`REACTION_S`] at ground speed.
pub fn turn_lead_m(gs_kt: f64, turn_deg: f64) -> f64 {
    let gs = gs_kt.max(0.0);
    let mut radius_nm = gs / STANDARD_RATE_KT_PER_NM;
    if gs > HALF_RATE_ABOVE_KT {
        radius_nm *= 2.0;
    }
    let turn = turn_deg.abs().min(170.0).to_radians();
    (radius_nm * (turn / 2.0).tan() + gs * REACTION_S / 3600.0) * M_PER_NM
}

/// Distance before the point where `to_ft` must be reached at which the descent from
/// `from_ft` must begin, at [`DESCENT_FT_PER_NM`]; 0 when no descent is needed.
pub fn descent_start_m(from_ft: f64, to_ft: i32) -> f64 {
    (from_ft - f64::from(to_ft)).max(0.0) / DESCENT_FT_PER_NM * M_PER_NM
}

/// The planned path from `pos` (ground track `track_deg`, altitude `alt_ft`) to the final
/// `f`. In order of preference:
/// - close to the course ([`NEAR_COURSE_NM`]), ahead of the threshold, the turn onto the
///   intercept not more than 90 degrees: an intercept from here at 30 degrees, steepened up
///   to 45 to join outside the FAF margin (a join inside it is still planned: see
///   [`VectorPlan::joins_in_time`]);
/// - the 30-degree intercept from here joins between the FAF margin and 6 NM beyond the
///   nominal join, the turn onto it not more than 90 degrees: that intercept;
/// - the turn onto the intercept at its lead point not more than 90 degrees: a base to the
///   lead point, then the intercept;
/// - otherwise the pattern: downwind, base, intercept.
///
/// The final leg runs from the join to the FAF.
pub fn plan_path(pos: LatLon, track_deg: f64, alt_ft: f64, f: &FinalCourse) -> VectorPlan {
    let frame = Frame(f);
    let course = f.course_deg;
    let (x, y) = frame.local(pos);
    let x_min = f.faf_m / M_PER_NM + JOIN_OUTSIDE_FAF_NM;
    let x_join = INTERCEPT_NM.max(x_min);
    // The side the aircraft is on, and the side of a pattern.
    let own = if y != 0.0 { y.signum() } else { f.circuit_side };
    let side = if y.abs() >= CENTRELINE_NM {
        y.signum()
    } else {
        f.circuit_side
    };
    let tan30 = INTERCEPT_ANGLE_DEG.to_radians().tan();
    let leg = |kind, track_deg: f64, from, to| VectorLeg {
        kind,
        track_deg: track_deg.rem_euclid(360.0),
        from,
        to,
        alt_ft: f.intercept_alt_ft,
    };
    let intercept_from_here = |angle_deg: f64| {
        let join = frame.point(x - y.abs() / angle_deg.to_radians().tan(), 0.0);
        leg(LegKind::Intercept, course - own * angle_deg, pos, join)
    };
    let onto_intercept = |s: f64| gap_deg(track_deg, course - s * INTERCEPT_ANGLE_DEG);

    let mut legs = if x > 0.0 && y.abs() <= NEAR_COURSE_NM && onto_intercept(own) <= PATTERN_GAP_DEG
    {
        let angle = if x - y.abs() / tan30 >= x_min {
            INTERCEPT_ANGLE_DEG
        } else if x > x_min {
            (y.abs() / (x - x_min))
                .atan()
                .to_degrees()
                .clamp(INTERCEPT_ANGLE_DEG, REINTERCEPT_MAX_DEG)
        } else {
            REINTERCEPT_MAX_DEG
        };
        vec![intercept_from_here(angle)]
    } else if x > 0.0
        && (x_min..=x_join + JOIN_BEYOND_NM).contains(&(x - y.abs() / tan30))
        && onto_intercept(own) <= PATTERN_GAP_DEG
    {
        vec![intercept_from_here(INTERCEPT_ANGLE_DEG)]
    } else {
        let join = frame.point(x_join, 0.0);
        let intercept_trk = course - side * INTERCEPT_ANGLE_DEG;
        let lead = destination(join, intercept_trk + 180.0, LEAD_NM * M_PER_NM);
        let to_lead = bearing_deg(pos, lead);
        if distance_m(pos, lead) < CENTRELINE_NM * M_PER_NM {
            vec![leg(LegKind::Intercept, intercept_trk, pos, join)]
        } else if gap_deg(to_lead, intercept_trk) <= PATTERN_GAP_DEG {
            vec![
                leg(LegKind::Base, to_lead, pos, lead),
                leg(LegKind::Intercept, intercept_trk, lead, join),
            ]
        } else {
            let base_x = (x_join + BASE_BEYOND_NM).max(x + 1.0);
            let downwind_end = frame.point(base_x, side * DOWNWIND_Y_NM);
            let base_end = frame.point(base_x, side * (base_x - x_join) * tan30);
            vec![
                leg(
                    LegKind::Downwind,
                    bearing_deg(pos, downwind_end),
                    pos,
                    downwind_end,
                ),
                leg(LegKind::Base, course - side * 90.0, downwind_end, base_end),
                leg(LegKind::Intercept, intercept_trk, base_end, join),
            ]
        }
    };
    let join = legs.last().map_or(pos, |l| l.to);
    legs.push(leg(
        LegKind::Final,
        course,
        join,
        frame.point(x_min - JOIN_OUTSIDE_FAF_NM, 0.0),
    ));
    plan_altitudes(&mut legs, alt_ft, f.intercept_alt_ft);
    VectorPlan { legs, current: 0 }
}

/// Altitudes of the legs before the intercept: the profile descending at
/// [`DESCENT_FT_PER_NM`] to `intercept_ft` at the start of the intercept, at each leg's end,
/// rounded down to 100 ft, not above the start altitude nor below `intercept_ft`.
fn plan_altitudes(legs: &mut [VectorLeg], start_ft: f64, intercept_ft: i32) {
    let Some(i) = legs.iter().position(|l| l.kind == LegKind::Intercept) else {
        return;
    };
    let start = (start_ft / ALT_STEP_FT).round() * ALT_STEP_FT;
    let mut to_go_nm = 0.0;
    for k in (0..i).rev() {
        let profile = f64::from(intercept_ft) + to_go_nm * DESCENT_FT_PER_NM;
        let alt = (profile.min(start) / ALT_STEP_FT).floor() * ALT_STEP_FT;
        legs[k].alt_ft = (alt as i32).max(intercept_ft);
        to_go_nm += distance_m(legs[k].from, legs[k].to) / M_PER_NM;
    }
}

/// The radar picture `r` against the current leg of `plan`: the next leg due (turn lead
/// reached), off the leg (0.5 NM, 1 NM on downwind) or off the planned altitude (300 ft)
/// without converging, or on plan. The planned altitude on a leg before the intercept is the
/// latest-descent profile to its `alt_ft` at the leg's end (not above the previous leg's);
/// from the intercept on it is the leg's `alt_ft`. Heading is not judged here (the radar has
/// only the track).
pub fn judge(plan: &VectorPlan, r: &Radar) -> Verdict {
    let leg = plan.leg();
    let (along, right) = RunwayFrame {
        origin: leg.from,
        course_deg: leg.track_deg,
    }
    .to_local(r.pos);
    let to_go = distance_m(leg.from, leg.to) - along;
    let last = plan.current + 1 >= plan.legs.len();
    if !last {
        let next = &plan.legs[plan.current + 1];
        if to_go <= turn_lead_m(r.gs_kt, gap_deg(leg.track_deg, next.track_deg)) {
            return Verdict::LegDue;
        }
    } else if to_go < 0.0 {
        return Verdict::OnPlan;
    }

    let tolerance_nm = if leg.kind == LegKind::Downwind {
        DOWNWIND_TOLERANCE_NM
    } else {
        LEG_TOLERANCE_NM
    };
    let rel = signed(r.track_deg - leg.track_deg).to_radians();
    let closing = -right.signum() * rel.sin() >= CONVERGING_DEG.to_radians().sin();
    if right.abs() > tolerance_nm * M_PER_NM && !closing {
        return Verdict::OffTrack {
            right_nm: right / M_PER_NM,
        };
    }

    let alt = f64::from(leg.alt_ft);
    let upper = if matches!(leg.kind, LegKind::Downwind | LegKind::Base) {
        let profile = alt + to_go.max(0.0) / M_PER_NM * DESCENT_FT_PER_NM;
        match plan.current.checked_sub(1).map(|k| &plan.legs[k]) {
            Some(prev) => profile.min(f64::from(prev.alt_ft).max(alt)),
            None => profile,
        }
    } else {
        alt
    };
    if r.alt_ft > upper + ALTITUDE_TOLERANCE_FT && r.vs_fpm > -CONVERGING_FPM {
        return Verdict::OffAltitude {
            ft: r.alt_ft - upper,
        };
    }
    if r.alt_ft < alt - ALTITUDE_TOLERANCE_FT && r.vs_fpm < CONVERGING_FPM {
        return Verdict::OffAltitude { ft: r.alt_ft - alt };
    }
    Verdict::OnPlan
}

/// Established on the final of `f` by radar: ahead of the threshold, within
/// [`ESTABLISHED_NM`] of the course and tracking within [`ESTABLISHED_DEG`] of it.
pub fn established(r: &Radar, f: &FinalCourse) -> bool {
    let (x, y) = Frame(f).local(r.pos);
    x > 0.0 && y.abs() <= ESTABLISHED_NM && gap_deg(r.track_deg, f.course_deg) <= ESTABLISHED_DEG
}

/// Flying through the final of `f`: more than [`THROUGH_NM`] off it and moving away, or
/// within [`NEAR_COURSE_NM`] of it converging at more than [`THROUGH_CONVERGE_DEG`].
pub fn through_course(r: &Radar, f: &FinalCourse) -> bool {
    let (_, y) = Frame(f).local(r.pos);
    // Rate to the right of the course, per unit of ground speed.
    let rightward = signed(r.track_deg - f.course_deg).to_radians().sin();
    let away = y * rightward > 0.0;
    (away && y.abs() > THROUGH_NM)
        || (!away
            && rightward != 0.0
            && y.abs() <= NEAR_COURSE_NM
            && gap_deg(r.track_deg, f.course_deg) > THROUGH_CONVERGE_DEG)
}

/// The final's frame in NM: `x` out along the approach from the threshold, `y` right of the
/// landing direction.
struct Frame<'a>(&'a FinalCourse);

impl Frame<'_> {
    fn local(&self, p: LatLon) -> (f64, f64) {
        let (along, right) = RunwayFrame {
            origin: self.0.thr,
            course_deg: self.0.course_deg,
        }
        .to_local(p);
        (-along / M_PER_NM, right / M_PER_NM)
    }

    fn point(&self, x_nm: f64, y_nm: f64) -> LatLon {
        let c = self.0.course_deg;
        let p = destination(self.0.thr, c + 180.0, x_nm * M_PER_NM);
        destination(p, c + 90.0, y_nm * M_PER_NM)
    }
}

/// `deg` in [-180, 180).
fn signed(deg: f64) -> f64 {
    (deg + 180.0).rem_euclid(360.0) - 180.0
}

/// Smallest difference of two bearings, degrees.
fn gap_deg(a: f64, b: f64) -> f64 {
    signed(a - b).abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use atc_geo::{M_PER_NM, destination};

    const COURSE: f64 = 255.0;
    const THR: LatLon = LatLon {
        lat: 50.08,
        lon: 19.80,
    };

    fn fin() -> FinalCourse {
        FinalCourse {
            thr: THR,
            course_deg: COURSE,
            faf_m: 4.0 * M_PER_NM,
            intercept_alt_ft: 2500,
            circuit_side: -1.0,
        }
    }

    /// The point `out_nm` out on the final and `right_nm` right of the landing direction.
    fn at(out_nm: f64, right_nm: f64) -> LatLon {
        let p = destination(THR, COURSE + 180.0, out_nm * M_PER_NM);
        destination(p, COURSE + 90.0, right_nm * M_PER_NM)
    }

    fn kinds(p: &VectorPlan) -> Vec<LegKind> {
        p.legs.iter().map(|l| l.kind).collect()
    }

    fn out_nm(p: LatLon) -> f64 {
        let (along, _) = RunwayFrame {
            origin: THR,
            course_deg: COURSE,
        }
        .to_local(p);
        -along / M_PER_NM
    }

    fn radar(pos: LatLon, track_deg: f64) -> Radar {
        Radar {
            pos,
            track_deg,
            gs_kt: 160.0,
            alt_ft: 2500.0,
            vs_fpm: 0.0,
        }
    }

    #[test]
    fn direct_join_has_intercept_and_final() {
        let plan = plan_path(at(15.0, -4.0), COURSE, 5000.0, &fin());
        assert_eq!(kinds(&plan), vec![LegKind::Intercept, LegKind::Final]);
        assert_eq!(plan.current, 0);
        let int = &plan.legs[0];
        // From the left of the landing direction the intercept turns right of the course.
        assert!(
            (int.track_deg - (COURSE + 30.0)).abs() < 1e-6,
            "{}",
            int.track_deg
        );
        assert_eq!(int.alt_ft, 2500);
        let join = out_nm(plan.legs[1].from);
        assert!(join >= 4.0 + 2.0, "join {join} NM");
        assert!(plan.joins_in_time(&fin()));
        assert!((out_nm(plan.legs[1].to) - 4.0).abs() < 0.01);
        assert!((plan.legs[1].track_deg - COURSE).abs() < 1e-6);
    }

    #[test]
    fn right_side_intercept_turns_left_of_course() {
        let plan = plan_path(at(15.0, 4.0), COURSE, 5000.0, &fin());
        assert_eq!(kinds(&plan), vec![LegKind::Intercept, LegKind::Final]);
        assert!((plan.legs[0].track_deg - (COURSE - 30.0)).abs() < 1e-6);
    }

    #[test]
    fn far_side_gets_pattern() {
        // Beyond the threshold, flying on in the landing direction.
        let plan = plan_path(at(-3.0, 0.0), COURSE, 4000.0, &fin());
        assert_eq!(
            kinds(&plan),
            vec![
                LegKind::Downwind,
                LegKind::Base,
                LegKind::Intercept,
                LegKind::Final
            ]
        );
        // On the centreline: the circuit side (left).
        let (_, right) = RunwayFrame {
            origin: THR,
            course_deg: COURSE,
        }
        .to_local(plan.legs[0].to);
        assert!((right / M_PER_NM + 4.0).abs() < 0.05, "downwind {right}");
        // Base towards the centreline from the left: course + 90.
        assert!((plan.legs[1].track_deg - (COURSE + 90.0).rem_euclid(360.0)).abs() < 1e-6);
        assert!((plan.legs[2].track_deg - (COURSE + 30.0)).abs() < 1e-6);
        assert!(plan.joins_in_time(&fin()));
    }

    #[test]
    fn altitudes_descend_to_intercept_altitude_before_the_intercept() {
        let plan = plan_path(at(-3.0, 0.0), COURSE, 6000.0, &fin());
        let alts: Vec<i32> = plan.legs.iter().map(|l| l.alt_ft).collect();
        assert!(alts.windows(2).all(|w| w[0] >= w[1]), "{alts:?}");
        assert!(alts.iter().all(|&a| (2500..=6000).contains(&a)), "{alts:?}");
        let i = plan
            .legs
            .iter()
            .position(|l| l.kind == LegKind::Intercept)
            .unwrap();
        assert_eq!(plan.legs[i].alt_ft, 2500);
        assert_eq!(plan.legs[i + 1].alt_ft, 2500);
        // The base is short: the downwind already ends below the start, at most 300 ft/NM
        // above the intercept altitude.
        let base_nm = distance_m(plan.legs[1].from, plan.legs[1].to) / M_PER_NM;
        assert!(f64::from(plan.legs[0].alt_ft) <= 2500.0 + base_nm * 300.0 + 1.0);
        assert!(plan.legs[0].alt_ft < 6000);
    }

    #[test]
    fn overshoot_near_the_faf_does_not_join_in_time() {
        // Through the course from the right, half a mile left, 6 NM out (FAF at 4).
        let plan = plan_path(at(6.0, -0.5), COURSE - 30.0, 2500.0, &fin());
        assert_eq!(kinds(&plan), vec![LegKind::Intercept, LegKind::Final]);
        let gap = (plan.legs[0].track_deg - COURSE).abs();
        assert!((44.999..=45.001).contains(&gap), "{gap}");
        assert!(!plan.joins_in_time(&fin()));
    }

    #[test]
    fn reintercept_steepens_up_to_45_to_join_in_time() {
        // 8 NM out, 0.9 NM left, crossing back: 30 degrees joins at 6.4 NM, outside the
        // margin (6 NM): kept at 30.
        let plan = plan_path(at(8.0, -0.9), COURSE - 20.0, 2500.0, &fin());
        assert!((plan.legs[0].track_deg - (COURSE + 30.0)).abs() < 1e-6);
        assert!(plan.joins_in_time(&fin()));
        // 7 NM out, 0.9 NM left: steeper than 30 to join at 6 NM.
        let plan = plan_path(at(7.0, -0.9), COURSE - 20.0, 2500.0, &fin());
        let angle = plan.legs[0].track_deg - COURSE;
        assert!(angle > 30.0 && angle <= 45.0, "{angle}");
        assert!(plan.joins_in_time(&fin()));
    }

    #[test]
    fn joins_in_time_false_inside_faf_margin() {
        let mut plan = plan_path(at(15.0, -4.0), COURSE, 5000.0, &fin());
        assert!(plan.joins_in_time(&fin()));
        // Join moved to 5 NM out: inside FAF + 2 NM.
        plan.legs[1].from = at(5.0, 0.0);
        assert!(!plan.joins_in_time(&fin()));
    }

    #[test]
    fn turn_lead_scales_with_speed() {
        assert!(turn_lead_m(100.0, 90.0) < turn_lead_m(180.0, 90.0));
        let expected = (120.0 / 188.5 + 10.0 * 120.0 / 3600.0) * M_PER_NM;
        let got = turn_lead_m(120.0, 90.0);
        assert!(
            (got - expected).abs() / expected < 0.05,
            "{got} vs {expected}"
        );
        assert!(turn_lead_m(120.0, 30.0) < got);
    }

    #[test]
    fn half_rate_above_250() {
        let reaction = |gs: f64| 10.0 * gs / 3600.0 * M_PER_NM;
        let radius = |gs: f64| turn_lead_m(gs, 90.0) - reaction(gs);
        let standard = 250.0 / 188.5 * M_PER_NM;
        assert!((radius(250.0) - standard).abs() < 1.0);
        let half = 2.0 * 260.0 / 188.5 * M_PER_NM;
        assert!((radius(260.0) - half).abs() < 1.0, "{}", radius(260.0));
    }

    #[test]
    fn descent_start_at_300_ft_per_nm() {
        assert!((descent_start_m(5000.0, 2000) - 10.0 * M_PER_NM).abs() < 1e-6);
        assert_eq!(descent_start_m(2000.0, 2500), 0.0);
    }

    #[test]
    fn drift_from_positions() {
        let mut rt = RadarTrack::default();
        assert_eq!(rt.drift_deg(), 0.0);
        assert_eq!(rt.track_deg(), None);
        let p0 = LatLon {
            lat: 50.0,
            lon: 20.0,
        };
        for k in 0..=20 {
            // 120 kt due north, heading 350.
            let t = f64::from(k) * 2.0;
            let p = destination(p0, 0.0, 120.0 * M_PER_NM / 3600.0 * t);
            rt.push(t, p, 350.0);
        }
        let track = rt.track_deg().unwrap();
        assert!(track < 0.5 || track > 359.5, "{track}");
        assert!((rt.drift_deg() - 10.0).abs() < 0.5, "{}", rt.drift_deg());
        assert!((heading_for_track(0.0, rt.drift_deg()) - 350.0).abs() < 0.5);
        // Only the last 30 s are kept.
        assert!(rt.samples.front().unwrap().0 >= 40.0 - 30.0);
    }

    #[test]
    fn heading_for_track_wraps() {
        assert!((heading_for_track(5.0, 10.0) - 355.0).abs() < 1e-9);
        assert!((heading_for_track(355.0, -10.0) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn judge_off_track_and_converging() {
        let plan = plan_path(at(15.0, -4.0), COURSE, 2500.0, &fin());
        let leg = plan.leg().clone();
        assert_eq!(leg.kind, LegKind::Intercept);
        // Halfway along the intercept leg, 0.6 NM right of it.
        let mid = destination(leg.from, leg.track_deg, distance_m(leg.from, leg.to) / 2.0);
        let off = destination(mid, leg.track_deg + 90.0, 0.6 * M_PER_NM);
        assert_eq!(judge(&plan, &radar(mid, leg.track_deg)), Verdict::OnPlan);
        assert_eq!(
            judge(&plan, &radar(off, leg.track_deg - 20.0)),
            Verdict::OnPlan
        );
        match judge(&plan, &radar(off, leg.track_deg)) {
            Verdict::OffTrack { right_nm } => assert!((right_nm - 0.6).abs() < 0.05),
            v => panic!("{v:?}"),
        }
        // Diverging further right.
        assert!(matches!(
            judge(&plan, &radar(off, leg.track_deg + 20.0)),
            Verdict::OffTrack { .. }
        ));
    }

    #[test]
    fn judge_leg_due_at_turn_lead() {
        let plan = plan_path(at(15.0, -4.0), COURSE, 2500.0, &fin());
        let leg = plan.leg().clone();
        let lead = turn_lead_m(160.0, 30.0);
        let before = destination(leg.to, leg.track_deg + 180.0, lead + 0.3 * M_PER_NM);
        let inside = destination(leg.to, leg.track_deg + 180.0, lead - 0.1 * M_PER_NM);
        assert_eq!(judge(&plan, &radar(before, leg.track_deg)), Verdict::OnPlan);
        assert_eq!(judge(&plan, &radar(inside, leg.track_deg)), Verdict::LegDue);
    }

    #[test]
    fn judge_altitude() {
        let plan = plan_path(at(15.0, -4.0), COURSE, 2500.0, &fin());
        let leg = plan.leg().clone();
        let mid = destination(leg.from, leg.track_deg, distance_m(leg.from, leg.to) / 2.0);
        let mut r = radar(mid, leg.track_deg);
        r.alt_ft = 3200.0;
        match judge(&plan, &r) {
            Verdict::OffAltitude { ft } => assert!((ft - 700.0).abs() < 1.0),
            v => panic!("{v:?}"),
        }
        // Descending towards it: on plan.
        r.vs_fpm = -800.0;
        assert_eq!(judge(&plan, &r), Verdict::OnPlan);
        // Low and level: off; climbing back: on plan.
        r.alt_ft = 2000.0;
        r.vs_fpm = 0.0;
        assert!(matches!(judge(&plan, &r), Verdict::OffAltitude { ft } if ft < 0.0));
        r.vs_fpm = 500.0;
        assert_eq!(judge(&plan, &r), Verdict::OnPlan);
        // Within 300 ft: on plan.
        r.alt_ft = 2700.0;
        r.vs_fpm = 0.0;
        assert_eq!(judge(&plan, &r), Verdict::OnPlan);
    }

    #[test]
    fn established_needs_position_and_track() {
        let f = fin();
        assert!(established(&radar(at(7.0, 0.2), COURSE + 10.0), &f));
        // On the course but tracking across it.
        assert!(!established(&radar(at(7.0, 0.0), COURSE + 30.0), &f));
        // Tracking along it but 0.5 NM off.
        assert!(!established(&radar(at(7.0, 0.5), COURSE), &f));
    }

    #[test]
    fn through_course_detects_overshoot() {
        let f = fin();
        // Came from the right, now 0.5 NM left and still going left.
        assert!(through_course(&radar(at(7.0, -0.5), COURSE - 30.0), &f));
        // 0.5 NM right, converging at 30: not through.
        assert!(!through_course(&radar(at(7.0, 0.5), COURSE - 30.0), &f));
        // 0.2 NM past: within the margin.
        assert!(!through_course(&radar(at(7.0, -0.2), COURSE - 30.0), &f));
        // 0.8 NM right, converging at 90: will fly through.
        assert!(through_course(&radar(at(7.0, 0.8), COURSE - 90.0), &f));
        // 2 NM right at 90: not yet.
        assert!(!through_course(&radar(at(9.0, 2.0), COURSE - 90.0), &f));
        // Established: not through.
        assert!(!through_course(&radar(at(7.0, 0.1), COURSE), &f));
    }

    #[test]
    fn plan_round_trips_through_serde() {
        let plan = plan_path(at(-3.0, 0.0), COURSE, 4000.0, &fin());
        let json = serde_json::to_string(&plan).unwrap();
        let back: VectorPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(kinds(&back), kinds(&plan));
        assert_eq!(back.current, plan.current);
        assert!(
            back.legs
                .iter()
                .zip(&plan.legs)
                .all(|(a, b)| a.alt_ft == b.alt_ft && (a.track_deg - b.track_deg).abs() < 1e-9)
        );
        let mut rt = RadarTrack::default();
        rt.push(0.0, THR, 10.0);
        let json = serde_json::to_string(&rt).unwrap();
        let back: RadarTrack = serde_json::from_str(&json).unwrap();
        assert_eq!(back.samples.len(), 1);
    }
}
