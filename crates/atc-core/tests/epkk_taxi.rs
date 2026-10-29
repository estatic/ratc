use atc_core::taxi::{TaxiGraph, TaxiTarget};
use atc_geo::{RunwayFrame, distance_m};
use atc_navdata::parse_apt_dat;

const APT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/airports/EPKK/apt.dat"
);

#[test]
fn epkk_holding_points_are_sorted_off_the_runway() {
    let text = std::fs::read_to_string(APT).unwrap();
    let ap = parse_apt_dat(&text, "EPKK").unwrap();
    let g = TaxiGraph::build(&ap.taxi);
    for rwy in ["07", "25"] {
        let rw = &ap.runways[0];
        let i = rw.ends.iter().position(|e| e.ident == rwy).unwrap();
        let frame = RunwayFrame::new(rw.ends[i].threshold, rw.ends[1 - i].threshold);
        let pts: Vec<(f64, f64)> = g
            .holding_points(&ap, rwy)
            .iter()
            .map(|id| frame.to_local(ap.taxi.nodes.iter().find(|n| n.id == *id).unwrap().pos))
            .collect();
        println!("{rwy}: {pts:?}");
        assert!(pts.len() >= 2);
        assert!(pts.windows(2).all(|w| w[0].0 <= w[1].0));
        assert!(pts.iter().all(|p| p.1.abs() > rw.width_m / 2.0));
    }
}

#[test]
fn epkk_every_stand_reaches_both_holding_points() {
    let text = std::fs::read_to_string(APT).unwrap();
    let ap = parse_apt_dat(&text, "EPKK").unwrap();
    let g = TaxiGraph::build(&ap.taxi);
    let mut failures = Vec::new();
    for rwy in ["07", "25"] {
        assert!(
            !g.holding_points(&ap, rwy).is_empty(),
            "no holding points for {rwy}"
        );
        for st in &ap.stands {
            let to = TaxiTarget::HoldingPoint { runway: rwy.into() };
            match g.route(&ap, st.pos, &to) {
                Err(e) => failures.push(format!("{} -> {rwy}: {e}", st.name)),
                Ok(r) => {
                    let end = *r.nodes.last().unwrap();
                    let node = |id: u32| ap.taxi.nodes.iter().find(|n| n.id == id).unwrap().pos;
                    let rw = ap
                        .runways
                        .iter()
                        .find(|r| r.ends.iter().any(|e| e.ident == rwy))
                        .unwrap();
                    let i = rw.ends.iter().position(|e| e.ident == rwy).unwrap();
                    let (thr, opp) = (rw.ends[i].threshold, rw.ends[1 - i].threshold);
                    let frame = RunwayFrame::new(thr, opp);
                    let (along, right) = frame.to_local(node(end));
                    if right.abs() <= rw.width_m / 2.0 {
                        failures.push(format!("{} -> {rwy}: ends on the runway", st.name));
                    }
                    if along >= distance_m(thr, opp) / 2.0 {
                        failures.push(format!("{} -> {rwy}: ends {along:.0} m along", st.name));
                    }
                    if end != g.holding_points(&ap, rwy)[0] {
                        failures.push(format!("{} -> {rwy}: not the first candidate", st.name));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
