//! Approach position, departures (spec 3.2 steps 5-6, AIP AD 2.22 §1): Tower hands an IFR
//! departure to Approach once it is airborne; Approach identifies it on its first call
//! ("airborne") and clears the climb to the cruise level when the SID's initial climb was
//! lower; once the aircraft has passed the SID's first turn (its second fix: within 1 NM or
//! past it along the path) and is still climbing, Approach clears it direct to the destination
//! (the SID's last fix when the destination is beyond the TMA and has no adjacent tower).
//! While on the SID, the track and the cleared level are monitored ([`IfrClearance`]).
//!
//! Hand-off: to the destination's adjacent tower 10 NM from it; otherwise at the TMA radius
//! from the airport, to the area control unit when the flight is cleared at or above its
//! lowest level, else "radar service terminated" and the flight information unit (neither
//! unit: "radar service terminated, frequency change approved", the flight leaves Approach's
//! service). Spec 3.2
//! step 6 also allows a hand-off "on reaching the cleared level near the boundary"; here it is
//! always made at the TMA radius (ruling: one rule, the same unit either way). Without a TMA
//! in `procedures.toml` there is no boundary hand-off. The adjacent unit answers the first
//! call on its frequency of the flight handed to it with "identified", and nothing after it.
//! An area or information unit called by an inbound IFR flight sends it to Approach
//! ("contact Kraków Approach"); an adjacent tower does not answer it. Spec 3.6: such a call,
//! or an inbound call to Approach, from a flight that was handed off or still holds its
//! departure plan begins a new flight ([`crate::strip::Strip::end_flight`]): it is served as
//! an arrival.
//!
//! Before the hand-off to the destination's tower, Approach clears the descent 20 NM out, to
//! 2000 ft above the destination's elevation rounded up to 500 ft (3000 ft when the elevation
//! is unknown), with the QNH.
//!
//! An airborne flight without a flight plan reporting its departure gets "no flight plan, say
//! intentions"; asking for an IFR clearance, it is cleared in the air
//! ([`AtcEngine::airborne_clearance`]: no SID or route, the cruise level, a squawk) and then
//! handed off like a SID departure; a request naming no flight rules gets "say flight rules",
//! a VFR one is not Approach's.
//!
//! Arrivals (spec 3.3): an airborne aircraft without a flight plan calling Approach inbound
//! (or asking for an approach) gets an IFR plan to this airport (level: for a flight handed
//! in, the one the previous unit cleared it to; else the one it reports, never the one after
//! "passing" (the highest said when climbing, the lowest when descending); the arrival level
//! when it reports only the level it is passing; else its altitude); a VFR one (it says VFR, or names a VFR point without saying IFR) is
//! sent to Tower.
//! The first reply: "identified", then the STAR chosen by [`choose_star`] when the aircraft is
//! still outside its start fix and on its side of the field (the start fix within 90 degrees
//! of the aircraft, seen from the airport; a STAR on the far side would be a detour around
//! the field) (never for a VOR/DME approach: AIP AD 2.22, VOR approaches are
//! not linked to STARs), otherwise direct to the IAF (full procedure, on request) or a
//! radar vector (default); the level (the IFR default level, or the intercept altitude when
//! the planned path needs the descent at once, never above the present level), the QNH and
//! the approach to expect: ILS on a runway that has one, else RNAV, VOR/DME on request only
//! (or when it is the only one); a requested type the runway lacks: "unable" and the default.
//! When the planned path starts on the intercept (or the final), the reply clears the
//! approach instead (distance to go, "cleared `<approach>`", "report established").
//! On vectors (after the STAR's last fix, if one was given) Approach plans the path to the
//! final in advance ([`vectoring::plan_path`]: a direct join when the turn onto the
//! intercept is not more than 90 degrees, else a pattern on the aircraft's side of the final,
//! downwind 4 NM abeam, base, intercept; the intercept meets the final at 20-30 degrees at
//! least 2 NM outside the FAF) and flies the aircraft along it: the heading of each leg is
//! its ground track corrected for the drift the radar shows (magnetic, rounded to 5 degrees),
//! the first when vectoring starts and the next one turn lead before the turn point (the
//! standard-rate radius at the radar ground speed plus 10 s, [`vectoring::judge`]); the
//! descent to the intercept altitude (the FAF's altitude constraint) is given when the plan
//! needs it, at 300 ft per NM, to be there by the turn onto the intercept where the path
//! allows (the plan itself only needs the final joined below the glide path and the
//! intercept altitude by the FAF, [`vectoring::descent_due`]); the intercept heading comes
//! with the distance to go,
//! "cleared `<approach>`" and "report established". Until the approach clearance the
//! aircraft is watched against the plan ([`AtcEngine::correction_due`]): not while a heading
//! or level readback is pending nor for 10 s after it, at most once per 20 s, only while it
//! flies its assigned heading (within 15 degrees) and is not turning onto it. Off the leg
//! (0.5 NM, 1 NM on downwind) and not converging: the path is planned anew from where it is
//! and the new heading given, with the reason "you are <tenths> <side> of the localizer"
//! (the final approach course) on a leg within 45 degrees of parallel to the final (on a
//! base the heading alone); above the planned altitude and not descending: the descent (the
//! heading only when it changes by 10 degrees or more). The monitor's heading (15 degrees
//! on planned vectors) and level checks are the planner's ([`AtcEngine::planned_checks`]):
//! still on its previous heading, "check heading, turn <left/right> heading <h>" (no
//! approach clearance with it: given once the heading is flown); turned onto a wrong
//! heading, the plain correction; a level bust, "check level" / "check altimeter setting",
//! the path planned anew. A check due within 20 s of a correction is made after them, if
//! still off. After a missed approach the plan is
//! made anew from the published missed-approach altitude, a pattern when the aircraft is
//! still on the final ([`vectoring::plan_pattern`]).
//! Cleared but not established by the FAF (4 NM without one): "check established" once,
//! from the station owning the flight (spec 3.5). Full
//! procedure: "cleared `<approach>`" at the IAF. "Established" (or the final inside the FAF)
//! transfers the flight to Tower; an arrival vectored onto the final only when the radar
//! agrees (within 0.3 NM of the course, tracking within 15 degrees of it,
//! [`vectoring::established`]), else "negative" with the re-intercept correction (or, already
//! turning back onto the course, where it is). Until then it is watched
//! ([`AtcEngine::watched_final`]): flying through the course by more than 0.3 NM, or towards
//! it at more than 45 degrees within 1 NM, and not turning the right way, gets "you are
//! <tenths> <side> of the <course>, turn <left/right> heading <h> to re-intercept the
//! <course>" (the intercept from where it is, 30 to 45 degrees,
//! [`vectoring::plan_reintercept`]), under the 20 s and readback rules of the corrections;
//! when that cannot join 2 NM outside the FAF it is broken off: the turn away towards the
//! side of a new pattern, "maintain" the higher of its altitude and the intercept altitude,
//! "I will vector you for a new approach", the approach clearance cancelled (not a missed
//! approach). A missed approach (Tower's go-around, or leaving the final
//! airborne near the threshold or climbing) goes back to Approach, which vectors it again
//! from the published missed-approach altitude when it calls; the published missed-approach
//! track itself is not monitored. A requested
//! approach type is given when the CIFP has it for the runway, else "unable"; a visual
//! approach is not available at night (21:00-05:00 UTC in summer, 20:00-04:00 in winter) and
//! by day is Tower's (contact Tower). Squawk 7600 (AIP AD 2.22 §2.1): Approach says nothing
//! more to the aircraft and calls no deviation; the strip records it.
//!
//! Approach and the adjacent units always speak English (spec 3.1).

use atc_geo::{LatLon, M_PER_NM, RunwayFrame, bearing_deg, distance_m};
use atc_navdata::procedures::CircuitSide;
use atc_navdata::procedures::{AdjacentKind, AdjacentUnit};
use atc_phraseology::{
    ApproachKind, AtcItem, AtcMessage, Compass8, CourseName, FlightRules, HeardItems, Lang,
    PilotIntent, PilotMessage, Token, Turn, UnableReason, normalize, render,
};

use super::delivery::{cleared_to, mentions_ifr};
use super::{AtcEngine, AtcOutput};
use crate::compliance::{IfrClearance, TURNING_DEG_S};
use crate::ifr::{
    Path, Vector, choose_star, constraint_floor_ft, distance_to_go_nm, iaf, joined_path,
    navdata_approach_kind, passed_fix, procedure_path, spoken_designator, vector_on,
};
use crate::phase::FlightPhase;
use crate::state::AircraftState;
use crate::strip::{FlightPlan, Role};
use crate::vectoring::{
    self, FinalCourse, LegKind, Radar, VectorLeg, VectorPlan, Verdict, heading_for_track,
};

/// A fix counts as passed within this distance of it.
const PASSED_FIX_M: f64 = M_PER_NM;
/// The destination's tower takes the flight this far from the destination.
const DEST_TOWER_M: f64 = 10.0 * M_PER_NM;
/// The destination's Approach (another loaded station) takes the flight this far from the
/// destination.
const DEST_APPROACH_M: f64 = 30.0 * M_PER_NM;
/// The destination's Approach (another loaded station) or adjacent tower works the flight
/// only when the destination's reference point lies within this distance of this station's
/// airport: a neighbour. A destination farther away gets the usual ACC/FIS hand-off (or
/// release) on leaving the TMA, as with neither unit.
const NEIGHBOUR_DEST_M: f64 = 60.0 * M_PER_NM;

/// A hand-off due: the unit's callsign (empty for a release), the ICAO code of the airport
/// for a destination Approach or Tower, and the items to say.
struct HandOff {
    unit: String,
    airport: Option<String>,
    items: Vec<AtcItem>,
}
/// Why an arrival on radar vectors is corrected ([`AtcEngine::vector_correction`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Correction {
    /// Off the planned leg and not converging on it, or flying a wrong heading (turned, but
    /// more than 15 degrees off): the reason on a leg nearly parallel to the final
    /// ([`off_course`]), then the heading. With a level call (the monitor's level check due
    /// at the same time): the call after the heading, the rules of [`Correction::Level`].
    Plain(Option<AtcItem>),
    /// Still on its previous heading after the reaction time: "check heading", then the
    /// heading (no approach clearance until it is flown); a level call as with `Plain`.
    NotTurned(Option<AtcItem>),
    /// Above the planned altitude and not descending: the descent (the heading only when it
    /// changes).
    Altitude,
    /// Off its cleared level: the monitor's wording, "check level" / "check altimeter
    /// setting" (this item, the only level item; for an aircraft at or above its cleared
    /// level, the descent instead when the new path needs it now), the heading only when it
    /// changes, never the approach clearance.
    Level(AtcItem),
}

/// A leg within this of parallel to the final approach course (either way), degrees:
/// downwind, intercept and final, where the offset from the course is the error.
const PARALLEL_DEG: f64 = 45.0;

/// "You are <tenths> <side> of the <course>": the aircraft at `pos` from the final approach
/// course of `f` (the localizer for an ILS), given only on a leg of `track_deg` nearly
/// parallel to it ([`PARALLEL_DEG`]; on a base or a direct leg the heading is said alone);
/// `None` also on the course (under 0.05 NM off).
fn off_course(f: &FinalCourse, kind: ApproachKind, pos: LatLon, track_deg: f64) -> Option<AtcItem> {
    let gap = vectoring::gap_deg(track_deg, f.course_deg);
    if gap > PARALLEL_DEG && gap < 180.0 - PARALLEL_DEG {
        return None;
    }
    let (_, right) = RunwayFrame {
        origin: f.thr,
        course_deg: f.course_deg,
    }
    .to_local(pos);
    let tenths_nm = (right.abs() / M_PER_NM * 10.0).round() as u16;
    (tenths_nm > 0).then(|| AtcItem::OffCourse {
        tenths_nm,
        side: Compass8::from_bearing(f.course_deg + 90.0f64.copysign(right)),
        course: course_name(kind),
    })
}

