//! Station generation from synthetic OpenAIP fixtures and the fictional ZZZZ X-Plane layout.

use std::path::{Path, PathBuf};

use atc_navdata::model::{Airport, FreqKind};
use atc_navdata::procedures::{AdjacentKind, AirspaceKind, Procedures};
use atc_stations::generate::{Choice, StationDoc, generate, offered_positions, write_station};
use atc_stations::openaip::{OaAirport, OaAirspace, OaReportingPoint};
use atc_stations::store::CountryData;

fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(p).unwrap()
}

fn data() -> CountryData {
    CountryData {
        airports: serde_json::from_str::<Vec<OaAirport>>(&fixture("zzzz_airports.json")).unwrap(),
        airspaces: serde_json::from_str::<Vec<OaAirspace>>(&fixture("zzzz_airspaces.json"))
            .unwrap(),
        points: serde_json::from_str::<Vec<OaReportingPoint>>(&fixture("zzzz_points.json"))
            .unwrap(),
        synced_at: "2026-10-29T00:00:00Z".into(),
    }
}

fn layout() -> Airport {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../atc-navdata/tests/fixtures/xplane/Resources/default scenery/default apt dat/Earth nav data/apt.dat");
    atc_navdata::apt::parse_apt_dat(&std::fs::read_to_string(p).unwrap(), "ZZZZ").unwrap()
}

fn doc_with(layout: &Airport, country: &str) -> StationDoc {
    let mut d = data();
    d.airports[0].country = country.into();
    let ap = d.airports[0].clone();
    let choice = Choice {
        positions: offered_positions(&ap),
    };
    generate(&ap, &d, layout, &choice, "2026-10-29")
}

fn doc() -> StationDoc {
    doc_with(&layout(), "ZZ")
}

#[test]
fn positions_follow_type_table() {
    let d = data();
    let p = offered_positions(&d.airports[0]);
    let roles: Vec<_> = p.iter().map(|x| x.role).collect();
    assert_eq!(
        roles,
        [
            FreqKind::Atis,
            FreqKind::Delivery,
            FreqKind::Ground,
            FreqKind::Tower,
            FreqKind::Approach
        ]
    );
    let calls: Vec<_> = p.iter().map(|x| x.callsign.as_str()).collect();
    assert_eq!(
        calls,
        [
            "Zeta ATIS",
            "Zeta Delivery",
            "Zeta Ground",
            "Zeta Tower",
            "Zeta Approach"
        ]
    );
    assert_eq!(p[3].khz, 118_300);
}

#[test]
fn approach_primary_first_rest_alternate() {
    let doc = doc();
    let app = doc
        .procs
        .positions
        .iter()
        .find(|p| p.role == FreqKind::Approach)
        .unwrap();
    assert_eq!(app.khz, 119_900);
    assert_eq!(doc.procs.approach_alt_khz, [119_100]);
}

#[test]
fn approach_without_primary_takes_lowest() {
    let mut d = data();
    for f in &mut d.airports[0].frequencies {
        f.primary = false;
    }
    let ap = d.airports[0].clone();
    let app = offered_positions(&ap)
        .into_iter()
        .find(|p| p.role == FreqKind::Approach)
        .unwrap();
    assert_eq!(app.khz, 119_100);
}

#[test]
fn acc_metre_limit_converts() {
    let doc = doc();
    let acc = doc
        .procs
        .airspaces
        .iter()
        .find(|a| a.name == "ZETA ACC")
        .unwrap();
    assert_eq!(acc.upper_ft, 2165);
}

