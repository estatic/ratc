use atc_geo::LatLon;
use atc_navdata::source::{SimDataConfig, SimDataSource, SourceError};
use atc_navdata::xplane::{
    XPlaneSource, apt_dat_candidates, load_destinations, load_procedure_set, resolve_airport,
};
use std::path::Path;

const XP: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/xp");
const XPLANE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/xplane");

#[test]
fn xplane_source_layout_matches_resolve_airport() {
    let layout = XPlaneSource::new(XP).airport_layout("EPKK").unwrap();
    let resolved = resolve_airport(Path::new(XP), "EPKK").unwrap();
    assert_eq!(
        layout.parse("EPKK").unwrap(),
        resolved.parse("EPKK").unwrap()
    );
    assert_eq!(layout.apt_dat, resolved.text);
    assert_eq!(layout.ident, resolved.ident);
    assert_eq!(layout.origin, resolved.source.display().to_string());
}

#[test]
fn missing_airport_is_not_found() {
    let r = XPlaneSource::new(XP).airport_layout("QQQQ");
    assert!(matches!(r, Err(SourceError::NotFound(_))), "{r:?}");
}

#[test]
fn procedures_match_load_procedure_set() {
    let src = XPlaneSource::new(XPLANE);
    let got = src.procedure_set("ZZZZ").unwrap();
    let want = load_procedure_set(Path::new(XPLANE), "ZZZZ").unwrap();
    assert_eq!(got.icao, want.icao);
    assert_eq!(got.cycle, want.cycle);
    assert_eq!(got.procedures.len(), want.procedures.len());
    assert_eq!(got.fixes.len(), want.fixes.len());
    let r = src.procedure_set("EPKK");
    assert!(matches!(r, Err(SourceError::NotFound(_))), "{r:?}");
}

#[test]
fn airports_match_load_destinations() {
    let around = LatLon { lat: 0.0, lon: 0.0 };
    let got = XPlaneSource::new(XPLANE)
        .airports(around, f64::INFINITY)
        .unwrap();
    let want = load_destinations(Path::new(XPLANE), around, f64::INFINITY, &[]).unwrap();
    let icaos = |v: &[atc_navdata::model::Destination]| {
        v.iter().map(|d| d.icao.clone()).collect::<Vec<_>>()
    };
    assert!(!want.is_empty());
    assert_eq!(icaos(&got), icaos(&want));
}

#[test]
fn cache_inputs_are_the_apt_dat_candidates() {
    assert_eq!(
        XPlaneSource::new(XPLANE).cache_inputs(),
        apt_dat_candidates(Path::new(XPLANE))
    );
}

#[test]
fn nonexistent_root_is_not_found_not_panic() {
    let src = XPlaneSource::new("/nonexistent/X");
    assert!(matches!(
        src.airport_layout("EPKK"),
        Err(SourceError::NotFound(_))
    ));
    assert!(matches!(
        src.procedure_set("EPKK"),
        Err(SourceError::NotFound(_))
    ));
    assert!(src.cache_inputs().is_empty());
}

#[test]
fn sim_data_config_parses() {
    let cfg: SimDataConfig = toml::from_str("sim = \"xplane\"\nroot = \"/a\"").unwrap();
    assert_eq!(cfg, SimDataConfig::Xplane { root: "/a".into() });
    assert!(toml::from_str::<SimDataConfig>("sim = \"fs98\"\nroot = \"/a\"").is_err());
    assert!(
        toml::from_str::<SimDataConfig>("sim = \"xplane\"\nroot = \"/a\"\nrooot = \"/a\"").is_err()
    );
    assert_eq!(SimDataConfig::from_legacy_root("/a".into()), cfg);
}

#[test]
fn open_gives_the_xplane_source() {
    let src = SimDataConfig::from_legacy_root(XP.into()).open();
    assert_eq!(src.label(), format!("X-Plane scenery at {XP}"));
}