/// The final approach course of an approach of `kind` as spoken: the localizer for an ILS.
fn course_name(kind: ApproachKind) -> CourseName {
    match kind {
        ApproachKind::Ils => CourseName::Localizer,
        ApproachKind::VorDme | ApproachKind::Rnav => CourseName::FinalApproachCourse,
    }
}

/// A published non-ILS final more than this off the runway's course is not taken as the
/// final (PANS-OPS straight-in alignment is within 30 degrees).
const STRAIGHT_IN_MAX_DEG: f64 = 30.0;

/// A re-intercept heading within this of the one flown is "fly heading", without a turn.
const SAME_HEADING_DEG: f64 = 5.0;

/// Notes a vector given to the arrival of plan `p` (magnetic heading `actual` when it was
/// given): the heading said (if any) and the heading it was flying, the planned path, and
/// the time of a correction (`corrected_at`).
fn note_vectors(
    p: &mut FlightPlan,
    heading: Option<u16>,
    actual: f64,
    vp: VectorPlan,
    corrected_at: Option<f64>,
) {
    if heading.is_some() {
        p.heading = heading;
        p.heading_from = Some(actual.round() as u16 % 360);
    }
    p.vector_plan = Some(vp);
    if corrected_at.is_some() {
        p.last_correction_s = corrected_at;
    }
}

/// Climbing: a vertical speed above this, or still this far below the cleared level.
const CLIMBING_FPM: f64 = 100.0;
const BELOW_LEVEL_FT: f64 = 200.0;
/// Approach clears a departure's descent this far from a destination with a tower.
const DEST_DESCENT_M: f64 = 20.0 * M_PER_NM;
/// The approach altitude of an airport: this far above its elevation, rounded up to
/// `LEVEL_STEP_FT`; `UNKNOWN_FIELD_ALT_FT` when the elevation is unknown.
const ABOVE_FIELD_FT: i32 = 2000;
const LEVEL_STEP_FT: i32 = 500;
const UNKNOWN_FIELD_ALT_FT: i32 = 3000;
/// Transponder code of a radio failure.
pub(super) const RADIO_FAILURE_SQUAWK: u16 = 7600;
/// Arrival level when `procedures.toml` has no IFR defaults.
const DEFAULT_ARRIVAL_FT: i32 = 5000;
/// Without a published FAF, the final approach fix is taken this far from the threshold
/// (transfer to Tower, approach-not-established check).
const DEFAULT_FAF_M: f64 = 4.0 * M_PER_NM;
/// Leaving the final climbing faster than this (fpm) after the approach clearance: a missed
/// approach.
const MISSED_CLIMB_FPM: f64 = 300.0;
/// An IFR arrival leaving the final this close to the threshold, still airborne, has gone
/// around.
const MISSED_APPROACH_M: f64 = 2.0 * M_PER_NM;
/// Within this of a level the aircraft is at it ("maintain" rather than "climb"/"descend").
const LEVEL_TOLERANCE_FT: f64 = 200.0;
/// Night for visual approaches, UTC hours `[from, to)` across midnight: summer, winter.
const NIGHT_SUMMER_UTC: (f64, f64) = (21.0, 5.0);
const NIGHT_WINTER_UTC: (f64, f64) = (20.0, 4.0);

impl AtcEngine {
    /// A call on the Approach frequency.
    pub(super) fn approach_request(&mut self, i: usize, msg: &PilotMessage) -> Vec<AtcItem> {
        if std::mem::take(&mut self.strips[i].handed_in) {
            return self.handed_in_call(i, msg);
        }
        if self.arrival(i).is_some() && self.strips[i].missed_approach {
            return self.after_missed_approach(i);
        }
        let visual =
            matches!(msg.intent, PilotIntent::RequestApproach { .. }) && says(&msg.raw, "visual");
        if visual {
            return self.visual_request(i);
        }
        let new = self.strips[i].plan.is_none();
        match &msg.intent {
            PilotIntent::ReportDeparture => self.identify_departure(i),
            PilotIntent::RequestClearance {
                destination,
                rules,
                level_ft,
                ..
            } if new && self.strips[i].phase.is_airborne() => match rules {
                None => vec![AtcItem::SayFlightRules],
                Some(FlightRules::Ifr) => {
                    self.airborne_clearance(i, destination.as_deref(), *level_ft)
                }
                Some(FlightRules::Vfr) => Vec::new(),
            },
            PilotIntent::ReportInbound { .. } if new => {
                self.inbound(i, Some(msg), None, false, None)
            }
            PilotIntent::RequestApproach {
                kind,
                full_procedure,
            } if new => self.inbound(i, Some(msg), *kind, *full_procedure, None),
            PilotIntent::RequestApproach {
                kind,
                full_procedure,
            } if self.arrival(i).is_some() => self.change_approach(i, *kind, *full_procedure),
            PilotIntent::ReportEstablished
                if self.arrival(i).is_some_and(|p| p.approach_cleared) =>
            {
                self.established_report(i)
            }
            _ => Vec::new(),
        }
    }

    /// The first call of a flight handed in from another station ([`AtcEngine::import_flight`]):
    /// a flight bound for this airport (or without a plan) is served as an inbound arrival
    /// ("identified" and the route to its approach; VFR: sent to Tower), a flight passing
    /// through is identified and keeps its plan.
    fn handed_in_call(&mut self, i: usize, msg: &PilotMessage) -> Vec<AtcItem> {
        let here = &self.airport.icao;
        let plan = self.strips[i].plan.clone();
        if plan
            .as_ref()
            .is_some_and(|p| !p.destination.eq_ignore_ascii_case(here))
        {
            self.strips[i].identified = true;
            return vec![AtcItem::Identified];
        }
        if plan.as_ref().is_some_and(|p| !p.ifr) {
            return self.redirect(i, Role::Tower);
        }
        let items = self.handed_in_arrival(i, Some(msg));
        if items.is_empty() {
            // No telemetry yet: still the flight handed over, served on its next call.
            self.strips[i].handed_in = true;
        }
        items
    }

    /// The arrival instruction for IFR flight `i` handed in bound for this airport: the
    /// route to its approach from the level the previous unit cleared it to (as
    /// [`AtcEngine::inbound`]), the approach type as `msg` requests. Without a call (`msg`
    /// `None`: handed in silently, already identified) no "identified". Nothing (the plan
    /// kept): no telemetry yet.
    fn handed_in_arrival(&mut self, i: usize, msg: Option<&PilotMessage>) -> Vec<AtcItem> {
        let plan = self.strips[i].plan.clone();
        // The level the previous unit cleared it to, kept as the level it reports.
        let cleared_ft = plan.as_ref().map(|p| p.cleared_ft).filter(|&ft| ft > 0);
        self.strips[i].plan = None;
        let (kind, full) = match msg.map(|m| &m.intent) {
            Some(&PilotIntent::RequestApproach {
                kind,
                full_procedure,
            }) => (kind, full_procedure),
            _ => (None, false),
        };
        let items = self.inbound(i, msg, kind, full, cleared_ft);
        if items.is_empty() {
            self.strips[i].plan = plan;
        }
        items
    }

    /// Per sample, for IFR flight `i` handed in silently in radar contact
    /// ([`crate::strip::Strip::radar_contact_kept`]): bound for this airport, Approach goes on with the
    /// arrival instruction unprompted, once; passing through, nothing to say.
    fn radar_contact_arrival(&mut self, i: usize) -> Vec<AtcOutput> {
        let here = self.strips[i]
            .plan
            .as_ref()
            .is_some_and(|p| p.destination.eq_ignore_ascii_case(&self.airport.icao));
        if !here {
            self.strips[i].radar_contact_kept = false;
            return Vec::new();
        }
        let items = self.handed_in_arrival(i, None);
        if items.is_empty() {
            return Vec::new();
        }
        self.strips[i].radar_contact_kept = false;
        self.transmit(i, Role::Approach, items)
    }

    /// The flight plan of aircraft `i` when it is an IFR arrival to this airport.
    pub(super) fn arrival(&self, i: usize) -> Option<&FlightPlan> {
        self.strips[i]
            .plan
            .as_ref()
            .filter(|p| p.ifr && p.approach.is_some())
    }

