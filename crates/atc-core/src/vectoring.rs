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
//! Every leg has an altitude to be reached by its end, descending at no more than 300 ft
//! per NM: the final is joined at least 300 ft below the 3-degree glide path (taken through
//! the FAF at the intercept altitude: the FAF altitude is the glide path's there) and the
//! intercept altitude is reached by the FAF, descending on the final leg; a path too short
//! for that is lengthened (a shallower intercept, or a pattern stretched out). An aircraft
//! already on the final gets the final leg only.
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
/// Drift is measured only on headings within this of each other, degrees ...
pub const DRIFT_SPREAD_DEG: f64 = 5.0;
/// ... held for at least this long, s.
pub const DRIFT_MIN_SPAN_S: f64 = 10.0;
/// The pattern is stretched out by up to this much to make the descent, NM.
pub const MAX_STRETCH_NM: f64 = 40.0;
/// Shallowest intercept angle, degrees (a shallower intercept is a longer one).
pub const INTERCEPT_MIN_DEG: f64 = 20.0;
/// Below this distance between the first and last radar position there is no track, m.
const MIN_TRACK_M: f64 = 50.0;
/// The glide path (3 degrees) climbs this much per NM from the threshold, ft ...
pub const GLIDE_FT_PER_NM: f64 = 318.0;
/// ... and the final is joined at least this far below it, ft.
pub const GLIDE_MARGIN_FT: f64 = 300.0;
/// A radar sample farther from the previous one than this many times the distance its
/// ground speed covers ...
pub const JUMP_FACTOR: f64 = 2.0;
/// ... plus this (m) is a jump: the radar track starts again.
pub const JUMP_SLACK_M: f64 = 500.0;
/// The start altitude is rounded to this, ft ...
const ALT_STEP_FT: f64 = 100.0;
/// ... and the planned leg altitudes down to this.
const LEG_ALT_STEP_FT: f64 = 500.0;

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

/// Stands in for the leg of a plan without legs (a damaged snapshot).
static NO_LEG: VectorLeg = VectorLeg {
    kind: LegKind::Final,
    track_deg: 0.0,
    from: LatLon { lat: 0.0, lon: 0.0 },
    to: LatLon { lat: 0.0, lon: 0.0 },
    alt_ft: 0,
};

impl VectorPlan {
    /// Index of the leg being flown: `current`, clamped to the last leg; `None` without legs.
    pub fn index(&self) -> Option<usize> {
        self.legs
            .len()
            .checked_sub(1)
            .map(|last| self.current.min(last))
    }

    /// The leg being flown (the last one once past the end; a placeholder final leg when the
    /// plan has no legs).
    pub fn leg(&self) -> &VectorLeg {
        self.index().map_or(&NO_LEG, |i| &self.legs[i])
    }

    /// Whether the path joins the final of `f` at least [`JOIN_OUTSIDE_FAF_NM`] outside the
    /// FAF: true for a plan that starts on the final (already established), false without a
    /// final leg. A fresh plan may be late (see [`plan_path`]): callers check this.
    pub fn joins_in_time(&self, f: &FinalCourse) -> bool {
        match self.legs.iter().position(|l| l.kind == LegKind::Final) {
            Some(0) => true,
            Some(i) => {
                Frame(f).local(self.legs[i].from).0
                    >= f.faf_m / M_PER_NM + JOIN_OUTSIDE_FAF_NM - 0.01
            }
            None => false,
        }
    }

