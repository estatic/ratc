use atc_navdata::model::{EdgeKind, FreqKind, StandCategory, StandKind, ZoneKind};
use atc_navdata::{NavdataError, parse_apt_dat};

const FIX: &str = include_str!("fixtures/mini.dat");

#[test]
fn parses_header() {
    let a = parse_apt_dat(FIX, "EPKK").unwrap();
    assert_eq!(a.icao, "EPKK");
    assert_eq!(a.name, "Krakow John Paul II Intl");
    assert_eq!(a.elevation_ft, 791);
}

#[test]
fn parses_runway_ends() {
    let a = parse_apt_dat(FIX, "EPKK").unwrap();
    assert_eq!(a.runways.len(), 1);
    let r = &a.runways[0];
    assert_eq!(r.width_m, 45.0);
    assert_eq!(r.ends[0].ident, "07");
    assert_eq!(r.ends[0].threshold.lat, 50.071);
    assert_eq!(r.ends[0].threshold.lon, 19.77);
    assert_eq!(r.ends[1].ident, "25");
    assert_eq!(r.ends[1].threshold.lat, 50.08);
    assert_eq!(r.ends[1].threshold.lon, 19.80);
    assert_eq!(r.ends[1].displaced_m, 120.5);
}

#[test]
fn parses_freq_both_formats() {
    let a = parse_apt_dat(FIX, "EPKK").unwrap();
    assert_eq!(a.frequencies.len(), 3);
    assert_eq!(a.frequencies[0].kind, FreqKind::Atis);
    assert_eq!(a.frequencies[0].khz, 122750);
    assert_eq!(a.frequencies[1].kind, FreqKind::Tower);
    assert_eq!(a.frequencies[1].khz, 118100);
    assert_eq!(a.frequencies[1].name, "KRAKOW TOWER");
    assert_eq!(a.frequencies[2].khz, 118105);
}

#[test]
fn parses_taxi_with_active_zone() {
    let a = parse_apt_dat(FIX, "EPKK").unwrap();
    assert_eq!(a.taxi.nodes.len(), 6);
    assert_eq!(a.taxi.edges.len(), 6);
    assert_eq!(a.taxi.nodes[2].id, 2);
    assert_eq!(a.taxi.nodes[2].name, "N3");
    let e2 = &a.taxi.edges[2];
    assert!(e2.one_way);
    assert_eq!(e2.kind, EdgeKind::Taxiway);
    assert_eq!(e2.name, "B");
    let rwy = &a.taxi.edges[3];
    assert_eq!(rwy.kind, EdgeKind::Runway);
    assert_eq!((rwy.from, rwy.to), (3, 4));
    assert_eq!(rwy.active_zones.len(), 1);
    assert_eq!(rwy.active_zones[0].kind, ZoneKind::Departure);
    assert_eq!(rwy.active_zones[0].runways, vec!["07", "25"]);
    assert!(a.taxi.edges[4].active_zones.is_empty());
}

#[test]
fn parses_stands() {
    let a = parse_apt_dat(FIX, "EPKK").unwrap();
    assert_eq!(a.stands.len(), 2);
    assert_eq!(a.stands[0].name, "A1");
    assert_eq!(a.stands[0].heading_deg, 90.5);
    assert_eq!(a.stands[0].pos.lat, 50.07);
    assert_eq!(a.stands[1].name, "B2 Stand");
}

#[test]
fn stops_at_next_airport() {
    let a = parse_apt_dat(FIX, "EPKK").unwrap();
    assert_eq!(a.runways.len(), 1);
    assert_eq!(a.stands.len(), 2);
    assert_eq!(a.frequencies.len(), 3);
    let b = parse_apt_dat(FIX, "EPKA").unwrap();
    assert_eq!(b.name, "Other Airport");
    assert_eq!(b.elevation_ft, 100);
    assert_eq!(b.stands.len(), 1);
    assert_eq!(b.frequencies[0].khz, 120000);
}

#[test]
fn missing_airport_is_error() {
    assert!(matches!(
        parse_apt_dat(FIX, "ZZZZ"),
        Err(NavdataError::AirportNotFound(_))
    ));
}

/// Row 1300 keeps the stand type (field 4) and the aircraft categories (field 5, `|`-separated;
/// `all` is no restriction).
#[test]
fn stand_row_keeps_type_and_categories() {
    let text = "I\n1100 Version\n1 1000 0 0 EPKT Test\n\
        1300 50.47 19.08 270.0 tie_down props APRON4 GA\n\
        1300 50.47 19.08 270.0 gate heavy|jets 81\n\
        1300 50.47 19.08 270.0 hangar all H1\n\
        1300 50.47 19.08 270.0 misc heavy 70 de-ice\n\
        99\n";
    let a = parse_apt_dat(text, "EPKT").unwrap();
    assert_eq!(a.stands[0].name, "APRON4 GA");
    assert_eq!(a.stands[0].kind, StandKind::TieDown);
    assert_eq!(a.stands[0].categories, vec![StandCategory::Props]);
    assert_eq!(a.stands[1].kind, StandKind::Gate);
    assert_eq!(
        a.stands[1].categories,
        vec![StandCategory::Heavy, StandCategory::Jets]
    );
    assert_eq!(a.stands[2].kind, StandKind::Hangar);
    assert!(a.stands[2].categories.is_empty());
    assert_eq!(a.stands[3].kind, StandKind::Misc);
    assert_eq!(a.stands[3].name, "70 de-ice");
}
