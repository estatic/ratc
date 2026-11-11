//! Tower position: departure and circuit clearances, and the landing of IFR arrivals
//! transferred by Approach (a missed approach goes back to Approach).

use atc_geo::{M_PER_NM, bearing_deg, distance_m};
use atc_phraseology::{AtcItem, CircuitLeg, ClearanceKind, Compass8, PilotIntent, PilotMessage};

use super::{AtcEngine, AtcOutput, standby};
use crate::ifr::distance_to_go_nm;
use crate::phase::{FINAL_HALF_WIDTH_M, FlightPhase};
use crate::runway::RunwayUse;
use crate::state::TransponderMode;
use crate::strip::Role;

/// No other traffic in v1: every aircraft in the circuit is number one.
const SEQUENCE: u8 = 1;

impl AtcEngine {
    pub(super) fn tower_request(&mut self, i: usize, msg: &PilotMessage) -> Vec<AtcItem> {
        let ready = self.ready_call(i, &msg.intent);
        // A Tower-kept arrival's first call after its missed approach: the approach again.
        let strip = &mut self.strips[i];
        let missed = strip.tower_inbound && strip.missed_approach;
        if missed {
            strip.missed_approach = false;
        }
        match &msg.intent {
            PilotIntent::ReportEstablished | PilotIntent::ReportLeg(CircuitLeg::Final) => {}
            _ if msg.items.kinds.contains(&ClearanceKind::GoAround) => {}
            _ if missed => return self.approach_again(i),
            _ => {}
        }
        match &msg.intent {
            _ if ready && self.departure_in_hand(i) => Vec::new(),
            _ if ready => self.departure(i),
            // "Join downwind ..., wilco": the readback of the joining instruction.
            PilotIntent::ReportLeg(CircuitLeg::Downwind)
                if super::approach::says(&msg.raw, "join") =>
            {
                Vec::new()
            }
            PilotIntent::ReportLeg(CircuitLeg::Downwind) => vec![
                AtcItem::Sequence(SEQUENCE),
                AtcItem::Report(CircuitLeg::Final),
            ],
            PilotIntent::ReportLeg(CircuitLeg::Final) => self.landing(i),
            PilotIntent::ReportEstablished if self.arrival(i).is_some() => self.landing(i),
            PilotIntent::ReportLeg(_) | PilotIntent::RequestFullStop => vec![AtcItem::Roger],
            PilotIntent::RequestTaxi { .. }
            | PilotIntent::RequestTaxiToStand { .. }
            | PilotIntent::RequestTaxiHelp
            | PilotIntent::RequestStartUp { .. } => self.redirect(i, Role::Ground),
            PilotIntent::RequestClearance { .. } => {
                let to = if self.delivery_open_now() {
                    Role::Delivery
                } else {
                    Role::Ground
                };
                self.redirect(i, to)
            }
            PilotIntent::RequestJoin { point } | PilotIntent::ReportInbound { point } => {
                self.vfr_join(i, point.as_deref())
            }
            PilotIntent::RequestLeave { point } => self.vfr_leave(i, point.as_deref()),
            _ if msg.items.kinds.contains(&ClearanceKind::GoAround)
                && self.strips[i].phase.is_airborne() =>
            {
                self.going_around(i)
            }
            _ if self.is_point_report(i, msg) => self.vfr_at_point(i, msg),
            _ => Vec::new(),
        }
    }

    /// The first call of a flight handed in to Tower is that of an airborne IFR arrival
    /// bound here without an approach yet, at a station without Approach to take it.
    pub(super) fn is_tower_inbound_call(&self, i: usize) -> bool {
        let strip = &self.strips[i];
        self.position(Role::Approach).is_none()
            && strip.phase.is_airborne()
            && strip.plan.as_ref().is_some_and(|p| {
                p.ifr
                    && p.destination.eq_ignore_ascii_case(&self.airport.icao)
                    && p.approach.is_none()
            })
    }

