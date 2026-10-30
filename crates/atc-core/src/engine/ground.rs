//! Ground position: taxi clearances and runway crossings.

use atc_geo::{LatLon, distance_m};
use atc_navdata::model::EdgeKind;
use atc_phraseology::{AtcItem, FlightRules, PilotIntent, PilotMessage, TaxiTo, UnableReason};

use super::{AtcEngine, AtcOutput, standby};
use crate::engine::stand::ON_STAND_M;
use crate::phase::FlightPhase;
use crate::runway::RunwayUse;
use crate::strip::{Role, RouteCrossing};
use crate::taxi::{RouteError, TaxiRoute, TaxiTarget};

/// `CrossRunway` is issued once the aircraft is this close to the node before the runway.
const CROSSING_CALL_M: f64 = 50.0;
/// The crossing is complete within this distance of the first node beyond the runway.
const CROSSED_M: f64 = 30.0;
/// Taxi help to a stand closer than this says where the stand is.
const STAND_HINT_M: f64 = 500.0;

impl AtcEngine {
    /// `t` is the sim time. An IFR flight without its clearance asking for start-up or taxi
    /// is sent to Delivery (while Delivery is closed: given the clearance first).
    pub(super) fn ground_request(&mut self, i: usize, msg: &PilotMessage, t: f64) -> Vec<AtcItem> {
        if matches!(msg.intent, PilotIntent::RequestStartUp { .. }) {
            self.park_after_flight(i);
        }
        let wants_out = matches!(
            msg.intent,
            PilotIntent::RequestTaxi { .. } | PilotIntent::RequestStartUp { .. }
        );
        if wants_out && self.needs_ifr_clearance(i, &msg.raw) {
            let level = msg.items.altitudes_ft.first().copied();
            let via = msg.items.points.first().map(String::as_str);
            return self.clearance_elsewhere(i, msg.items.destination.as_deref(), true, level, via);
        }
        match &msg.intent {
            PilotIntent::RequestClearance {
                destination,
                rules,
                level_ft,
            } => {
                let via = msg.items.points.first().map(String::as_str);
                match rules {
                    None => vec![AtcItem::SayFlightRules],
                    Some(rules) => self.clearance_elsewhere(
                        i,
                        destination.as_deref(),
                        *rules == FlightRules::Ifr,
                        *level_ft,
                        via,
                    ),
                }
            }
            PilotIntent::RequestDepartureRunway { runway }
                if self.runway_request_is_ready(i, runway) =>
            {
                self.redirect(i, Role::Tower)
            }
            PilotIntent::RequestDepartureRunway { runway } => {
                self.request_departure_runway(i, runway)
            }
            PilotIntent::RequestStartUp { push } => self.start_up(i, *push),
            PilotIntent::RequestTaxi { atis, .. } => {
                self.taxi_to_holding_point(i, *atis, Vec::new())
            }
            PilotIntent::RequestTaxiToStand { stand } => self.taxi_to_stand(i, stand.clone()),
            PilotIntent::RequestTaxiHelp => self.taxi_help(i, t),
            PilotIntent::RequestFollowMe => self.follow_me(i, t),
            PilotIntent::ReportOnStand => self.report_on_stand(i, msg.farewell),
            PilotIntent::ReadyForDeparture { .. }
            | PilotIntent::ReportLeg(_)
            | PilotIntent::RequestFullStop => self.redirect(i, Role::Tower),
            _ => Vec::new(),
        }
    }

    /// Start-up (with push-back when asked) approved and the QNH, for a parked aircraft.
    /// An aircraft stopped after a flight away from every stand (on the apron) that asks for
    /// start-up begins its next flight where it is: parked, the last flight ended.
    fn park_after_flight(&mut self, i: usize) {
        if self.strips[i].phase != FlightPhase::TaxiIn {
            return;
        }
        let Some(sv) = self.tracks.get_mut(&self.strips[i].callsign) else {
            return;
        };
        if sv.last.gs_kt >= crate::phase::STOPPED_KT {
            return;
        }
        sv.tracker.begin_new_cycle();
        let strip = &mut self.strips[i];
        strip.phase = FlightPhase::Parked;
        super::end_flight_keeping_contact(strip);
    }

