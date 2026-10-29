//! IFR geometry: procedure paths, SID/STAR choice and radar vectors to final.
//!
//! Heading convention: all geometry is in true degrees (aircraft telemetry heading and
//! `atc_geo` bearings are true). Spoken headings are magnetic, as on the pilot's heading
//! bug: `magnetic = true - mag_var_deg` (variation east positive). CIFP courses are
//! magnetic (ARINC 424); convert with `true = magnetic + mag_var_deg` before use.

use atc_geo::{LatLon, M_PER_NM, RunwayFrame, bearing_deg, destination, distance_m};
use atc_navdata::cifp::{self, AltConstraint, PathTerm};
use atc_navdata::fixes::{ProcedureSet, ResolvedProcedure};
use atc_phraseology::Turn;

/// A procedure flown as a polyline of named fixes.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Path {
    /// Fixes in flying order.
    pub points: Vec<(String, LatLon)>,
    /// Altitude constraints as (index into `points`, constraint).
    pub constraints: Vec<(usize, AltConstraint)>,
}

/// Fixes of `p` in order; legs without a position (VM, FA) are dropped.
pub fn procedure_path(p: &ResolvedProcedure) -> Path {
    let mut path = Path {
        points: Vec::new(),
        constraints: Vec::new(),
    };
    for leg in &p.legs {
        if matches!(leg.leg.path, PathTerm::Vm | PathTerm::Fa) {
            continue;
        }
        let (Some(fix), Some(pos)) = (&leg.leg.fix, leg.pos) else {
            continue;
        };
        if leg.leg.alt != AltConstraint::None {
            path.constraints.push((path.points.len(), leg.leg.alt));
        }
        path.points.push((fix.ident.clone(), pos));
    }
    path
}

/// Distance in metres from `pos` to the nearest segment of `path` (to the point for a
/// one-point path, infinity for an empty one).
pub fn cross_track_m(path: &Path, pos: LatLon) -> f64 {
    match path.points.as_slice() {
        [] => f64::INFINITY,
        [(_, only)] => distance_m(*only, pos),
        pts => pts
            .windows(2)
            .map(|w| segment_distance_m(w[0].1, w[1].1, pos))
            .fold(f64::INFINITY, f64::min),
    }
}

/// The aircraft at `pos` has passed fix `k` of `path`: it is within `within_m` of the fix, or
/// the path segment nearest to it starts at the fix or later.
pub fn passed_fix(path: &Path, k: usize, pos: LatLon, within_m: f64) -> bool {
    let Some((_, fix)) = path.points.get(k) else {
        return false;
    };
    if distance_m(*fix, pos) <= within_m {
        return true;
    }
    path.points
        .windows(2)
        .map(|w| segment_distance_m(w[0].1, w[1].1, pos))
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .is_some_and(|(seg, _)| seg >= k)
}

/// The aircraft at `pos` has joined `path`: it is within `within_m` of the first fix, or the
/// point of the path nearest to it is not the first fix (an aircraft still flying to the
/// start fix has not joined it).
pub fn joined_path(path: &Path, pos: LatLon, within_m: f64) -> bool {
    let Some((_, first)) = path.points.first() else {
        return false;
    };
    // Both on the flat projection of `cross_track_m`.
    let to_first = segment_distance_m(*first, *first, pos);
    distance_m(*first, pos) <= within_m || cross_track_m(path, pos) < to_first - 1.0
}

/// Distance from `p` to the segment `a`-`b`, on a local flat projection around `p`.
fn segment_distance_m(a: LatLon, b: LatLon, p: LatLon) -> f64 {
    let k = p.lat.to_radians().cos();
    let to_xy = |q: LatLon| {
        (
            (q.lon - p.lon).to_radians() * k * 6_371_000.0,
            (q.lat - p.lat).to_radians() * 6_371_000.0,
        )
    };
    let (ax, ay) = to_xy(a);
    let (bx, by) = to_xy(b);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (-(ax * dx + ay * dy) / len2).clamp(0.0, 1.0)
    };
    (ax + t * dx).hypot(ay + t * dy)
}

/// Lowest altitude a constraint refers to, in feet.
pub fn constraint_floor_ft(c: AltConstraint) -> Option<i32> {
    match c {
        AltConstraint::None => None,
        AltConstraint::At(a) | AltConstraint::AtOrAbove(a) | AltConstraint::AtOrBelow(a) => Some(a),
        AltConstraint::Between { above, .. } => Some(above),
    }
}

/// Constraints that still apply at the cleared altitude: those above it are dropped.
pub fn applicable_constraints(path: &Path, cleared_ft: i32) -> Vec<(usize, AltConstraint)> {
    path.constraints
        .iter()
        .copied()
        .filter(|(_, c)| constraint_floor_ft(*c).is_some_and(|a| a <= cleared_ft))
        .collect()
}