    /// A Tower-kept arrival after its missed approach: "expect <approach> runway <runway>,
    /// report established" again (identified all along).
    fn approach_again(&mut self, i: usize) -> Vec<AtcItem> {
        let Some((kind, runway)) = self.arrival(i).and_then(|p| p.approach.clone()) else {
            return vec![AtcItem::Report(CircuitLeg::Final)];
        };
        let items = vec![
            AtcItem::ExpectApproach { kind, runway },
            AtcItem::ReportEstablished,
        ];
        let strip = &mut self.strips[i];
        strip.identified = true;
        strip.expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// Tower takes a handed-in IFR arrival ([`AtcEngine::is_tower_inbound_call`]) and keeps
    /// it: names itself, then the default approach to the runway in use and "report
    /// established", or the visual approach and "report final" without a published one.
    pub(super) fn handed_in_ifr_inbound(&mut self, i: usize) -> Vec<AtcItem> {
        let runway = self.strips[i].runway.clone();
        let kind = self.default_approach(&runway);
        let mut items: Vec<AtcItem> = self
            .position(Role::Tower)
            .map(|p| AtcItem::Station {
                station: p.callsign.clone(),
            })
            .into_iter()
            .collect();
        match kind {
            Some(kind) => items.extend([
                AtcItem::ExpectApproach {
                    kind,
                    runway: runway.clone(),
                },
                AtcItem::ReportEstablished,
            ]),
            None => items.extend([
                AtcItem::ExpectVisualApproach {
                    runway: runway.clone(),
                },
                AtcItem::Report(CircuitLeg::Final),
            ]),
        }
        let strip = &mut self.strips[i];
        if let (Some(kind), Some(plan)) = (kind, strip.plan.as_mut()) {
            plan.approach = Some((kind, runway));
        }
        strip.owner = Role::Tower;
        strip.identified = true;
        strip.tower_inbound = true;
        strip.expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// The pilot reports going around on his own ("going around"): an IFR arrival has missed
    /// the approach and goes back to Approach at once (contact Approach), or, kept by Tower
    /// (`tower_inbound`), climbs to the published missed-approach altitude; any other
    /// aircraft is acknowledged ("roger"). With published missed-approach legs Tower says
    /// "follow published missed approach" (`FollowMissedApproach`); otherwise the flight
    /// climbs to its cleared level as before.
    fn going_around(&mut self, i: usize) -> Vec<AtcItem> {
        if self.arrival(i).is_none() {
            return vec![AtcItem::Roger];
        }
        let follow = self
            .arrival(i)
            .and_then(|p| p.approach.clone())
            .is_some_and(|(kind, runway)| self.published_missed_ft(&runway, kind).is_some());
        self.set_missed_approach(i);
        if !self.strips[i].tower_inbound {
            let mut items = Vec::new();
            if follow {
                items.push(AtcItem::FollowMissedApproach);
            }
            items.extend(self.hand_over(i, Role::Approach));
            return items;
        }
        if follow {
            let items = vec![AtcItem::FollowMissedApproach];
            self.strips[i].expect_readback(&items, self.cfg.strictness, self.now);
            return items;
        }
        let Some(ft) = self.strips[i].plan.as_ref().map(|p| p.cleared_ft) else {
            return vec![AtcItem::Roger];
        };
        let items = vec![AtcItem::ClimbTo(self.level(ft))];
        self.strips[i].expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// AIP AD 2.20 §2.2: the pilot omits the initial call after the transfer from Ground, so
    /// Tower calls the aircraft with the departure once it is `HoldingShort`, owned by Tower
    /// and tuned to the Tower frequency (COM1 of its latest sample), and has not been given
    /// the departure yet.
    pub(super) fn tower_call(&mut self, i: usize) -> Vec<AtcOutput> {
        let strip = &self.strips[i];
        if strip.owner != Role::Tower
            || strip.phase != FlightPhase::HoldingShort
            || strip.departure_sent
            || strip.handoff_to.is_some()
            || strip.pending_readback.is_some()
        {
            return Vec::new();
        }
        let strip = &self.strips[i];
        let tuned = self.position(Role::Tower).is_some_and(|pos| {
            self.tracks
                .get(&strip.callsign)
                .is_some_and(|sv| atc_proto::same_channel(sv.last.com1_khz, pos.khz))
        });
        if !tuned || self.wind().is_none() {
            return Vec::new();
        }
        let items = self.departure(i);
        self.transmit(i, Role::Tower, items)
    }

    /// The pilot calls ready after Tower already cleared the aircraft or asked for the
    /// transponder: the departure is never transmitted twice.
    pub(super) fn repeated_ready(&self, i: usize, intent: &PilotIntent) -> bool {
        self.ready_call(i, intent) && self.departure_in_hand(i)
    }

    /// "Ready for departure", or a departure runway request taxiing, at the holding point or
    /// on the runway ("request departure runway two seven"): every call gets an answer, and
    /// to Tower there it can only mean the departure.
    fn ready_call(&self, i: usize, intent: &PilotIntent) -> bool {
        match intent {
            PilotIntent::ReadyForDeparture { .. } => true,
            PilotIntent::RequestDepartureRunway { .. } => matches!(
                self.strips[i].phase,
                FlightPhase::TaxiOut | FlightPhase::HoldingShort | FlightPhase::OnRunway
            ),
            _ => false,
        }
    }

    /// Tower has already issued a take-off clearance, or asked for the transponder, in this
    /// departure cycle.
    fn departure_in_hand(&self, i: usize) -> bool {
        let strip = &self.strips[i];
        strip.departure_sent && (strip.permissions.may_take_off || strip.transponder_check)
    }

    /// Take-off clearance at the holding point or on the runway when the runway is free,
    /// otherwise hold position. Still taxiing: continue to the holding point first. A
    /// transponder not set as assigned is put right first, with no clearance: the pilot's
    /// acknowledgement (a readback of the code for "check squawk") runs this again.
    pub(super) fn departure(&mut self, i: usize) -> Vec<AtcItem> {
        match self.strips[i].phase {
            FlightPhase::HoldingShort | FlightPhase::OnRunway => {}
            FlightPhase::TaxiOut => {
                return vec![AtcItem::TaxiAndReportReady {
                    runway: self.strips[i].runway.clone(),
                }];
            }
            _ => return Vec::new(),
        }
        let check = self.transponder_check(i);
        self.strips[i].transponder_check = !check.is_empty();
        if !check.is_empty() {
            self.strips[i].departure_sent = true;
            self.strips[i].expect_readback(&check, self.cfg.strictness, self.now);
            return check;
        }
        let Some(wind) = self.wind() else {
            return standby();
        };
        let strip = &mut self.strips[i];
        strip.departure_sent = true;
        // `LineUp` has the same blockers as `Takeoff`, so "line up and wait" is never
        // possible when take-off is not: hold position.
        let items = match self.runway.occupy(&strip.callsign, RunwayUse::Takeoff) {
            Ok(()) => {
                strip.permissions.may_enter_runway = true;
                strip.permissions.may_take_off = true;
                let mut items = vec![AtcItem::ClearedTakeoff {
                    runway: strip.runway.clone(),
                    wind,
                }];
                // A flight with a plan leaves; only circuit traffic reports downwind.
                if strip.plan.is_none() {
                    items.push(AtcItem::Report(CircuitLeg::Downwind));
                }
                items
            }
            Err(_) => vec![AtcItem::HoldPosition],
        };
        strip.expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// "check squawk" when the transponder shows another code than the one assigned,
    /// "squawk Charlie" when it is off or on standby. Nothing when the aircraft has no code
    /// assigned or the code or mode is unknown.
    fn transponder_check(&self, i: usize) -> Vec<AtcItem> {
        let strip = &self.strips[i];
        let (Some(assigned), Some(sv)) = (strip.squawk, self.tracks.get(&strip.callsign)) else {
            return Vec::new();
        };
        let mut items = Vec::new();
        if sv.last.xpdr_code.is_some_and(|c| c != assigned) {
            items.push(AtcItem::CheckSquawk(assigned));
        }
        if sv.last.xpdr_mode.is_some_and(TransponderMode::is_silent) {
            items.push(AtcItem::SquawkCharlie);
        }
        items
    }

    /// On final: cleared to land after a full-stop request (always for an IFR arrival), touch
    /// and go otherwise; go around when the runway is not available. An IFR arrival on a
    /// published approach not on the final yet is told where it is
    /// ([`AtcEngine::not_on_final`]); one on a visual approach is asked to report final.
    fn landing(&mut self, i: usize) -> Vec<AtcItem> {
        // A Tower-kept IFR arrival is one also on a visual approach.
        let published = self.arrival(i).is_some();
        let arrival = published || self.strips[i].tower_inbound;
        if !matches!(self.strips[i].phase, FlightPhase::Circuit(_)) {
            let airborne = self.strips[i].phase == FlightPhase::Airborne;
            return match self.not_on_final(i) {
                Some(items) if published && airborne => items,
                _ if arrival && airborne => vec![AtcItem::Report(CircuitLeg::Final)],
                _ => Vec::new(),
            };
        }
        let Some(wind) = self.wind() else {
            return standby();
        };
        let follow = self
            .arrival(i)
            .and_then(|p| p.approach.clone())
            .is_some_and(|(kind, runway)| self.published_missed_ft(&runway, kind).is_some());
        let strip = &mut self.strips[i];
        let mut items = match self.runway.occupy(&strip.callsign, RunwayUse::Land) {
            Ok(()) => {
                strip.permissions.may_land = true;
                let runway = strip.runway.clone();
                vec![if strip.full_stop_requested || arrival {
                    AtcItem::ClearedLand { runway, wind }
                } else {
                    AtcItem::ClearedTouchAndGo { runway, wind }
                }]
            }
            Err(_) => vec![AtcItem::GoAround],
        };
        // An IFR arrival sent around flies its published missed approach.
        let sent_around = published && items == [AtcItem::GoAround];
        if sent_around && follow {
            items.push(AtcItem::FollowMissedApproach);
        }
        strip.expect_readback(&items, self.cfg.strictness, self.now);
        // An IFR arrival sent around goes back to Approach.
        if sent_around {
            self.set_missed_approach(i);
        }
        items
    }

    /// An IFR arrival not on the final yet, from its last sample: on the extended centreline
    /// (within the final's half-width) "continue approach, <N> miles from touchdown"; off it
    /// "you are <N> miles <side> of the final approach course", `side` the direction of the
    /// aircraft from the course. `None` without a sample or runway geometry.
    fn not_on_final(&self, i: usize) -> Option<Vec<AtcItem>> {
        let strip = &self.strips[i];
        let pos = self.tracks.get(&strip.callsign)?.last.pos;
        let (thr, course) = self.final_geometry(&strip.runway)?;
        // Across the extended centreline, positive to the right of the landing course.
        let rel = (bearing_deg(thr, pos) - course).to_radians();
        let right = distance_m(thr, pos) * rel.sin();
        if right.abs() < FINAL_HALF_WIDTH_M {
            return Some(vec![
                AtcItem::ContinueApproach,
                AtcItem::DistanceToGo(distance_to_go_nm(pos, thr)),
            ]);
        }
        let side = Compass8::from_bearing(course + 90.0_f64.copysign(right));
        Some(vec![AtcItem::OffFinal {
            nm: (right.abs() / M_PER_NM).round() as u16,
            side,
        }])
    }

    /// Tower clears an IFR arrival it works to land unprompted on the final: armed when the
    /// aircraft turns onto the final (`prev` is the phase before this sample) without a
    /// landing clearance, issued once the runway is free and the aircraft is on the Tower
    /// frequency, while it stays on the final. Never a "go around": a busy runway only
    /// delays it; one transmission in all.
    pub(super) fn landing_unprompted(&mut self, i: usize, prev: FlightPhase) -> Vec<AtcOutput> {
        let on_final = FlightPhase::Circuit(CircuitLeg::Final);
        let arrival = self.arrival(i).is_some();
        let strip = &mut self.strips[i];
        if strip.phase != on_final {
            strip.land_unprompted = false;
            return Vec::new();
        }
        if prev != on_final && arrival && strip.owner == Role::Tower {
            strip.land_unprompted = true;
        }
        if !strip.land_unprompted
            || strip.owner != Role::Tower
            || strip.permissions.may_land
            || strip.missed_approach
            || strip.handoff_to.is_some()
        {
            return Vec::new();
        }
        let strip = &self.strips[i];
        let tuned = self.position(Role::Tower).is_some_and(|pos| {
            self.tracks
                .get(&strip.callsign)
                .is_some_and(|sv| atc_proto::same_channel(sv.last.com1_khz, pos.khz))
        });
        if !tuned
            || self.wind().is_none()
            || self
                .runway
                .can_clear(&strip.callsign, RunwayUse::Land)
                .is_err()
        {
            return Vec::new();
        }
        self.strips[i].land_unprompted = false;
        let items = self.landing(i);
        self.transmit(i, Role::Tower, items)
    }
}
