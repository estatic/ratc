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
use atc_phraseology::{AtcItem, Callsign, TaxiTo};

use super::ground::unable;
use super::{AtcEngine, AtcOutput, standby};
use crate::phase::FlightPhase;
use crate::strip::{FollowMe, Role};
use crate::taxi::TaxiTarget;

/// "Follow the follow-me car" this long after "on the way".
pub const READY_S: f64 = 12.0;
/// To a holding point the car's path ends this far before it.
const SHORT_OF_HOLD_M: f64 = 20.0;
/// Radius of the arcs that round the path's corners.
const CORNER_RADIUS_M: f64 = 25.0;
/// Corners turning less than this stay sharp.
const MIN_ROUNDED_TURN_DEG: f64 = 10.0;
/// Spacing of the points along a rounded corner.
const ARC_STEP_M: f64 = 2.0;
/// A route shorter than this is not worth a car: plain taxi instructions are given.
const MIN_ROUTE_M: f64 = 150.0;
/// An aircraft this close to the end of the car's path needs no car to lead it.
const ALREADY_THERE_M: f64 = 30.0;

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

/// "Follow the follow-me car to <where the taxi target is>".
fn follow_items(target: &TaxiTarget) -> Vec<AtcItem> {
    let to = match target {
        TaxiTarget::Stand { name } => TaxiTo::Stand { name: name.clone() },
        TaxiTarget::HoldingPoint { runway } => TaxiTo::HoldingPoint {
            runway: runway.clone(),
        },
    };
    vec![AtcItem::FollowTheFollowMe { to }]
}

