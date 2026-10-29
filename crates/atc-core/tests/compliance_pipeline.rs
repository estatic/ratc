#![cfg(feature = "testkit")]
//! Real pipeline: synthetic circuit -> PhaseTracker -> ComplianceMonitor, with the permissions a
//! correct controller would issue. A compliant flight must produce no deviations.

use atc_core::compliance::{ComplianceMonitor, Permissions, hold_line_cross_m};
use atc_core::phase::{FlightPhase, PhaseTracker};
use atc_core::taxi::TaxiGraph;
use atc_core::testkit::circuit_track;
use atc_geo::{RunwayFrame, distance_m};
use atc_navdata::parse_apt_dat;
use atc_navdata::procedures::parse_procedures;
use atc_phraseology::CircuitLeg;

const APT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/airports/EPKK/apt.dat"
);
const PROCS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/airports/EPKK/procedures.toml"
);

#[test]
fn compliant_circuit_has_no_deviations() {
    let ap = parse_apt_dat(&std::fs::read_to_string(APT).unwrap(), "EPKK").unwrap();
    let procs = parse_procedures(&std::fs::read_to_string(PROCS).unwrap()).unwrap();
    let rw = ap
        .runways
        .iter()
        .find(|r| r.ends.iter().any(|e| e.ident == "25"))
        .unwrap();
    let i = rw.ends.iter().position(|e| e.ident == "25").unwrap();
    let (thr, opp) = (rw.ends[i].threshold, rw.ends[1 - i].threshold);
    let frame = RunwayFrame::new(thr, opp);
    let length = distance_m(thr, opp);
    let circuit = procs.circuits.iter().find(|c| c.runway == "25").unwrap();
    let g = TaxiGraph::build(&ap.taxi);
    let holds: Vec<_> = g
        .holding_points(&ap, "25")
        .into_iter()
        .map(|id| ap.taxi.nodes.iter().find(|n| n.id == id).unwrap().pos)
        .collect();
    let cross = hold_line_cross_m(&frame, &holds).unwrap();
    let circuit_ft = circuit.altitude_ft_msl;

    for dt in [0.5, 1.0, 2.0] {
        let mut tracker = PhaseTracker::new(
            frame,
            length,
            rw.width_m / 2.0,
            circuit.side,
            f64::from(ap.elevation_ft),
            holds.clone(),
            ap.stands.iter().map(|s| s.pos).collect(),
        );
        let mut mon = ComplianceMonitor::new(
            "25",
            frame,
            length,
            rw.width_m / 2.0,
            cross,
            f64::from(circuit_ft),
        );
        let (mut cleared_dep, mut cleared_land) = (false, false);
        let mut samples = 0;
        for s in circuit_track(&ap, &procs, "25", dt) {
            let phase = tracker.update(&s);
            match phase {
                FlightPhase::HoldingShort => cleared_dep = true,
                FlightPhase::Circuit(CircuitLeg::Final) => cleared_land = true,
                _ => {}
            }
            if matches!(phase, FlightPhase::Circuit(_)) {
                cleared_dep = false;
            }
            if phase == FlightPhase::TaxiIn {
                cleared_land = false;
            }
            let perm = Permissions {
                may_enter_runway: cleared_dep,
                may_take_off: cleared_dep,
                may_land: cleared_land,
                assigned_alt_ft: (phase == FlightPhase::Circuit(CircuitLeg::Downwind))
                    .then_some(circuit_ft),
            };
            let dev = mon.update(&s, phase, &perm);
            assert!(dev.is_empty(), "dt={dt} t={} {phase:?}: {dev:?}", s.t.0);
            samples += 1;
        }
        assert!(samples > 100);
    }
}
