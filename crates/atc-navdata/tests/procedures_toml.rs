use std::collections::BTreeSet;

use atc_navdata::model::FreqKind;
use atc_navdata::procedures::{AdjacentKind, parse_procedures};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/airports/EPKK");

#[test]
fn epkk_procedures_have_vfr_points_and_units() {
    let p = parse_procedures(&std::fs::read_to_string(format!("{DIR}/procedures.toml")).unwrap())
        .unwrap();
    assert_eq!(p.vfr_points.len(), 13);
    let zulu = p.vfr_point("zulu").unwrap();
    assert!((zulu.lat - (50.0 + 6.0 / 60.0 + 33.0 / 3600.0)).abs() < 1e-6);
    assert!((zulu.lon - (19.0 + 46.0 / 60.0 + 46.0 / 3600.0)).abs() < 1e-6);
    let south: BTreeSet<&str> = p
        .vfr_points
        .iter()
        .filter(|v| v.south)
        .map(|v| v.name.as_str())
        .collect();
    assert_eq!(south, BTreeSet::from(["KILO", "SIERRA"]));
    assert!(p.vfr_point("NOPE").is_none());
    assert_eq!(p.vfr_holds, ["KILO", "ZULU"]);
    assert_eq!(p.vfr_hold_max_ft, Some(2000));

    assert_eq!(p.adjacent.len(), 3);
    let acc = &p.adjacent[0];
    assert_eq!(
        (acc.khz, acc.kind, acc.min_level_ft),
        (127_660, AdjacentKind::Acc, Some(9500))
    );
    assert!(
        p.adjacent
            .iter()
            .any(|a| a.kind == AdjacentKind::Fis && a.khz == 119_275)
    );
    assert!(
        p.adjacent
            .iter()
            .any(|a| a.kind == AdjacentKind::Tower && a.airport.as_deref() == Some("EPKT"))
    );

    assert!(
        p.positions
            .iter()
            .any(|x| x.role == FreqKind::Approach && x.khz == 121_075)
    );
    assert_eq!(p.approach_alt_khz, [126_530, 135_405]);
    assert_eq!(p.magnetic_variation_deg, 6.0);
    let tma = p.tma.unwrap();
    assert_eq!((tma.radius_nm, tma.ceiling_ft), (40.0, 9500));
    let d = p.delivery_hours.unwrap();
    assert_eq!((d.summer_utc, d.winter_utc), ([6, 22], [5, 21]));
    assert_eq!(p.ifr.unwrap().cruise_ft, 5000);
}

#[test]
fn old_procedures_file_still_parses() {
    let text = r#"
icao = "ZZZZ"
source = "t"
airac = "t"
transition_altitude_ft = 5000
preferred_runway = "09"
circuits = []
[[positions]]
role = "Tower"
callsign = "Z Tower"
khz = 118000
"#;
    let p = parse_procedures(text).unwrap();
    assert!(p.vfr_points.is_empty() && p.adjacent.is_empty());
    assert!(p.tma.is_none() && p.delivery_hours.is_none() && p.ifr.is_none());
    assert!(p.vfr_point("X").is_none());
}
