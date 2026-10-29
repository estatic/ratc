//! VFR entry to and exit from the control zone via the published points (AIP AD 2.22 §3).
//!
//! Entry: a request to join (also an inbound call) from an airborne aircraft outside the
//! circuit gets `enter via <point>` and `report at <point>`; from the south (bearing from the
//! airport between 090 and 270) only a point marked `south` is accepted, otherwise it is
//! refused with the nearest southern point. While the runway is busy (another aircraft
//! cleared to land or on final, or an IFR arrival within [`BUSY_ARRIVAL_NM`]) the aircraft is
//! told to hold over the nearest published holding point instead, and gets the entry when it
//! reports there or calls again with the runway free. At the entry point it is told to join
//! downwind; from then on the circuit logic of [`super::tower`] takes over.
//!
//! Exit: after take-off a flight cleared by Delivery via a point is asked to report at it
//! (a circuit flight asks to leave); at the point it is approved to leave the zone and sent to
//! the flight information unit. A VFR flight stays on Tower throughout (AIP AD 2.22 §1).

use atc_geo::{LatLon, M_PER_NM, bearing_deg, distance_m};
use atc_navdata::procedures::VfrPoint;
use atc_phraseology::{AtcItem, CircuitLeg, PilotIntent, PilotMessage, UnableReason};

use super::{AtcEngine, AtcOutput};
use crate::phase::FlightPhase;
use crate::strip::{Role, VfrState};

/// An IFR arrival within this distance of the airport keeps VFR traffic out of the zone.
const BUSY_ARRIVAL_NM: f64 = 10.0;

/// A position report names a point the aircraft is within this distance of.
const AT_POINT_NM: f64 = 2.0;
/// A VFR aircraft this close to the airport, in a circuit leg, is in the circuit.
const CIRCUIT_NM: f64 = 3.0;

impl AtcEngine {
    /// A request to join (or an inbound call) at `point` from aircraft `i`. Only an airborne
    /// VFR flight (no flight plan, or a VFR one) is answered.
    pub(super) fn vfr_join(&mut self, i: usize, point: Option<&str>) -> Vec<AtcItem> {
        let strip = &self.strips[i];
        if !strip.phase.is_airborne() || strip.plan.as_ref().is_some_and(|p| p.ifr) {
            return Vec::new();
        }
        // Already in the circuit: it stays there.
        if self.in_circuit(i) {
            return vec![AtcItem::Roger];
        }
        let Some(pos) = self.last_pos(i) else {
            return Vec::new();
        };
        let Some(alt_ft) = self.entry_alt_ft(i) else {
            return Vec::new();
        };
        let requested = point
            .and_then(|p| self.procs.vfr_point(p))
            .or_else(|| self.nearest(pos, |_| true))
            .map(|p| p.name.to_uppercase());
        let Some(entry) = requested else {
            return Vec::new();
        };
        // From the south only via the southern points.
        let any_south = self.procs.vfr_points.iter().any(|p| p.south);
        let from_south = self.airport_ref().is_some_and(|home| {
            let b = bearing_deg(home, pos);
            b > 90.0 && b < 270.0
        });
        let is_south = self.procs.vfr_point(&entry).is_some_and(|p| p.south);
        if any_south
            && from_south
            && !is_south
            && let Some(south) = self
                .nearest(pos, |p| p.south)
                .map(|p| p.name.to_uppercase())
        {
            self.set_vfr(
                i,
                VfrState::Entering {
                    point: south.clone(),
                },
            );
            return vec![
                AtcItem::Unable(UnableReason::SouthernEntry),
                AtcItem::EnterControlZoneVia {
                    point: south,
                    alt_ft,
                },
            ];
        }
        self.vfr_entry(i, entry, pos, alt_ft)
    }

    /// Entry via `entry`, or the hold over the nearest holding point when the runway is busy.
    fn vfr_entry(&mut self, i: usize, entry: String, pos: LatLon, alt_ft: u32) -> Vec<AtcItem> {
        if self.runway_busy(i)
            && let (Some(max_ft), Some(hold)) = (
                self.procs.vfr_hold_max_ft,
                self.nearest(pos, |p| {
                    self.procs
                        .vfr_holds
                        .iter()
                        .any(|h| h.eq_ignore_ascii_case(&p.name))
                })
                .map(|p| p.name.to_uppercase()),
            )
        {
            self.set_vfr(
                i,
                VfrState::Holding {
                    entry,
                    hold: hold.clone(),
                },
            );
            return vec![AtcItem::HoldOver {
                point: hold,
                max_ft: u32::try_from(max_ft).unwrap_or(0),
            }];
        }
        self.set_vfr(
            i,
            VfrState::Entering {
                point: entry.clone(),
            },
        );
        vec![
            AtcItem::EnterControlZoneVia {
                point: entry.clone(),
                alt_ft,
            },
            AtcItem::ReportAt { point: entry },
        ]
    }