#[test]
fn accented_fis_suffix_stripped_and_icao_provenance() {
    let mut d = data();
    d.airspaces[3].name = "FIS ZETA-ZACHÓD".into();
    d.airports[0].icao_code = None;
    d.airports[0].elevation = None;
    let ap = d.airports[0].clone();
    let choice = Choice {
        positions: offered_positions(&ap),
    };
    let doc = generate(&ap, &d, &layout(), &choice, "2026-10-29");
    assert!(
        doc.procs
            .adjacent
            .iter()
            .any(|a| a.callsign == "Zeta Information")
    );
    assert_eq!(doc.procs.icao, "ZZZZ");
    assert_eq!(doc.provenance["icao"], "X-Plane apt.dat");
    // No OpenAIP elevation: the layout's 100 ft is used for the AGL ceiling.
    let ctr = doc
        .procs
        .airspaces
        .iter()
        .find(|a| a.name == "ZETA CTR")
        .unwrap();
    assert_eq!(ctr.upper_ft, 1600);
}

#[test]
fn only_ticked_positions_are_kept() {
    let d = data();
    let ap = d.airports[0].clone();
    let mut p = offered_positions(&ap);
    p.retain(|x| x.role == FreqKind::Tower);
    let doc = generate(&ap, &d, &layout(), &Choice { positions: p }, "2026-10-29");
    assert_eq!(doc.procs.positions.len(), 1);
    assert!(doc.procs.approach_alt_khz.is_empty());
}

#[test]
fn circuit_height_rounds_up() {
    let doc = doc();
    assert_eq!(doc.procs.circuits.len(), 2);
    assert!(doc.procs.circuits.iter().all(|c| c.altitude_ft_msl == 1100));
    let mut l = layout();
    l.elevation_ft = 791;
    let doc = doc_with(&l, "ZZ");
    assert!(doc.procs.circuits.iter().all(|c| c.altitude_ft_msl == 1800));
}

#[test]
fn preferred_runway_closest_to_270() {
    assert_eq!(doc().procs.preferred_runway, "27");
}

#[test]
fn vfr_points_of_this_airport_only() {
    let names: Vec<_> = doc()
        .procs
        .vfr_points
        .iter()
        .map(|p| p.name.clone())
        .collect();
    assert_eq!(names, ["NOVEMBER", "SIERRA"]);
    assert!(doc().procs.vfr_points.iter().all(|p| !p.south));
    let n = &doc().procs.vfr_points[0];
    assert_eq!((n.lat, n.lon), (10.1, 10.0));
}

#[test]
fn tma_from_smallest_containing() {
    let tma = doc().procs.tma.unwrap();
    assert_eq!(tma.ceiling_ft, 6500);
    // Half-width 0.5 degrees: vertices are 0.5*sqrt(2) deg away, about 42 NM.
    assert!((tma.radius_nm - 42.0).abs() <= 2.0, "{}", tma.radius_nm);
}

#[test]
fn agl_limits_become_amsl() {
    let doc = doc();
    let ctr = doc
        .procs
        .airspaces
        .iter()
        .find(|a| a.kind == AirspaceKind::Ctr)
        .unwrap();
    assert_eq!((ctr.lower_ft, ctr.upper_ft), (0, 1600));
    assert_eq!(ctr.class, Some('D'));
    let tma = doc
        .procs
        .airspaces
        .iter()
        .find(|a| a.name == "ZETA TMA")
        .unwrap();
    assert_eq!((tma.lower_ft, tma.upper_ft), (1500, 6500));
    // Airspaces that do not contain the reference point are left out.
    assert!(doc.procs.airspaces.iter().all(|a| a.name != "FAR CTA"));
}

#[test]
fn fis_callsign_from_sector_name() {
    let doc = doc();
    let fis = doc
        .procs
        .adjacent
        .iter()
        .find(|a| a.kind == AdjacentKind::Fis)
        .unwrap();
    assert_eq!(fis.callsign, "Zeta Information");
    assert_eq!(fis.khz, 119_500);
    let acc = doc
        .procs
        .adjacent
        .iter()
        .find(|a| a.kind == AdjacentKind::Acc)
        .unwrap();
    assert_eq!((acc.callsign.as_str(), acc.khz), ("Zeta Control", 133_250));
    assert_eq!(acc.min_level_ft, Some(9500));
}

