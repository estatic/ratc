//! Taxi conflict prediction between moving aircraft, and ways to resolve a conflict.
//!
//! Model: each mover follows `route` (node ids of the [`TaxiGraph`]) at constant
//! `speed_mps`, `progress_m` metres from the route's first node. It occupies a node
//! during `[t_arrive - NODE_HALF_WINDOW_S, t_arrive + NODE_HALF_WINDOW_S]` (so two
//! aircraft reaching one node less than 15 s apart conflict) and an edge from entering
//! to leaving it. Times are seconds from now. Stationary movers (`speed_mps <= 0`) are
//! ignored, and a route is followed only as far as consecutive nodes have an edge (or,
//! across a runway crossing, a straight-line leg) of known length.

use atc_geo::distance_m;
use atc_phraseology::Callsign;

use crate::taxi::TaxiGraph;

/// Half of the 15 s node separation.
const NODE_HALF_WINDOW_S: f64 = 7.5;

#[derive(Clone, Debug, PartialEq)]
pub struct TaxiMover {
    pub callsign: Callsign,
    /// Node-id path in the taxi graph.
    pub route: Vec<u32>,
    /// Distance travelled along the path from its first node.
    pub progress_m: f64,
    pub speed_mps: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConflictKind {
    /// Opposite directions on one edge, given in `a`'s direction of travel.
    HeadOn { edge: (u32, u32) },
    /// Both occupy one node within 15 s of each other.
    Crossing { node: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaxiConflict {
    pub a: Callsign,
    pub b: Callsign,
    pub kind: ConflictKind,
    /// Seconds from now until the occupancy windows first overlap.
    pub t_s: f64,
    /// When `a` reaches the conflict point (the node, or the entry node of the edge).
    pub a_arrive_s: f64,
    /// When `b` reaches the conflict point.
    pub b_arrive_s: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Resolution {
    GiveWay { who: Callsign, to: Callsign },
    HoldPosition { who: Callsign },
    Follow { who: Callsign, leader: Callsign },
}

/// Occupancy of nodes and edges ahead of a mover, in seconds from now.
struct Timeline {
    /// (node, arrival time) for nodes not yet passed.
    nodes: Vec<(u32, f64)>,
    /// (from, to, time entered, time left) for edges not yet fully traversed.
    edges: Vec<(u32, u32, f64, f64)>,
}

fn leg_len(g: &TaxiGraph, a: u32, b: u32) -> Option<f64> {
    g.edge_len(a, b)
        .or_else(|| Some(distance_m(g.node_pos(a)?, g.node_pos(b)?)))
}

fn timeline(g: &TaxiGraph, m: &TaxiMover) -> Timeline {
    let mut t = Timeline {
        nodes: Vec::new(),
        edges: Vec::new(),
    };
    if !(m.speed_mps > 0.0 && m.speed_mps.is_finite()) {
        return t;
    }
    // Distance from the route start to each node, as far as the route is measurable.
    let mut cum = vec![0.0];
    for w in m.route.windows(2) {
        let Some(len) = leg_len(g, w[0], w[1]) else {
            break;
        };
        cum.push(cum.last().copied().unwrap_or(0.0) + len);
    }
    let at = |d: f64| (d - m.progress_m) / m.speed_mps;
    // `zip` bounds the walk by the route, so an empty route yields nothing.
    for (i, (&d, &node)) in cum.iter().zip(&m.route).enumerate() {
        if d >= m.progress_m {
            t.nodes.push((node, at(d)));
        }
        if i + 1 < cum.len() && cum[i + 1] > m.progress_m {
            t.edges
                .push((node, m.route[i + 1], at(d).max(0.0), at(cum[i + 1])));
        }
    }
    t
}

/// Predicts conflicts among `movers` within `horizon_s`. Each pair is reported at most
/// once, with its earliest conflict; a head-on on an edge supersedes node conflicts at
/// that edge's end nodes. Output is ordered by `t_s`.
pub fn predict_conflicts(g: &TaxiGraph, movers: &[TaxiMover], horizon_s: f64) -> Vec<TaxiConflict> {
    let timelines: Vec<Timeline> = movers.iter().map(|m| timeline(g, m)).collect();
    let mut out = Vec::new();
    for i in 0..movers.len() {
        for j in i + 1..movers.len() {
            let (ta, tb) = (&timelines[i], &timelines[j]);
            // (t_s, kind, a arrival, b arrival)
            let mut found: Vec<(f64, ConflictKind, f64, f64)> = Vec::new();
            for &(ax, ay, a_in, a_out) in &ta.edges {
                for &(bx, by, b_in, b_out) in &tb.edges {
                    if (ax, ay) == (by, bx) && a_in < b_out && b_in < a_out {
                        let kind = ConflictKind::HeadOn { edge: (ax, ay) };
                        found.push((a_in.max(b_in), kind, a_in, b_in));
                    }
                }
            }
            let head_on_nodes: Vec<u32> = found
                .iter()
                .filter_map(|f| match f.1 {
                    ConflictKind::HeadOn { edge } => Some(edge),
                    ConflictKind::Crossing { .. } => None,
                })
                .flat_map(|e| [e.0, e.1])
                .collect();
            for &(na, a_t) in &ta.nodes {
                for &(nb, b_t) in &tb.nodes {
                    if na == nb
                        && (a_t - b_t).abs() < 2.0 * NODE_HALF_WINDOW_S
                        && !head_on_nodes.contains(&na)
                    {
                        let t = (a_t.max(b_t) - NODE_HALF_WINDOW_S).max(0.0);
                        found.push((t, ConflictKind::Crossing { node: na }, a_t, b_t));
                    }
                }
            }
            let earliest = found
                .into_iter()
                .filter(|f| f.0 <= horizon_s)
                .min_by(|x, y| x.0.total_cmp(&y.0));
            if let Some((t_s, kind, a_arrive_s, b_arrive_s)) = earliest {
                out.push(TaxiConflict {
                    a: movers[i].callsign.clone(),
                    b: movers[j].callsign.clone(),
                    kind,
                    t_s,
                    a_arrive_s,
                    b_arrive_s,
                });
            }
        }
    }
    out.sort_by(|x, y| x.t_s.total_cmp(&y.t_s));
    out
}

fn find<'a>(movers: &'a [TaxiMover], who: &Callsign) -> Option<&'a TaxiMover> {
    movers.iter().find(|m| &m.callsign == who)
}

/// (earlier, later) at the conflict point; `b` counts as later on a tie.
fn order(c: &TaxiConflict) -> (&Callsign, &Callsign) {
    if c.a_arrive_s < c.b_arrive_s {
        (&c.a, &c.b)
    } else {
        (&c.b, &c.a)
    }
}

/// True when both movers pass the conflict node on the same path in the same direction:
/// they enter it from, or leave it towards, the same neighbouring node.
fn same_path(c: &TaxiConflict, movers: &[TaxiMover]) -> bool {
    let ConflictKind::Crossing { node } = c.kind else {
        return false;
    };
    let (Some(ma), Some(mb)) = (find(movers, &c.a), find(movers, &c.b)) else {
        return false;
    };
    let around = |m: &TaxiMover| {
        m.route.iter().position(|&n| n == node).map(|i| {
            (
                i.checked_sub(1).map(|p| m.route[p]),
                m.route.get(i + 1).copied(),
            )
        })
    };
    let (Some((pa, na)), Some((pb, nb))) = (around(ma), around(mb)) else {
        return false;
    };
    (na.is_some() && na == nb) || (pa.is_some() && pa == pb)
}

fn preferred(c: &TaxiConflict, movers: &[TaxiMover]) -> Resolution {
    let (first, later) = order(c);
    match c.kind {
        ConflictKind::HeadOn { .. } => Resolution::HoldPosition { who: later.clone() },
        ConflictKind::Crossing { .. } if same_path(c, movers) => Resolution::Follow {
            who: later.clone(),
            leader: first.clone(),
        },
        ConflictKind::Crossing { .. } => Resolution::GiveWay {
            who: later.clone(),
            to: first.clone(),
        },
    }
}

/// Every sensible way out of `c`, the preferred one first: the later aircraft yields
/// (hold, give way, or follow), then alternatives such as the earlier aircraft yielding
/// or holding. Empty if a conflicting callsign is not in `movers`.
pub fn resolution_options(c: &TaxiConflict, movers: &[TaxiMover]) -> Vec<Resolution> {
    if find(movers, &c.a).is_none() || find(movers, &c.b).is_none() {
        return Vec::new();
    }
    let (first, later) = order(c);
    let hold = |who: &Callsign| Resolution::HoldPosition { who: who.clone() };
    let mut v = vec![preferred(c, movers)];
    if matches!(c.kind, ConflictKind::Crossing { .. }) && !same_path(c, movers) {
        v.push(Resolution::GiveWay {
            who: first.clone(),
            to: later.clone(),
        });
    }
    v.push(hold(later));
    v.push(hold(first));
    let mut seen = Vec::new();
    v.retain(|r| {
        let new = !seen.contains(r);
        seen.push(r.clone());
        new
    });
    v
}

/// The preferred option: the aircraft reaching the conflict point later (`b` on a tie)
/// yields. HeadOn: it holds position. Crossing: it gives way to the other, or follows it
/// when both continue along the same path in the same direction. Falls back to the first
/// option if the preferred one is not offered.
pub fn default_resolution(
    options: &[Resolution],
    c: &TaxiConflict,
    movers: &[TaxiMover],
) -> Resolution {
    let want = preferred(c, movers);
    if options.contains(&want) {
        want
    } else {
        options.first().cloned().unwrap_or(want)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atc_geo::LatLon;
    use atc_navdata::model::{EdgeKind, TaxiEdge, TaxiNetwork, TaxiNode};

    fn cs(s: &str) -> Callsign {
        Callsign::new(s)
    }

    /// 0 (west) - 1 (centre) - 2 (east), and 3 south of the centre.
    fn junction() -> TaxiGraph {
        let node = |id, lat, lon| TaxiNode {
            id,
            pos: LatLon { lat, lon },
            name: String::new(),
        };
        let edge = |from, to| TaxiEdge {
            from,
            to,
            one_way: false,
            kind: EdgeKind::Taxiway,
            name: "A".into(),
            active_zones: vec![],
        };
        TaxiGraph::build(&TaxiNetwork {
            nodes: vec![
                node(0, 50.0, 19.000),
                node(1, 50.0, 19.002),
                node(2, 50.0, 19.004),
                node(3, 49.998, 19.002),
            ],
            edges: vec![edge(0, 1), edge(1, 2), edge(3, 1)],
        })
    }

    fn mover(name: &str, route: &[u32], progress_m: f64, speed_mps: f64) -> TaxiMover {
        TaxiMover {
            callsign: cs(name),
            route: route.to_vec(),
            progress_m,
            speed_mps,
        }
    }

    /// Speed at which `route`'s first leg is covered in `secs`.
    fn speed_for(g: &TaxiGraph, from: u32, to: u32, secs: f64) -> f64 {
        g.edge_len(from, to).unwrap() / secs
    }

    #[test]
    fn head_on_detected() {
        let g = junction();
        let movers = [
            mover("SPAAA", &[0, 1], 0.0, 5.0),
            mover("SPBBB", &[1, 0], 0.0, 5.0),
        ];
        let c = predict_conflicts(&g, &movers, 120.0);
        assert_eq!(c.len(), 1);
        assert_eq!((&c[0].a, &c[0].b), (&cs("SPAAA"), &cs("SPBBB")));
        assert_eq!(c[0].kind, ConflictKind::HeadOn { edge: (0, 1) });
        assert_eq!(c[0].t_s, 0.0);
    }

    #[test]
    fn crossing_node_detected() {
        let g = junction();
        // Both reach node 1 after 20 s.
        let movers = [
            mover("SPAAA", &[0, 1, 2], 0.0, speed_for(&g, 0, 1, 20.0)),
            mover("SPBBB", &[3, 1], 0.0, speed_for(&g, 3, 1, 20.0)),
        ];
        let c = predict_conflicts(&g, &movers, 120.0);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].kind, ConflictKind::Crossing { node: 1 });
        assert!((c[0].t_s - 12.5).abs() < 1e-6, "{}", c[0].t_s);
        assert!((c[0].a_arrive_s - 20.0).abs() < 1e-6);
        assert!((c[0].b_arrive_s - 20.0).abs() < 1e-6);
    }

    #[test]
    fn no_conflict_when_separated_in_time() {
        let g = junction();
        // 20 s vs 40 s at node 1: 20 s apart, outside the 15 s window.
        let movers = [
            mover("SPAAA", &[0, 1, 2], 0.0, speed_for(&g, 0, 1, 20.0)),
            mover("SPBBB", &[3, 1], 0.0, speed_for(&g, 3, 1, 40.0)),
        ];
        assert!(predict_conflicts(&g, &movers, 120.0).is_empty());
    }

    #[test]
    fn conflicts_beyond_the_horizon_are_ignored() {
        let g = junction();
        let movers = [
            mover("SPAAA", &[0, 1, 2], 0.0, speed_for(&g, 0, 1, 100.0)),
            mover("SPBBB", &[3, 1], 0.0, speed_for(&g, 3, 1, 100.0)),
        ];
        assert!(predict_conflicts(&g, &movers, 60.0).is_empty());
        assert_eq!(predict_conflicts(&g, &movers, 120.0).len(), 1);
    }

    #[test]
    fn same_direction_pair_is_reported_once_and_followed() {
        let g = junction();
        let len = g.edge_len(0, 1).unwrap();
        // Same speed, SPAAA 30 m ahead of SPBBB on the same path.
        let movers = [
            mover("SPAAA", &[0, 1, 2], 30.0, 5.0),
            mover("SPBBB", &[0, 1, 2], 0.0, 5.0),
        ];
        assert!(len > 30.0);
        let c = predict_conflicts(&g, &movers, 120.0);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].kind, ConflictKind::Crossing { node: 1 });
        let opts = resolution_options(&c[0], &movers);
        let d = default_resolution(&opts, &c[0], &movers);
        assert_eq!(
            d,
            Resolution::Follow {
                who: cs("SPBBB"),
                leader: cs("SPAAA")
            }
        );
        assert!(opts.contains(&d));
    }

    #[test]
    fn default_resolution_later_gives_way() {
        let g = junction();
        // Crossing at node 1: SPAAA at 20 s, SPBBB at 25 s -> SPBBB gives way.
        let movers = [
            mover("SPAAA", &[0, 1, 2], 0.0, speed_for(&g, 0, 1, 20.0)),
            mover("SPBBB", &[3, 1], 0.0, speed_for(&g, 3, 1, 25.0)),
        ];
        let c = predict_conflicts(&g, &movers, 120.0);
        let opts = resolution_options(&c[0], &movers);
        assert!(opts.contains(&Resolution::HoldPosition { who: cs("SPAAA") }));
        assert_eq!(
            default_resolution(&opts, &c[0], &movers),
            Resolution::GiveWay {
                who: cs("SPBBB"),
                to: cs("SPAAA")
            }
        );

        // Head-on: SPAAA enters edge 0-1 at once, SPBBB only after 28 s -> SPBBB holds.
        let movers = [
            mover("SPAAA", &[0, 1], 0.0, 2.0),
            mover("SPBBB", &[2, 1, 0], 0.0, 5.0),
        ];
        let c = predict_conflicts(&g, &movers, 120.0);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].kind, ConflictKind::HeadOn { edge: (0, 1) });
        let opts = resolution_options(&c[0], &movers);
        assert_eq!(
            default_resolution(&opts, &c[0], &movers),
            Resolution::HoldPosition { who: cs("SPBBB") }
        );
    }

    #[test]
    fn empty_route_mover_yields_no_conflicts() {
        let g = junction();
        let movers = [
            mover("SPAAA", &[], 0.0, 5.0),
            mover("SPBBB", &[], -3.0, 5.0),
            mover("SPCCC", &[1], 0.0, 5.0),
            mover("SPDDD", &[0, 1, 2], 0.0, 5.0),
        ];
        assert!(predict_conflicts(&g, &movers, 120.0).is_empty());
    }
}