    fn start_up(&mut self, i: usize, push: bool) -> Vec<AtcItem> {
        if self.strips[i].phase != FlightPhase::Parked {
            return Vec::new();
        }
        let Some(qnh) = self.atis.as_ref().map(|a| a.current().weather.qnh_hpa) else {
            return standby();
        };
        let approved = if push {
            AtcItem::PushAndStartApproved { facing: None }
        } else {
            AtcItem::StartUpApproved
        };
        let items = vec![approved, AtcItem::Qnh(qnh)];
        self.strips[i].qnh_with_start_up = Some(qnh);
        self.strips[i].expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// Taxi to the holding point of the runway in use, the transponder code last. A runway
    /// change since the flight's clearance is announced first (with the new SID). A missing
    /// or stale ATIS letter gets the current letter and QNH first (QNH is then not repeated
    /// after the route). The QNH given with the start-up and the code given with the flight's
    /// clearance are not repeated. `prefix` opens the transmission (and its readback).
    fn taxi_to_holding_point(
        &mut self,
        i: usize,
        heard_atis: Option<char>,
        prefix: Vec<AtcItem>,
    ) -> Vec<AtcItem> {
        if !matches!(
            self.strips[i].phase,
            FlightPhase::Parked | FlightPhase::TaxiOut | FlightPhase::TaxiIn
        ) {
            return Vec::new();
        }
        let amended = if matches!(
            self.strips[i].phase,
            FlightPhase::Parked | FlightPhase::TaxiOut
        ) {
            self.refresh_runway(i)
        } else {
            None
        };
        let (Some(atis), Some(pos)) = (
            self.atis.as_ref().map(|a| a.current().clone()),
            self.last_pos(i),
        ) else {
            return standby();
        };
        let runway = self.strips[i].runway.clone();
        let target = TaxiTarget::HoldingPoint {
            runway: runway.clone(),
        };
        let route = match self.graph.route(&self.airport, pos, &target) {
            Ok(r) => r,
            Err(e) => return vec![unable(&e)],
        };
        self.new_taxi_clearance(i, target);
        let stale = heard_atis.map(|c| c.to_ascii_uppercase()) != Some(atis.letter);
        let qnh = AtcItem::Qnh(atis.weather.qnh_hpa);
        let mut items = prefix;
        if let Some((change, plan)) = amended {
            items.extend(change);
            self.strips[i].plan = Some(plan);
        }
        if stale {
            items.push(AtcItem::AtisCurrent(atis.letter));
            items.push(qnh.clone());
        }
        items.push(self.taxi_item(
            i,
            TaxiTo::HoldingPoint {
                runway: runway.clone(),
            },
            &route,
        ));
        items.push(AtcItem::HoldShort(runway));
        // Not repeated: the QNH given with this departure's start-up, the code given with its
        // clearance.
        let strip = &self.strips[i];
        let qnh_given = strip.qnh_with_start_up == Some(atis.weather.qnh_hpa);
        let code_given = strip.code_with_clearance;
        if !stale && !qnh_given {
            items.push(qnh);
        }
        if !code_given && let Some(code) = self.squawk(i) {
            items.push(AtcItem::Squawk(code));
        }
        self.strips[i].expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// A taxiing departure was given another departure `runway`: `prefix` ("runway ...
    /// approved") with the runway change and a new taxi clearance to its holding point, one
    /// readback. When there is no route to that holding point (or no position or ATIS) the
    /// runway is not given: "unable" or "standby".
    pub(super) fn retaxi_to_runway(
        &mut self,
        i: usize,
        runway: &str,
        prefix: Vec<AtcItem>,
    ) -> Vec<AtcItem> {
        let (Some(letter), Some(pos)) = (
            self.atis.as_ref().map(|a| a.current().letter),
            self.last_pos(i),
        ) else {
            return standby();
        };
        let target = TaxiTarget::HoldingPoint {
            runway: runway.to_string(),
        };
        if let Err(e) = self.graph.route(&self.airport, pos, &target) {
            return vec![unable(&e)];
        }
        self.strips[i].requested_runway = Some(runway.to_string());
        self.taxi_to_holding_point(i, Some(letter), prefix)
    }

    /// Taxi to the requested stand, or back to the stand the aircraft started from.
    fn taxi_to_stand(&mut self, i: usize, stand: Option<String>) -> Vec<AtcItem> {
        let phase = self.strips[i].phase;
        if phase.is_airborne() || matches!(phase, FlightPhase::OnRunway | FlightPhase::TakeoffRoll)
        {
            return Vec::new();
        }
        let Some(pos) = self.last_pos(i) else {
            return standby();
        };
        // A heard stand keeps the navdata spelling when it matches one.
        let heard = stand.map(|s| {
            self.airport
                .stands
                .iter()
                .find(|st| st.name.eq_ignore_ascii_case(&s))
                .map_or(s, |st| st.name.clone())
        });
        let Some(name) = heard.or_else(|| self.strips[i].home_stand.clone()) else {
            return vec![AtcItem::SayStand];
        };
        let target = TaxiTarget::Stand { name: name.clone() };
        let route = match self.graph.route(&self.airport, pos, &target) {
            Ok(r) => r,
            Err(e) => return vec![unable(&e)],
        };
        self.new_taxi_clearance(i, target);
        let items = vec![self.taxi_item(i, TaxiTo::Stand { name: name.clone() }, &route)];
        self.strips[i].expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// A taxi clearance to `target`: help requests count from here, a follow-me car
    /// leading the aircraft goes away.
    fn new_taxi_clearance(&mut self, i: usize, target: TaxiTarget) {
        self.cancel_follow_me(i);
        let strip = &mut self.strips[i];
        strip.taxi_target = Some(target);
        strip.taxi_helps = 0;
    }

    /// Taxi help ("request progressive taxi", "lost"): the route again from where the
    /// aircraft is to its taxi target, worded as the clearance (without QNH and squawk) and
    /// read back the same; to a stand within [`STAND_HINT_M`], where the stand is. The
    /// second request since the clearance (or any with a car already sent) gets the
    /// follow-me car. No target: "say intentions".
    fn taxi_help(&mut self, i: usize, t: f64) -> Vec<AtcItem> {
        let Some(target) = self.strips[i].taxi_target.clone() else {
            return vec![AtcItem::SayIntentions];
        };
        let strip = &mut self.strips[i];
        strip.taxi_helps += 1;
        if strip.taxi_helps >= 2 || strip.follow_me.is_some() {
            return self.follow_me(i, t);
        }
        self.taxi_instruction(i, target)
    }

    /// The taxi clearance's route again from where the aircraft is (see [`Self::taxi_help`]);
    /// also the answer when a follow-me car is not worth sending.
    pub(super) fn taxi_instruction(&mut self, i: usize, target: TaxiTarget) -> Vec<AtcItem> {
        let Some(pos) = self.last_pos(i) else {
            return standby();
        };
        let route = match self.graph.route(&self.airport, pos, &target) {
            Ok(r) => r,
            Err(e) => return vec![unable(&e)],
        };
        let items = match target {
            TaxiTarget::HoldingPoint { runway } => vec![
                self.taxi_item(
                    i,
                    TaxiTo::HoldingPoint {
                        runway: runway.clone(),
                    },
                    &route,
                ),
                AtcItem::HoldShort(runway),
            ],
            TaxiTarget::Stand { name } => {
                let mut items =
                    vec![self.taxi_item(i, TaxiTo::Stand { name: name.clone() }, &route)];
                items.extend(
                    self.stand_hint_at(i, &name)
                        .filter(|(d, _)| *d <= STAND_HINT_M && *d > ON_STAND_M)
                        .map(|(_, hint)| hint),
                );
                items
            }
        };
        self.strips[i].expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// `Taxi` item for `route`; its runway crossings are recorded on the strip to be
    /// cleared on approach.
    fn taxi_item(&mut self, i: usize, to: TaxiTo, route: &TaxiRoute) -> AtcItem {
        let in_use = &self.strips[i].runway;
        let labels: Vec<String> = route
            .crossings
            .iter()
            .map(|label| spoken_runway(label, in_use))
            .collect();
        let crossings = self
            .crossing_points(&route.nodes)
            .into_iter()
            .zip(&labels)
            .map(|((entry, exit), runway)| RouteCrossing {
                runway: runway.clone(),
                entry,
                exit,
                cleared: false,
            })
            .collect();
        self.strips[i].crossings = crossings;
        AtcItem::Taxi {
            to,
            via: route.via.clone(),
            crossings: labels,
        }
    }

    /// For each runway step of a route (consecutive nodes joined by a runway edge, or not
    /// joined at all: a crossing skips the runway-interior nodes), the node before the
    /// runway and the first node beyond it.
    fn crossing_points(&self, nodes: &[u32]) -> Vec<(LatLon, LatLon)> {
        let is_runway_step = |a: u32, b: u32| {
            self.graph.edge_len(a, b).is_none()
                || self.airport.taxi.edges.iter().any(|e| {
                    e.kind == EdgeKind::Runway
                        && ((e.from, e.to) == (a, b) || (e.from, e.to) == (b, a))
                })
        };
        let last = nodes.len().saturating_sub(1);
        (0..last)
            .filter(|&k| is_runway_step(nodes[k], nodes[k + 1]))
            .filter_map(|k| {
                let entry = self.graph.node_pos(nodes[k.saturating_sub(1)])?;
                let exit = self.graph.node_pos(nodes[(k + 2).min(last)])?;
                Some((entry, exit))
            })
            .collect()
    }

    /// Clears the next crossing when the aircraft approaches it and the runway is free;
    /// ends the runway permission once across.
    pub(super) fn crossing_update(&mut self, i: usize, pos: LatLon) -> Vec<AtcOutput> {
        let strip = &mut self.strips[i];
        let Some(c) = strip.crossings.first_mut() else {
            return Vec::new();
        };
        if c.cleared {
            if distance_m(pos, c.exit) < CROSSED_M {
                strip.crossings.remove(0);
                strip.permissions.may_enter_runway = false;
                self.runway.release(&strip.callsign);
            }
            return Vec::new();
        }
        if strip.pending_readback.is_some()
            || distance_m(pos, c.entry) > CROSSING_CALL_M
            || self
                .runway
                .occupy(&strip.callsign, RunwayUse::Cross)
                .is_err()
        {
            return Vec::new();
        }
        c.cleared = true;
        let items = vec![AtcItem::CrossRunway(c.runway.clone())];
        strip.permissions.may_enter_runway = true;
        strip.expect_readback(&items, self.cfg.strictness, self.now);
        self.transmit(i, Role::Ground, items)
    }
}

/// A runway edge label (`"07/25"`) spoken as the runway-in-use ident when it names it.
fn spoken_runway(label: &str, in_use: &str) -> String {
    if label.split('/').any(|r| r == in_use) {
        in_use.to_string()
    } else {
        label.to_string()
    }
}

pub(super) fn unable(e: &RouteError) -> AtcItem {
    AtcItem::Unable(match e {
        RouteError::UnknownStand(s) => UnableReason::UnknownStand { stand: s.clone() },
        RouteError::UnknownRunway(_) | RouteError::Unreachable => UnableReason::NoRoute,
    })
}