    /// Whether an aircraft at `alt_ft` at the start of the plan can make every leg's altitude
    /// by the leg's end descending at no more than [`DESCENT_FT_PER_NM`], the final leg's (the
    /// intercept altitude) by the FAF.
    pub fn descent_in_time(&self, alt_ft: f64) -> bool {
        let mut entry = alt_ft;
        for l in &self.legs {
            let len_nm = distance_m(l.from, l.to) / M_PER_NM;
            if entry - f64::from(l.alt_ft) > len_nm * DESCENT_FT_PER_NM + 1.0 {
                return false;
            }
            entry = f64::from(l.alt_ft);
        }
        true
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
    /// The last drift measured on a steady heading.
    #[serde(default)]
    drift_deg: f64,
}

impl RadarTrack {
    /// Adds a radar sample at ground speed `gs_kt`; samples older than [`DRIFT_WINDOW_S`]
    /// are dropped, all of them when time went back or the aircraft jumped (moved more than
    /// [`JUMP_FACTOR`] times its ground speed allows, plus [`JUMP_SLACK_M`]: a slew or a
    /// reposition). The drift is kept.
    pub fn push(&mut self, t: f64, pos: LatLon, heading_deg: f64, gs_kt: f64) {
        let jumped = |s: &(f64, LatLon, f64)| {
            let could_m = gs_kt.max(0.0) * JUMP_FACTOR * (t - s.0) / 3600.0 * M_PER_NM;
            distance_m(s.1, pos) > could_m + JUMP_SLACK_M
        };
        if self.samples.back().is_some_and(|s| s.0 > t || jumped(s)) {
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
        if let Some(d) = self.steady_drift() {
            self.drift_deg = d;
        }
    }

    /// Drift over the latest run of samples whose headings stay within
    /// [`DRIFT_SPREAD_DEG`] of each other, when it spans at least [`DRIFT_MIN_SPAN_S`]: the
    /// chord bearing of the run minus its mean heading. `None` while turning.
    fn steady_drift(&self) -> Option<f64> {
        let last = self.samples.back()?;
        let (mut lo, mut hi) = (0.0f64, 0.0f64);
        let mut first = last;
        for s in self.samples.iter().rev() {
            let off = signed(s.2 - last.2);
            if hi.max(off) - lo.min(off) > DRIFT_SPREAD_DEG {
                break;
            }
            (lo, hi, first) = (lo.min(off), hi.max(off), s);
        }
        if last.0 - first.0 < DRIFT_MIN_SPAN_S || distance_m(first.1, last.1) < MIN_TRACK_M {
            return None;
        }
        let mean = last.2 + (lo + hi) / 2.0;
        Some(signed(bearing_deg(first.1, last.1) - mean))
    }

    /// Ground track from the first to the last kept position; `None` while too short.
    pub fn track_deg(&self) -> Option<f64> {
        let (first, last) = (self.samples.front()?, self.samples.back()?);
        (distance_m(first.1, last.1) >= MIN_TRACK_M).then(|| bearing_deg(first.1, last.1))
    }

    /// Radar track minus heading, degrees in [-180, 180): measured on the latest steady
    /// heading (at least [`DRIFT_MIN_SPAN_S`] within [`DRIFT_SPREAD_DEG`]); while turning the
    /// last such value is kept; 0 before any.
    pub fn drift_deg(&self) -> f64 {
        self.drift_deg
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

/// The altitude to clear an aircraft flying `plan` (radar `r`) down to now, if the plan
/// needs the descent: the intercept altitude (the final leg's), as soon as one of the
/// remaining legs needs the descent to begin at [`DESCENT_FT_PER_NM`] ([`descent_start_m`]
/// from the present altitude) — each leg's altitude at the leg's end, and the intercept
/// altitude itself at the turn onto the intercept (one turn lead before the intercept leg):
/// the controller descends early enough to have it before the intercept heading where the
/// path allows, though the plan accepts less (below the glide path at the join, the
/// intercept altitude by the FAF). On the intercept leg or the final: at once. `None`
/// without legs or while not yet due.
pub fn descent_due(plan: &VectorPlan, r: &Radar) -> Option<i32> {
    let i = plan.index()?;
    let legs = &plan.legs;
    let rest: Vec<&VectorLeg> = legs[i..]
        .iter()
        .take_while(|l| l.kind != LegKind::Final)
        .collect();
    let target = legs.last().map_or(legs[i].alt_ft, |l| l.alt_ft);
    if rest.is_empty() {
        return Some(target);
    }
    let cur = &legs[i];
    let (along, _) = RunwayFrame {
        origin: cur.from,
        course_deg: cur.track_deg,
    }
    .to_local(r.pos);
    // Distance to go to the end of each remaining leg.
    let mut to_end = (distance_m(cur.from, cur.to) - along).max(0.0);
    let mut start = 0.0;
    for (k, l) in rest.iter().enumerate() {
        if k > 0 {
            to_end += distance_m(l.from, l.to);
        }
        let (need_at, alt_ft) = if l.kind == LegKind::Intercept {
            let lead = if k > 0 {
                turn_lead_m(r.gs_kt, gap_deg(rest[k - 1].track_deg, l.track_deg))
            } else {
                0.0
            };
            ((start - lead).max(0.0), target)
        } else {
            (to_end, l.alt_ft)
        };
        if need_at <= descent_start_m(r.alt_ft, alt_ft) {
            return Some(target);
        }
        start = to_end;
    }
    None
}

/// The planned path from `pos` (ground track `track_deg`, altitude `alt_ft`) to the final
/// `f`. In order of preference:
/// - already on the final (within [`ESTABLISHED_NM`], tracking within [`ESTABLISHED_DEG`]):
///   the final leg only;
/// - close to the course ([`NEAR_COURSE_NM`]), ahead of the threshold, the turn onto the
///   intercept not more than 90 degrees: an intercept from here at 30 degrees, steepened up
///   to 45 to join outside the FAF margin. A join inside it is still planned, so a caller
///   checks [`VectorPlan::joins_in_time`], on a fresh plan too;
/// - the 30-degree intercept from here joins between the FAF margin and 6 NM beyond the
///   nominal join, the turn onto it not more than 90 degrees: that intercept, made shallower
///   (down to 20 degrees, so longer) when the descent needs it;
/// - the turn onto the intercept at its lead point not more than 90 degrees: a base to the
///   lead point, then the intercept;
/// - otherwise, or when the choice above cannot make the descent at [`DESCENT_FT_PER_NM`],
///   the pattern: downwind, base, intercept, stretched out (base and join further out) until
///   the descent fits (up to [`MAX_STRETCH_NM`]; check [`VectorPlan::descent_in_time`]).
///
/// The final leg runs from the join to the FAF (to the threshold for a join inside the FAF).
/// The join is below the glide path and the intercept altitude is reached by the FAF.
pub fn plan_path(pos: LatLon, track_deg: f64, alt_ft: f64, f: &FinalCourse) -> VectorPlan {
    let g = Setup::new(pos, f);
    let (x, y, course) = (g.x, g.y, f.course_deg);
    if x > 0.0 && y.abs() <= ESTABLISHED_NM && gap_deg(track_deg, course) <= ESTABLISHED_DEG {
        return g.plan(Vec::new(), alt_ft);
    }
    let tan30 = INTERCEPT_ANGLE_DEG.to_radians().tan();
    let onto_intercept =
        gap_deg(track_deg, course - g.own * INTERCEPT_ANGLE_DEG) <= PATTERN_GAP_DEG;
    let joins = |angle_deg: f64| x - y.abs() / angle_deg.to_radians().tan();
    let first = if x > 0.0 && y.abs() <= NEAR_COURSE_NM && onto_intercept {
        let angle = if joins(INTERCEPT_ANGLE_DEG) >= g.x_min {
            INTERCEPT_ANGLE_DEG
        } else if x > g.x_min {
            (y.abs() / (x - g.x_min))
                .atan()
                .to_degrees()
                .clamp(INTERCEPT_ANGLE_DEG, REINTERCEPT_MAX_DEG)
        } else {
            REINTERCEPT_MAX_DEG
        };
        Some(g.plan(vec![g.intercept_from_here(angle)], alt_ft))
    } else if x > 0.0
        && onto_intercept
        && (g.x_min..=g.x_join + JOIN_BEYOND_NM).contains(&(x - y.abs() / tan30))
    {
        let steps = (INTERCEPT_ANGLE_DEG - INTERCEPT_MIN_DEG) as u32;
        (0..=steps)
            .map(|k| INTERCEPT_ANGLE_DEG - f64::from(k))
            .take_while(|&a| joins(a) >= g.x_min)
            .map(|a| g.plan(vec![g.intercept_from_here(a)], alt_ft))
            .find(|p| p.descent_in_time(alt_ft))
    } else {
        g.lead_route().map(|legs| g.plan(legs, alt_ft))
    };
    match first {
        Some(p) if p.descent_in_time(alt_ft) => p,
        _ => g.stretched_pattern(alt_ft),
    }
}

/// The pattern from `pos` (altitude `alt_ft`) to the final `f` wherever the aircraft is, for
/// a new approach (after a missed approach): downwind on its side of the final (the circuit
/// side on the centreline), base, intercept, stretched out until the descent fits, as in
/// [`plan_path`].
pub fn plan_pattern(pos: LatLon, alt_ft: f64, f: &FinalCourse) -> VectorPlan {
    Setup::new(pos, f).stretched_pattern(alt_ft)
}

/// The aircraft's place relative to the final, for [`plan_path`].
struct Setup<'a> {
    f: &'a FinalCourse,
    pos: LatLon,
    /// NM out along the approach, and right of the landing direction.
    x: f64,
    y: f64,
    /// Nearest join allowed (FAF + margin), and the nominal join, NM out.
    x_min: f64,
    x_join: f64,
    /// The side the aircraft is on, and the side of a pattern (+1 right).
    own: f64,
    side: f64,
}

impl<'a> Setup<'a> {
    fn new(pos: LatLon, f: &'a FinalCourse) -> Self {
        let (x, y) = Frame(f).local(pos);
        let x_min = f.faf_m / M_PER_NM + JOIN_OUTSIDE_FAF_NM;
        Self {
            f,
            pos,
            x,
            y,
            x_min,
            x_join: INTERCEPT_NM.max(x_min),
            own: if y != 0.0 { y.signum() } else { f.circuit_side },
            side: if y.abs() >= CENTRELINE_NM {
                y.signum()
            } else {
                f.circuit_side
            },
        }
    }

    fn leg(&self, kind: LegKind, track_deg: f64, from: LatLon, to: LatLon) -> VectorLeg {
        VectorLeg {
            kind,
            track_deg: track_deg.rem_euclid(360.0),
            from,
            to,
            alt_ft: self.f.intercept_alt_ft,
        }
    }

    fn point(&self, x_nm: f64, y_nm: f64) -> LatLon {
        Frame(self.f).point(x_nm, y_nm)
    }

    /// The intercept from the present position at `angle_deg` onto the course.
    fn intercept_from_here(&self, angle_deg: f64) -> VectorLeg {
        let join = self.point(self.x - self.y.abs() / angle_deg.to_radians().tan(), 0.0);
        let track = self.f.course_deg - self.own * angle_deg;
        self.leg(LegKind::Intercept, track, self.pos, join)
    }

    /// Base to the lead point of the nominal intercept, then the intercept; `None` when the
    /// turn at the lead point would be more than [`PATTERN_GAP_DEG`].
    fn lead_route(&self) -> Option<Vec<VectorLeg>> {
        let join = self.point(self.x_join, 0.0);
        let intercept = self.f.course_deg - self.side * INTERCEPT_ANGLE_DEG;
        let lead = destination(join, intercept + 180.0, LEAD_NM * M_PER_NM);
        let to_lead = bearing_deg(self.pos, lead);
        if distance_m(self.pos, lead) < CENTRELINE_NM * M_PER_NM {
            Some(vec![self.leg(
                LegKind::Intercept,
                intercept,
                self.pos,
                join,
            )])
        } else if gap_deg(to_lead, intercept) <= PATTERN_GAP_DEG {
            Some(vec![
                self.leg(LegKind::Base, to_lead, self.pos, lead),
                self.leg(LegKind::Intercept, intercept, lead, join),
            ])
        } else {
            None
        }
    }

    /// The pattern on `side`, base and join `stretch_nm` further out than nominal (and at
    /// least 1 NM beyond the aircraft).
    fn pattern(&self, stretch_nm: f64) -> Vec<VectorLeg> {
        let (course, side) = (self.f.course_deg, self.side);
        let base_x = (self.x_join + BASE_BEYOND_NM + stretch_nm).max(self.x + 1.0);
        let join_x = base_x - BASE_BEYOND_NM;
        let tan30 = INTERCEPT_ANGLE_DEG.to_radians().tan();
        let downwind_end = self.point(base_x, side * DOWNWIND_Y_NM);
        let base_end = self.point(base_x, side * BASE_BEYOND_NM * tan30);
        let join = self.point(join_x, 0.0);
        vec![
            self.leg(
                LegKind::Downwind,
                bearing_deg(self.pos, downwind_end),
                self.pos,
                downwind_end,
            ),
            self.leg(LegKind::Base, course - side * 90.0, downwind_end, base_end),
            self.leg(
                LegKind::Intercept,
                course - side * INTERCEPT_ANGLE_DEG,
                base_end,
                join,
            ),
        ]
    }

    /// The pattern stretched out (up to [`MAX_STRETCH_NM`]) until the descent from `alt_ft`
    /// fits.
    fn stretched_pattern(&self, alt_ft: f64) -> VectorPlan {
        let stretched = |k: u32| self.plan(self.pattern(f64::from(k)), alt_ft);
        (0..=MAX_STRETCH_NM as u32)
            .map(stretched)
            .find(|p| p.descent_in_time(alt_ft))
            .unwrap_or_else(|| stretched(MAX_STRETCH_NM as u32))
    }

    /// The highest altitude to join the final at `join_x_nm` out: at least
    /// [`GLIDE_MARGIN_FT`] below the glide path and able to make the intercept altitude by
    /// the FAF at [`DESCENT_FT_PER_NM`]; the intercept altitude for a join inside the FAF.
    fn join_limit_ft(&self, join_x_nm: f64) -> f64 {
        let ia = f64::from(self.f.intercept_alt_ft);
        let to_faf_nm = join_x_nm - self.f.faf_m / M_PER_NM;
        if to_faf_nm <= 0.0 {
            return ia;
        }
        let glide = ia + to_faf_nm * GLIDE_FT_PER_NM - GLIDE_MARGIN_FT;
        glide.min(ia + to_faf_nm * DESCENT_FT_PER_NM).max(ia)
    }

    /// `legs` with the final leg appended and the altitudes planned.
    fn plan(&self, mut legs: Vec<VectorLeg>, alt_ft: f64) -> VectorPlan {
        let join = legs.last().map_or(self.pos, |l| l.to);
        let faf_nm = self.f.faf_m / M_PER_NM;
        let end = if Frame(self.f).local(join).0 > faf_nm {
            self.point(faf_nm, 0.0)
        } else {
            self.f.thr
        };
        legs.push(self.leg(LegKind::Final, self.f.course_deg, join, end));
        let join_x = Frame(self.f).local(join).0;
        plan_altitudes(
            &mut legs,
            alt_ft,
            self.join_limit_ft(join_x),
            self.f.intercept_alt_ft,
        );
        VectorPlan { legs, current: 0 }
    }
}

/// Altitudes of the legs before the final, each to be reached at the leg's end: the latest
/// descent at [`DESCENT_FT_PER_NM`] to `join_ft` at the join (see
/// [`Setup::join_limit_ft`]), rounded down to 500 ft, never below `intercept_ft`; a leg the
/// profile does not yet descend on keeps the previous altitude (the start rounded to
/// 100 ft). The final leg keeps `intercept_ft`, to be reached by the FAF.
fn plan_altitudes(legs: &mut [VectorLeg], start_ft: f64, join_ft: f64, intercept_ft: i32) {
    let n = legs
        .iter()
        .position(|l| l.kind == LegKind::Final)
        .unwrap_or(legs.len());
    let mut to_go_nm = vec![0.0; n];
    for k in (0..n.saturating_sub(1)).rev() {
        to_go_nm[k] = to_go_nm[k + 1] + distance_m(legs[k + 1].from, legs[k + 1].to) / M_PER_NM;
    }
    let ia = f64::from(intercept_ft);
    // The join altitude in 500 ft steps, so that each leg's descent stays on the profile.
    let join = ((join_ft / LEG_ALT_STEP_FT).floor() * LEG_ALT_STEP_FT).max(ia);
    let mut entry = (start_ft / ALT_STEP_FT).round() * ALT_STEP_FT;
    for k in 0..n {
        let profile = join + to_go_nm[k] * DESCENT_FT_PER_NM;
        let alt = if profile >= entry {
            entry
        } else {
            (profile / LEG_ALT_STEP_FT).floor() * LEG_ALT_STEP_FT
        }
        .max(ia);
        legs[k].alt_ft = alt as i32;
        entry = alt;
    }
}

/// The radar picture `r` against the current leg of `plan`: the next leg due (turn lead
/// reached), off the leg (0.5 NM, 1 NM on downwind) or off the planned altitude (300 ft)
/// without converging, or on plan. The planned altitude on a leg before the final is the
/// latest-descent profile to its `alt_ft` at the leg's end (not above the previous leg's),
/// on the final the profile to the intercept altitude at the FAF. Below it the aircraft is
/// off only under the lower of the leg's altitude and `cleared_ft`, the level it is cleared
/// to (`None`: the leg's altitude). Heading is not judged here (the radar has only the
/// track). A plan without legs is always on plan.
pub fn judge(plan: &VectorPlan, r: &Radar, cleared_ft: Option<i32>) -> Verdict {
    let Some(i) = plan.index() else {
        return Verdict::OnPlan;
    };
    let leg = &plan.legs[i];
    let (along, right) = RunwayFrame {
        origin: leg.from,
        course_deg: leg.track_deg,
    }
    .to_local(r.pos);
    let to_go = distance_m(leg.from, leg.to) - along;
    if let Some(next) = plan.legs.get(i + 1) {
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
    let upper = {
        let profile = alt + to_go.max(0.0) / M_PER_NM * DESCENT_FT_PER_NM;
        match i.checked_sub(1).map(|k| &plan.legs[k]) {
            Some(prev) => profile.min(f64::from(prev.alt_ft).max(alt)),
            None => profile,
        }
    };
    if r.alt_ft > upper + ALTITUDE_TOLERANCE_FT && r.vs_fpm > -CONVERGING_FPM {
        return Verdict::OffAltitude {
            ft: r.alt_ft - upper,
        };
    }
    // Below the plan is no deviation down to the level the aircraft is cleared to (one
    // descent clearance to the intercept altitude covers the legs planned higher).
    let lower = cleared_ft.map_or(alt, |c| alt.min(f64::from(c)));
    if r.alt_ft < lower - ALTITUDE_TOLERANCE_FT && r.vs_fpm < CONVERGING_FPM {
        return Verdict::OffAltitude {
            ft: r.alt_ft - lower,
        };
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

    /// Every leg, the final to the FAF too, descends at no more than 300 ft/NM from
    /// `start_ft`.
    fn assert_gradient(plan: &VectorPlan, start_ft: f64) {
        assert!(plan.descent_in_time(start_ft));
        let mut entry = start_ft;
        for l in &plan.legs {
            let nm = distance_m(l.from, l.to) / M_PER_NM;
            let grad = (entry - f64::from(l.alt_ft)) / nm;
            assert!(grad <= 300.0 + 1.0, "{:?} {grad} ft/NM", l.kind);
            entry = f64::from(l.alt_ft);
        }
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
        let plan = plan_path(at(15.0, -4.0), COURSE, 4000.0, &fin());
        assert_eq!(kinds(&plan), vec![LegKind::Intercept, LegKind::Final]);
        assert_eq!(plan.current, 0);
        let int = &plan.legs[0];
        // From the left of the landing direction the intercept turns right of the course.
        assert!(
            (int.track_deg - (COURSE + 30.0)).abs() < 1e-6,
            "{}",
            int.track_deg
        );
        let join = out_nm(plan.legs[1].from);
        // Joined at least 300 ft below the glide path (2500 ft at the FAF, 318 ft/NM), the
        // intercept altitude by the FAF.
        let glide = 2500.0 + (join - 4.0) * 318.0;
        assert!(
            f64::from(int.alt_ft) <= glide - 300.0,
            "{} at {join}",
            int.alt_ft
        );
        assert!(join >= 4.0 + 2.0, "join {join} NM");
        assert!(plan.joins_in_time(&fin()));
        assert!((out_nm(plan.legs[1].to) - 4.0).abs() < 0.01);
        assert!((plan.legs[1].track_deg - COURSE).abs() < 1e-6);
    }

    #[test]
    fn right_side_intercept_turns_left_of_course() {
        let plan = plan_path(at(15.0, 4.0), COURSE, 4000.0, &fin());
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
    fn altitudes_descend_to_below_the_glide_path_at_the_join() {
        let plan = plan_path(at(-3.0, 0.0), COURSE, 6000.0, &fin());
        let alts: Vec<i32> = plan.legs.iter().map(|l| l.alt_ft).collect();
        assert!(alts.windows(2).all(|w| w[0] >= w[1]), "{alts:?}");
        assert!(alts.iter().all(|&a| (2500..=6000).contains(&a)), "{alts:?}");
        let i = plan
            .legs
            .iter()
            .position(|l| l.kind == LegKind::Intercept)
            .unwrap();
        let join = out_nm(plan.legs[i + 1].from);
        let glide = 2500.0 + (join - 4.0) * 318.0;
        assert!(f64::from(plan.legs[i].alt_ft) <= glide - 300.0, "{alts:?}");
        assert_eq!(plan.legs[i + 1].alt_ft, 2500);
        // Leg altitudes in 500 ft steps, the descent not steeper than 300 ft/NM.
        assert!(alts.iter().all(|a| a % 500 == 0), "{alts:?}");
        assert_gradient(&plan, 6000.0);
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
            rt.push(t, p, 350.0, 120.0);
        }
        let track = rt.track_deg().unwrap();
        assert!(track < 0.5 || track > 359.5, "{track}");
        assert!((rt.drift_deg() - 10.0).abs() < 0.5, "{}", rt.drift_deg());
        assert!((heading_for_track(0.0, rt.drift_deg()) - 350.0).abs() < 0.5);
        // Only the last 30 s are kept.
        assert!(rt.samples.front().unwrap().0 >= 40.0 - 30.0);
    }

    #[test]
    fn high_direct_join_is_lengthened() {
        let short = plan_path(at(15.0, -4.0), COURSE, 2500.0, &fin());
        let short_nm = path_nm(&short);
        // From 5000 ft the join at 30 degrees is below the glide path (about 3500 ft).
        let p5 = plan_path(at(15.0, -4.0), COURSE, 5000.0, &fin());
        assert_gradient(&p5, 5000.0);
        assert!((path_nm(&p5) - short_nm).abs() < 0.01);
        // From 6000 ft the intercept cannot get below the glide path: lengthened (the
        // join further out, descending on a longer final).
        let p6 = plan_path(at(15.0, -4.0), COURSE, 6000.0, &fin());
        assert_gradient(&p6, 6000.0);
        assert!(p6.joins_in_time(&fin()));
        assert!(total_nm(&p6) > total_nm(&short) + 1.0);
        // From 7000 ft even 20 degrees is too short: the pattern, stretched out.
        let p7 = plan_path(at(15.0, -4.0), COURSE, 7000.0, &fin());
        assert_eq!(p7.legs[0].kind, LegKind::Downwind);
        assert_gradient(&p7, 7000.0);
        assert!(p7.joins_in_time(&fin()));
        // A level aircraft at the start altitude is on plan on the first update.
        for (plan, alt) in [(&p5, 5000.0), (&p7, 7000.0)] {
            let mut r = radar(at(15.0, -4.0), plan.legs[0].track_deg);
            r.alt_ft = alt;
            assert_eq!(judge(plan, &r, None), Verdict::OnPlan, "{alt}");
        }
    }

    /// Length of the whole path to the FAF, NM.
    fn total_nm(plan: &VectorPlan) -> f64 {
        plan.legs
            .iter()
            .map(|l| distance_m(l.from, l.to) / M_PER_NM)
            .sum()
    }

    /// Length of the path to the join, NM.
    fn path_nm(plan: &VectorPlan) -> f64 {
        plan.legs
            .iter()
            .take_while(|l| l.kind != LegKind::Final)
            .map(|l| distance_m(l.from, l.to) / M_PER_NM)
            .sum()
    }

    #[test]
    fn join_inside_the_faf_ends_at_the_threshold() {
        // 45 degrees from 4.5 NM out, 0.5 NM left: joins at 4 NM, at the FAF.
        let plan = plan_path(at(3.5, -0.5), COURSE - 30.0, 2500.0, &fin());
        assert!(!plan.joins_in_time(&fin()));
        let fin_leg = plan.legs.last().unwrap();
        assert_eq!(fin_leg.kind, LegKind::Final);
        assert!(out_nm(fin_leg.from) < 4.0);
        assert_eq!(fin_leg.to, THR);
    }

    #[test]
    fn on_the_centreline_is_final_only() {
        let plan = plan_path(at(9.0, 0.2), COURSE + 5.0, 2500.0, &fin());
        assert_eq!(kinds(&plan), vec![LegKind::Final]);
        assert!(plan.joins_in_time(&fin()));
        assert!((out_nm(plan.legs[0].to) - 4.0).abs() < 0.01);
        assert_eq!(
            judge(&plan, &radar(at(9.0, 0.2), COURSE), None),
            Verdict::OnPlan
        );
        // Established inside the FAF: still joined.
        let plan = plan_path(at(3.0, 0.1), COURSE, 1500.0, &fin());
        assert_eq!(kinds(&plan), vec![LegKind::Final]);
        assert!(plan.joins_in_time(&fin()));
    }

    #[test]
    fn empty_or_out_of_range_plan_does_not_panic() {
        let empty: VectorPlan = serde_json::from_str(r#"{"legs":[],"current":3}"#).unwrap();
        assert_eq!(empty.index(), None);
        assert_eq!(empty.leg().kind, LegKind::Final);
        assert_eq!(judge(&empty, &radar(THR, 0.0), None), Verdict::OnPlan);
        assert!(!empty.joins_in_time(&fin()));
        assert!(empty.descent_in_time(5000.0));
        let mut plan = plan_path(at(-3.0, 0.0), COURSE, 4000.0, &fin());
        plan.current = 9;
        assert_eq!(plan.index(), Some(3));
        assert_eq!(plan.leg().kind, LegKind::Final);
        let _ = judge(&plan, &radar(at(5.0, 0.0), COURSE), None);
    }

    #[test]
    fn drift_held_through_a_turn() {
        let mut rt = RadarTrack::default();
        let mut p = LatLon {
            lat: 50.0,
            lon: 20.0,
        };
        let mut hdg = 350.0;
        let step = 120.0 * M_PER_NM / 3600.0 * 2.0;
        for k in 0..60 {
            let t = f64::from(k) * 2.0;
            // Steady for 40 s, then a standard-rate turn of 90 degrees left, then steady.
            if (20..35).contains(&k) {
                hdg -= 6.0;
            }
            rt.push(t, p, hdg, 120.0);
            if k >= 6 {
                assert!(
                    (rt.drift_deg() - 10.0).abs() < 1.0,
                    "t {t}: {}",
                    rt.drift_deg()
                );
            }
            p = destination(p, hdg + 10.0, step);
        }
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
        assert_eq!(
            judge(&plan, &radar(mid, leg.track_deg), None),
            Verdict::OnPlan
        );
        assert_eq!(
            judge(&plan, &radar(off, leg.track_deg - 20.0), None),
            Verdict::OnPlan
        );
        match judge(&plan, &radar(off, leg.track_deg), None) {
            Verdict::OffTrack { right_nm } => assert!((right_nm - 0.6).abs() < 0.05),
            v => panic!("{v:?}"),
        }
        // Diverging further right.
        assert!(matches!(
            judge(&plan, &radar(off, leg.track_deg + 20.0), None),
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
        assert_eq!(
            judge(&plan, &radar(before, leg.track_deg), None),
            Verdict::OnPlan
        );
        assert_eq!(
            judge(&plan, &radar(inside, leg.track_deg), None),
            Verdict::LegDue
        );
    }

    #[test]
    fn judge_altitude() {
        let plan = plan_path(at(15.0, -4.0), COURSE, 2500.0, &fin());
        let leg = plan.leg().clone();
        let mid = destination(leg.from, leg.track_deg, distance_m(leg.from, leg.to) / 2.0);
        let mut r = radar(mid, leg.track_deg);
        // The intercept altitude is due at the join, 4 NM on: up to 3700 ft is on profile.
        r.alt_ft = 4100.0;
        match judge(&plan, &r, None) {
            Verdict::OffAltitude { ft } => assert!((ft - 400.0).abs() < 5.0, "{ft}"),
            v => panic!("{v:?}"),
        }
        r.alt_ft = 3900.0;
        assert_eq!(judge(&plan, &r, None), Verdict::OnPlan);
        r.alt_ft = 4100.0;
        // Descending towards it: on plan.
        r.vs_fpm = -800.0;
        assert_eq!(judge(&plan, &r, None), Verdict::OnPlan);
        // Low and level: off; climbing back: on plan.
        r.alt_ft = 2000.0;
        r.vs_fpm = 0.0;
        assert!(matches!(judge(&plan, &r, None), Verdict::OffAltitude { ft } if ft < 0.0));
        r.vs_fpm = 500.0;
        assert_eq!(judge(&plan, &r, None), Verdict::OnPlan);
        // Within 300 ft: on plan.
        r.alt_ft = 2700.0;
        r.vs_fpm = 0.0;
        assert_eq!(judge(&plan, &r, None), Verdict::OnPlan);
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
        rt.push(0.0, THR, 10.0, 0.0);
        let json = serde_json::to_string(&rt).unwrap();
        let back: RadarTrack = serde_json::from_str(&json).unwrap();
        assert_eq!(back.samples.len(), 1);
    }
    /// The EPKK ILS 25 final (FAF 6.67 NM, 3000 ft).
    fn fin_epkk() -> FinalCourse {
        FinalCourse {
            faf_m: 6.67 * M_PER_NM,
            intercept_alt_ft: 3000,
            ..fin()
        }
    }

    #[test]
    fn star_end_near_the_centreline_joins_directly() {
        // The end of MAPIK 1G: 18 NM out, 0.9 NM right, 5000 ft, tracking across the course:
        // a short intercept, joined below the glide path, descending on the final to the FAF.
        let f = fin_epkk();
        let plan = plan_path(at(18.1, 0.94), COURSE - 82.0, 5000.0, &f);
        assert_eq!(kinds(&plan), vec![LegKind::Intercept, LegKind::Final]);
        assert!(plan.joins_in_time(&f));
        assert!(plan.descent_in_time(5000.0));
        assert_eq!(plan.legs[1].alt_ft, 3000);
        // On the final leg above the intercept altitude but on the profile: on plan.
        let mut r = radar(at(14.0, 0.0), COURSE);
        r.alt_ft = 4500.0;
        assert_eq!(judge(&plan_on_final(&plan), &r, None), Verdict::OnPlan);
        r.alt_ft = 5500.0;
        assert!(matches!(
            judge(&plan_on_final(&plan), &r, None),
            Verdict::OffAltitude { .. }
        ));
    }

    fn plan_on_final(plan: &VectorPlan) -> VectorPlan {
        VectorPlan {
            current: plan.legs.len() - 1,
            ..plan.clone()
        }
    }

    #[test]
    fn join_above_the_glide_path_is_not_accepted() {
        // 9 NM out, 1 NM left: the 30-degree intercept joins at 7.3 NM after 2 NM. The glide
        // path there (FAF 4 NM at 2500 ft) is about 3540 ft, so the join must be at or below
        // 3240 ft.
        let near = |alt: f64| plan_path(at(9.0, -1.0), COURSE + 60.0, alt, &fin());
        let low = near(3500.0);
        assert_eq!(kinds(&low), vec![LegKind::Intercept, LegKind::Final]);
        assert!(
            f64::from(low.legs[0].alt_ft) <= 3240.0,
            "{}",
            low.legs[0].alt_ft
        );
        // From 5000 ft it cannot get below the glide path by the join: lengthened.
        let high = near(5000.0);
        assert_ne!(kinds(&high), vec![LegKind::Intercept, LegKind::Final]);
        assert_gradient(&high, 5000.0);
    }

    #[test]
    fn fast_descent_after_the_clearance_stays_on_plan() {
        // The pattern from 6000 ft: legs planned at 500 ft steps above the intercept
        // altitude. Cleared to it when the plan needs it, the aircraft descends at 1000 fpm
        // (600 ft/NM at 100 kt), below the higher legs' altitudes.
        let f = fin();
        let plan = plan_path(at(-3.0, 0.0), COURSE, 6000.0, &f);
        let alts: Vec<i32> = plan.legs.iter().map(|l| l.alt_ft).collect();
        assert!(alts[0] > 2500 && alts[1] > 2500, "{alts:?}");
        let (mut alt, mut cleared, mut below_plan) = (6000.0f64, 6000, false);
        let (mut was_off, mut off_without) = (false, false);
        for (k, leg) in plan.legs.iter().enumerate() {
            if leg.kind == LegKind::Final {
                break;
            }
            let on = VectorPlan {
                current: k,
                ..plan.clone()
            };
            let len = distance_m(leg.from, leg.to);
            let mut d = 0.0;
            while d <= len {
                let mut r = radar(destination(leg.from, leg.track_deg, d), leg.track_deg);
                (r.gs_kt, r.alt_ft) = (100.0, alt);
                if cleared > 2500 && descent_due(&on, &r).is_some() {
                    cleared = 2500;
                }
                let descending = cleared == 2500 && alt > 2500.0;
                (r.alt_ft, r.vs_fpm) = (alt, if descending { -1000.0 } else { 0.0 });
                let v = judge(&on, &r, Some(cleared));
                assert!(
                    matches!(v, Verdict::OnPlan | Verdict::LegDue),
                    "{:?} at {alt:.0} ft, {d:.0} m: {v:?}",
                    leg.kind
                );
                below_plan |= alt < f64::from(leg.alt_ft) - 300.0;
                // Level again at the intercept altitude, below a leg planned higher: off
                // without the cleared level.
                if !descending && alt < f64::from(leg.alt_ft) - 300.0 {
                    was_off = true;
                    off_without |= matches!(judge(&on, &r, None), Verdict::OffAltitude { .. });
                }
                if descending {
                    alt = (alt - 600.0 * 0.05).max(2500.0);
                }
                d += 0.05 * M_PER_NM;
            }
        }
        assert_eq!(cleared, 2500);
        assert!(
            below_plan,
            "the fast descent went below a planned leg altitude"
        );
        assert!(was_off && off_without, "{was_off} {off_without}");
    }

    #[test]
    fn pattern_from_the_final() {
        // On the final 1 NM out (a missed approach): the plan would be the final alone; the
        // pattern for a new approach goes round on the circuit side.
        assert_eq!(
            kinds(&plan_path(at(1.0, 0.0), COURSE, 2000.0, &fin())),
            vec![LegKind::Final]
        );
        let plan = plan_pattern(at(1.0, 0.0), 4000.0, &fin());
        assert_eq!(
            kinds(&plan),
            vec![
                LegKind::Downwind,
                LegKind::Base,
                LegKind::Intercept,
                LegKind::Final
            ]
        );
        let (_, right) = RunwayFrame {
            origin: THR,
            course_deg: COURSE,
        }
        .to_local(plan.legs[0].to);
        assert!((right / M_PER_NM + 4.0).abs() < 0.05, "downwind {right}");
        assert!(plan.joins_in_time(&fin()));
        assert_gradient(&plan, 4000.0);
    }

    #[test]
    fn position_jump_clears_the_track() {
        let mut rt = RadarTrack::default();
        let p0 = LatLon {
            lat: 50.0,
            lon: 20.0,
        };
        let step = 120.0 * M_PER_NM / 3600.0 * 2.0;
        let mut p = p0;
        for k in 0..10 {
            rt.push(f64::from(k) * 2.0, p, 350.0, 120.0);
            p = destination(p, 0.0, step);
        }
        let drift = rt.drift_deg();
        assert!((drift - 10.0).abs() < 0.5, "{drift}");
        assert!(rt.track_deg().is_some());
        // Slewed 5 NM east within one sample: the old positions are dropped, the drift kept.
        let jumped = destination(p, 90.0, 5.0 * M_PER_NM);
        rt.push(20.0, jumped, 350.0, 120.0);
        assert_eq!(rt.samples.len(), 1);
        assert_eq!(rt.track_deg(), None);
        assert_eq!(rt.drift_deg(), drift);
        // A normal step after it is kept.
        rt.push(22.0, destination(jumped, 0.0, step), 350.0, 120.0);
        assert_eq!(rt.samples.len(), 2);
    }

    #[test]
    fn descent_due_by_the_turn_onto_the_intercept() {
        let f = fin();
        let plan = plan_path(at(25.0, -5.0), COURSE + 15.0, 5000.0, &f);
        assert_eq!(
            kinds(&plan),
            vec![LegKind::Base, LegKind::Intercept, LegKind::Final]
        );
        let base = plan.legs[0].clone();
        let lead = turn_lead_m(160.0, gap_deg(base.track_deg, plan.legs[1].track_deg));
        // 2500 ft to lose at 300 ft/NM: 8.33 NM before the turn onto the intercept.
        let need = 2500.0 / 300.0 * M_PER_NM + lead;
        let mut r = radar(
            destination(base.to, base.track_deg + 180.0, need + 0.3 * M_PER_NM),
            base.track_deg,
        );
        r.alt_ft = 5000.0;
        assert_eq!(descent_due(&plan, &r), None);
        r.pos = destination(base.to, base.track_deg + 180.0, need - 0.1 * M_PER_NM);
        assert_eq!(descent_due(&plan, &r), Some(2500));
        // Lower down, later.
        r.alt_ft = 3500.0;
        assert_eq!(descent_due(&plan, &r), None);
        // On the intercept leg: at once.
        let mut on = plan.clone();
        on.current = 1;
        r.pos = plan.legs[1].from;
        assert_eq!(descent_due(&on, &r), Some(2500));
        // A plan on the final only.
        let fin_only = plan_path(at(9.0, 0.0), COURSE, 3000.0, &f);
        assert_eq!(
            descent_due(&fin_only, &radar(at(9.0, 0.0), COURSE)),
            Some(2500)
        );
    }
}