#[test]
fn adjacent_towers_within_100_km() {
    let doc = doc();
    let towers: Vec<_> = doc
        .procs
        .adjacent
        .iter()
        .filter(|a| a.kind == AdjacentKind::Tower)
        .collect();
    assert_eq!(towers.len(), 1);
    // From the tower frequency's name, "YONDER TWR".
    assert_eq!(towers[0].callsign, "Yonder Tower");
    assert_eq!(towers[0].airport.as_deref(), Some("YYYY"));
    assert_eq!(towers[0].khz, 122_500);
}

#[test]
fn adjacent_tower_callsign_without_a_place_in_the_name() {
    let mut d = data();
    let yonder = d
        .airports
        .iter_mut()
        .find(|a| a.name == "YONDER FIELD")
        .unwrap();
    yonder.name = "YONDER/SOUTH".into();
    yonder.frequencies[0].name = Some("TOWER".into());
    let tower = |d: &CountryData| {
        let ap = d.airports[0].clone();
        let choice = Choice {
            positions: offered_positions(&ap),
        };
        generate(&ap, d, &layout(), &choice, "2026-10-29")
            .procs
            .adjacent
            .into_iter()
            .find(|a| a.kind == AdjacentKind::Tower)
            .unwrap()
            .callsign
    };
    assert_eq!(tower(&d), "Yonder Tower");
    d.airports
        .iter_mut()
        .find(|a| a.name == "YONDER/SOUTH")
        .unwrap()
        .frequencies[0]
        .name = None;
    assert_eq!(tower(&d), "Yonder Tower");
}

#[test]
fn unknown_country_ta_default() {
    let doc = doc();
    assert_eq!(doc.procs.transition_altitude_ft, 5000);
    assert_eq!(
        doc.provenance["transition_altitude_ft"],
        "default: no national value known"
    );
    let pl = doc_with(&layout(), "PL");
    assert_eq!(pl.procs.transition_altitude_ft, 6500);
    assert_eq!(
        pl.provenance["transition_altitude_ft"],
        "default: AIP Polska ENR 1.7"
    );
}

#[test]
fn simple_fields() {
    let p = doc().procs;
    assert_eq!(p.icao, "ZZZZ");
    assert_eq!(p.magnetic_variation_deg, 4.5);
    assert_eq!(p.ifr.unwrap().cruise_ft, 5000);
    assert!(p.delivery_hours.is_none());
    assert_eq!(p.source, "OpenAIP 2026-10-29, X-Plane apt.dat");
    assert_eq!(p.airac, "");
}

#[test]
fn provenance_for_every_top_level_field() {
    let doc = doc();
    for k in [
        "icao",
        "source",
        "airac",
        "transition_altitude_ft",
        "circuits",
        "preferred_runway",
        "positions",
        "vfr_points",
        "vfr_holds",
        "vfr_hold_max_ft",
        "adjacent",
        "tma",
        "delivery_hours",
        "ifr",
        "approach_alt_khz",
        "magnetic_variation_deg",
        "airspaces",
    ] {
        assert!(doc.provenance.contains_key(k), "missing provenance for {k}");
    }
    assert_eq!(doc.provenance["circuits"], "X-Plane apt.dat");
    assert_eq!(doc.provenance["positions"], "OpenAIP");
}

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vatc-gen-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn write_station_has_source_comments() {
    let doc = doc();
    let dir = tmp("comments");
    let path = dir.join("station.toml");
    write_station(&doc, &path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# source: OpenAIP\nicao = "), "{text}");
    assert!(text.contains("# source: default: closest to 270°\npreferred_runway"));
    assert!(
        text.contains("# source: X-Plane apt.dat\n[[circuits]]"),
        "{text}"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn written_station_loads_back_identical() {
    let doc = doc();
    let dir = tmp("roundtrip");
    write_station(&doc, &dir.join("station.toml")).unwrap();
    let back: Procedures = atc_navdata::station::load_station(&dir).unwrap();
    assert_eq!(back, doc.procs);
    std::fs::remove_dir_all(dir).ok();
}