impl AtcEngine {
    /// Sends a follow-me car to aircraft `i` at sim time `t` (see the module docs). One
    /// already sent is not sent again: the call it is at is repeated.
    pub(super) fn follow_me(&mut self, i: usize, t: f64) -> Vec<AtcItem> {
        if let Some(fm) = &self.strips[i].follow_me {
            return if fm.leading {
                self.strips[i]
                    .taxi_target
                    .as_ref()
                    .map_or_else(|| vec![AtcItem::SayIntentions], follow_items)
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
            TaxiTarget::HoldingPoint { .. } => match path[..] {
                [.., a, b] => bearing_deg(a, b),
                _ => self.tracks[&self.strips[i].callsign].last.heading_deg,
            },
        };
        let route_m = path.first().map_or(0.0, |&first| distance_m(pos, first))
            + path.windows(2).map(|w| distance_m(w[0], w[1])).sum::<f64>();
        if route_m < MIN_ROUTE_M {
            return self.taxi_instruction(i, target);
        }
        let mut path = smooth_corners(&path, CORNER_RADIUS_M);
        if matches!(target, TaxiTarget::HoldingPoint { .. }) {
            trim_end(&mut path, SHORT_OF_HOLD_M);
        }
        let end = path.last().copied();
        self.next_vehicle += 1;
        let id = self.next_vehicle;
        let strip = &mut self.strips[i];
        strip.taxi_target = Some(target);
        strip.follow_me = Some(FollowMe {
            id,
            sent_s: t,
            leading: false,
            end,
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
        strip.expect_readback(&items, self.cfg.strictness, self.now);
        items
    }

    /// "Follow the follow-me car" once the car has had [`READY_S`] to get in front.
    /// An aircraft already at the end of the car's path (it taxied there on its own while
    /// the car was on the way) gets no call: the car goes away silently.
    pub(super) fn follow_me_update(&mut self, i: usize, t: f64) -> Vec<AtcOutput> {
        let pos = self.last_pos(i);
        let Some(fm) = &self.strips[i].follow_me else {
            return Vec::new();
        };
        if fm.leading || t - fm.sent_s < READY_S {
            return Vec::new();
        }
        if matches!((pos, fm.end), (Some(p), Some(e)) if distance_m(p, e) <= ALREADY_THERE_M) {
            self.cancel_follow_me(i);
            return Vec::new();
        }
        let strip = &mut self.strips[i];
        if let Some(fm) = &mut strip.follow_me {
            fm.leading = true;
        }
        // Needs no readback; it replaces the open "hold position".
        let Some(target) = &strip.taxi_target else {
            return Vec::new();
        };
        let items = follow_items(target);
        strip.expect_readback(&items, self.cfg.strictness, self.now);
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

/// Rounds the corners of the polyline with arcs of `radius_m`. An arc never uses more than
/// half of either adjacent segment (its radius shrinks to fit); corners turning less than
/// [`MIN_ROUNDED_TURN_DEG`] stay as they are, and so do the end points. Arcs are sampled
/// every [`ARC_STEP_M`].
fn smooth_corners(path: &[LatLon], radius_m: f64) -> Vec<LatLon> {
    if path.len() < 3 {
        return path.to_vec();
    }
    let origin = path[0];
    let local: Vec<(f64, f64)> = path
        .iter()
        .map(|&p| {
            let (d, b) = (distance_m(origin, p), bearing_deg(origin, p).to_radians());
            (d * b.sin(), d * b.cos())
        })
        .collect();
    let back = |(x, y): (f64, f64)| {
        destination(
            origin,
            x.atan2(y).to_degrees().rem_euclid(360.0),
            x.hypot(y),
        )
    };
    let mut out = vec![path[0]];
    for k in 1..path.len() - 1 {
        let (p, a, b) = (local[k], local[k - 1], local[k + 1]);
        let (l1, l2) = (dist(a, p), dist(p, b));
        if l1 < 1e-6 || l2 < 1e-6 {
            out.push(path[k]);
            continue;
        }
        let u1 = ((p.0 - a.0) / l1, (p.1 - a.1) / l1);
        let u2 = ((b.0 - p.0) / l2, (b.1 - p.1) / l2);
        let cross = u1.0 * u2.1 - u1.1 * u2.0;
        let turn = cross.atan2(u1.0 * u2.0 + u1.1 * u2.1).abs();
        let tan_half = (turn / 2.0).tan();
        if turn.to_degrees() < MIN_ROUNDED_TURN_DEG || tan_half > 1e6 {
            out.push(path[k]);
            continue;
        }
        // Tangent length along each leg, and the radius it gives.
        let t = (radius_m * tan_half).min(l1.min(l2) / 2.0);
        let r = t / tan_half;
        let side = cross.signum();
        let start = (p.0 - u1.0 * t, p.1 - u1.1 * t);
        let centre = (start.0 - side * u1.1 * r, start.1 + side * u1.0 * r);
        let steps = (r * turn / ARC_STEP_M).ceil().max(1.0) as usize;
        for s in 0..=steps {
            let phi = side * turn * s as f64 / steps as f64;
            let (sn, cs) = phi.sin_cos();
            let v = (start.0 - centre.0, start.1 - centre.1);
            out.push(back((
                centre.0 + v.0 * cs - v.1 * sn,
                centre.1 + v.0 * sn + v.1 * cs,
            )));
        }
    }
    out.push(path[path.len() - 1]);
    out
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
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

    fn origin() -> LatLon {
        LatLon {
            lat: 50.0,
            lon: 19.0,
        }
    }

    /// `origin` then a point at each (bearing, distance) step in turn.
    fn polyline(steps: &[(f64, f64)]) -> Vec<LatLon> {
        let mut path = vec![origin()];
        for &(bearing, m) in steps {
            let last = *path.last().unwrap();
            path.push(destination(last, bearing, m));
        }
        path
    }

    fn turn_deg(a: f64, b: f64) -> f64 {
        ((b - a + 540.0) % 360.0 - 180.0).abs()
    }

    #[test]
    fn trim_shortens_the_end() {
        let a = origin();
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

    #[test]
    fn right_angle_becomes_an_arc() {
        let path = polyline(&[(90.0, 100.0), (0.0, 100.0)]);
        let smooth = smooth_corners(&path, 25.0);
        assert!(smooth.len() > 10, "{smooth:?}");
        let headings: Vec<f64> = smooth.windows(2).map(|w| bearing_deg(w[0], w[1])).collect();
        for w in headings.windows(2) {
            assert!(turn_deg(w[0], w[1]) < 10.0, "{smooth:?}");
        }
        // The arc cuts the corner, but stays within the radius of it.
        let corner = path[1];
        let nearest = smooth
            .iter()
            .map(|&p| distance_m(p, corner))
            .fold(f64::INFINITY, f64::min);
        assert!(nearest > 5.0 && nearest < 25.5, "{nearest}");
        // Points sit within 25 m + 0.5 of the original legs.
        for &p in &smooth {
            let near_leg = path.windows(2).any(|w| {
                let n = ((distance_m(w[0], w[1]) / 0.5) as usize).max(1);
                (0..=n).any(|k| {
                    let f = k as f64 / n as f64;
                    let q = destination(w[0], bearing_deg(w[0], w[1]), f * distance_m(w[0], w[1]));
                    distance_m(p, q) <= 25.5
                })
            });
            assert!(near_leg, "{p:?}");
        }
        // Samples are about 2 m apart.
        for w in smooth.windows(2) {
            assert!(distance_m(w[0], w[1]) <= 2.0 + 0.01 || distance_m(w[0], w[1]) > 50.0);
        }
    }

    #[test]
    fn straight_path_unchanged() {
        let path = polyline(&[(90.0, 100.0), (90.0, 50.0), (90.0, 80.0)]);
        assert_eq!(smooth_corners(&path, 25.0), path);
    }

    #[test]
    fn small_turn_unchanged() {
        let path = polyline(&[(90.0, 100.0), (98.0, 100.0)]);
        assert_eq!(smooth_corners(&path, 25.0), path);
    }

    #[test]
    fn short_segments_shrink_the_arc() {
        let path = polyline(&[(90.0, 8.0), (0.0, 8.0)]);
        let smooth = smooth_corners(&path, 25.0);
        assert!(smooth.len() > 3, "{smooth:?}");
        // The arc starts at most half of the first leg from the corner (and ends likewise).
        let corner = path[1];
        assert!(distance_m(smooth[1], corner) >= 4.0 - 0.05, "{smooth:?}");
        assert!(distance_m(smooth[smooth.len() - 2], corner) >= 4.0 - 0.05);
        // Moving monotonically along the way: no self-intersection, no doubling back.
        let along = |p: LatLon| {
            let (d, b) = (distance_m(path[0], p), bearing_deg(path[0], p));
            (d * (b - 90.0).to_radians().cos(), d * b.to_radians().cos())
        };
        let pts: Vec<(f64, f64)> = smooth.iter().map(|&p| along(p)).collect();
        for w in pts.windows(2) {
            assert!(
                w[1].0 >= w[0].0 - 0.01 && w[1].1 >= w[0].1 - 0.01,
                "{pts:?}"
            );
        }
        let len: f64 = smooth.windows(2).map(|w| distance_m(w[0], w[1])).sum();
        assert!(len < 16.0 && len > 11.0, "{len}");
    }

    #[test]
    fn endpoints_kept() {
        let path = polyline(&[(90.0, 100.0), (0.0, 100.0), (270.0, 60.0)]);
        let smooth = smooth_corners(&path, 25.0);
        assert_eq!(smooth.first(), path.first());
        assert_eq!(smooth.last(), path.last());
        assert_eq!(smooth_corners(&path[..2], 25.0), path[..2]);
    }
}
