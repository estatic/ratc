use std::path::Path;

use atc_navdata::procedures::parse_procedures;
use atc_server::layout::{
    adjacent_names, load_airport, load_destinations_near, load_procedure_set_for, taxiway_names,
};

const XP: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../atc-navdata/tests/fixtures/xp"
);
const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/airports/EPKK");

#[test]
fn layout_comes_from_x_plane_when_configured() {
    let ap = load_airport(Some(Path::new(XP)), Path::new(REPO), "EPKK").unwrap();
    assert_eq!(ap.icao, "EPKK");
    assert_eq!(ap.name, "Global Krakow");
    assert_eq!(taxiway_names(&ap), ["A", "J"]);
}

#[test]
fn layout_falls_back_to_the_repo() {
    for xp in [None, Some(Path::new("/nonexistent/X-Plane 11"))] {
        let ap = load_airport(xp, Path::new(REPO), "EPKK").unwrap();
        assert_eq!(ap.name, "Krakow-Balice");
        assert!(taxiway_names(&ap).iter().any(|n| n == "W1"));
    }
}

#[test]
fn missing_repo_layout_is_an_error() {
    assert!(load_airport(None, Path::new("/nonexistent/EPKK"), "EPKK").is_err());
}

const XPLANE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../atc-navdata/tests/fixtures/xplane"
);

#[test]
fn procedure_set_loads_from_the_install_cifp() {
    let set = load_procedure_set_for(Some(Path::new(XPLANE)), "ZZZZ").unwrap();
    assert_eq!(set.icao, "ZZZZ");
    assert!(!set.procedures.is_empty());
}

#[test]
fn procedure_set_is_none_without_cifp_or_install() {
    // The install has no CIFP for this airport, or there is no install configured.
    assert!(load_procedure_set_for(Some(Path::new(XPLANE)), "EPKK").is_none());
    assert!(load_procedure_set_for(Some(Path::new("/nonexistent/X-Plane 11")), "EPKK").is_none());
    assert!(load_procedure_set_for(None, "EPKK").is_none());
}

#[test]
fn adjacent_towers_give_destination_names() {
    let text = std::fs::read_to_string(format!("{REPO}/procedures.toml")).unwrap();
    let procs = parse_procedures(&text).unwrap();
    assert_eq!(
        adjacent_names(&procs),
        [("EPKT".to_string(), "Katowice".to_string())]
    );
}

#[test]
fn destinations_come_from_the_install_and_are_empty_without_it() {
    let ap = load_airport(Some(Path::new(XPLANE)), Path::new("/nonexistent"), "ZZZZ").unwrap();
    let names = [("ZZZY".to_string(), "Testville".to_string())];
    let list = load_destinations_near(Some(Path::new(XPLANE)), &ap, &names);
    assert!(
        list.iter()
            .any(|d| d.icao == "ZZZY" && d.name == "Testville")
    );
    assert!(load_destinations_near(None, &ap, &names).is_empty());
}
