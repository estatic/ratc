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
//! (or asking for an approach) gets an IFR plan to this airport (level: the one it reports,
//! the last said when it reports passing one and climbing to another, else its altitude); a VFR one (it says VFR, or names a VFR point without saying IFR) is
//! sent to Tower.
//! The first reply: "identified", then the STAR chosen by [`choose_star`] when the aircraft is
//! still outside its start fix (never for a VOR/DME approach: AIP AD 2.22, VOR approaches are
//! not linked to STARs), otherwise direct to the IAF (full procedure, on request) or a
//! radar vector (default); the level (the IFR default level, or the intercept altitude when
//! already within 15 NM of the intercept point, never above the present level), the QNH and
//! the approach to expect: ILS on a runway that has one, else RNAV, VOR/DME on request only
//! (or when it is the only one); a requested type the runway lacks: "unable" and the default.
//! On vectors (after the STAR's last fix, if one was given) Approach steers the aircraft to
//! join the final at the point 8 NM out ([`vector_to_final`]: towards a lead point 3 NM back
//! along the 30-degree intercept track): a new heading whenever it changes by 20 degrees or
//! more, the descent to the intercept altitude (the FAF's altitude constraint) within 15 NM
//! of the point, and near the lead point (or within 3 NM of the intercept point, 2 NM at the
//! latest) the intercept heading with the distance to go, "cleared `<approach>`" and "report established". An
//! aircraft that would have to turn more than 90 degrees onto the intercept (it comes from
//! the far side of the field) is vectored around a pattern instead: downwind 4 NM abeam the
//! final, base 11 NM out, the intercept within 2 NM of the centreline ([`AtcEngine::steer`]).
//! Cleared but not established by the FAF (4 NM without one): "check established" once,
//! from the station owning the flight (spec 3.5). Full
//! procedure: "cleared `<approach>`" at the IAF. "Established" (or the final inside the FAF)
//! transfers the flight to Tower. A missed approach (Tower's go-around, or leaving the final
//! airborne near the threshold or climbing) goes back to Approach, which vectors it again
//! from the published missed-approach altitude when it calls; the published missed-approach
//! track itself is not monitored. A requested
//! approach type is given when the CIFP has it for the runway, else "unable"; a visual
//! approach is not available at night (21:00-05:00 UTC in summer, 20:00-04:00 in winter) and
//! by day is Tower's (contact Tower). Squawk 7600 (AIP AD 2.22 §2.1): Approach says nothing
//! more to the aircraft and calls no deviation; the strip records it.
//!
//! Approach and the adjacent units always speak English (spec 3.1).

use atc_geo::{LatLon, M_PER_NM, RunwayFrame, bearing_deg, destination, distance_m};
use atc_navdata::procedures::CircuitSide;
use atc_navdata::procedures::{AdjacentKind, AdjacentUnit};
use atc_phraseology::{
    ApproachKind, AtcItem, AtcMessage, FlightRules, Lang, PilotIntent, PilotMessage, Token,
    UnableReason, normalize, render,
};

use super::delivery::{cleared_to, mentions_ifr};
use super::{AtcEngine, AtcOutput};
use crate::compliance::IfrClearance;
use crate::ifr::{
    Path, Vector, choose_star, constraint_floor_ft, distance_to_go_nm, iaf, joined_path,
    lead_point, navdata_approach_kind, passed_fix, procedure_path, spoken_designator, vector_on,
    vector_to_final,
};
use crate::phase::FlightPhase;
use crate::state::AircraftState;
use crate::strip::{FlightPlan, PatternLeg, Role};

