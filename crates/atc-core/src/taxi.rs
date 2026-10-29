//! Taxi graph and route finding.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use atc_geo::{LatLon, RunwayFrame, distance_m};
use atc_navdata::model::{ActiveZone, Airport, EdgeKind, Stand, TaxiNetwork};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TaxiTarget {
    HoldingPoint { runway: String },
    Stand { name: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaxiRoute {
    pub nodes: Vec<u32>,
    /// Taxiway names in order, consecutive repeats collapsed, unnamed edges skipped.
    pub via: Vec<String>,
    /// Runways crossed, in order.
    pub crossings: Vec<String>,
    pub hold_short: Option<String>,
    pub length_m: f64,
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum RouteError {
    #[error("unknown stand {0}")]
    UnknownStand(String),
    #[error("unknown runway {0}")]
    UnknownRunway(String),
    #[error("no taxi route")]
    Unreachable,
}

#[derive(Clone, Debug)]
struct EdgeInfo {
    name: String,
    kind: EdgeKind,
    /// For runway edges: runway identifiers from active zones and the "07/25" name.
    runways: Vec<String>,
    length_m: f64,
    /// Taxiway edges: runways from the edge's active zones (hold-line marking).
    zone_runways: Vec<String>,
    ends: (usize, usize),
}

/// One search step: a taxiway edge, or a runway crossing of one or two runway edges.
#[derive(Clone, Debug)]
struct Arc {
    to: usize,
    edges: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct TaxiGraph {
    ids: Vec<u32>,
    pos: Vec<LatLon>,
    infos: Vec<EdgeInfo>,
    arcs: Vec<Vec<Arc>>,
    /// Nodes touched by at least one edge; edgeless nodes never snap targets.
    connected: Vec<bool>,
    /// (node index, runway edge index) for both ends of every runway edge.
    runway_ends: Vec<(usize, usize)>,
}

fn edge_runways(name: &str, zones: &[ActiveZone]) -> Vec<String> {
    let mut v: Vec<String> = zones
        .iter()
        .flat_map(|z| z.runways.iter().cloned())
        .collect();
    v.extend(
        name.split('/')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    );
    v.sort();
    v.dedup();
    v
}

/// Side (+1 right, -1 left, 0 on the axis) of a point relative to the axis `from`->`to`.
fn side_of(axis: &RunwayFrame, p: LatLon) -> i8 {
    let (_, right) = axis.to_local(p);
    if right > 1.0 {
        1
    } else if right < -1.0 {
        -1
    } else {
        0
    }
}

/// A crossing is legitimate when the taxiway node before the entry and the taxiway
/// node after the exit lie on opposite sides of the runway line entry -> exit.
fn crosses(axis: &RunwayFrame, before: LatLon, after: LatLon) -> bool {
    let (a, b) = (side_of(axis, before), side_of(axis, after));
    a != 0 && a == -b
}

/// Search state: node reached, taxiway node it was reached from (`NONE` at the start),
/// and, right after a runway crossing, the crossing's entry node (exit not yet verified).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct State {
    node: usize,
    from: usize,
    entry: Option<usize>,
}

const NONE: usize = usize::MAX;

struct Queued(f64, usize);

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for Queued {}
impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Queued {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0) // min-heap
    }
}

impl TaxiGraph {
    pub fn build(net: &TaxiNetwork) -> Self {
        let index: HashMap<u32, usize> = net
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        let n = net.nodes.len();
        let mut infos = Vec::new();
        let mut taxi: Vec<Vec<Arc>> = vec![Vec::new(); n];
        let mut rwy: Vec<Vec<Arc>> = vec![Vec::new(); n];
        let mut has_taxiway = vec![false; n];
        let mut connected = vec![false; n];
        let mut runway_ends = Vec::new();
        for e in &net.edges {
            let (Some(&a), Some(&b)) = (index.get(&e.from), index.get(&e.to)) else {
                continue;
            };
            let k = infos.len();
            let is_runway = e.kind == EdgeKind::Runway;
            connected[a] = true;
            connected[b] = true;
            infos.push(EdgeInfo {
                name: e.name.clone(),
                kind: e.kind,
                runways: if is_runway {
                    edge_runways(&e.name, &e.active_zones)
                } else {
                    Vec::new()
                },
                length_m: distance_m(net.nodes[a].pos, net.nodes[b].pos),
                zone_runways: if is_runway {
                    Vec::new()
                } else {
                    e.active_zones
                        .iter()
                        .flat_map(|z| z.runways.iter().cloned())
                        .collect()
                },
                ends: (a, b),
            });
            let adj = if is_runway { &mut rwy } else { &mut taxi };
            adj[a].push(Arc {
                to: b,
                edges: vec![k],
            });
            if !e.one_way {
                adj[b].push(Arc {
                    to: a,
                    edges: vec![k],
                });
            }
            if is_runway {
                runway_ends.push((a, k));
                runway_ends.push((b, k));
            } else {
                has_taxiway[a] = true;
                has_taxiway[b] = true;
            }
        }
        // A crossing enters a runway from a taxiway node and leaves onto a taxiway
        // node on the other side of the centreline within two runway edges.
        let mut arcs = taxi;
        for s in 0..n {
            if !has_taxiway[s] {
                continue;
            }
            for first in &rwy[s] {
                let mut chains = vec![(first.to, first.edges.clone())];
                for second in &rwy[first.to] {
                    if second.to != s {
                        chains.push((
                            second.to,
                            [first.edges.clone(), second.edges.clone()].concat(),
                        ));
                    }
                }
                for (to, edges) in chains {
                    if to != s && has_taxiway[to] {
                        arcs[s].push(Arc { to, edges });
                    }
                }
            }
        }
        Self {
            ids: net.nodes.iter().map(|n| n.id).collect(),
            pos: net.nodes.iter().map(|n| n.pos).collect(),
            infos,
            arcs,
            connected,
            runway_ends,
        }
    }

    /// Hold-line nodes for `runway`: for each taxiway edge whose active zones list `runway`,
    /// the endpoint that is not on any runway edge, kept only in the departure-end half of
    /// the runway, sorted by distance along the runway from its threshold (full-length
    /// departure first). Empty if the runway is unknown.
    pub fn holding_points(&self, airport: &Airport, runway: &str) -> Vec<u32> {
        self.holding_indices(airport, runway)
            .into_iter()
            .map(|i| self.ids[i])
            .collect()
    }

    fn holding_indices(&self, airport: &Airport, runway: &str) -> Vec<usize> {
        let Some((threshold, opposite)) = airport.runways.iter().find_map(|r| {
            let i = r.ends.iter().position(|e| e.ident == runway)?;
            Some((r.ends[i].threshold, r.ends[1 - i].threshold))
        }) else {
            return Vec::new();
        };
        let frame = RunwayFrame::new(threshold, opposite);
        let half = distance_m(threshold, opposite) / 2.0;
        let mut on_runway = vec![false; self.pos.len()];
        for &(node, _) in &self.runway_ends {
            on_runway[node] = true;
        }
        let mut out: Vec<(f64, usize)> = self
            .infos
            .iter()
            .filter(|e| e.kind == EdgeKind::Taxiway && e.zone_runways.iter().any(|r| r == runway))
            .flat_map(|e| [e.ends.0, e.ends.1])
            .filter(|&n| !on_runway[n])
            .map(|n| (frame.to_local(self.pos[n]).0, n))
            .filter(|&(along, _)| along < half)
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        out.dedup_by_key(|x| x.1);
        out.into_iter().map(|x| x.1).collect()
    }

    /// Position of the node with this navdata id.
    pub fn node_pos(&self, id: u32) -> Option<LatLon> {
        let i = self.ids.iter().position(|&n| n == id)?;
        Some(self.pos[i])
    }

    /// Length in metres of the direct edge between two nodes (either direction;
    /// one-way restrictions are ignored). `None` if no edge joins them, e.g. across a
    /// runway crossing, whose route nodes skip the runway-interior nodes.
    pub fn edge_len(&self, a: u32, b: u32) -> Option<f64> {
        self.infos
            .iter()
            .find(|e| {
                let ends = (self.ids[e.ends.0], self.ids[e.ends.1]);
                ends == (a, b) || ends == (b, a)
            })
            .map(|e| e.length_m)
    }

    /// Nearest node that has at least one edge.
    fn nearest(&self, p: LatLon) -> Option<usize> {
        (0..self.pos.len())
            .filter(|&i| self.connected[i])
            .min_by(|&a, &b| distance_m(self.pos[a], p).total_cmp(&distance_m(self.pos[b], p)))
    }

    /// Route from the node nearest `from` to `to`. A holding-point target is the first
    /// reachable hold line in `holding_points` order (full-length departure preferred).
    pub fn route(
        &self,
        airport: &Airport,
        from: LatLon,
        to: &TaxiTarget,
    ) -> Result<TaxiRoute, RouteError> {
        let (goal_sets, forbidden, hold_short): (Vec<Vec<usize>>, Option<&str>, Option<String>) =
            match to {
                TaxiTarget::Stand { name } => {
                    let st: &Stand = airport
                        .stands
                        .iter()
                        .find(|s| &s.name == name)
                        .ok_or_else(|| RouteError::UnknownStand(name.clone()))?;
                    (vec![self.nearest(st.pos).into_iter().collect()], None, None)
                }
                TaxiTarget::HoldingPoint { runway } => {
                    if !airport
                        .runways
                        .iter()
                        .any(|r| r.ends.iter().any(|e| &e.ident == runway))
                    {
                        return Err(RouteError::UnknownRunway(runway.clone()));
                    }
                    (
                        self.holding_indices(airport, runway)
                            .into_iter()
                            .map(|i| vec![i])
                            .collect(),
                        Some(runway.as_str()),
                        Some(runway.clone()),
                    )
                }
            };
        let mut result = Err(RouteError::Unreachable);
        for goals in goal_sets {
            result = self.route_to(from, &goals, forbidden, hold_short.clone());
            if result.is_ok() {
                break;
            }
        }
        result
    }

    fn route_to(
        &self,
        from: LatLon,
        goals: &[usize],
        forbidden: Option<&str>,
        hold_short: Option<String>,
    ) -> Result<TaxiRoute, RouteError> {
        let start = self.nearest(from).ok_or(RouteError::Unreachable)?;
        if goals.is_empty() {
            return Err(RouteError::Unreachable);
        }

        let init = State {
            node: start,
            from: NONE,
            entry: None,
        };
        let mut states = vec![init];
        let mut index: HashMap<State, usize> = HashMap::from([(init, 0)]);
        let mut dist = vec![0.0_f64];
        // (previous state id, arc index in arcs[previous node])
        let mut prev: Vec<Option<(usize, usize)>> = vec![None];
        let mut heap = BinaryHeap::new();
        heap.push(Queued(0.0, 0));
        let mut reached = None;
        while let Some(Queued(d, id)) = heap.pop() {
            if d > dist[id] {
                continue;
            }
            let st = states[id];
            if goals.contains(&st.node) {
                reached = Some(id);
                break;
            }
            for (ai, arc) in self.arcs[st.node].iter().enumerate() {
                let is_crossing = arc
                    .edges
                    .iter()
                    .any(|&e| self.infos[e].kind == EdgeKind::Runway);
                if let Some(rw) = forbidden
                    && arc.edges.iter().any(|&e| {
                        self.infos[e].kind == EdgeKind::Runway
                            && self.infos[e].runways.iter().any(|r| r == rw)
                    })
                {
                    continue;
                }
                let next = if is_crossing {
                    // Needs a known taxiway predecessor; chained crossings are not allowed.
                    if st.from == NONE || st.entry.is_some() {
                        continue;
                    }
                    State {
                        node: arc.to,
                        from: st.from,
                        entry: Some(st.node),
                    }
                } else {
                    if arc.to == st.from {
                        continue; // no immediate U-turns
                    }
                    if let Some(entry) = st.entry {
                        let axis = RunwayFrame::new(self.pos[entry], self.pos[st.node]);
                        if !crosses(&axis, self.pos[st.from], self.pos[arc.to]) {
                            continue;
                        }
                    }
                    State {
                        node: arc.to,
                        from: st.node,
                        entry: None,
                    }
                };
                let nd = d + arc
                    .edges
                    .iter()
                    .map(|&e| self.infos[e].length_m)
                    .sum::<f64>();
                let nid = *index.entry(next).or_insert_with(|| {
                    states.push(next);
                    dist.push(f64::INFINITY);
                    prev.push(None);
                    states.len() - 1
                });
                if nd < dist[nid] {
                    dist[nid] = nd;
                    prev[nid] = Some((id, ai));
                    heap.push(Queued(nd, nid));
                }
            }
        }
        let end_id = reached.ok_or(RouteError::Unreachable)?;
        let end = states[end_id].node;

        let mut path = vec![end];
        let mut steps: Vec<&Arc> = Vec::new();
        let mut cur = end_id;
        while let Some((p, ai)) = prev[cur] {
            let pn = states[p].node;
            steps.push(&self.arcs[pn][ai]);
            path.push(pn);
            cur = p;
        }
        path.reverse();
        steps.reverse();

        let mut via: Vec<String> = Vec::new();
        let mut crossings: Vec<String> = Vec::new();
        for arc in steps {
            // Repeats collapse only within one arc, so two separate crossings stay two.
            let mut arc_last: Option<String> = None;
            for &e in &arc.edges {
                let info = &self.infos[e];
                if info.kind == EdgeKind::Runway {
                    let label = if info.name.is_empty() {
                        info.runways.join("/")
                    } else {
                        info.name.clone()
                    };
                    if arc_last.as_ref() != Some(&label) {
                        crossings.push(label.clone());
                    }
                    arc_last = Some(label);
                } else if !info.name.is_empty() && via.last() != Some(&info.name) {
                    via.push(info.name.clone());
                }
            }
        }
        Ok(TaxiRoute {
            nodes: path.into_iter().map(|i| self.ids[i]).collect(),
            via,
            crossings,
            hold_short,
            length_m: dist[end_id],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atc_navdata::model::{ActiveZone, Runway, RunwayEnd, TaxiEdge, TaxiNode, ZoneKind};
    use atc_navdata::parse_apt_dat;

    const MINI: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../atc-navdata/tests/fixtures/mini.dat"
    );

    fn mini() -> Airport {
        parse_apt_dat(&std::fs::read_to_string(MINI).unwrap(), "EPKK").unwrap()
    }

    fn node(id: u32, lat: f64, lon: f64) -> TaxiNode {
        TaxiNode {
            id,
            pos: LatLon { lat, lon },
            name: String::new(),
        }
    }

    fn taxi_edge(from: u32, to: u32, one_way: bool, name: &str) -> TaxiEdge {
        TaxiEdge {
            from,
            to,
            one_way,
            kind: EdgeKind::Taxiway,
            name: name.into(),
            active_zones: vec![],
        }
    }

    /// Marks taxiway edges (by index) as lying on the hold line of the given runways.
    fn hold_zones(ap: &mut Airport, edges: &[usize], rwys: &[&str]) {
        for &i in edges {
            ap.taxi.edges[i].active_zones = vec![ActiveZone {
                kind: ZoneKind::Departure,
                runways: rwys.iter().map(|r| r.to_string()).collect(),
            }];
        }
    }

    fn synthetic(nodes: Vec<TaxiNode>, edges: Vec<TaxiEdge>, stands: Vec<Stand>) -> Airport {
        let mut ap = mini();
        ap.taxi = TaxiNetwork { nodes, edges };
        ap.stands = stands;
        ap
    }

    fn stand(name: &str, pos: LatLon) -> Stand {
        Stand {
            name: name.into(),
            pos,
            heading_deg: 0.0,
        }
    }

    #[test]
    fn route_to_holding_point_names_taxiways() {
        let mut ap = mini();
        hold_zones(&mut ap, &[2, 4], &["07"]);
        let g = TaxiGraph::build(&ap.taxi);
        let from = ap.taxi.nodes[0].pos;
        let r = g
            .route(
                &ap,
                from,
                &TaxiTarget::HoldingPoint {
                    runway: "07".into(),
                },
            )
            .unwrap();
        assert_eq!(r.nodes, vec![0, 1, 2]);
        assert_eq!(r.via, vec!["A".to_string()]);
        assert_eq!(r.hold_short.as_deref(), Some("07"));
        assert!(r.crossings.is_empty());
        assert!(r.length_m > 100.0 && r.length_m < 400.0);
    }

    #[test]
    fn holding_points_are_hold_line_nodes_off_the_runway_sorted_by_along() {
        let mut ap = mini();
        hold_zones(&mut ap, &[2, 4], &["07"]);
        let g = TaxiGraph::build(&ap.taxi);
        // Edges B (2-3) and C (4-5): endpoints off the runway edge 3-4 are 2 and 5, but 5
        // lies in the far half of the runway and is dropped.
        assert_eq!(g.holding_points(&ap, "07"), vec![2]);
        // Edges without hold-line zones yield nothing.
        assert!(g.holding_points(&ap, "25").is_empty());
        assert!(g.holding_points(&ap, "13").is_empty());
    }

    /// Runway 07/25 along lat 50.0, lon 19.000..19.020 (about 1.43 km).
    fn line_runway() -> Runway {
        let end = |ident: &str, lon: f64| RunwayEnd {
            ident: ident.into(),
            threshold: LatLon { lat: 50.0, lon },
            displaced_m: 0.0,
        };
        Runway {
            width_m: 45.0,
            ends: [end("07", 19.0), end("25", 19.02)],
        }
    }

    /// Runway edges 1-2-3 on the centreline with four taxiway nodes (two per end): A/B at the west end,
    /// C/D at the east end; `north` positions taxiway nodes off the centreline.
    fn runway_airport(taxi_lat: [f64; 4]) -> Airport {
        let nodes = vec![
            node(1, 50.0, 19.004),
            node(2, 50.0, 19.010),
            node(3, 50.0, 19.016),
            node(10, taxi_lat[0], 19.004),
            node(11, taxi_lat[1], 19.004),
            node(12, taxi_lat[2], 19.016),
            node(13, taxi_lat[3], 19.016),
        ];
        let zone = vec![ActiveZone {
            kind: ZoneKind::Departure,
            runways: vec!["07".into(), "25".into()],
        }];
        let rwy = |a, b| TaxiEdge {
            from: a,
            to: b,
            one_way: false,
            kind: EdgeKind::Runway,
            name: "07/25".into(),
            active_zones: zone.clone(),
        };
        let edges = vec![
            rwy(1, 2),
            rwy(2, 3),
            taxi_edge(10, 1, false, "A"),
            taxi_edge(11, 1, false, "A"),
            taxi_edge(12, 3, false, "D"),
            taxi_edge(13, 3, false, "D"),
        ];
        let mut ap = synthetic(nodes, edges, vec![]);
        hold_zones(&mut ap, &[2, 3], &["07"]);
        hold_zones(&mut ap, &[4, 5], &["25"]);
        Airport {
            runways: vec![line_runway()],
            ..ap
        }
    }

    #[test]
    fn holding_points_07_and_25_are_at_opposite_ends() {
        let ap = runway_airport([50.001, 50.001, 49.999, 49.999]);
        let g = TaxiGraph::build(&ap.taxi);
        assert_eq!(g.holding_points(&ap, "07"), vec![10, 11]);
        assert_eq!(g.holding_points(&ap, "25"), vec![12, 13]);
        // Crossing the target runway is forbidden, so each end is reached from its own side.
        let end = |from: usize, rwy: &str| {
            let to = TaxiTarget::HoldingPoint { runway: rwy.into() };
            *g.route(&ap, ap.taxi.nodes[from].pos, &to)
                .unwrap()
                .nodes
                .last()
                .unwrap()
        };
        assert_eq!((end(3, "07"), end(5, "25")), (10, 12));
    }

    #[test]
    fn crossing_requires_opposite_sides() {
        // West taxiways north, east taxiways south: legitimate crossing 1 -> 3.
        let ap = runway_airport([50.001, 50.001, 49.999, 49.999]);
        let g = TaxiGraph::build(&ap.taxi);
        let s = TaxiTarget::Stand { name: "S".into() };
        let mut ap2 = ap.clone();
        ap2.stands = vec![stand("S", ap.taxi.nodes[5].pos)];
        let r = g.route(&ap2, ap.taxi.nodes[3].pos, &s).unwrap();
        assert_eq!(r.nodes, vec![10, 1, 3, 12]);
        assert_eq!(r.via, vec!["A".to_string(), "D".to_string()]);
        assert_eq!(r.crossings, vec!["07/25".to_string()]);

        // All taxiways on the same side: the two-edge runway chain is not a crossing.
        let ap = runway_airport([50.001, 50.001, 50.001, 50.001]);
        let g = TaxiGraph::build(&ap.taxi);
        let mut ap2 = ap.clone();
        ap2.stands = vec![stand("S", ap.taxi.nodes[5].pos)];
        assert_eq!(
            g.route(&ap2, ap.taxi.nodes[3].pos, &s),
            Err(RouteError::Unreachable)
        );
    }

    #[test]
    fn same_side_move_along_runway_at_junction_is_not_a_crossing() {
        // Junction 2 on the centreline has taxiways north (20) and south (21); node 3
        // likewise (12 north, 13 south).
        let zone = vec![ActiveZone {
            kind: ZoneKind::Departure,
            runways: vec!["07".into(), "25".into()],
        }];
        let nodes = vec![
            node(2, 50.0, 19.010),
            node(3, 50.0, 19.016),
            node(20, 50.001, 19.010),
            node(21, 49.999, 19.010),
            node(12, 50.001, 19.016),
            node(13, 49.999, 19.016),
        ];
        let edges = vec![
            TaxiEdge {
                from: 2,
                to: 3,
                one_way: false,
                kind: EdgeKind::Runway,
                name: "07/25".into(),
                active_zones: zone,
            },
            taxi_edge(20, 2, false, "A"),
            taxi_edge(21, 2, false, "A"),
            taxi_edge(3, 12, false, "D"),
            taxi_edge(3, 13, false, "D"),
        ];
        let stands = vec![
            stand(
                "NORTH",
                LatLon {
                    lat: 50.001,
                    lon: 19.016,
                },
            ),
            stand(
                "SOUTH",
                LatLon {
                    lat: 49.999,
                    lon: 19.016,
                },
            ),
        ];
        let ap = synthetic(nodes, edges, stands);
        let g = TaxiGraph::build(&ap.taxi);
        let from = LatLon {
            lat: 50.001,
            lon: 19.010,
        };
        // North -> junction -> along runway -> north again: same side, no crossing.
        let same = g.route(
            &ap,
            from,
            &TaxiTarget::Stand {
                name: "NORTH".into(),
            },
        );
        assert_eq!(same, Err(RouteError::Unreachable));
        // North -> junction -> runway -> south: a genuine crossing at the same junction.
        let r = g
            .route(
                &ap,
                from,
                &TaxiTarget::Stand {
                    name: "SOUTH".into(),
                },
            )
            .unwrap();
        assert_eq!(r.nodes, vec![20, 2, 3, 13]);
        assert_eq!(r.crossings, vec!["07/25".to_string()]);
    }

    #[test]
    fn crossing_same_runway_twice_gives_two_entries() {
        // 10 -A- 1 =rwy= 2 -B- 20 -C- 21 -D- 3 =rwy= 4 -E- 30 (all off-centreline sides alternate).
        let zone = vec![ActiveZone {
            kind: ZoneKind::Departure,
            runways: vec!["07".into(), "25".into()],
        }];
        let rwy = |a, b| TaxiEdge {
            from: a,
            to: b,
            one_way: false,
            kind: EdgeKind::Runway,
            name: "07/25".into(),
            active_zones: zone.clone(),
        };
        // Runway along lat 50.0; taxi nodes north (+lat) or south of it.
        let nodes = vec![
            node(10, 50.001, 19.000),
            node(1, 50.0, 19.000),
            node(2, 50.0, 19.001),
            node(20, 49.999, 19.001),
            node(21, 49.999, 19.002),
            node(3, 50.0, 19.002),
            node(4, 50.0, 19.003),
            node(30, 50.001, 19.003),
        ];
        // Second crossing runs back north-to-south reversed: 21 -D- 3 enters south side.
        let edges = vec![
            taxi_edge(10, 1, false, "A"),
            rwy(1, 2),
            taxi_edge(2, 20, false, "B"),
            taxi_edge(20, 21, false, "C"),
            taxi_edge(21, 3, false, "D"),
            rwy(3, 4),
            taxi_edge(4, 30, false, "E"),
        ];
        let ap = synthetic(
            nodes,
            edges,
            vec![stand(
                "S",
                LatLon {
                    lat: 50.001,
                    lon: 19.003,
                },
            )],
        );
        let g = TaxiGraph::build(&ap.taxi);
        let r = g
            .route(
                &ap,
                LatLon {
                    lat: 50.001,
                    lon: 19.0,
                },
                &TaxiTarget::Stand { name: "S".into() },
            )
            .unwrap();
        assert_eq!(r.crossings, vec!["07/25".to_string(), "07/25".to_string()]);
    }

    #[test]
    fn route_collapses_repeated_names() {
        // 0 -A- 1 -A- 2 -(unnamed)- 3 -C- 4
        let nodes = (0..5)
            .map(|i| node(i, 50.0, 19.0 + f64::from(i) * 0.001))
            .collect();
        let edges = vec![
            taxi_edge(0, 1, false, "A"),
            taxi_edge(1, 2, false, "A"),
            taxi_edge(2, 3, false, ""),
            taxi_edge(3, 4, false, "C"),
        ];
        let end = LatLon {
            lat: 50.0,
            lon: 19.004,
        };
        let ap = synthetic(nodes, edges, vec![stand("S", end)]);
        let g = TaxiGraph::build(&ap.taxi);
        let r = g
            .route(
                &ap,
                LatLon {
                    lat: 50.0,
                    lon: 19.0,
                },
                &TaxiTarget::Stand { name: "S".into() },
            )
            .unwrap();
        assert_eq!(r.nodes, vec![0, 1, 2, 3, 4]);
        assert_eq!(r.via, vec!["A".to_string(), "C".to_string()]);
        assert_eq!(r.hold_short, None);
    }

    #[test]
    fn route_respects_one_way() {
        // Triangle: 0->1 is one-way, so 1 -> 0 must go through 2.
        let nodes = vec![
            node(0, 50.0, 19.0),
            node(1, 50.0, 19.001),
            node(2, 50.001, 19.0005),
        ];
        let edges = vec![
            taxi_edge(0, 1, true, "A"),
            taxi_edge(1, 2, false, "B"),
            taxi_edge(2, 0, false, "C"),
        ];
        let ap = synthetic(
            nodes,
            edges,
            vec![stand(
                "S",
                LatLon {
                    lat: 50.0,
                    lon: 19.0,
                },
            )],
        );
        let g = TaxiGraph::build(&ap.taxi);
        let r = g
            .route(
                &ap,
                LatLon {
                    lat: 50.0,
                    lon: 19.001,
                },
                &TaxiTarget::Stand { name: "S".into() },
            )
            .unwrap();
        assert_eq!(r.nodes, vec![1, 2, 0]);
        assert_eq!(r.via, vec!["B".to_string(), "C".to_string()]);
    }

    #[test]
    fn route_unreachable_is_error() {
        // Two separate islands: 0-1 and 8-9.
        let nodes = vec![
            node(0, 50.0, 19.0),
            node(1, 50.0, 19.001),
            node(8, 51.0, 20.0),
            node(9, 51.0, 20.001),
        ];
        let edges = vec![taxi_edge(0, 1, false, "A"), taxi_edge(8, 9, false, "B")];
        let ap = synthetic(
            nodes,
            edges,
            vec![stand(
                "S",
                LatLon {
                    lat: 51.0,
                    lon: 20.0,
                },
            )],
        );
        let g = TaxiGraph::build(&ap.taxi);
        let r = g.route(
            &ap,
            LatLon {
                lat: 50.0,
                lon: 19.0,
            },
            &TaxiTarget::Stand { name: "S".into() },
        );
        assert_eq!(r, Err(RouteError::Unreachable));
    }

    #[test]
    fn edgeless_nodes_do_not_capture_targets() {
        // Node 5 sits exactly on the stand but has no edges; the stand snaps to node 1.
        let nodes = vec![
            node(0, 50.0, 19.0),
            node(1, 50.0, 19.001),
            node(5, 50.0, 19.002),
        ];
        let edges = vec![taxi_edge(0, 1, false, "A")];
        let ap = synthetic(
            nodes,
            edges,
            vec![stand(
                "S",
                LatLon {
                    lat: 50.0,
                    lon: 19.002,
                },
            )],
        );
        let g = TaxiGraph::build(&ap.taxi);
        let r = g
            .route(
                &ap,
                LatLon {
                    lat: 50.0,
                    lon: 19.0,
                },
                &TaxiTarget::Stand { name: "S".into() },
            )
            .unwrap();
        assert_eq!(r.nodes, vec![0, 1]);
    }

    #[test]
    fn unknown_stand_is_error() {
        let ap = mini();
        let g = TaxiGraph::build(&ap.taxi);
        let r = g.route(
            &ap,
            ap.taxi.nodes[0].pos,
            &TaxiTarget::Stand { name: "ZZ".into() },
        );
        assert_eq!(r, Err(RouteError::UnknownStand("ZZ".into())));
        let r = g.route(
            &ap,
            ap.taxi.nodes[0].pos,
            &TaxiTarget::HoldingPoint {
                runway: "13".into(),
            },
        );
        assert_eq!(r, Err(RouteError::UnknownRunway("13".into())));
    }

    #[test]
    fn zone_runways_are_used_when_edge_name_is_generic() {
        let nodes = vec![
            node(0, 50.0, 19.0),
            node(1, 50.0, 19.001),
            node(2, 50.0, 19.002),
        ];
        let mut rw = taxi_edge(1, 2, false, "");
        rw.kind = EdgeKind::Runway;
        rw.active_zones = vec![ActiveZone {
            kind: ZoneKind::Departure,
            runways: vec!["07".into()],
        }];
        let edges = vec![taxi_edge(0, 1, false, "A"), rw];
        let mut ap = synthetic(nodes, edges, vec![]);
        hold_zones(&mut ap, &[0], &["07"]);
        let g = TaxiGraph::build(&ap.taxi);
        let ap = Airport {
            runways: vec![line_runway()],
            ..ap
        };
        assert_eq!(g.holding_points(&ap, "07"), vec![0]);
    }

    #[test]
    fn node_pos_and_edge_len_lookup_by_id() {
        let nodes = vec![
            node(7, 50.0, 19.0),
            node(9, 50.0, 19.001),
            node(11, 50.0, 19.002),
        ];
        let net = TaxiNetwork {
            nodes,
            edges: vec![taxi_edge(7, 9, true, "A")],
        };
        let g = TaxiGraph::build(&net);
        assert_eq!(
            g.node_pos(9),
            Some(LatLon {
                lat: 50.0,
                lon: 19.001
            })
        );
        assert_eq!(g.node_pos(8), None);
        let l = g.edge_len(7, 9).unwrap();
        assert!(l > 60.0 && l < 80.0);
        // Either direction, even for a one-way edge.
        assert_eq!(g.edge_len(9, 7), Some(l));
        assert_eq!(g.edge_len(9, 11), None);
        assert_eq!(g.edge_len(7, 99), None);
    }
}