    /// A request to leave the zone via `point` (nearest point when none was named) from an
    /// airborne aircraft: approved, with a report at the point.
    pub(super) fn vfr_leave(&mut self, i: usize, point: Option<&str>) -> Vec<AtcItem> {
        let strip = &self.strips[i];
        if !strip.phase.is_airborne() || strip.plan.as_ref().is_some_and(|p| p.ifr) {
            return Vec::new();
        }
        let named = point.and_then(|p| self.procs.vfr_point(p));
        let Some(point) = named
            .or_else(|| self.last_pos(i).and_then(|pos| self.nearest(pos, |_| true)))
            .map(|p| p.name.to_uppercase())
        else {
            return Vec::new();
        };
        self.set_vfr(
            i,
            VfrState::Leaving {
                point: point.clone(),
            },
        );
        vec![
            AtcItem::LeaveControlZoneVia {
                point: point.clone(),
            },
            AtcItem::ReportAt { point },
        ]
    }

    /// The pilot's message names a point while entering, holding or leaving: a position
    /// report ("at Zulu", "over Kilo, two thousand feet"). Readbacks of the instruction just
    /// given are filtered out first ([`AtcEngine::vfr_readback`]).
    pub(super) fn is_point_report(&self, i: usize, msg: &PilotMessage) -> bool {
        !msg.items.points.is_empty() && self.strips[i].vfr.is_some()
    }

    /// Called for every Tower transmission of an aircraft with a VFR state. While Tower's
    /// last VFR instruction is unanswered, a transmission naming a point away from it is the
    /// readback and is not answered; any other answer closes the wait. (Within
    /// [`AT_POINT_NM`] of the point the pilot has evidently arrived, and it is a report.)
    pub(super) fn vfr_readback(&mut self, i: usize, msg: &PilotMessage) -> bool {
        if self.strips[i].vfr.is_none() || !self.strips[i].vfr_unacked {
            return false;
        }
        // The point is named with or without "at" / "via" ("Romeo, two thousand feet").
        let named = msg.items.points.first().cloned().or_else(|| {
            let raw = super::fold(&msg.raw.to_lowercase());
            raw.split(|c: char| !c.is_alphanumeric())
                .find_map(|w| self.procs.vfr_point(w).map(|p| p.name.clone()))
        });
        let far = named.is_some_and(|p| !self.near(i, &p));
        self.strips[i].vfr_unacked = false;
        far && msg.intent == PilotIntent::Readback
    }

    /// Aircraft `i` is within [`AT_POINT_NM`] of the VFR point `name`.
    fn near(&self, i: usize, name: &str) -> bool {
        self.last_pos(i)
            .zip(self.procs.vfr_point(name))
            .is_some_and(|(pos, p)| {
                let at = LatLon {
                    lat: p.lat,
                    lon: p.lon,
                };
                distance_m(pos, at) <= AT_POINT_NM * M_PER_NM
            })
    }

    /// Sets the VFR state after an instruction: the pilot is expected to read it back.
    fn set_vfr(&mut self, i: usize, state: VfrState) {
        let strip = &mut self.strips[i];
        strip.vfr = Some(state);
        strip.vfr_unacked = true;
    }

    /// An airborne VFR flight flying the circuit near the field.
    fn in_circuit(&self, i: usize) -> bool {
        let strip = &self.strips[i];
        matches!(strip.phase, FlightPhase::Circuit(_))
            && strip.vfr.is_none()
            && self
                .airport_ref()
                .zip(self.last_pos(i))
                .is_some_and(|(home, pos)| distance_m(home, pos) <= CIRCUIT_NM * M_PER_NM)
    }

    /// The pilot reports at a point while entering, holding or leaving.
    pub(super) fn vfr_at_point(&mut self, i: usize, msg: &PilotMessage) -> Vec<AtcItem> {
        let reported = msg.items.points.first().cloned().unwrap_or_default();
        let same = |p: &str| p.eq_ignore_ascii_case(&reported);
        // Not at the point it names: reminded where to report.
        if !self.near(i, &reported) {
            let point = match self.strips[i].vfr.clone() {
                Some(VfrState::Entering { point } | VfrState::Leaving { point }) => point,
                Some(VfrState::Holding { hold, .. }) => hold,
                Some(VfrState::Left) | None => return Vec::new(),
            };
            self.strips[i].vfr_unacked = true;
            return vec![AtcItem::ReportAt { point }];
        }
        match self.strips[i].vfr.clone() {
            Some(VfrState::Entering { point }) if same(&point) => self.vfr_join_circuit(i, point),
            Some(VfrState::Entering { point }) => vec![AtcItem::ReportAt { point }],
            // At (or over) the hold point: the entry when the runway is free again.
            Some(VfrState::Holding { entry, hold }) if same(&hold) => {
                match (self.last_pos(i), self.entry_alt_ft(i)) {
                    (Some(pos), Some(alt_ft)) => self.vfr_entry(i, entry, pos, alt_ft),
                    _ => Vec::new(),
                }
            }
            Some(VfrState::Holding { hold, .. }) => vec![AtcItem::ReportAt { point: hold }],
            Some(VfrState::Leaving { point }) if same(&point) => self.vfr_hand_off(i, point),
            Some(VfrState::Leaving { point }) => vec![AtcItem::ReportAt { point }],
            Some(VfrState::Left) | None => Vec::new(),
        }
    }