/// A fix counts as passed within this distance of it.
const PASSED_FIX_M: f64 = M_PER_NM;
/// The destination's tower takes the flight this far from the destination.
const DEST_TOWER_M: f64 = 10.0 * M_PER_NM;
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
/// Radar vectors aim at the point this far out on the final.
const INTERCEPT_NM: f64 = 8.0;
/// Within this of the lead point of the intercept a vector is not changed by 20 degrees
/// just because the bearing to the lead point swings.
const NEAR_LEAD_M: f64 = 2.0 * M_PER_NM;
/// The intercept heading is given at the latest this close to the point.
const NEAR_INTERCEPT_M: f64 = 2.0 * M_PER_NM;
/// A new heading when the vector changed by at least this much.
const REVECTOR_DEG: f64 = 20.0;
/// The descent to the intercept altitude is cleared this far from the intercept point.
const DESCENT_M: f64 = 15.0 * M_PER_NM;
/// Without a published FAF, the final approach fix is taken this far from the threshold
/// (transfer to Tower, approach-not-established check).
const DEFAULT_FAF_M: f64 = 4.0 * M_PER_NM;
/// Pattern vectors: the downwind is flown this far abeam the final (NM) out to this far
/// beyond the intercept point, then the base; the intercept heading within `INTERCEPT_Y_NM`
/// of the centreline.
const DOWNWIND_Y_NM: f64 = 4.0;
/// The downwind heading aims this far beyond the base turn, NM.
const DOWNWIND_AIM_BEYOND_NM: f64 = 3.0;
const BASE_BEYOND_NM: f64 = 3.0;
const INTERCEPT_Y_NM: f64 = 2.0;
/// The direct heading to the intercept point more than this off the intercept heading:
/// vectored around the pattern instead.
const PATTERN_GAP_DEG: f64 = 90.0;
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
            } if new && self.strips[i].phase.is_airborne() => match rules {
                None => vec![AtcItem::SayFlightRules],
                Some(FlightRules::Ifr) => {
                    self.airborne_clearance(i, destination.as_deref(), *level_ft)
                }
                Some(FlightRules::Vfr) => Vec::new(),
            },
            PilotIntent::ReportInbound { .. } if new => self.inbound(i, msg, None, false),
            PilotIntent::RequestApproach {
                kind,
                full_procedure,
            } if new => self.inbound(i, msg, *kind, *full_procedure),
            PilotIntent::RequestApproach {
                kind,
                full_procedure,
            } if self.arrival(i).is_some() => self.change_approach(i, *kind, *full_procedure),
            PilotIntent::ReportEstablished
                if self.arrival(i).is_some_and(|p| p.approach_cleared) =>
            {
                self.redirect(i, Role::Tower)
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
        self.strips[i].plan = None;
        let (kind, full) = match msg.intent {
            PilotIntent::RequestApproach {
                kind,
                full_procedure,
            } => (kind, full_procedure),
            _ => (None, false),
        };
        let items = self.inbound(i, msg, kind, full);
        if items.is_empty() {
            // No telemetry yet: still the flight handed over, served on its next call.
            let strip = &mut self.strips[i];
            strip.plan = plan;
            strip.handed_in = true;
        }
        items
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
    /// the module docs). `kind` / `full`: as requested.
    fn inbound(
        &mut self,
        i: usize,
        msg: &PilotMessage,
        kind: Option<ApproachKind>,
        full: bool,
    ) -> Vec<AtcItem> {
        if !self.strips[i].phase.is_airborne() {
            return Vec::new();
        }
        let vfr =
            says(&msg.raw, "vfr") || (!mentions_ifr(&msg.raw) && !msg.items.points.is_empty());
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
        let Some((thr, course)) = self.final_geometry(&runway) else {
            return vec![AtcItem::Unable(UnableReason::NoIfrData)];
        };
        // The level it reports: the last one said ("passing four thousand climbing five
        // thousand" reports five thousand), else its altitude.
        let actual_ft = ((s.alt_ft_msl / 100.0).round() as i32) * 100;
        let present_ft = msg.items.altitudes_ft.last().copied().unwrap_or(actual_ft);
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
        let intercept_ft = self.intercept_alt_ft(&runway, kind);
        let point = intercept_point(thr, course);
        let vectors = !full;
        let mut target = self.procs.ifr.map_or(DEFAULT_ARRIVAL_FT, |d| d.cruise_ft);
        if star_item.is_none() && vectors && distance_m(s.pos, point) <= DESCENT_M {
            target = target.min(intercept_ft);
        }
        let level_ft = present_ft.min(target);
        let mut items = vec![AtcItem::Identified];
        let (mut heading, mut direct, mut pattern) = (None, None, None);
        match (&star_item, full, &iaf_fix) {
            (Some((_, item)), _, _) => items.push(item.clone()),
            (None, true, Some(fix)) => {
                items.push(AtcItem::DirectTo { fix: fix.clone() });
                direct = Some(fix.clone());
            }
            _ => {
                let (v, _, leg) = self.steer(&s, &runway, thr, course, None, true, level_ft);
                items.push(AtcItem::Heading {
                    deg: v.heading_deg,
                    turn: Some(v.turn),
                });
                heading = Some(v.heading_deg);
                pattern = leg;
            }
        }
        // Climb, descent or maintain from where it is now.
        let now = s.alt_ft_msl - f64::from(level_ft);
        items.push(if level_ft < present_ft || now > LEVEL_TOLERANCE_FT {
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
        items.push(AtcItem::ExpectApproach {
            kind,
            runway: runway.clone(),
        });
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
            approach_cleared: false,
            pattern,
            established_check: false,
        };
        let strip = &mut self.strips[i];
        strip.plan = Some(plan);
        strip.identified = true;
        strip.expect_readback(&items, self.cfg.strictness);
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
            // AIP AD 2.22: VOR approaches are not linked to STARs.
            if new_kind == ApproachKind::VorDme {
                plan.star = None;
            }
        }
        if full && let Some(fix) = iaf_fix {
            plan.vectors = false;
            plan.heading = None;
            plan.direct = Some(fix.clone());
            plan.approach_cleared = false;
            items.push(AtcItem::DirectTo { fix });
        } else if !full && kind.is_none() {
            // "Request vectors".
            plan.vectors = true;
            plan.direct = None;
            items.push(AtcItem::Roger);
        }
        strip.expect_readback(&items, self.cfg.strictness);
        items
    }

    /// "Request visual approach": not available at night; by day it is Tower's.
    fn visual_request(&mut self, i: usize) -> Vec<AtcItem> {
        if self.is_night() {
            vec![AtcItem::VisualApproachNotAvailable]
        } else {
            self.redirect(i, Role::Tower)
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
    /// still had the flight): identified again and vectored. The published missed-approach
    /// track is not monitored: a missed approach is Tower's go-around and Approach's vectors.
    fn after_missed_approach(&mut self, i: usize) -> Vec<AtcItem> {
        let strip = &mut self.strips[i];
        strip.missed_approach = false;
        strip.identified = true;
        let items = self.vector_items(i, true);
        self.strips[i].expect_readback(&items, self.cfg.strictness);
        items
    }

    /// The published approach of `kind` to `runway` exists.
    fn has_approach(&self, runway: &str, kind: ApproachKind) -> bool {
        self.procedure_set
            .as_ref()
            .is_some_and(|set| set.approach(runway, navdata_approach_kind(kind)).is_some())
    }

    /// The default approach to `runway`: ILS, else RNAV; VOR/DME only when it is the only one.
    fn default_approach(&self, runway: &str) -> Option<ApproachKind> {
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

    /// The published missed-approach altitude: the highest altitude constraint of the
    /// missed-approach legs, else the intercept altitude.
    pub(super) fn missed_alt_ft(&self, runway: &str, kind: ApproachKind) -> i32 {
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
            if self.strips[i].phase == FlightPhase::Circuit(atc_phraseology::CircuitLeg::Final)
                && inside
            {
                let items = self.redirect(i, Role::Tower);
                return self.transmit(i, Role::Approach, items);
            }
            return Vec::new();
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
        self.strips[i].expect_readback(&items, self.cfg.strictness);
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

    /// The next radar vector for arrival `i` (see the module docs), applied to its plan;
    /// nothing when none is due. `force`: a heading and the level now (after a missed
    /// approach).
    fn vector_items(&mut self, i: usize, force: bool) -> Vec<AtcItem> {
        let Some(plan) = self.arrival(i).cloned() else {
            return Vec::new();
        };
        let Some((kind, runway)) = plan.approach.clone() else {
            return Vec::new();
        };
        let (Some((thr, course)), Some(s)) = (
            self.final_geometry(&runway),
            self.tracks
                .get(&self.strips[i].callsign)
                .map(|t| t.last.clone()),
        ) else {
            return Vec::new();
        };
        let intercept_ft = self.intercept_alt_ft(&runway, kind);
        let point = intercept_point(thr, course);
        let to_point = distance_m(s.pos, point);
        let (v, at_intercept, leg) = self.steer(
            &s,
            &runway,
            thr,
            course,
            plan.pattern,
            plan.heading.is_none(),
            intercept_ft,
        );
        // Near the lead point the bearing to it swings: no new heading for that alone.
        let near_lead = leg.is_none()
            && distance_m(s.pos, lead_point(s.pos, thr, course, INTERCEPT_NM)) <= NEAR_LEAD_M;
        let changed = force
            || leg != plan.pattern
            || plan.heading.is_none_or(|h| {
                heading_gap(h, v.heading_deg) >= REVECTOR_DEG && (!near_lead || at_intercept)
            });
        let descend = plan.cleared_ft > intercept_ft && to_point <= DESCENT_M;
        let intercept = at_intercept && (leg.is_some() || changed || to_point <= NEAR_INTERCEPT_M);
        if !(changed || descend || intercept) {
            return Vec::new();
        }
        let say_heading = changed || intercept;
        let mut items = Vec::new();
        if say_heading {
            items.push(AtcItem::Heading {
                deg: v.heading_deg,
                turn: Some(v.turn),
            });
        }
        let mut cleared_ft = plan.cleared_ft;
        if descend || (intercept && plan.cleared_ft > intercept_ft) {
            cleared_ft = intercept_ft;
        }
        if cleared_ft != plan.cleared_ft || force {
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
        if intercept {
            items.push(AtcItem::DistanceToGo(distance_to_go_nm(s.pos, thr)));
            items.push(AtcItem::ClearedApproach {
                kind,
                runway: runway.clone(),
            });
            items.push(AtcItem::ReportEstablished);
        }
        if let Some(p) = self.strips[i].plan.as_mut() {
            if say_heading {
                p.heading = Some(v.heading_deg);
            }
            p.cleared_ft = cleared_ft;
            p.approach_cleared = intercept;
            p.pattern = leg;
        }
        items
    }

    /// Where radar vectors steer arrival `s` onto the final of `runway` (threshold `thr`,
    /// true course `course`), with the vector, whether it is the intercept heading and the
    /// pattern leg. Normally direct to the intercept point, then the 30-degree intercept
    /// within 3 NM of it ([`vector_to_final`]). An aircraft nearer the threshold than the
    /// intercept point (or beyond it) whose direct heading to the point is more than 90
    /// degrees off the intercept heading is taken around a pattern instead, on its side of
    /// the final (the circuit side when it is on the centreline): downwind 4 NM abeam out to
    /// 3 NM beyond the point, base towards the centreline, the 30-degree intercept within
    /// 2 NM of it. The pattern is chosen only when vectoring starts (`fresh`); `pattern` is
    /// the leg so far (a leg is never flown again).
    #[allow(clippy::too_many_arguments)]
    fn steer(
        &self,
        s: &AircraftState,
        runway: &str,
        thr: LatLon,
        course: f64,
        pattern: Option<PatternLeg>,
        fresh: bool,
        alt_ft: i32,
    ) -> (Vector, bool, Option<PatternLeg>) {
        let var = self.procs.magnetic_variation_deg;
        let point = intercept_point(thr, course);
        let (along, right) = RunwayFrame {
            origin: thr,
            course_deg: course,
        }
        .to_local(s.pos);
        // Out along the approach, and to the right of the landing direction, in NM.
        let (x, y) = (-along / M_PER_NM, right / M_PER_NM);
        let side = if y.abs() >= 0.5 {
            y.signum()
        } else {
            self.circuit_side(runway)
        };
        let intercept_hdg = course - side * 30.0;
        let base_x = INTERCEPT_NM + BASE_BEYOND_NM;
        let start = || {
            let direct = bearing_deg(s.pos, point);
            (fresh && x < INTERCEPT_NM && gap_deg(direct, intercept_hdg) > PATTERN_GAP_DEG)
                .then_some(if x < base_x - 0.5 {
                    PatternLeg::Downwind
                } else {
                    PatternLeg::Base
                })
        };
        let Some(leg) = pattern.or_else(start) else {
            let (v, intercept) =
                vector_to_final(s.pos, s.heading_deg, thr, course, INTERCEPT_NM, alt_ft, var);
            return (v, intercept, None);
        };
        let leg = match leg {
            PatternLeg::Downwind if x >= base_x - 0.5 => PatternLeg::Base,
            l => l,
        };
        let (hdg, intercept) = match leg {
            PatternLeg::Downwind => {
                // Aimed past the base turn, so the bearing does not swing near it (the base
                // starts by position, not on reaching the point).
                let abeam = destination(
                    thr,
                    course + 180.0,
                    (base_x + DOWNWIND_AIM_BEYOND_NM) * M_PER_NM,
                );
                let end = destination(abeam, course + 90.0 * side, DOWNWIND_Y_NM * M_PER_NM);
                (bearing_deg(s.pos, end), false)
            }
            PatternLeg::Base if y.abs() > INTERCEPT_Y_NM => (course - side * 90.0, false),
            PatternLeg::Base => (intercept_hdg, true),
        };
        (
            vector_on(hdg, s.heading_deg, alt_ft, var),
            intercept,
            Some(leg),
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
        if strip.owner == Role::Tower || strip.handoff_to == Some(Role::Tower) {
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
            strip.readback_complete();
        }
        if let Some(p) = strip.plan.as_mut() {
            p.approach_cleared = false;
            p.established_check = false;
            p.heading = None;
            p.direct = None;
            p.star = None;
            p.pattern = None;
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
        strip.expect_readback(&items, self.cfg.strictness);
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
            pattern: None,
            established_check: false,
        });
        strip.identified = true;
        strip.expect_readback(&items, self.cfg.strictness);
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
            strip.expect_readback(&items, self.cfg.strictness);
            return self.transmit(i, Role::Approach, items);
        }
        if let Some((unit, items)) = self.hand_off(&plan, s.pos) {
            let strip = &mut self.strips[i];
            strip.handed_off = Some(unit);
            strip.handed_off_khz = items.iter().find_map(|it| match it {
                AtcItem::Contact { khz, .. } => Some(*khz),
                _ => None,
            });
            strip.expect_readback(&items, self.cfg.strictness);
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
            strip.expect_readback(&items, self.cfg.strictness);
            return self.transmit(i, Role::Approach, items);
        }
        Vec::new()
    }

    /// The descent due at `pos` before the hand-off to the destination's tower: 20 NM out,
    /// to the destination's approach altitude ([`approach_alt_ft`]) with the QNH, once.
    fn descent_before_tower(&self, plan: &FlightPlan, pos: LatLon) -> Option<(i32, Vec<AtcItem>)> {
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

    /// The hand-off due at `pos`: the unit's callsign (empty for a release) and the items.
    fn hand_off(&self, plan: &FlightPlan, pos: LatLon) -> Option<(String, Vec<AtcItem>)> {
        let contact = |u: &AdjacentUnit| AtcItem::Contact {
            station: u.callsign.clone(),
            khz: u.khz,
        };
        if let Some(tower) = self.destination_tower(plan) {
            let dest = self.destination_pos(plan)?;
            return (distance_m(pos, dest) <= DEST_TOWER_M)
                .then(|| (tower.callsign.clone(), vec![contact(tower)]));
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
            return Some((acc.callsign.clone(), vec![contact(acc)]));
        }
        // Neither unit: released, no unit to contact.
        let Some(fis) = units.iter().find(|u| u.kind == AdjacentKind::Fis) else {
            return Some((
                String::new(),
                vec![
                    AtcItem::RadarServiceTerminated,
                    AtcItem::FrequencyChangeApproved,
                ],
            ));
        };
        Some((
            fis.callsign.clone(),
            vec![AtcItem::RadarServiceTerminated, contact(fis)],
        ))
    }

    /// The adjacent tower of the flight's destination.
    fn destination_tower(&self, plan: &FlightPlan) -> Option<&AdjacentUnit> {
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
    /// still climbing: the destination when it is within the TMA or has an adjacent tower,
    /// otherwise the SID's last fix.
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
        if near || self.destination_tower(plan).is_some() {
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
            // Calling the unit completes the hand-off, read back or not.
            if strip.pending_readback.is_some() {
                strip.readback_complete();
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

/// The point `INTERCEPT_NM` out on the final to the threshold `thr` (final course `course`).
fn intercept_point(thr: LatLon, course: f64) -> LatLon {
    destination(thr, course + 180.0, INTERCEPT_NM * M_PER_NM)
}

/// Smallest difference of two bearings, degrees.
fn gap_deg(a: f64, b: f64) -> f64 {
    ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs()
}

/// Smallest difference of two magnetic headings, degrees.
fn heading_gap(a: u16, b: u16) -> f64 {
    let d = (i32::from(a) - i32::from(b)).rem_euclid(360);
    f64::from(d.min(360 - d))
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
    fn approach_altitude_rounds_up() {
        assert_eq!(approach_alt_ft(Some(50)), 2500);
        assert_eq!(approach_alt_ft(Some(994)), 3000);
        assert_eq!(approach_alt_ft(Some(1000)), 3000);
        assert_eq!(approach_alt_ft(None), 3000);
    }

    #[test]
    fn heading_gap_wraps() {
        assert_eq!(heading_gap(350, 10), 20.0);
        assert_eq!(heading_gap(90, 270), 180.0);
    }

    #[test]
    fn says_words_and_spelled_letters() {
        assert!(says("VFR inbound", "vfr"));
        assert!(says("victor foxtrot romeo, inbound", "vfr"));
        assert!(says("request visual approach", "visual"));
        assert!(!says("request ILS approach", "visual"));
    }
}