/// Initial climb of a departure along `path` to `cruise_ft`: the cruise level, lowered only
/// by the first constraint that is a ceiling at or below it (`At`, `AtOrBelow`, the upper
/// bound of `Between`). "At or above" constraints are minimums, never a stop altitude.
pub fn initial_climb_ft(path: &Path, cruise_ft: i32) -> i32 {
    path.constraints
        .iter()
        .find_map(|(_, c)| {
            let ceiling = match *c {
                AltConstraint::At(a) | AltConstraint::AtOrBelow(a) => a,
                AltConstraint::Between { below, .. } => below,
                AltConstraint::None | AltConstraint::AtOrAbove(_) => return None,
            };
            (ceiling <= cruise_ft).then_some(ceiling)
        })
        .unwrap_or(cruise_ft)
}

/// Smallest absolute difference of two bearings, degrees.
fn bearing_gap(a: f64, b: f64) -> f64 {
    ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs()
}

fn last_point(p: &ResolvedProcedure) -> Option<LatLon> {
    p.legs.iter().rev().find_map(|l| l.pos)
}

fn first_point(p: &ResolvedProcedure) -> Option<LatLon> {
    p.legs.iter().find_map(|l| l.pos)
}

/// The SID of `runway` whose last fix lies closest in bearing to the direction from `from`
/// to `dest`; ties go to the one with fewer legs.
pub fn choose_sid<'a>(
    set: &'a ProcedureSet,
    runway: &str,
    from: LatLon,
    dest: LatLon,
) -> Option<&'a ResolvedProcedure> {
    let want = bearing_deg(from, dest);
    set.sids(runway)
        .into_iter()
        .filter_map(|p| {
            let end = last_point(p)?;
            Some((bearing_gap(bearing_deg(from, end), want), p))
        })
        .min_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then(a.1.legs.len().cmp(&b.1.legs.len()))
        })
        .map(|(_, p)| p)
}

/// The STAR of `runway` whose start fix is closest in bearing (from the airport) to the
/// aircraft, among those whose start fix is still ahead: nearer the airport than the
/// aircraft is. `None` when the aircraft is already inside every start fix.
pub fn choose_star<'a>(
    set: &'a ProcedureSet,
    runway: &str,
    ac: LatLon,
    airport: LatLon,
) -> Option<&'a ResolvedProcedure> {
    let ac_bearing = bearing_deg(airport, ac);
    let ac_dist = distance_m(airport, ac);
    set.stars(runway)
        .into_iter()
        .filter_map(|p| {
            let start = first_point(p)?;
            (distance_m(airport, start) < ac_dist)
                .then(|| (bearing_gap(bearing_deg(airport, start), ac_bearing), p))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p)
}

/// Initial approach fix of the `kind` approach to `runway`: the IAF of its transition, else
/// the first fix of the final segment.
pub fn iaf(set: &ProcedureSet, runway: &str, kind: cifp::ApproachKind) -> Option<(String, LatLon)> {
    let fin = set.approach(runway, kind)?;
    let from_leg = |p: &ResolvedProcedure, flagged: bool| {
        p.legs
            .iter()
            .filter(|l| !flagged || l.leg.iaf)
            .find_map(|l| Some((l.leg.fix.as_ref()?.ident.clone(), l.pos?)))
    };
    set.procedures
        .iter()
        .filter(|p| {
            p.procedure.kind == cifp::ProcKind::Approach
                && p.procedure.ident == fin.procedure.ident
                && !p.procedure.transition.is_empty()
        })
        .find_map(|p| from_leg(p, true))
        .or_else(|| from_leg(fin, false))
}

/// Navdata approach type to the spoken one.
pub fn spoken_approach_kind(k: cifp::ApproachKind) -> atc_phraseology::ApproachKind {
    match k {
        cifp::ApproachKind::Ils => atc_phraseology::ApproachKind::Ils,
        cifp::ApproachKind::VorDme => atc_phraseology::ApproachKind::VorDme,
        cifp::ApproachKind::Rnav => atc_phraseology::ApproachKind::Rnav,
    }
}

/// Spoken approach type to the navdata one.
pub fn navdata_approach_kind(k: atc_phraseology::ApproachKind) -> cifp::ApproachKind {
    match k {
        atc_phraseology::ApproachKind::Ils => cifp::ApproachKind::Ils,
        atc_phraseology::ApproachKind::VorDme => cifp::ApproachKind::VorDme,
        atc_phraseology::ApproachKind::Rnav => cifp::ApproachKind::Rnav,
    }
}

/// A radar vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vector {
    /// Magnetic heading to fly, rounded to 5 degrees, 1..=360.
    pub heading_deg: u16,
    /// Direction of the turn from the aircraft's current heading.
    pub turn: Turn,
    /// Altitude to maintain, ft.
    pub alt_ft: i32,
}

/// Distance back along the intercept track from the intercept point to the lead point, NM.
const LEAD_NM: f64 = 3.0;
/// The intercept heading is given when the 30-degree track from the aircraft would join the
/// final between the intercept point and this far outside it, NM (the turn onto the
/// intercept takes the join point a little further in).
const JOIN_OUTSIDE_NM: f64 = 6.0;
/// Within this of the intercept point the intercept heading is given in any case, NM.
const NEAR_POINT_NM: f64 = 2.0;

