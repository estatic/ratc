//! Tower position: departure and circuit clearances, and the landing of IFR arrivals
//! transferred by Approach (a missed approach goes back to Approach).

use atc_phraseology::{AtcItem, CircuitLeg, ClearanceKind, PilotIntent, PilotMessage};

use super::{AtcEngine, AtcOutput, standby};
use crate::phase::FlightPhase;
use crate::runway::RunwayUse;
use crate::state::TransponderMode;
use crate::strip::Role;

/// No other traffic in v1: every aircraft in the circuit is number one.
const SEQUENCE: u8 = 1;

impl AtcEngine {
    pub(super) fn tower_request(&mut self, i: usize, msg: &PilotMessage) -> Vec<AtcItem> {
        let ready = self.ready_call(i, &msg.intent);
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

    /// The pilot reports going around on his own ("going around"): an IFR arrival has missed
    /// the approach and goes back to Approach at once (contact Approach); any other aircraft
    /// is acknowledged ("roger").
    fn going_around(&mut self, i: usize) -> Vec<AtcItem> {
        if self.arrival(i).is_none() {
            return vec![AtcItem::Roger];
        }
        self.set_missed_approach(i);
        self.redirect(i, Role::Approach)
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
    /// and go otherwise; go around when the runway is not available. An IFR arrival not on
    /// the final yet is asked to report final.
    fn landing(&mut self, i: usize) -> Vec<AtcItem> {
        let arrival = self.arrival(i).is_some();
        if !matches!(self.strips[i].phase, FlightPhase::Circuit(_)) {
            return if arrival && self.strips[i].phase == FlightPhase::Airborne {
                vec![AtcItem::Report(CircuitLeg::Final)]
            } else {
                Vec::new()
            };
        }
        let Some(wind) = self.wind() else {
            return standby();
        };
        let strip = &mut self.strips[i];
        let items = match self.runway.occupy(&strip.callsign, RunwayUse::Land) {
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
        strip.expect_readback(&items, self.cfg.strictness, self.now);
        // An IFR arrival sent around goes back to Approach.
        if arrival && items == [AtcItem::GoAround] {
            self.set_missed_approach(i);
        }
        items
    }
}