    /// The first call of an airborne aircraft without a flight plan: a VFR flight is sent to
    /// Tower; an IFR flight gets a plan to this airport and the route to its approach (see
    /// the module docs). `kind` / `full`: as requested; `cleared_ft`: the level a handed-in
    /// flight was cleared to by the previous unit. `msg` `None`: no call (a flight handed in
    /// silently, already identified), so no "identified".
    fn inbound(
        &mut self,
        i: usize,
        msg: Option<&PilotMessage>,
        kind: Option<ApproachKind>,
        full: bool,
        cleared_ft: Option<i32>,
    ) -> Vec<AtcItem> {
        if !self.strips[i].phase.is_airborne() {
            return Vec::new();
        }
        let vfr = msg.is_some_and(|msg| {
            says(&msg.raw, "vfr") || (!mentions_ifr(&msg.raw) && !msg.items.points.is_empty())
        });
        if vfr {
            return self.redirect(i, Role::Tower);
        }
        let Some(s) = self
            .tracks
            .get(&self.strips[i].callsign)
            .map(|t| t.last.clone())
        else {
            return Vec::new();
        };
        let runway = self.strips[i].runway.clone();
        let requested = kind.filter(|&k| self.has_approach(&runway, k));
        let unavailable = kind.is_some() && requested.is_none();
        let (Some(set), Some(kind)) = (
            self.procedure_set.as_ref(),
            requested.or_else(|| self.default_approach(&runway)),
        ) else {
            return vec![AtcItem::Unable(UnableReason::NoIfrData)];
        };
        let Some((thr, _)) = self.final_geometry(&runway) else {
            return vec![AtcItem::Unable(UnableReason::NoIfrData)];
        };
        // AIP AD 2.22: VOR approaches are not linked to STARs.
        let star = self
            .airport_ref()
            .filter(|_| kind != ApproachKind::VorDme)
            .and_then(|home| choose_star(set, &runway, s.pos, home));
        let star_item = star.map(|p| {
            let (fix, number, letter) = spoken_designator(p);
            (
                p.procedure.ident.clone(),
                AtcItem::Star {
                    fix,
                    number,
                    letter,
                },
            )
        });
        let iaf_fix = iaf(set, &runway, navdata_approach_kind(kind)).map(|(name, _)| name);
        let vectors = !full;
        let target = self.procs.ifr.map_or(DEFAULT_ARRIVAL_FT, |d| d.cruise_ft);
        // The level it is cleared to: the one the previous unit cleared it to, else the one
        // it reports ([`reported_level`]), else its altitude; reporting only the altitude it
        // is passing, the arrival level (a passing altitude is never given back as a level).
        let actual_ft = ((s.alt_ft_msl / 100.0).round() as i32) * 100;
        let reported = msg.map_or(Reported::default(), |msg| {
            reported_level(&msg.raw, self.strips[i].lang, &msg.items)
        });
        let present_ft = cleared_ft
            .or(reported.level_ft)
            .unwrap_or(if reported.passing { target } else { actual_ft });
        let mut level_ft = present_ft.min(target);
        let mut items = if msg.is_some() {
            vec![AtcItem::Identified]
        } else {
            Vec::new()
        };
        let (mut heading, mut direct, mut vector_plan) = (None, None, None);
        // A plan that starts on the intercept (or the final): cleared in this reply.
        let mut cleared_now = false;
        match (&star_item, full, &iaf_fix) {
            (Some((_, item)), _, _) => items.push(item.clone()),
            (None, true, Some(fix)) => {
                items.push(AtcItem::DirectTo { fix: fix.clone() });
                direct = Some(fix.clone());
            }
            _ => {
                // Radar vectors start here: the planned path from the level it is cleared
                // to, its first heading, and the descent at once when the plan needs it.
                let (Some(f), Some(r)) = (self.final_course(&runway, kind), self.radar(i)) else {
                    return vec![AtcItem::Unable(UnableReason::NoIfrData)];
                };
                let vp = self.new_vector_plan(&r, level_ft, &f);
                let v = self.leg_vector(i, vp.leg(), &s);
                items.push(AtcItem::Heading {
                    deg: v.heading_deg,
                    turn: Some(v.turn),
                });
                if let Some(ft) = vectoring::descent_due(&vp, &r) {
                    level_ft = level_ft.min(ft);
                }
                heading = Some(v.heading_deg);
                cleared_now = matches!(vp.leg().kind, LegKind::Intercept | LegKind::Final);
                vector_plan = Some(vp);
            }
        }
        // Climb, descent or maintain from where it is now (a flight climbing through the
        // level to a higher one is stopped there with "maintain").
        let now = s.alt_ft_msl - f64::from(level_ft);
        items.push(if now > LEVEL_TOLERANCE_FT {
            AtcItem::DescendTo(self.level(level_ft))
        } else if now < -LEVEL_TOLERANCE_FT {
            AtcItem::ClimbTo(self.level(level_ft))
        } else {
            AtcItem::MaintainLevel(self.level(level_ft))
        });
        if let Some(atis) = &self.atis {
            items.push(AtcItem::Qnh(atis.current().weather.qnh_hpa));
        }
        if unavailable {
            items.push(AtcItem::Unable(UnableReason::ApproachNotAvailable));
        }
        if cleared_now {
            items.push(AtcItem::DistanceToGo(distance_to_go_nm(s.pos, thr)));
            items.push(AtcItem::ClearedApproach {
                kind,
                runway: runway.clone(),
            });
            items.push(AtcItem::ReportEstablished);
        } else {
            items.push(AtcItem::ExpectApproach {
                kind,
                runway: runway.clone(),
            });
        }
        let plan = FlightPlan {
            ifr: true,
            destination: self.airport.icao.clone(),
            dest_name: self.airport.name.clone(),
            cruise_ft: present_ft,
            sid: None,
            dep_runway: None,
            star: star_item.map(|(ident, _)| ident),
            approach: Some((kind, runway)),
            vectors,
            cleared_ft: level_ft,
            direct,
            heading,
            approach_cleared: cleared_now,
            vector_plan,
            last_correction_s: None,
            heading_from: heading.map(|_| self.magnetic_heading(&s).round() as u16 % 360),
            deferred_heading: false,
            deferred_level: false,
            established_check: false,
            established_reported: false,
        };
        let strip = &mut self.strips[i];
        strip.plan = Some(plan);
        strip.identified = true;
        strip.expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// A request for another approach type or method from an arrival.
    fn change_approach(
        &mut self,
        i: usize,
        kind: Option<ApproachKind>,
        full: bool,
    ) -> Vec<AtcItem> {
        let Some((current, runway)) = self.arrival(i).and_then(|p| p.approach.clone()) else {
            return Vec::new();
        };
        let mut items = Vec::new();
        let mut new_kind = current;
        if let Some(k) = kind {
            if !self.has_approach(&runway, k) {
                return vec![AtcItem::Unable(UnableReason::ApproachNotAvailable)];
            }
            new_kind = k;
            items.push(AtcItem::ExpectApproach {
                kind: k,
                runway: runway.clone(),
            });
        }
        let iaf_fix = self
            .procedure_set
            .as_ref()
            .and_then(|set| iaf(set, &runway, navdata_approach_kind(new_kind)))
            .map(|(name, _)| name);
        let strip = &mut self.strips[i];
        let Some(plan) = strip.plan.as_mut() else {
            return Vec::new();
        };
        if new_kind != current {
            plan.approach = Some((new_kind, runway));
            plan.approach_cleared = false;
            plan.established_check = false;
            plan.established_reported = false;
            // Another final (FAF, intercept altitude): planned again.
            plan.vector_plan = None;
            // AIP AD 2.22: VOR approaches are not linked to STARs.
            if new_kind == ApproachKind::VorDme {
                plan.star = None;
            }
        }
        if full && let Some(fix) = iaf_fix {
            plan.vectors = false;
            plan.heading = None;
            plan.vector_plan = None;
            plan.direct = Some(fix.clone());
            plan.approach_cleared = false;
            items.push(AtcItem::DirectTo { fix });
        } else if !full && kind.is_none() {
            // "Request vectors".
            plan.vectors = true;
            plan.direct = None;
            items.push(AtcItem::Roger);
        }
        strip.expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// "Request visual approach": not available at night; by day it is Tower's.
    fn visual_request(&mut self, i: usize) -> Vec<AtcItem> {
        if self.is_night() {
            vec![AtcItem::VisualApproachNotAvailable]
        } else {
            self.hand_over(i, Role::Tower)
        }
    }

    /// Night at the sim UTC time of the latest telemetry (unknown time: day; unknown
    /// season: night in both).
    fn is_night(&self) -> bool {
        let Some(utc) = self.clock.0 else {
            return false;
        };
        let hour = (utc / 3600.0).rem_euclid(24.0);
        let night = |(from, to): (f64, f64)| hour >= from || hour < to;
        match self.clock.1 {
            Some(d) if super::delivery::SUMMER_DAYS.contains(&d) => night(NIGHT_SUMMER_UTC),
            Some(_) => night(NIGHT_WINTER_UTC),
            None => night(NIGHT_SUMMER_UTC) && night(NIGHT_WINTER_UTC),
        }
    }

    /// The first call after a missed approach (or the first sample after one while Approach
    /// still had the flight): identified again and vectored. The first vectors keep the
    /// published missed-approach altitude; the descent to the intercept altitude waits until
    /// the aircraft has reached it. The published missed-approach track is not monitored: a
    /// missed approach is Tower's go-around and Approach's vectors.
    fn after_missed_approach(&mut self, i: usize) -> Vec<AtcItem> {
        let strip = &mut self.strips[i];
        strip.missed_approach = false;
        strip.identified = true;
        let items = self.vector_items(i, true);
        self.strips[i].expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// The published approach of `kind` to `runway` exists.
    fn has_approach(&self, runway: &str, kind: ApproachKind) -> bool {
        self.procedure_set
            .as_ref()
            .is_some_and(|set| set.approach(runway, navdata_approach_kind(kind)).is_some())
    }

    /// The default approach to `runway`: ILS, else RNAV; VOR/DME only when it is the only one.
    pub(super) fn default_approach(&self, runway: &str) -> Option<ApproachKind> {
        [ApproachKind::Ils, ApproachKind::Rnav, ApproachKind::VorDme]
            .into_iter()
            .find(|&k| self.has_approach(runway, k))
    }

    /// Threshold of `runway` and its final approach course (true).
    pub(super) fn final_geometry(&self, runway: &str) -> Option<(LatLon, f64)> {
        let rw = self
            .airport
            .runways
            .iter()
            .find(|r| r.ends.iter().any(|e| e.ident == runway))?;
        let k = rw.ends.iter().position(|e| e.ident == runway)?;
        let (thr, opp) = (rw.ends[k].threshold, rw.ends[1 - k].threshold);
        Some((thr, bearing_deg(thr, opp)))
    }

    /// Intercept altitude of the approach: the altitude constraint of its final approach
    /// fix (the final is intercepted at the FAF altitude; a higher one published at the start
    /// of the final segment, such as +5000 at the EPKK VOR/DME 07 IF, is above the 8 NM
    /// intercept point), else the first altitude constraint of its final segment, else the
    /// airport's approach altitude ([`approach_alt_ft`]).
    fn intercept_alt_ft(&self, runway: &str, kind: ApproachKind) -> i32 {
        self.procedure_set
            .as_ref()
            .and_then(|set| set.approach(runway, navdata_approach_kind(kind)))
            .and_then(|p| {
                let first = |faf_only: bool| {
                    p.legs
                        .iter()
                        .filter(|l| !faf_only || l.leg.faf)
                        .find_map(|l| constraint_floor_ft(l.leg.alt))
                };
                first(true).or_else(|| first(false))
            })
            .unwrap_or_else(|| approach_alt_ft(Some(self.airport.elevation_ft)))
    }

    /// The highest altitude constraint of the missed-approach legs of the approach, if the
    /// procedure publishes any.
    pub(super) fn published_missed_ft(&self, runway: &str, kind: ApproachKind) -> Option<i32> {
        self.procedure_set
            .as_ref()
            .and_then(|set| set.approach(runway, navdata_approach_kind(kind)))
            .and_then(|p| {
                p.legs
                    .iter()
                    .filter(|l| l.leg.missed)
                    .filter_map(|l| constraint_floor_ft(l.leg.alt))
                    .max()
            })
    }

    /// The published missed-approach altitude, else the intercept altitude.
    pub(super) fn missed_alt_ft(&self, runway: &str, kind: ApproachKind) -> i32 {
        self.published_missed_ft(runway, kind)
            .unwrap_or_else(|| self.intercept_alt_ft(runway, kind))
    }

    /// Distance of the final approach fix from the threshold of `runway`: the published FAF,
    /// else 4 NM.
    fn faf_m(&self, runway: &str, kind: ApproachKind, thr: LatLon) -> f64 {
        self.procedure_set
            .as_ref()
            .and_then(|set| set.approach(runway, navdata_approach_kind(kind)))
            .and_then(|p| p.legs.iter().find(|l| l.leg.faf).and_then(|l| l.pos))
            .map_or(DEFAULT_FAF_M, |faf| distance_m(faf, thr))
    }

    /// Per sample, for an identified arrival owned by Approach (see the module docs): the
    /// transfer to Tower, the approach clearance at the IAF, or the next radar vector.
    fn arrival_update(&mut self, i: usize, s: &AircraftState) -> Vec<AtcOutput> {
        let Some(plan) = self.arrival(i).cloned() else {
            return Vec::new();
        };
        let Some((kind, runway)) = plan.approach.clone() else {
            return Vec::new();
        };
        let Some((thr, _)) = self.final_geometry(&runway) else {
            return Vec::new();
        };
        if self.strips[i].missed_approach {
            let items = self.after_missed_approach(i);
            return self.transmit(i, Role::Approach, items);
        }
        if plan.approach_cleared {
            let inside = distance_m(s.pos, thr) <= self.faf_m(&runway, kind, thr) + 50.0;
            let on_final = self.strips[i].phase
                == FlightPhase::Circuit(atc_phraseology::CircuitLeg::Final)
                && inside;
            // Vectored onto the final: watched until established (spec 14b goal 5).
            let items = match self.watched_final(i) {
                Some((f, r)) if vectoring::established(&r, &f) => {
                    // On the final inside the FAF, or reported established while turning
                    // onto it ([`AtcEngine::established_report`]).
                    if !(on_final || plan.established_reported) {
                        return Vec::new();
                    }
                    self.hand_over(i, Role::Tower)
                }
                Some((f, r)) => {
                    if !vectoring::through_course(&r, &f)
                        || !self.may_correct(i, &plan)
                        || self.turning_back(i, &plan, &f, &r)
                    {
                        return Vec::new();
                    }
                    let items = self.reintercept(i);
                    self.strips[i].expect_readback(&items, self.cfg.strictness, self.now);
                    items
                }
                None if on_final => self.hand_over(i, Role::Tower),
                None => return Vec::new(),
            };
            return self.transmit(i, Role::Approach, items);
        }
        if self.strips[i].phase != FlightPhase::Airborne || self.on_star(i, &plan, s.pos) {
            return Vec::new();
        }
        let items = if plan.vectors {
            self.vector_items(i, false)
        } else {
            let Some((fix, pos)) = self
                .procedure_set
                .as_ref()
                .and_then(|set| iaf(set, &runway, navdata_approach_kind(kind)))
            else {
                return Vec::new();
            };
            let strip = &mut self.strips[i];
            let Some(p) = strip.plan.as_mut() else {
                return Vec::new();
            };
            if distance_m(s.pos, pos) <= PASSED_FIX_M {
                p.approach_cleared = true;
                vec![
                    AtcItem::ClearedApproach { kind, runway },
                    AtcItem::ReportEstablished,
                ]
            } else if p.direct.as_deref() != Some(fix.as_str()) {
                p.direct = Some(fix.clone());
                vec![AtcItem::DirectTo { fix }]
            } else {
                Vec::new()
            }
        };
        if items.is_empty() {
            return Vec::new();
        }
        self.strips[i].expect_readback(&items, self.cfg.strictness, self.now);
        self.transmit(i, Role::Approach, items)
    }

    /// Still flying the STAR: one was given, the aircraft is not on a heading or direct, and
    /// it has not reached the STAR's last fix.
    fn on_star(&self, i: usize, plan: &FlightPlan, pos: LatLon) -> bool {
        if plan.heading.is_some() {
            return false;
        }
        self.star_path(i, plan)
            .is_some_and(|path| !passed_fix(&path, path.points.len() - 1, pos, PASSED_FIX_M))
    }

    /// Path of the STAR of `plan` to the runway of aircraft `i`.
    fn star_path(&self, i: usize, plan: &FlightPlan) -> Option<Path> {
        let ident = plan.star.as_deref()?;
        let star = self
            .procedure_set
            .as_ref()?
            .stars(&self.strips[i].runway)
            .into_iter()
            .find(|p| p.procedure.ident == ident)?;
        Some(procedure_path(star)).filter(|p| !p.points.is_empty())
    }

    /// The next radar vector for arrival `i` along its planned path (see the module docs),
    /// applied to its plan; nothing when none is due. The plan is made when vectoring starts
    /// (or anew with `force`); the heading of the leg being flown is given then, the next
    /// leg's one turn lead before its turn point ([`vectoring::judge`]), the descent when
    /// the plan needs it ([`vectoring::descent_due`]), and on the intercept leg (or the
    /// final) the approach clearance (after a "check heading" or level correction, once
    /// the heading is flown). `force`: a heading and the level now (after a missed
    /// approach). With nothing due, an aircraft off the plan may get a correction
    /// ([`AtcEngine::correction_due`]).
    fn vector_items(&mut self, i: usize, force: bool) -> Vec<AtcItem> {
        self.vectors(i, force, None)
    }

    /// A correction for arrival `i` off its planned path, for `why`: the path planned anew
    /// from where the aircraft is, the reason ([`Correction`]), the new heading (for an
    /// altitude or level correction only when it differs by
    /// [`vectoring::MIN_HEADING_CHANGE_DEG`] or more from the one given), the descent when
    /// due and, for a plain or altitude correction, the approach clearance when the new path
    /// starts on the intercept; applied to the plan, with the time of the correction.
    /// Nothing, the plan kept, when there is nothing to say.
    pub(super) fn vector_correction(&mut self, i: usize, why: Correction) -> Vec<AtcItem> {
        self.vectors(i, false, Some(why))
    }

    /// [`AtcEngine::vector_items`], or with `correction` [`AtcEngine::vector_correction`].
    fn vectors(&mut self, i: usize, force: bool, correction: Option<Correction>) -> Vec<AtcItem> {
        let Some(plan) = self.arrival(i).cloned() else {
            return Vec::new();
        };
        let Some((kind, runway)) = plan.approach.clone() else {
            return Vec::new();
        };
        let (Some(f), Some(r), Some(s)) = (
            self.final_course(&runway, kind),
            self.radar(i),
            self.tracks
                .get(&self.strips[i].callsign)
                .map(|t| t.last.clone()),
        ) else {
            return Vec::new();
        };
        let intercept_ft = f.intercept_alt_ft;
        // The leg the aircraft was off, for the reason.
        let off_leg = plan.vector_plan.as_ref().map(|vp| vp.leg().track_deg);
        let (mut vp, fresh) = match plan.vector_plan.clone() {
            Some(vp) if !force && correction.is_none() => (vp, false),
            _ => (self.new_vector_plan(&r, plan.cleared_ft, &f), true),
        };
        // After a missed approach the aircraft is still near the final: never cleared again
        // from there, it goes round a pattern for a new approach.
        if force
            && (vp.legs.first().is_none_or(|l| l.kind == LegKind::Final) || !vp.joins_in_time(&f))
        {
            let alt_ft = r.alt_ft.max(f64::from(plan.cleared_ft));
            vp = vectoring::plan_pattern(r.pos, alt_ft, &f);
        }
        // The next leg is due (several at once only when the aircraft is already past them).
        let mut next_leg = false;
        while !fresh
            && vp.index().is_some_and(|k| k + 1 < vp.legs.len())
            && vectoring::judge(&vp, &r, Some(plan.cleared_ft)) == Verdict::LegDue
        {
            vp.current += 1;
            next_leg = true;
        }
        let leg = vp.leg().clone();
        let intercept = matches!(leg.kind, LegKind::Intercept | LegKind::Final);
        // After a missed approach the published altitude is kept until the aircraft is
        // there (or the intercept is given).
        if intercept || (s.alt_ft_msl - f64::from(plan.cleared_ft)).abs() <= LEVEL_TOLERANCE_FT {
            self.strips[i].missed_climb = false;
        }
        let mut descend_to = vectoring::descent_due(&vp, &r)
            .filter(|&ft| !self.strips[i].missed_climb && ft < plan.cleared_ft);
        // Below its cleared level within 20 s of a correction (just told "check level,
        // maintain <level>"): no descent yet, it would contradict the call.
        let below = s.alt_ft_msl - f64::from(plan.cleared_ft) <= -LEVEL_TOLERANCE_FT;
        if correction.is_none()
            && below
            && plan
                .last_correction_s
                .is_some_and(|t| (0.0..vectoring::CORRECTION_INTERVAL_S).contains(&(self.now - t)))
        {
            descend_to = None;
        }
        let v = self.leg_vector(i, &leg, &s);
        let actual = self.magnetic_heading(&s);
        let changed = plan.heading.is_none_or(|h| {
            vectoring::gap_deg(f64::from(h), f64::from(v.heading_deg))
                >= vectoring::MIN_HEADING_CHANGE_DEG
        });
        // On the intercept leg with its heading given but not the clearance (after a "check
        // heading" or level correction): the clearance once the heading is flown.
        let awaiting_clearance =
            correction.is_none() && !fresh && !next_leg && intercept && !changed;
        let flown = plan.heading.is_some_and(|h| {
            vectoring::gap_deg(actual, f64::from(h)) <= vectoring::HEADING_TOLERANCE_DEG
        });
        let mut say_heading = (fresh || next_leg || intercept) && !awaiting_clearance;
        let mut clear = intercept && (!awaiting_clearance || flown);
        let mut items = Vec::new();
        // Above the cleared level and not descending to it: the descent again.
        let mut descend_again = false;
        // The monitor's level call said after the heading.
        let mut level_call = None;
        if let Some(why) = &correction {
            match why {
                Correction::Plain(call) => {
                    say_heading = true;
                    items.extend(off_leg.and_then(|t| off_course(&f, kind, r.pos, t)));
                    level_call = call.clone();
                }
                Correction::NotTurned(call) => {
                    say_heading = true;
                    clear = false;
                    items.push(AtcItem::CheckHeading(v.heading_deg));
                    level_call = call.clone();
                }
                Correction::Altitude => {
                    say_heading = intercept || changed;
                    descend_again = descend_to.is_none()
                        && s.alt_ft_msl - f64::from(plan.cleared_ft) > LEVEL_TOLERANCE_FT;
                }
                Correction::Level(item) => {
                    say_heading = changed;
                    clear = false;
                    level_call = Some(item.clone());
                }
            }
            // The level call is the only level item and no approach clearance goes with it.
            // An aircraft at or above its cleared level, where the new path needs the
            // descent now: the descent instead (it answers the bust as well, and the call
            // would contradict it). Below it: the call, and no descent with it.
            if level_call.is_some() {
                clear = false;
                if below {
                    descend_to = None;
                } else if descend_to.is_some() {
                    level_call = None;
                }
            }
        }
        let something = say_heading || clear || descend_to.is_some() || descend_again;
        if !(something || level_call.is_some() || !items.is_empty()) {
            if correction.is_none()
                && let Some(why) = self.correction_due(i, &plan, &vp, &r, &s)
            {
                return self.vectors(i, false, Some(why));
            }
            return Vec::new();
        }
        if say_heading {
            items.push(AtcItem::Heading {
                deg: v.heading_deg,
                turn: Some(v.turn),
            });
        }
        items.extend(level_call);
        let mut cleared_ft = descend_to.unwrap_or(plan.cleared_ft);
        if clear && cleared_ft > intercept_ft {
            cleared_ft = intercept_ft;
        }
        if cleared_ft != plan.cleared_ft || force || descend_again {
            // From the aircraft's present altitude: after a missed approach it may be below.
            let level = self.level(cleared_ft);
            let now = s.alt_ft_msl - f64::from(cleared_ft);
            items.push(if now > LEVEL_TOLERANCE_FT {
                AtcItem::DescendTo(level)
            } else if now < -LEVEL_TOLERANCE_FT {
                AtcItem::ClimbTo(level)
            } else {
                AtcItem::MaintainLevel(level)
            });
        }
        if clear {
            items.push(AtcItem::DistanceToGo(distance_to_go_nm(s.pos, f.thr)));
            items.push(AtcItem::ClearedApproach {
                kind,
                runway: runway.clone(),
            });
            items.push(AtcItem::ReportEstablished);
        }
        let now = self.now;
        if let Some(p) = self.strips[i].plan.as_mut() {
            note_vectors(
                p,
                say_heading.then_some(v.heading_deg),
                actual,
                vp,
                correction.is_some().then_some(now),
            );
            p.cleared_ft = cleared_ft;
            p.approach_cleared = clear;
            // Cleared, or vectored anew after a missed approach: checks noted before are
            // not made.
            if clear || force {
                p.deferred_heading = false;
                p.deferred_level = false;
                p.established_reported = false;
            }
        }
        items
    }

    /// The magnetic heading of an aircraft in state `s`, degrees in [0, 360).
    fn magnetic_heading(&self, s: &AircraftState) -> f64 {
        (s.heading_deg - self.procs.magnetic_variation_deg).rem_euclid(360.0)
    }

    /// Whether arrival `i` (plan `plan`, radar `r`, state `s`) is off its planned path `vp`
    /// and gets a correction now, and why: off the leg ([`Verdict::OffTrack`]) or above the
    /// planned altitude ([`Verdict::OffAltitude`]; below it is a level bust under the
    /// cleared level, the monitor's level check, [`AtcEngine::planned_checks`]); only when
    /// the aircraft flies its assigned heading (within
    /// [`vectoring::HEADING_TOLERANCE_DEG`] and not turning onto it: one that does not is
    /// the monitor's heading check, [`AtcEngine::planned_checks`]), not while the heading
    /// and level checks are held after a readback (14c), and at most once per
    /// [`vectoring::CORRECTION_INTERVAL_S`].
    fn correction_due(
        &self,
        i: usize,
        plan: &FlightPlan,
        vp: &VectorPlan,
        r: &Radar,
        s: &AircraftState,
    ) -> Option<Correction> {
        if !self.may_correct(i, plan) {
            return None;
        }
        let assigned = f64::from(plan.heading?);
        let actual = self.magnetic_heading(s);
        if vectoring::gap_deg(actual, assigned) > vectoring::HEADING_TOLERANCE_DEG
            || self.turning_onto(i, actual, assigned)
        {
            return None;
        }
        match vectoring::judge(vp, r, Some(plan.cleared_ft)) {
            Verdict::OffTrack { .. } => Some(Correction::Plain(None)),
            Verdict::OffAltitude { ft } if ft > 0.0 => Some(Correction::Altitude),
            _ => None,
        }
    }

    /// Aircraft `i` (magnetic heading `actual`) is turning onto `assigned` at
    /// [`TURNING_DEG_S`] or more.
    fn turning_onto(&self, i: usize, actual: f64, assigned: f64) -> bool {
        let towards = (assigned - actual + 540.0).rem_euclid(360.0) - 180.0;
        let rate = self
            .tracks
            .get(&self.strips[i].callsign)
            .map_or(0.0, |t| t.radar.turn_rate_dps());
        rate * towards.signum() >= TURNING_DEG_S
    }

    /// A correction to arrival `i` (plan `plan`) may be made now: the heading and level
    /// checks are not held after a readback (14c) and at least
    /// [`vectoring::CORRECTION_INTERVAL_S`] have run since the last correction (a sim clock
    /// gone back allows one).
    fn may_correct(&self, i: usize, plan: &FlightPlan) -> bool {
        !super::holds_clearance_checks(&self.strips[i], self.now)
            && plan
                .last_correction_s
                .is_none_or(|t| !(0.0..vectoring::CORRECTION_INTERVAL_S).contains(&(self.now - t)))
    }

    /// Spec 14b, decision 2: the monitor's heading and level checks of an arrival on planned
    /// vectors (the path planned, not cleared yet), answered by the planner. A check that
    /// fires is noted on the plan and made when a correction may be ([`Self::may_correct`]),
    /// if the aircraft is still off then: more than [`vectoring::HEADING_TOLERANCE_DEG`]
    /// off the assigned heading and not turning onto it (still on the heading it had when
    /// told: [`Correction::NotTurned`], else [`Correction::Plain`]), more than 200 ft off
    /// the cleared level ([`Correction::Level`] with the monitor's wording,
    /// [`AtcEngine::level_call`], or appended to a heading correction). `None`: not a
    /// planned flight (the monitor's calls are made as usual); else the items to say (maybe
    /// none), their readback expected.
    pub(super) fn planned_checks(
        &mut self,
        i: usize,
        heading_off: bool,
        level_off: bool,
    ) -> Option<Vec<AtcItem>> {
        let plan = self
            .arrival(i)
            .filter(|p| p.vector_plan.is_some() && !p.approach_cleared)?
            .clone();
        let deferred_heading = plan.deferred_heading || heading_off;
        let deferred_level = plan.deferred_level || level_off;
        if let Some(p) = self.strips[i].plan.as_mut() {
            p.deferred_heading = deferred_heading;
            p.deferred_level = deferred_level;
        }
        if !(deferred_heading || deferred_level) || !self.may_correct(i, &plan) {
            return Some(Vec::new());
        }
        let s = self.tracks.get(&self.strips[i].callsign)?.last.clone();
        let actual = self.magnetic_heading(&s);
        let heading = plan.heading.filter(|&h| {
            deferred_heading
                && vectoring::gap_deg(actual, f64::from(h)) > vectoring::HEADING_TOLERANCE_DEG
                && !self.turning_onto(i, actual, f64::from(h))
        });
        let level = deferred_level
            && (s.alt_ft_msl - f64::from(plan.cleared_ft)).abs() > LEVEL_TOLERANCE_FT;
        let call = level.then(|| self.level_call(plan.cleared_ft, s.alt_ft_msl.round() as i32));
        let items = match (heading, call.clone()) {
            (Some(_), _) => {
                let not_turned = plan.heading_from.is_some_and(|from| {
                    vectoring::gap_deg(actual, f64::from(from)) <= vectoring::MIN_HEADING_CHANGE_DEG
                });
                self.vector_correction(
                    i,
                    if not_turned {
                        Correction::NotTurned(call)
                    } else {
                        Correction::Plain(call)
                    },
                )
            }
            (None, Some(call)) => self.vector_correction(i, Correction::Level(call)),
            (None, None) => Vec::new(),
        };
        // Said, or nothing left to say: done; else (nothing could be planned) still due.
        if (!items.is_empty() || (heading.is_none() && !level))
            && let Some(p) = self.strips[i].plan.as_mut()
        {
            p.deferred_heading = false;
            p.deferred_level = false;
        }
        // A level call alone needs no readback and leaves an open one as it is.
        if crate::strip::steers(&items) {
            self.strips[i].expect_readback(&items, self.cfg.strictness, self.now);
        }
        Some(items)
    }

    /// Spec 14b goal 5, decision 3: arrival `i` cleared for the approach after Approach
    /// vectored it onto the final (its planned path kept; a full procedure from the IAF has
    /// none) is watched until established. Its final approach course and what the radar
    /// shows, with the track taken as the heading plus the drift last measured (the track
    /// from positions lags a turn by up to [`vectoring::DRIFT_WINDOW_S`]); `None` for a
    /// flight not watched.
    fn watched_final(&self, i: usize) -> Option<(FinalCourse, Radar)> {
        let plan = self
            .arrival(i)
            .filter(|p| p.approach_cleared && p.vector_plan.is_some())?;
        let (kind, runway) = plan.approach.clone()?;
        let f = self.final_course(&runway, kind)?;
        let t = self.tracks.get(&self.strips[i].callsign)?;
        let mut r = self.radar(i)?;
        r.track_deg = (t.last.heading_deg + t.radar.drift_deg()).rem_euclid(360.0);
        Some((f, r))
    }

    /// Decision 1: "established" reported by arrival `i`, cleared for the approach. Tower
    /// takes it when the radar agrees ([`vectoring::established`]), or when its final is not
    /// watched ([`AtcEngine::watched_final`]); otherwise "negative" and the correction
    /// ([`AtcEngine::reintercept`], or the break-off), the flight kept. Already turning onto
    /// the course ([`AtcEngine::turning_back`]): no reply now (a heading would undo the
    /// turn); the report is noted and Tower takes the flight as soon as the radar shows it
    /// established ([`AtcEngine::arrival_update`]).
    fn established_report(&mut self, i: usize) -> Vec<AtcItem> {
        let plan = self.arrival(i).cloned();
        if let (Some((f, r)), Some(plan)) = (self.watched_final(i), plan)
            && !vectoring::established(&r, &f)
        {
            if self.turning_back(i, &plan, &f, &r) {
                if let Some(p) = self.strips[i].plan.as_mut() {
                    p.established_reported = true;
                }
                return Vec::new();
            }
            let correction = self.reintercept(i);
            if !correction.is_empty() {
                let mut items = vec![AtcItem::Negative];
                items.extend(correction);
                self.strips[i].expect_readback(&items, self.cfg.strictness, self.now);
                return items;
            }
        }
        self.hand_over(i, Role::Tower)
    }

    /// Arrival `i` (plan `plan`, radar `r`) is turning the right way, not corrected: at
    /// [`TURNING_DEG_S`] or more onto its assigned heading where that heading takes it back
    /// towards the final `f` from the side it is on (the intercept or re-intercept heading),
    /// or, converging on the course, turning to align with it (the localizer capture's turn,
    /// or the turn of an aircraft that has already turned back past the course direction on
    /// its own). An aircraft moving away from the course and merely turning towards parallel
    /// is not exempt.
    fn turning_back(&self, i: usize, plan: &FlightPlan, f: &FinalCourse, r: &Radar) -> bool {
        let Some(t) = self.tracks.get(&self.strips[i].callsign) else {
            return false;
        };
        let (_, right) = RunwayFrame {
            origin: f.thr,
            course_deg: f.course_deg,
        }
        .to_local(r.pos);
        let onto_assigned = plan.heading.is_some_and(|h| {
            let assigned = f64::from(h);
            let rightward = (assigned + self.procs.magnetic_variation_deg - f.course_deg)
                .to_radians()
                .sin();
            right * rightward < 0.0
                && self.turning_onto(i, self.magnetic_heading(&t.last), assigned)
        });
        // Converging: its rate to the right of the course has the other sign than its side.
        let off = (r.track_deg - f.course_deg + 540.0).rem_euclid(360.0) - 180.0;
        let converging = right * off.to_radians().sin() < 0.0;
        let rate = t.radar.turn_rate_dps();
        let aligning = converging && rate.abs() >= TURNING_DEG_S && rate * off < 0.0;
        onto_assigned || aligning
    }

    /// The correction of arrival `i`, cleared for the approach and off its final (watched,
    /// [`AtcEngine::watched_final`]): the re-intercept from where it is
    /// ([`vectoring::plan_reintercept`], at most [`vectoring::REINTERCEPT_MAX_DEG`] onto the
    /// course): "you are <tenths> <side> of the <course>", the heading ("fly heading" when
    /// it is the one flown) "to re-intercept the <course>"; the approach clearance kept, and
    /// no "check established" for it after the correction. When it cannot join
    /// [`vectoring::JOIN_OUTSIDE_FAF_NM`] outside the FAF, the break-off instead
    /// ([`AtcEngine::break_off`]). Applied to the plan, with the time of the correction; the
    /// caller expects the readback.
    fn reintercept(&mut self, i: usize) -> Vec<AtcItem> {
        let Some((kind, _)) = self.arrival(i).and_then(|p| p.approach.clone()) else {
            return Vec::new();
        };
        let (Some((f, r)), Some(s)) = (
            self.watched_final(i),
            self.tracks
                .get(&self.strips[i].callsign)
                .map(|t| t.last.clone()),
        ) else {
            return Vec::new();
        };
        let vp = vectoring::plan_reintercept(r.pos, r.alt_ft, &f);
        if !vp.joins_in_time(&f) {
            return self.break_off(i, &f, &r, &s);
        }
        let v = self.leg_vector(i, vp.leg(), &s);
        let actual = self.magnetic_heading(&s);
        let turn = (vectoring::gap_deg(actual, f64::from(v.heading_deg)) >= SAME_HEADING_DEG)
            .then_some(v.turn);
        let mut items: Vec<AtcItem> = off_course(&f, kind, r.pos, f.course_deg)
            .into_iter()
            .collect();
        items.push(AtcItem::Heading {
            deg: v.heading_deg,
            turn,
        });
        items.push(AtcItem::ReIntercept(course_name(kind)));
        let now = self.now;
        if let Some(p) = self.strips[i].plan.as_mut() {
            note_vectors(p, Some(v.heading_deg), actual, vp, Some(now));
            p.established_check = true;
        }
        items
    }

    /// Decision 5: arrival `i` (state `s`, radar `r`) taken off the approach to the final `f`
    /// for a new one: the pattern from where it is ([`vectoring::plan_pattern`]), from the
    /// higher of its altitude (rounded to 100 ft) and the intercept altitude; the turn onto
    /// its first leg away from the final, towards the side the pattern is flown on (never
    /// through the heading to the other side), "maintain" (or "climb") that altitude, "I will
    /// vector you for a new approach". The approach clearance is cancelled on the strip; not
    /// a missed approach (Tower does not own the flight).
    fn break_off(
        &mut self,
        i: usize,
        f: &FinalCourse,
        r: &Radar,
        s: &AircraftState,
    ) -> Vec<AtcItem> {
        let alt_ft = (((r.alt_ft / 100.0).round() as i32) * 100).max(f.intercept_alt_ft);
        let vp = vectoring::plan_pattern(r.pos, f64::from(alt_ft), f);
        let mut v = self.leg_vector(i, vp.leg(), s);
        let frame = RunwayFrame {
            origin: f.thr,
            course_deg: f.course_deg,
        };
        let side = frame.to_local(vp.leg().to).1.signum();
        // The heading (true) towards the side away from the pattern's: not turned through.
        let other = f.course_deg - side * 90.0;
        let target = f64::from(v.heading_deg) + self.procs.magnetic_variation_deg;
        let signed = |d: f64| (d + 540.0).rem_euclid(360.0) - 180.0;
        let (delta, through) = (
            signed(target - s.heading_deg),
            signed(other - s.heading_deg),
        );
        if through != 0.0 && through.signum() == delta.signum() && through.abs() < delta.abs() {
            v.turn = match v.turn {
                Turn::Left => Turn::Right,
                Turn::Right => Turn::Left,
            };
        }
        let level = self.level(alt_ft);
        let items = vec![
            AtcItem::Heading {
                deg: v.heading_deg,
                turn: Some(v.turn),
            },
            if r.alt_ft - f64::from(alt_ft) < -LEVEL_TOLERANCE_FT {
                AtcItem::ClimbTo(level)
            } else {
                AtcItem::MaintainLevel(level)
            },
            AtcItem::NewApproach,
        ];
        let (now, actual) = (self.now, self.magnetic_heading(s));
        if let Some(p) = self.strips[i].plan.as_mut() {
            note_vectors(p, Some(v.heading_deg), actual, vp, Some(now));
            p.approach_cleared = false;
            p.established_check = false;
            p.established_reported = false;
            p.cleared_ft = alt_ft;
            p.deferred_heading = false;
            p.deferred_level = false;
        }
        items
    }

    /// The final approach course radar vectors lead to on `runway` for approach `kind`: the
    /// runway's course from its threshold for an ILS (the localizer); for another approach
    /// the published final, the line from its FAF to its missed-approach point
    /// ([`AtcEngine::published_final`]), else the runway's course.
    fn final_course(&self, runway: &str, kind: ApproachKind) -> Option<FinalCourse> {
        let (thr, course_deg) = self.final_geometry(runway)?;
        let (origin, course_deg, faf_m) = self.published_final(runway, kind, thr).unwrap_or((
            thr,
            course_deg,
            self.faf_m(runway, kind, thr),
        ));
        Some(FinalCourse {
            thr: origin,
            course_deg,
            faf_m,
            intercept_alt_ft: self.intercept_alt_ft(runway, kind),
            circuit_side: self.circuit_side(runway),
        })
    }

    /// The final of a non-ILS approach of `kind` to `runway` (threshold `thr`) as published,
    /// when its course differs from the runway's (an offset VOR/DME final): the point of the
    /// line from the FAF to the missed-approach point abeam the threshold, the course of
    /// that line (true) and the FAF's distance from that point. `None` for an ILS, or without
    /// the FAF and the missed-approach point, or when the line is more than
    /// [`STRAIGHT_IN_MAX_DEG`] off the runway's course (not a straight-in final: data that
    /// does not fit).
    fn published_final(
        &self,
        runway: &str,
        kind: ApproachKind,
        thr: LatLon,
    ) -> Option<(LatLon, f64, f64)> {
        if kind == ApproachKind::Ils {
            return None;
        }
        let p = self
            .procedure_set
            .as_ref()?
            .approach(runway, navdata_approach_kind(kind))?;
        let faf = p.legs.iter().find(|l| l.leg.faf).and_then(|l| l.pos)?;
        let map = p.legs.iter().find(|l| l.leg.map).and_then(|l| l.pos)?;
        let course_deg = bearing_deg(faf, map);
        let (_, runway_deg) = self.final_geometry(runway)?;
        if vectoring::gap_deg(course_deg, runway_deg) > STRAIGHT_IN_MAX_DEG {
            return None;
        }
        let (along, _) = RunwayFrame {
            origin: map,
            course_deg,
        }
        .to_local(thr);
        let origin = atc_geo::destination(
            map,
            course_deg + if along < 0.0 { 180.0 } else { 0.0 },
            along.abs(),
        );
        Some((origin, course_deg, distance_m(faf, origin)))
    }

    /// The planned path from radar `r` to the final `f`, for an aircraft cleared to
    /// `cleared_ft`: planned from the higher of its altitude and that level (climbing to
    /// it, it will be there). A plan that does not join in time is still flown.
    fn new_vector_plan(&self, r: &Radar, cleared_ft: i32, f: &FinalCourse) -> VectorPlan {
        let alt_ft = r.alt_ft.max(f64::from(cleared_ft));
        vectoring::plan_path(r.pos, r.track_deg, alt_ft, f)
    }

    /// The vector that makes good the ground track of `leg` for aircraft `i` (state `s`):
    /// the track corrected for the drift the radar shows, magnetic, rounded to 5 degrees.
    fn leg_vector(&self, i: usize, leg: &VectorLeg, s: &AircraftState) -> Vector {
        let drift = self
            .tracks
            .get(&self.strips[i].callsign)
            .map_or(0.0, |t| t.radar.drift_deg());
        vector_on(
            heading_for_track(leg.track_deg, drift),
            s.heading_deg,
            leg.alt_ft,
            self.procs.magnetic_variation_deg,
        )
    }

    /// The side of the final the circuit of `runway` is on: +1 right, -1 left (of the
    /// landing direction); left without a circuit.
    fn circuit_side(&self, runway: &str) -> f64 {
        match self.procs.circuits.iter().find(|c| c.runway == runway) {
            Some(c) if c.side == CircuitSide::Right => 1.0,
            _ => -1.0,
        }
    }

    /// Spec 3.5, approach not established: an arrival cleared for the approach still off the
    /// final (not `Circuit(Final)`) between its FAF and the threshold is told once per
    /// clearance, in English, by the station owning it: "check established".
    pub(super) fn establish_check(&mut self, i: usize) -> Vec<AtcOutput> {
        let strip = &self.strips[i];
        if strip.radio_failure
            || strip.handed_off.is_some()
            || strip.phase != FlightPhase::Airborne
            || !matches!(strip.owner, Role::Approach | Role::Tower)
        {
            return Vec::new();
        }
        let Some(plan) = self
            .arrival(i)
            .filter(|p| p.approach_cleared && !p.established_check)
        else {
            return Vec::new();
        };
        let Some((kind, runway)) = plan.approach.clone() else {
            return Vec::new();
        };
        let (Some((thr, course)), Some(pos)) = (self.final_geometry(&runway), self.last_pos(i))
        else {
            return Vec::new();
        };
        let (along, _) = RunwayFrame {
            origin: thr,
            course_deg: course,
        }
        .to_local(pos);
        if along > 0.0 || -along > self.faf_m(&runway, kind, thr) {
            return Vec::new();
        }
        let owner = self.strips[i].owner;
        if let Some(p) = self.strips[i].plan.as_mut() {
            p.established_check = true;
        }
        self.transmit_in(
            i,
            owner,
            vec![AtcItem::CheckEstablished { kind, runway }],
            Lang::En,
        )
    }

    /// Aircraft `i`, an IFR arrival, has gone around or missed the approach: the landing
    /// clearance and the runway are given up and the flight is set up to be vectored again
    /// from the published missed-approach altitude. Owned by Tower, it goes back to Approach
    /// ([`AtcEngine::handover`]) and is identified again when it calls; owned by Approach,
    /// it is vectored at the next sample.
    pub(super) fn set_missed_approach(&mut self, i: usize) {
        let Some((kind, runway)) = self.arrival(i).and_then(|p| p.approach.clone()) else {
            return;
        };
        let missed_ft = self.missed_alt_ft(&runway, kind);
        let strip = &mut self.strips[i];
        strip.missed_approach = true;
        strip.missed_climb = true;
        // Checks noted on the approach just missed are not made after it.
        if let Some(p) = strip.plan.as_mut() {
            p.deferred_heading = false;
            p.deferred_level = false;
        }
        // Back to Approach, to be identified again; a Tower-kept arrival stays identified.
        if (strip.owner == Role::Tower || strip.handoff_to == Some(Role::Tower))
            && !strip.tower_inbound
        {
            strip.identified = false;
        }
        strip.permissions.may_land = false;
        self.runway.release(&strip.callsign);
        let landing = strip.pending_items.iter().any(|it| {
            matches!(
                it,
                AtcItem::ClearedLand { .. } | AtcItem::ClearedTouchAndGo { .. }
            )
        });
        if landing {
            strip.readback_complete_at(self.now);
        }
        if let Some(p) = strip.plan.as_mut() {
            p.approach_cleared = false;
            p.established_check = false;
            p.established_reported = false;
            p.heading = None;
            p.direct = None;
            p.star = None;
            p.vector_plan = None;
            p.vectors = true;
            p.cleared_ft = missed_ft;
        }
    }

    /// Per sample: an IFR arrival that leaves the final airborne (`prev` -> `phase`, at
    /// `pos`, climbing at `vs_fpm`) after its approach clearance or Tower's transfer has
    /// missed the approach when it is within 2 NM of the threshold or climbing.
    pub(super) fn note_missed_approach(
        &mut self,
        i: usize,
        prev: FlightPhase,
        phase: FlightPhase,
        pos: LatLon,
        vs_fpm: f64,
    ) {
        let strip = &self.strips[i];
        if prev != FlightPhase::Circuit(atc_phraseology::CircuitLeg::Final)
            || phase != FlightPhase::Airborne
            || strip.missed_approach
        {
            return;
        }
        let on_tower = strip.owner == Role::Tower || strip.handoff_to == Some(Role::Tower);
        let Some(plan) = self.arrival(i).filter(|p| p.approach_cleared || on_tower) else {
            return;
        };
        let Some((_, runway)) = plan.approach.clone() else {
            return;
        };
        let near = self
            .final_geometry(&runway)
            .is_some_and(|(thr, _)| distance_m(thr, pos) <= MISSED_APPROACH_M);
        if near || vs_fpm > MISSED_CLIMB_FPM {
            self.set_missed_approach(i);
        }
    }

    /// "Identified", and the climb to the cruise level when the aircraft was cleared lower;
    /// only for an airborne IFR flight not yet handed off. An airborne flight without a plan:
    /// "no flight plan, say intentions".
    fn identify_departure(&mut self, i: usize) -> Vec<AtcItem> {
        let strip = &self.strips[i];
        if strip.handed_off.is_some() {
            return Vec::new();
        }
        // Without a plan the phase tracker follows the circuit legs: any airborne phase.
        if strip.plan.is_none() && strip.phase.is_airborne() {
            return vec![AtcItem::NoFlightPlan, AtcItem::SayIntentions];
        }
        if strip.phase != FlightPhase::Airborne {
            return Vec::new();
        }
        let Some(plan) = strip.plan.clone().filter(|p| p.ifr) else {
            return Vec::new();
        };
        let mut items = vec![AtcItem::Identified];
        let climb = plan.cleared_ft < plan.cruise_ft;
        if climb {
            items.push(AtcItem::ClimbTo(self.level(plan.cruise_ft)));
        }
        let strip = &mut self.strips[i];
        strip.identified = true;
        if climb && let Some(p) = strip.plan.as_mut() {
            p.cleared_ft = p.cruise_ft;
        }
        strip.expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// The IFR clearance in the air of airborne aircraft `i` without a flight plan, to be read
    /// back: identified, cleared to `destination` (ICAO, as heard), the climb to the cruise
    /// level (`level_ft`, else the IFR default) and a squawk; no SID or route. Unknown
    /// destination: "unable, say again destination"; no level: "unable, IFR not available".
    fn airborne_clearance(
        &mut self,
        i: usize,
        destination: Option<&str>,
        level_ft: Option<i32>,
    ) -> Vec<AtcItem> {
        let Some(dest) = self.destination(destination) else {
            return vec![AtcItem::Unable(UnableReason::UnknownDestination)];
        };
        let Some(cruise_ft) = level_ft.or(self.procs.ifr.map(|d| d.cruise_ft)) else {
            return vec![AtcItem::Unable(UnableReason::NoIfrData)];
        };
        let mut items = vec![
            AtcItem::Identified,
            cleared_to(&dest),
            AtcItem::ClimbTo(self.level(cruise_ft)),
        ];
        self.clearance_squawk(i, &mut items);
        let strip = &mut self.strips[i];
        strip.plan = Some(FlightPlan {
            ifr: true,
            destination: dest.icao.clone(),
            dest_name: dest.name.clone(),
            cruise_ft,
            sid: None,
            dep_runway: None,
            star: None,
            approach: None,
            vectors: false,
            cleared_ft: cruise_ft,
            direct: None,
            heading: None,
            approach_cleared: false,
            vector_plan: None,
            last_correction_s: None,
            heading_from: None,
            deferred_heading: false,
            deferred_level: false,
            established_check: false,
            established_reported: false,
        });
        strip.identified = true;
        strip.expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// Per sample, for an identified IFR flight owned by Approach, not handed off and not on
    /// radio failure: an arrival's vectors and clearances ([`AtcEngine::arrival_update`]); a
    /// departure's descent before the destination's tower, the hand-off, else the direct
    /// routing after the SID's first turn.
    pub(super) fn approach_update(&mut self, i: usize) -> Vec<AtcOutput> {
        let strip = &self.strips[i];
        if strip.owner != Role::Approach
            || !strip.identified
            || strip.handed_off.is_some()
            || strip.pending_readback.is_some()
            || strip.handoff_to.is_some()
            || strip.radio_failure
            || !strip.phase.is_airborne()
        {
            return Vec::new();
        }
        let Some(plan) = strip.plan.clone().filter(|p| p.ifr) else {
            return Vec::new();
        };
        let Some(s) = self.tracks.get(&strip.callsign).map(|t| t.last.clone()) else {
            return Vec::new();
        };
        if strip.radar_contact_kept && plan.approach.is_none() {
            return self.radar_contact_arrival(i);
        }
        if plan.approach.is_some() {
            return self.arrival_update(i, &s);
        }
        if strip.phase != FlightPhase::Airborne {
            return Vec::new();
        }
        if let Some((level_ft, items)) = self.descent_before_tower(&plan, s.pos) {
            let strip = &mut self.strips[i];
            if let Some(p) = strip.plan.as_mut() {
                p.cleared_ft = level_ft;
            }
            strip.expect_readback(&items, self.cfg.strictness, self.now);
            return self.transmit(i, Role::Approach, items);
        }
        if let Some(HandOff {
            unit,
            airport,
            items,
        }) = self.hand_off(&plan, s.pos)
        {
            let silent = self.same_unit_and_frequency(i, &items);
            let strip = &mut self.strips[i];
            // An empty unit: released, no unit to contact (see `Strip::handed_off`).
            strip.handed_off = Some(unit);
            strip.handoff_acked = false;
            strip.handed_off_khz = items.iter().find_map(|it| match it {
                AtcItem::Contact { khz, .. } => Some(*khz),
                _ => None,
            });
            strip.handed_off_airport = airport;
            if silent {
                // Already on the unit's frequency: nothing to say or read back, radar
                // contact carries over to the station taking the flight.
                strip.handoff_acked = true;
                strip.radar_contact_kept = true;
                return Vec::new();
            }
            strip.expect_readback(&items, self.cfg.strictness, self.now);
            return self.transmit(i, Role::Approach, items);
        }
        if plan.direct.is_none()
            && let Some(fix) = self.direct_after_turn(i, &plan, &s)
        {
            let items = vec![AtcItem::DirectTo { fix: fix.clone() }];
            let strip = &mut self.strips[i];
            if let Some(p) = strip.plan.as_mut() {
                p.direct = Some(fix);
            }
            strip.expect_readback(&items, self.cfg.strictness, self.now);
            return self.transmit(i, Role::Approach, items);
        }
        Vec::new()
    }

    /// The hand-off `items` only name the unit aircraft `i` is already working: a contact to
    /// this Approach's callsign on the frequency the aircraft is on (another station's
    /// Approach position of the same unit, such as EPKK and EPKT Approach).
    fn same_unit_and_frequency(&self, i: usize, items: &[AtcItem]) -> bool {
        let Some(pos) = self.position(Role::Approach) else {
            return false;
        };
        let on = self.strips[i].approach_khz.unwrap_or(pos.khz);
        matches!(items, [AtcItem::Contact { station, khz }]
            if *station == pos.callsign && atc_proto::same_channel(*khz, on))
    }

    /// The descent due at `pos` before the hand-off to the destination's tower: 20 NM out,
    /// to the destination's approach altitude ([`approach_alt_ft`]) with the QNH, once.
    /// None when the destination has an Approach of its own: that Approach descends it.
    fn descent_before_tower(&self, plan: &FlightPlan, pos: LatLon) -> Option<(i32, Vec<AtcItem>)> {
        if self.destination_approach(plan).is_some() {
            return None;
        }
        self.destination_tower(plan)?;
        let dest = self
            .destinations
            .iter()
            .find(|d| d.icao.eq_ignore_ascii_case(&plan.destination))?;
        if distance_m(pos, dest.pos) > DEST_DESCENT_M {
            return None;
        }
        let alt = approach_alt_ft(dest.elevation_ft);
        if plan.cleared_ft <= alt {
            return None;
        }
        let mut items = vec![AtcItem::DescendTo(self.level(alt))];
        if let Some(atis) = &self.atis {
            items.push(AtcItem::Qnh(atis.current().weather.qnh_hpa));
        }
        Some((alt, items))
    }

    /// The hand-off due at `pos`: to the destination's Approach (another loaded station)
    /// within 30 NM of the destination, else to its tower within 10 NM; with neither, to the
    /// ACC or FIS (or a release) on leaving the TMA. Nothing farther out from a neighbouring
    /// destination with an Approach or a tower; a tower or Approach beyond the neighbour
    /// radius counts as neither.
    fn hand_off(&self, plan: &FlightPlan, pos: LatLon) -> Option<HandOff> {
        let contact = |u: &AdjacentUnit| AtcItem::Contact {
            station: u.callsign.clone(),
            khz: u.khz,
        };
        let to_unit = |u: &AdjacentUnit| HandOff {
            unit: u.callsign.clone(),
            airport: u.airport.clone(),
            items: vec![contact(u)],
        };
        if let Some(app) = self.destination_approach(plan) {
            let dest = self.destination_pos(plan)?;
            return (distance_m(pos, dest) <= DEST_APPROACH_M).then(|| to_unit(app));
        }
        if let Some(tower) = self.destination_tower(plan) {
            let dest = self.destination_pos(plan)?;
            return (distance_m(pos, dest) <= DEST_TOWER_M).then(|| to_unit(tower));
        }
        let tma = self.procs.tma?;
        if distance_m(self.airport_ref()?, pos) < tma.radius_nm * M_PER_NM {
            return None;
        }
        let units = &self.procs.adjacent;
        let acc = units.iter().find(|u| {
            u.kind == AdjacentKind::Acc && u.min_level_ft.is_none_or(|m| plan.cleared_ft >= m)
        });
        if let Some(acc) = acc {
            return Some(HandOff {
                unit: acc.callsign.clone(),
                airport: None,
                items: vec![contact(acc)],
            });
        }
        // Neither unit: released, no unit to contact.
        let Some(fis) = units.iter().find(|u| u.kind == AdjacentKind::Fis) else {
            return Some(HandOff {
                unit: String::new(),
                airport: None,
                items: vec![
                    AtcItem::RadarServiceTerminated,
                    AtcItem::FrequencyChangeApproved,
                ],
            });
        };
        Some(HandOff {
            unit: fis.callsign.clone(),
            airport: None,
            items: vec![AtcItem::RadarServiceTerminated, contact(fis)],
        })
    }

    /// The Approach of the flight's destination, when another loaded station
    /// ([`AtcEngine::set_station_units`]) has one and the destination is a neighbour: within
    /// [`NEIGHBOUR_DEST_M`] of this station's airport.
    fn destination_approach(&self, plan: &FlightPlan) -> Option<&AdjacentUnit> {
        self.neighbour_destination(plan)?;
        self.station_units.iter().find(|u| {
            u.kind == AdjacentKind::Approach
                && u.airport
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(&plan.destination))
        })
    }

    /// The destination's reference point when it is a neighbour: within
    /// [`NEIGHBOUR_DEST_M`] of this station's airport.
    fn neighbour_destination(&self, plan: &FlightPlan) -> Option<LatLon> {
        let dest = self.destination_pos(plan)?;
        (distance_m(self.airport_ref()?, dest) <= NEIGHBOUR_DEST_M).then_some(dest)
    }

    /// The adjacent tower of the flight's destination, when the destination is a neighbour
    /// ([`NEIGHBOUR_DEST_M`]); a tower farther away is not worked.
    fn destination_tower(&self, plan: &FlightPlan) -> Option<&AdjacentUnit> {
        self.neighbour_destination(plan)?;
        self.procs.adjacent.iter().find(|u| {
            u.kind == AdjacentKind::Tower
                && u.airport
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(&plan.destination))
        })
    }

    fn destination_pos(&self, plan: &FlightPlan) -> Option<LatLon> {
        self.destinations
            .iter()
            .find(|d| d.icao.eq_ignore_ascii_case(&plan.destination))
            .map(|d| d.pos)
    }

    /// Where to clear the aircraft direct to, once it has passed the SID's second fix and is
    /// still climbing: the destination when it is within the TMA or has an adjacent tower or
    /// an Approach of another loaded station, otherwise the SID's last fix.
    fn direct_after_turn(&self, i: usize, plan: &FlightPlan, s: &AircraftState) -> Option<String> {
        let path = self.sid_path(i)?;
        let climbing =
            s.vs_fpm > CLIMBING_FPM || s.alt_ft_msl < f64::from(plan.cleared_ft) - BELOW_LEVEL_FT;
        if !climbing || !passed_fix(&path, 1, s.pos, PASSED_FIX_M) {
            return None;
        }
        let near = match (
            self.procs.tma,
            self.destination_pos(plan),
            self.airport_ref(),
        ) {
            (Some(tma), Some(dest), Some(home)) => {
                distance_m(home, dest) <= tma.radius_nm * M_PER_NM
            }
            _ => false,
        };
        if near
            || self.destination_tower(plan).is_some()
            || self.destination_approach(plan).is_some()
        {
            Some(plan.dest_name.clone())
        } else {
            path.points.last().map(|(fix, _)| fix.clone())
        }
    }

    /// Path of the SID aircraft `i` was cleared, from its runway.
    fn sid_path(&self, i: usize) -> Option<Path> {
        let strip = &self.strips[i];
        let ident = strip.plan.as_ref()?.sid.as_deref()?;
        let set = self.procedure_set.as_ref()?;
        let sid = set
            .sids(&strip.runway)
            .into_iter()
            .find(|p| p.procedure.ident == ident)?;
        Some(procedure_path(sid))
    }

    /// What Approach monitors for aircraft `i`: while it is identified and on its SID (not
    /// cleared direct, the SID's last fix not passed yet) the SID track, flown from the end of
    /// the departure runway; always the cleared level. Nothing for other aircraft.
    pub(super) fn ifr_clearance(&self, i: usize) -> IfrClearance {
        let strip = &self.strips[i];
        let Some(plan) = strip.plan.as_ref().filter(|p| p.ifr) else {
            return IfrClearance::default();
        };
        if strip.owner != Role::Approach || !strip.identified || strip.handed_off.is_some() {
            return IfrClearance::default();
        }
        if plan.approach.is_some() {
            // An arrival: the STAR track while on it, the assigned heading, the cleared level;
            // nothing once cleared for the approach (it intercepts and descends on it).
            if plan.approach_cleared {
                return IfrClearance::default();
            }
            // Flying to the STAR's start fix is not off its track: monitored once joined.
            let procedure = self
                .last_pos(i)
                .filter(|&pos| self.on_star(i, plan, pos) && plan.direct.is_none())
                .and_then(|pos| {
                    let path = self.star_path(i, plan)?;
                    joined_path(&path, pos, PASSED_FIX_M).then_some(())?;
                    Some((plan.star.clone()?, path))
                });
            return IfrClearance {
                procedure,
                heading_deg: plan.heading,
                level_ft: (plan.cleared_ft > 0).then_some(plan.cleared_ft),
                heading_tol_deg: plan
                    .vector_plan
                    .is_some()
                    .then_some(vectoring::HEADING_TOLERANCE_DEG),
            };
        }
        let procedure = self
            .sid_path(i)
            .filter(|_| plan.direct.is_none())
            .filter(|path| {
                let last = path.points.len().saturating_sub(1);
                self.last_pos(i)
                    .is_none_or(|pos| !passed_fix(path, last, pos, PASSED_FIX_M))
            })
            .map(|mut path| {
                if let Some(end) = self.departure_end(&strip.runway) {
                    path.points.insert(0, ("runway end".into(), end));
                    for (k, _) in &mut path.constraints {
                        *k += 1;
                    }
                }
                (plan.sid.clone().unwrap_or_default(), path)
            });
        IfrClearance {
            procedure,
            heading_deg: None,
            level_ft: (plan.cleared_ft > 0).then_some(plan.cleared_ft),
            heading_tol_deg: None,
        }
    }

    /// The far end of `runway`: where a departure from it leaves the runway.
    fn departure_end(&self, runway: &str) -> Option<LatLon> {
        let rw = self
            .airport
            .runways
            .iter()
            .find(|r| r.ends.iter().any(|e| e.ident == runway))?;
        let k = rw.ends.iter().position(|e| e.ident == runway)?;
        Some(rw.ends[1 - k].threshold)
    }

    /// A transmission on the frequency of an adjacent unit: "identified" from it to the
    /// flight handed to it, once; an inbound IFR call to an area or information unit is
    /// sent to Approach ([`AtcEngine::inbound_to_approach`]). Anything else on the
    /// frequency stays unanswered.
    pub(super) fn adjacent_call(
        &mut self,
        freq_khz: u32,
        text: &str,
        lang: Lang,
    ) -> Vec<AtcOutput> {
        let Some(unit) = self
            .procs
            .adjacent
            .iter()
            .find(|u| atc_proto::same_channel(u.khz, freq_khz))
            .cloned()
        else {
            return Vec::new();
        };
        let (msg, Some(i), _) = self.hear(text, lang) else {
            return Vec::new();
        };
        self.strips[i].heard += 1;
        // Spec 3.6: an IFR inbound call from a flight that went away (handed off, or still on
        // its departure plan) begins a new flight, which the unit sends to Approach.
        if is_ifr_inbound(&unit, &msg)
            && self.strips[i].phase.is_airborne()
            && self.flight_went_away(i)
        {
            self.strips[i].end_flight();
        }
        let items = if self.strips[i].handed_off.as_deref() == Some(unit.callsign.as_str()) {
            let strip = &mut self.strips[i];
            if strip.adjacent_identified {
                return Vec::new();
            }
            strip.adjacent_identified = true;
            strip.handoff_acked = true;
            // Calling the unit completes the hand-off, read back or not.
            if strip.pending_readback.is_some() {
                strip.readback_complete_at(self.now);
            }
            vec![AtcItem::Identified]
        } else {
            match self.inbound_to_approach(i, &unit, &msg) {
                Some(contact) => vec![contact],
                None => return Vec::new(),
            }
        };
        let strip = &mut self.strips[i];
        strip.issued.extend(items.iter().cloned());
        let message = AtcMessage {
            to: Some(strip.callsign.clone()),
            from: unit.callsign.clone(),
            items,
            lang: Lang::En,
        };
        let text = render(&message, true);
        self.last_told.insert(strip.callsign.clone(), text.clone());
        vec![AtcOutput {
            freq_khz: unit.khz,
            station: unit.callsign,
            message,
            text,
            reminder: None,
        }]
    }
}

impl AtcEngine {
    /// Spec 3.1: an area or information unit hands an inbound flight to Approach: an
    /// airborne aircraft without a flight plan, not handed off, calling inbound or for an
    /// approach (not VFR) gets "contact <Approach>". Adjacent towers do not (their airport's
    /// traffic is not modelled).
    fn inbound_to_approach(
        &self,
        i: usize,
        unit: &AdjacentUnit,
        msg: &PilotMessage,
    ) -> Option<AtcItem> {
        let strip = &self.strips[i];
        if !is_ifr_inbound(unit, msg)
            || strip.plan.is_some()
            || strip.handed_off.is_some()
            || !strip.phase.is_airborne()
        {
            return None;
        }
        let pos = self.position(Role::Approach)?;
        Some(AtcItem::Contact {
            station: pos.callsign.clone(),
            khz: pos.khz,
        })
    }
}

/// An inbound call or approach request, not VFR, to an area or information unit (an
/// adjacent tower does not take inbound flights: its airport's traffic is not modelled).
fn is_ifr_inbound(unit: &AdjacentUnit, msg: &PilotMessage) -> bool {
    unit.kind != AdjacentKind::Tower
        && matches!(
            msg.intent,
            PilotIntent::ReportInbound { .. } | PilotIntent::RequestApproach { .. }
        )
        && !says(&msg.raw, "vfr")
}

/// The levels of an inbound call: the level reported and whether a passing level was said.
#[derive(Debug, Default, PartialEq, Eq)]
struct Reported {
    /// The level climbed or descended to, or kept: never the one after "passing".
    level_ft: Option<i32>,
    /// "Passing <level>" was said.
    passing: bool,
}

/// The level an inbound call in `lang` reports, by the pilot's words: the levels said
/// (altitudes and flight levels, after a verb, "for" or "to") other than the one after
/// "passing"; climbing, the highest of them, descending, the lowest, otherwise the last one
/// ("passing four thousand seven hundred feet, climbing flight level one zero zero" reports
/// FL100; "passing four thousand seven hundred" alone reports none). When the words give no
/// level, the levels the parser heard (`items`) except the passing one, picked the same way.
fn reported_level(text: &str, lang: Lang, items: &HeardItems) -> Reported {
    let toks = match lang {
        Lang::En => normalize(text),
        Lang::Pl => atc_phraseology::pl::normalize::normalize(text),
    };
    let word = |k: usize, w: &str| matches!(toks.get(k), Some(Token::Word(x)) if x == w);
    // The level starting at `k` ("flight level N", "FL N", "altitude N", "N feet"), with the
    // index after it.
    let level_at = |mut k: usize| -> Option<(i32, usize)> {
        while matches!(toks.get(k), Some(Token::Break))
            || word(k, "through")
            || word(k, "to")
            || word(k, "for")
        {
            k += 1;
        }
        let fl = if word(k, "flight") && word(k + 1, "level") {
            k += 2;
            true
        } else if word(k, "fl") {
            k += 1;
            true
        } else {
            if word(k, "altitude") {
                k += 1;
            }
            false
        };
        while matches!(toks.get(k), Some(Token::Break)) {
            k += 1;
        }
        let Some(Token::Number(n)) = toks.get(k) else {
            return None;
        };
        let v: i32 = n.parse().ok()?;
        let ft = if fl { v * 100 } else { v };
        (fl && (10..=600).contains(&v) || !fl && ft >= 500).then_some((ft, k + 1))
    };
    let (mut levels, mut passing_ft) = (Vec::new(), None);
    let (mut up, mut down) = (false, false);
    let mut k = 0;
    while k < toks.len() {
        let w = match &toks[k] {
            Token::Word(w) => w.as_str(),
            _ => "",
        };
        match w {
            "climb" | "climbing" => up = true,
            "descend" | "descending" => down = true,
            _ => {}
        }
        let after_verb = matches!(
            w,
            "passing"
                | "climb"
                | "climbing"
                | "descend"
                | "descending"
                | "maintain"
                | "maintaining"
                | "for"
                | "to"
        );
        let start = if after_verb { k + 1 } else { k };
        let leading = matches!(w, "flight" | "fl" | "altitude")
            || matches!(toks.get(k), Some(Token::Number(_)))
                && (word(k + 1, "feet") || word(k + 1, "foot"));
        if (after_verb || leading)
            && let Some((ft, next)) = level_at(start)
        {
            if w == "passing" {
                passing_ft = Some(ft);
            } else {
                levels.push(ft);
            }
            k = next;
            continue;
        }
        k += 1;
    }
    if levels.is_empty() {
        // Words the scan does not know: the parser's levels, less the passing one.
        let mut heard: Vec<i32> = items
            .altitudes_ft
            .iter()
            .copied()
            .chain(items.flight_levels.iter().map(|&fl| i32::from(fl) * 100))
            .collect();
        if let Some(p) = passing_ft
            && let Some(at) = heard.iter().position(|&ft| ft == p)
        {
            heard.remove(at);
        }
        levels = heard;
    }
    let level_ft = match (up, down) {
        (true, false) => levels.iter().copied().max(),
        (false, true) => levels.iter().copied().min(),
        _ => levels.last().copied(),
    };
    Reported {
        level_ft,
        passing: passing_ft.is_some(),
    }
}

/// The approach altitude of an airport of elevation `elevation_ft`: 2000 ft above it,
/// rounded up to the next 500 ft; 3000 ft when the elevation is unknown.
fn approach_alt_ft(elevation_ft: Option<i32>) -> i32 {
    elevation_ft.map_or(UNKNOWN_FIELD_ALT_FT, |e| {
        let alt = e + ABOVE_FIELD_FT;
        alt.div_euclid(LEVEL_STEP_FT) * LEVEL_STEP_FT
            + if alt.rem_euclid(LEVEL_STEP_FT) > 0 {
                LEVEL_STEP_FT
            } else {
                0
            }
    })
}

/// `word` (lowercase) said in `text` ("V F R" spelled counts as "vfr").
pub(super) fn says(text: &str, word: &str) -> bool {
    let toks = normalize(text);
    let letters: Vec<char> = word.chars().map(|c| c.to_ascii_uppercase()).collect();
    toks.iter().enumerate().any(|(k, t)| match t {
        Token::Word(w) => w == word,
        Token::Letter(_) => letters
            .iter()
            .enumerate()
            .all(|(n, c)| matches!(toks.get(k + n), Some(Token::Letter(l)) if l == c)),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_level_by_the_pilots_words() {
        let r = |t: &str| reported_level(t, Lang::En, &HeardItems::default());
        let lvl = |ft: i32, passing: bool| Reported {
            level_ft: Some(ft),
            passing,
        };
        assert_eq!(
            r("passing four thousand seven hundred feet, climbing flight level one zero zero"),
            lvl(10000, true)
        );
        assert_eq!(
            r("passing four thousand seven hundred feet climbing five thousand feet"),
            lvl(5000, true)
        );
        assert_eq!(
            r("climbing five thousand feet, passing four thousand seven hundred"),
            lvl(5000, true)
        );
        assert_eq!(
            r("passing flight level eight zero descending five thousand feet"),
            lvl(5000, true)
        );
        assert_eq!(
            r("maintaining six thousand feet, inbound"),
            lvl(6000, false)
        );
        assert_eq!(r("flight level one zero zero, inbound"), lvl(10000, false));
        assert_eq!(
            r("passing four thousand seven hundred feet, inbound"),
            Reported {
                level_ft: None,
                passing: true
            }
        );
        assert_eq!(
            r("IFR, inbound, squawk four six zero one"),
            Reported::default()
        );
        // A level after "for" or "to" is the target.
        assert_eq!(
            r("passing four thousand seven hundred feet for five thousand feet"),
            lvl(5000, true)
        );
        assert_eq!(
            r("passing four thousand seven hundred feet, climbing for five thousand"),
            lvl(5000, true)
        );
        assert_eq!(r("request descent to three thousand"), lvl(3000, false));
        // Polish words through the Polish tokenizer.
        let none = HeardItems::default();
        assert_eq!(
            reported_level("wznoszenie na pięć tysięcy stóp", Lang::Pl, &none),
            lvl(5000, false)
        );
        // Nothing the scan knows: the parser's levels, less the passing one.
        let heard = HeardItems {
            altitudes_ft: vec![4700, 6000],
            ..HeardItems::default()
        };
        assert_eq!(
            reported_level("passing four thousand seven hundred feet", Lang::En, &heard),
            lvl(6000, true)
        );
        let heard = HeardItems {
            altitudes_ft: vec![6000],
            ..HeardItems::default()
        };
        assert_eq!(
            reported_level("six thousand", Lang::En, &heard),
            lvl(6000, false)
        );
    }

    #[test]
    fn approach_altitude_rounds_up() {
        assert_eq!(approach_alt_ft(Some(50)), 2500);
        assert_eq!(approach_alt_ft(Some(994)), 3000);
        assert_eq!(approach_alt_ft(Some(1000)), 3000);
        assert_eq!(approach_alt_ft(None), 3000);
    }

    #[test]
    fn says_words_and_spelled_letters() {
        assert!(says("VFR inbound", "vfr"));
        assert!(says("victor foxtrot romeo, inbound", "vfr"));
        assert!(says("request visual approach", "visual"));
        assert!(!says("request ILS approach", "visual"));
    }
}
