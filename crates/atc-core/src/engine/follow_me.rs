//! Ground position: the follow-me car.
//!
//! Sent on request ("request follow-me") or on the second taxi help request since the last
//! taxi clearance: "hold position, follow-me car is on the way", with a [`VehicleCommand`]
//! carrying the car's path (the taxi route from where the aircraft is, plus the last leg to
//! the stand; to a holding point it stops [`SHORT_OF_HOLD_M`] short). The plugin shows the
//! car at once, ahead of the aircraft on the path; the engine has no feedback from it and
//! says "follow the follow-me car" a fixed [`READY_S`] seconds of sim time later. The car
//! is the aircraft's until it parks or holds short (or a new taxi clearance cancels it).

use atc_geo::{LatLon, bearing_deg, destination, distance_m};
use atc_phraseology::{AtcItem, Callsign};

use super::ground::unable;
use super::{AtcEngine, AtcOutput, standby};
use crate::phase::FlightPhase;
use crate::strip::{FollowMe, Role};
use crate::taxi::TaxiTarget;

/// "Follow the follow-me car" this long after "on the way".
pub const READY_S: f64 = 12.0;
/// To a holding point the car's path ends this far before it.
const SHORT_OF_HOLD_M: f64 = 20.0;

/// An order for the simulator's ground vehicles, for one aircraft's client.
#[derive(Clone, Debug, PartialEq)]
pub enum VehicleCommand {
    /// Show a follow-me car leading along `path` (first point near the aircraft); it ends
    /// facing `end_heading_deg` (true).
    FollowMe {
        id: u32,
        path: Vec<LatLon>,
        end_heading_deg: f64,
    },
    /// Remove follow-me car `id`.
    FollowMeCancel { id: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct VehicleOrder {
    pub callsign: Callsign,
    pub command: VehicleCommand,
}

fn on_the_way() -> Vec<AtcItem> {
    vec![AtcItem::HoldPosition, AtcItem::FollowMeOnTheWay]
}

impl AtcEngine {
    /// Sends a follow-me car to aircraft `i` at sim time `t` (see the module docs). One
    /// already sent is not sent again: the call it is at is repeated.
    pub(super) fn follow_me(&mut self, i: usize, t: f64) -> Vec<AtcItem> {
        if let Some(fm) = &self.strips[i].follow_me {
            return if fm.leading {
                vec![AtcItem::FollowTheFollowMe]
            } else {
                on_the_way()
            };
        }
        let strip = &self.strips[i];
        let target = strip.taxi_target.clone().or_else(|| {
            let home = strip
                .home_stand
                .clone()
                .filter(|_| matches!(strip.phase, FlightPhase::TaxiIn | FlightPhase::Landed))?;
            Some(TaxiTarget::Stand { name: home })
        });
        let Some(target) = target else {
            return vec![AtcItem::SayIntentions];
        };
        let Some(pos) = self.last_pos(i) else {
            return standby();
        };
        let route = match self.graph.route(&self.airport, pos, &target) {
            Ok(r) => r,
            Err(e) => return vec![unable(&e)],
        };
        let mut path: Vec<LatLon> = route
            .nodes
            .iter()
            .filter_map(|&n| self.graph.node_pos(n))
            .collect();
        let end_heading_deg = match &target {
            TaxiTarget::Stand { name } => {
                let Some(st) = self.airport.stands.iter().find(|s| &s.name == name) else {
                    return vec![AtcItem::SayIntentions];
                };
                path.push(st.pos);
                st.heading_deg
            }
            TaxiTarget::HoldingPoint { .. } => {
                let heading = match path[..] {
                    [.., a, b] => bearing_deg(a, b),
                    _ => self.tracks[&self.strips[i].callsign].last.heading_deg,
                };
                trim_end(&mut path, SHORT_OF_HOLD_M);
                heading
            }
        };
        self.next_vehicle += 1;
        let id = self.next_vehicle;
        let strip = &mut self.strips[i];
        strip.taxi_target = Some(target);
        strip.follow_me = Some(FollowMe {
            id,
            sent_s: t,
            leading: false,
        });
        self.vehicle_orders.push(VehicleOrder {
            callsign: strip.callsign.clone(),
            command: VehicleCommand::FollowMe {
                id,
                path,
                end_heading_deg,
            },
        });
        let items = on_the_way();
        strip.expect_readback(&items, self.cfg.strictness);
        items
    }

    /// "Follow the follow-me car" once the car has had [`READY_S`] to get in front.
    pub(super) fn follow_me_update(&mut self, i: usize, t: f64) -> Vec<AtcOutput> {
        let strip = &mut self.strips[i];
        match &mut strip.follow_me {
            Some(fm) if !fm.leading && t - fm.sent_s >= READY_S => fm.leading = true,
            _ => return Vec::new(),
        }
        // Needs no readback; it replaces the open "hold position".
        let items = vec![AtcItem::FollowTheFollowMe];
        strip.expect_readback(&items, self.cfg.strictness);
        self.transmit(i, Role::Ground, items)
    }

    /// A new taxi clearance: the car leading aircraft `i`, if any, goes away.
    pub(super) fn cancel_follow_me(&mut self, i: usize) {
        let strip = &mut self.strips[i];
        if let Some(fm) = strip.follow_me.take() {
            self.vehicle_orders.push(VehicleOrder {
                callsign: strip.callsign.clone(),
                command: VehicleCommand::FollowMeCancel { id: fm.id },
            });
        }
    }

    /// Vehicle orders issued since the last call, oldest first.
    pub fn take_vehicle_orders(&mut self) -> Vec<VehicleOrder> {
        std::mem::take(&mut self.vehicle_orders)
    }
}

/// Shortens the polyline by `m` metres at its end (keeping at least its first point).
fn trim_end(path: &mut Vec<LatLon>, mut m: f64) {
    while let [.., a, b] = path[..] {
        let len = distance_m(a, b);
        if len > m {
            *path.last_mut().expect("two points") = destination(b, bearing_deg(b, a), m);
            return;
        }
        m -= len;
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_shortens_the_end() {
        let a = LatLon {
            lat: 50.0,
            lon: 19.0,
        };
        let b = destination(a, 90.0, 100.0);
        let c = destination(b, 0.0, 10.0);
        let mut path = vec![a, b, c];
        trim_end(&mut path, 30.0);
        assert_eq!(path.len(), 2);
        assert!((distance_m(a, path[1]) - 80.0).abs() < 0.1, "{path:?}");
        let mut short = vec![a, b];
        trim_end(&mut short, 500.0);
        assert_eq!(short, vec![a]);
    }
}
