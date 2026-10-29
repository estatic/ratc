//! Checks the EPKK layout of a real X-Plane install (`ATC_XPLANE_ROOT`, default: the Steam
//! path) routes: `cargo test -p atc-core --test xplane_install -- --ignored --nocapture`.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use atc_core::atis::Weather;
use atc_core::engine::{AtcEngine, EngineConfig};
use atc_core::readback::Strictness;
use atc_core::state::SimTime;
use atc_core::strip::PilotProfile;
use atc_core::taxi::{TaxiGraph, TaxiTarget};
use atc_geo::{LatLon, distance_m};
use atc_navdata::model::{Airport, EdgeKind};
use atc_navdata::procedures::parse_procedures;
use atc_navdata::xplane::{
    apt_dat_candidates, extract_airport, load_destinations, load_procedure_set, resolve_airport,
};
use atc_phraseology::{AtcItem, Callsign, Lang, Level, Wind};

fn xp_root() -> PathBuf {
    std::env::var_os("ATC_XPLANE_ROOT").map_or_else(
        || {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local/share/Steam/steamapps/common/X-Plane 11")
        },
        PathBuf::from,
    )
}

/// Routing failures of `ap`: stand -> both holding points, holding point area -> every stand.
fn route_failures(ap: &Airport) -> Vec<String> {
    let g = TaxiGraph::build(&ap.taxi);
    let mut failures = Vec::new();
    for rwy in ["07", "25"] {
        let holds = g.holding_points(ap, rwy);
        println!("  holding points {rwy}: {}", holds.len());
        let Some(&hold) = holds.first() else {
            failures.push(format!("no holding point for {rwy}"));
            continue;
        };
        for st in &ap.stands {
            let to = TaxiTarget::HoldingPoint { runway: rwy.into() };
            if let Err(e) = g.route(ap, st.pos, &to) {
                failures.push(format!("{} -> {rwy}: {e}", st.name));
            }
            let to = TaxiTarget::Stand {
                name: st.name.clone(),
            };
            if let Err(e) = g.route(ap, g.node_pos(hold).unwrap(), &to) {
                failures.push(format!("hold {rwy} -> {}: {e}", st.name));
            }
        }
    }
    failures
}

fn report(ap: &Airport) -> Vec<String> {
    let names: BTreeSet<&str> = ap
        .taxi
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Taxiway && !e.name.is_empty())
        .map(|e| e.name.as_str())
        .collect();
    let unnamed = ap
        .taxi
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Taxiway && e.name.is_empty())
        .count();
    // Stands far from the network get routed from a distant node.
    let far: Vec<String> = ap
        .stands
        .iter()
        .filter_map(|st| {
            let d = ap
                .taxi
                .nodes
                .iter()
                .map(|n| distance_m(n.pos, st.pos))
                .fold(f64::INFINITY, f64::min);
            (d > 60.0).then(|| format!("{} {d:.0} m", st.name))
        })
        .collect();
    println!(
        "  {} nodes, {} edges, {} stands, {unnamed} unnamed taxiway edges\n  taxiways: {names:?}\n  \
         stands > 60 m from the network: {far:?}",
        ap.taxi.nodes.len(),
        ap.taxi.edges.len(),
        ap.stands.len()
    );
    route_failures(ap)
}

#[test]
#[ignore = "needs a local X-Plane install"]
fn installed_epkk_layout_routes() {
    let root = xp_root();
    // Every installed EPKK, for comparison; the resolved one is what X-Plane shows.
    for path in apt_dat_candidates(&root) {
        let reader = BufReader::new(File::open(&path).unwrap());
        if let Some((ident, text)) = extract_airport(reader, "EPKK").unwrap() {
            let ap = atc_navdata::parse_apt_dat(&text, &ident).unwrap();
            println!("{}", path.display());
            let failures = report(&ap);
            println!("  routing failures: {failures:#?}");
        }
    }
    let resolved = resolve_airport(&root, "EPKK").unwrap();
    println!("resolved: {}", resolved.source.display());
    let failures = report(&resolved.parse("EPKK").unwrap());
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The EPKK engine with the installed CIFP clears an IFR flight to Katowice via a SID
/// towards the west-north-west, climbing to the 5000 ft cruise level. Skips (with a
/// note) when the install has no EPKK CIFP.
#[test]
fn installed_epkk_clearance_to_katowice() {
    let root = xp_root();
    let Ok(set) = load_procedure_set(&root, "EPKK") else {
        println!("skipped: no EPKK CIFP under {}", root.display());
        return;
    };
    let ap = resolve_airport(&root, "EPKK")
        .unwrap()
        .parse("EPKK")
        .unwrap();
    let procs = parse_procedures(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/airports/EPKK/procedures.toml"
        ))
        .unwrap(),
    )
    .unwrap();
    let epkk = LatLon {
        lat: 50.0777,
        lon: 19.7848,
    };
    let names = [("EPKT".to_string(), "Katowice".to_string())];
    let dests = load_destinations(&root, epkk, 100.0, &names).unwrap();
    let mut e = AtcEngine::new(ap, procs, EngineConfig::new(Strictness::Standard))
        .with_procedure_set(set)
        .with_destinations(dests);
    e.register_pilot(PilotProfile {
        callsign: Callsign::new("SPABC"),
        aircraft_type: "C172".into(),
    });
    e.on_weather(Weather {
        wind: Wind {
            dir_deg: 250,
            speed_kt: 8,
            calm: false,
        },
        visibility_m: 10_000,
        clouds: vec![],
        temp_c: 15,
        dewpoint_c: 10,
        qnh_hpa: 1013,
    });
    let out = e.on_transmission(
        121_980,
        "Kraków Delivery, Sierra Papa Alpha Bravo Charlie, IFR to Katowice, request clearance",
        Lang::En,
        SimTime(0.0),
    );
    println!("{}", out[0].text);
    let items = &out[0].message.items;
    assert_eq!(
        items[0],
        AtcItem::ClearedTo {
            destination: "Katowice".into(),
            icao: "EPKT".into(),
        }
    );
    assert!(
        matches!(&items[1], AtcItem::Sid { fix, .. } if ["SUDOL", "NUMBA", "EMBIK"].contains(&fix.as_str())),
        "{items:?}"
    );
    // The SID's "at or above" constraints are minimums: climb to the 5000 ft cruise level.
    assert_eq!(items[2], AtcItem::ClimbTo(Level::Altitude(5000)));
    assert!(
        out[0]
            .text
            .contains(", climb altitude five thousand feet, "),
        "{}",
        out[0].text
    );
    assert!(matches!(items[3], AtcItem::Squawk(_)));
}