/// Vector towards the final approach course, and whether it is the intercept heading. The
/// intercept point lies on the final course `intercept_nm` before the threshold `thr`; the
/// intercept track joins it at 30 degrees from the side the aircraft is on. The heading is
/// direct to the lead point 3 NM back along that intercept track; the intercept heading is
/// given once the 30-degree track from the aircraft would join the final between the
/// intercept point and 6 NM outside it (or within 2 NM of it): an aircraft coming from the
/// side joins the final at about the intercept point, not miles inside it.
///
/// `ac_hdg` and `course_deg` are true; `mag_var_deg` is the variation (east positive) and the
/// returned heading is magnetic (`true - mag_var_deg`), rounded to 5 degrees.
pub fn vector_to_final(
    ac: LatLon,
    ac_hdg: f64,
    thr: LatLon,
    course_deg: f64,
    intercept_nm: f64,
    alt_ft: i32,
    mag_var_deg: f64,
) -> (Vector, bool) {
    let point = destination(thr, course_deg + 180.0, intercept_nm * M_PER_NM);
    let (along, right) = RunwayFrame {
        origin: thr,
        course_deg,
    }
    .to_local(ac);
    // Out along the approach, and to the right of the landing direction, in NM.
    let (x, y) = (-along / M_PER_NM, right / M_PER_NM);
    // From the right of the landing direction the intercept turns left of the course.
    let intercept_hdg = if y > 0.0 {
        course_deg - 30.0
    } else {
        course_deg + 30.0
    };
    let lead = lead_point(ac, thr, course_deg, intercept_nm);
    let join_nm = x - y.abs() * 3f64.sqrt();
    let intercept = (x > 0.0 && (intercept_nm..=intercept_nm + JOIN_OUTSIDE_NM).contains(&join_nm))
        || distance_m(ac, point) <= NEAR_POINT_NM * M_PER_NM;
    let true_hdg = if intercept {
        intercept_hdg
    } else {
        bearing_deg(ac, lead)
    };
    (vector_on(true_hdg, ac_hdg, alt_ft, mag_var_deg), intercept)
}

/// The lead point [`vector_to_final`] steers aircraft `ac` to: 3 NM back along the
/// 30-degree intercept track (from the side `ac` is on) that joins the final course
/// `course_deg` (true) of threshold `thr` `intercept_nm` out.
pub fn lead_point(ac: LatLon, thr: LatLon, course_deg: f64, intercept_nm: f64) -> LatLon {
    let point = destination(thr, course_deg + 180.0, intercept_nm * M_PER_NM);
    let (_, right) = RunwayFrame {
        origin: thr,
        course_deg,
    }
    .to_local(ac);
    let intercept_hdg = if right > 0.0 {
        course_deg - 30.0
    } else {
        course_deg + 30.0
    };
    destination(point, intercept_hdg + 180.0, LEAD_NM * M_PER_NM)
}

/// The vector to fly true heading `true_hdg` from true heading `ac_hdg`: magnetic
/// (`true - mag_var_deg`), rounded to 5 degrees, with the shorter turn.
pub fn vector_on(true_hdg: f64, ac_hdg: f64, alt_ft: i32, mag_var_deg: f64) -> Vector {
    let true_hdg = true_hdg.rem_euclid(360.0);
    let delta = (true_hdg - ac_hdg + 540.0).rem_euclid(360.0) - 180.0;
    let mag = ((true_hdg - mag_var_deg) / 5.0).round() as i64 * 5;
    let mag = mag.rem_euclid(360);
    Vector {
        heading_deg: if mag == 0 { 360 } else { mag as u16 },
        turn: if delta >= 0.0 {
            Turn::Right
        } else {
            Turn::Left
        },
        alt_ft,
    }
}

/// Distance from `ac` to the threshold in whole nautical miles.
pub fn distance_to_go_nm(ac: LatLon, thr: LatLon) -> u16 {
    (distance_m(ac, thr) / M_PER_NM).round() as u16
}

/// Spoken form of a SID or STAR designator: (fix name, number, letter), e.g. `SUDO2J` of a
/// SID ending at SUDOL is ("SUDOL", 2, 'J'). The name is the last fix of a SID and the first
/// of a STAR; number and letter are the last two characters of the ident.
pub fn spoken_designator(p: &ResolvedProcedure) -> (String, u8, char) {
    let fixes = p.legs.iter().filter_map(|l| l.leg.fix.as_ref());
    let name = if p.procedure.kind == cifp::ProcKind::Star {
        fixes.map(|f| f.ident.clone()).next()
    } else {
        fixes.map(|f| f.ident.clone()).next_back()
    }
    .unwrap_or_default();
    let mut tail = p.procedure.ident.chars().rev();
    let letter = tail.next().unwrap_or(' ');
    let number = tail.next().and_then(|c| c.to_digit(10)).unwrap_or(0) as u8;
    (name, number, letter)
}
