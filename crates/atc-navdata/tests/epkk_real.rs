use atc_navdata::model::FreqKind;
use atc_navdata::parse_apt_dat;
use atc_navdata::procedures::{CircuitSide, parse_procedures};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/airports/EPKK");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{DIR}/{name}")).unwrap()
}

fn airport() -> atc_navdata::model::Airport {
    parse_apt_dat(&read("apt.dat"), "EPKK").unwrap()
}

#[test]
fn epkk_has_runway_07_25() {
    let ap = airport();
    assert!(
        ap.runways
            .iter()
            .any(|r| r.ends[0].ident == "07" && r.ends[1].ident == "25")
    );
    assert!(!ap.taxi.nodes.is_empty() && !ap.taxi.edges.is_empty());
}

#[test]
fn epkk_has_ground_and_tower_freqs() {
    let ap = airport();
    for kind in [FreqKind::Ground, FreqKind::Tower] {
        assert!(ap.frequencies.iter().any(|f| f.kind == kind), "{kind:?}");
    }
}

#[test]
fn procedures_cover_both_runways() {
    let p = parse_procedures(&read("procedures.toml")).unwrap();
    assert_eq!(p.icao, "EPKK");
    assert_eq!(p.transition_altitude_ft, 6500);
    assert_eq!(p.preferred_runway, "25");
    for rwy in ["07", "25"] {
        let c = p.circuits.iter().find(|c| c.runway == rwy).unwrap();
        assert_eq!(c.side, CircuitSide::Left);
        assert_eq!(c.altitude_ft_msl, 1800);
    }
    assert!(!p.source.is_empty() && !p.airac.is_empty());
}

#[test]
fn procedures_freqs_match_positions() {
    // AIP uses 8.33 kHz channels, apt.dat 25 kHz labels: allow 5 kHz (see SOURCE.md).
    let ap = airport();
    let p = parse_procedures(&read("procedures.toml")).unwrap();
    for pos in &p.positions {
        assert!(
            ap.frequencies
                .iter()
                .any(|f| f.kind == pos.role && f.khz.abs_diff(pos.khz) <= 5),
            "{pos:?}"
        );
    }
    assert!(
        p.positions
            .iter()
            .any(|x| x.role == FreqKind::Tower && x.khz == 123_255)
    );
    assert!(read("SOURCE.md").contains("8.33"));
}

#[test]
fn procedures_reject_garbage() {
    assert!(parse_procedures("not = [valid").is_err());
}