    /// At the entry point: join the circuit (or hold when the runway got busy meanwhile).
    fn vfr_join_circuit(&mut self, i: usize, point: String) -> Vec<AtcItem> {
        if self.runway_busy(i) {
            let (Some(pos), Some(alt_ft)) = (self.last_pos(i), self.entry_alt_ft(i)) else {
                return Vec::new();
            };
            let items = self.vfr_entry(i, point, pos, alt_ft);
            if matches!(items.as_slice(), [AtcItem::HoldOver { .. }]) {
                return items;
            }
        }
        // The circuit logic (legs, landing clearance) takes over from the next sample. A VFR
        // flight plan to elsewhere is over: the flight turned back and lands here.
        let strip = &mut self.strips[i];
        strip.vfr = None;
        if strip.plan.as_ref().is_some_and(|p| !p.ifr) {
            strip.plan = None;
            strip.vfr_exit = None;
        }
        vec![
            AtcItem::JoinDownwind {
                runway: self.strips[i].runway.clone(),
            },
            AtcItem::Report(CircuitLeg::Downwind),
        ]
    }

    /// At the exit point: leaving approved, frequency change to the flight information unit.
    fn vfr_hand_off(&mut self, i: usize, point: String) -> Vec<AtcItem> {
        let mut items = vec![
            AtcItem::LeaveControlZoneVia { point },
            AtcItem::FrequencyChangeApproved,
        ];
        let fis = self
            .procs
            .adjacent
            .iter()
            .find(|u| u.kind == atc_navdata::procedures::AdjacentKind::Fis)
            .cloned();
        let strip = &mut self.strips[i];
        strip.vfr = Some(VfrState::Left);
        if let Some(unit) = fis {
            items.push(AtcItem::Contact {
                station: unit.callsign.clone(),
                khz: unit.khz,
            });
            strip.handed_off = Some(unit.callsign);
            strip.handed_off_khz = Some(unit.khz);
            strip.handoff_acked = false;
        }
        strip.expect_readback(&items, self.cfg.strictness);
        items
    }

    /// After take-off, a VFR flight cleared by Delivery via a point is asked to report at it
    /// (once per departure). Tower speaks unprompted, as for the departure itself.
    pub(super) fn vfr_exit_update(&mut self, i: usize) -> Vec<AtcOutput> {
        let strip = &self.strips[i];
        if strip.owner != Role::Tower
            || strip.phase != FlightPhase::Airborne
            || strip.vfr.is_some()
            || strip.handed_off.is_some()
            || strip.handoff_to.is_some()
            || strip.pending_readback.is_some()
            || strip.plan.as_ref().is_none_or(|p| p.ifr)
        {
            return Vec::new();
        }
        let Some(point) = strip.vfr_exit.clone() else {
            return Vec::new();
        };
        self.set_vfr(
            i,
            VfrState::Leaving {
                point: point.clone(),
            },
        );
        self.transmit(i, Role::Tower, vec![AtcItem::ReportAt { point }])
    }

    /// Another aircraft is cleared to land or on final, or an IFR arrival is within
    /// [`BUSY_ARRIVAL_NM`] of the airport.
    fn runway_busy(&self, i: usize) -> bool {
        let home = self.airport_ref();
        self.strips.iter().enumerate().any(|(j, s)| {
            j != i
                && (s.permissions.may_land
                    || s.phase == FlightPhase::Circuit(CircuitLeg::Final)
                    || (s.phase.is_airborne()
                        && self.arrival(j).is_some()
                        && home
                            .zip(self.last_pos(j))
                            .is_some_and(|(h, p)| distance_m(h, p) <= BUSY_ARRIVAL_NM * M_PER_NM)))
        })
    }

    /// Altitude of the entry and the hold: the published VFR hold ceiling, else the circuit
    /// altitude of the runway in use.
    fn entry_alt_ft(&self, i: usize) -> Option<u32> {
        let runway = &self.strips[i].runway;
        let ft = self.procs.vfr_hold_max_ft.or_else(|| {
            self.procs
                .circuits
                .iter()
                .find(|c| &c.runway == runway)
                .map(|c| c.altitude_ft_msl)
        })?;
        u32::try_from(ft).ok()
    }

    /// The published point closest to `pos` that satisfies `keep`.
    fn nearest(&self, pos: LatLon, keep: impl Fn(&VfrPoint) -> bool) -> Option<&VfrPoint> {
        self.procs
            .vfr_points
            .iter()
            .filter(|p| keep(p))
            .min_by(|a, b| {
                let d = |p: &VfrPoint| {
                    distance_m(
                        pos,
                        LatLon {
                            lat: p.lat,
                            lon: p.lon,
                        },
                    )
                };
                d(a).total_cmp(&d(b))
            })
    }
}
