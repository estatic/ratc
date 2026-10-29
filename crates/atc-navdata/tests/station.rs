use std::path::PathBuf;

use atc_navdata::procedures::parse_procedures;
use atc_navdata::station::{load_station, merge_overrides};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/airports/EPKK");

fn table(text: &str) -> toml::Table {
    text.parse().unwrap()
}

fn unique_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "atc-station-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn epkk_text() -> String {
    std::fs::read_to_string(format!("{DIR}/procedures.toml")).unwrap()
}

#[test]
fn round_trip() {
    let p = parse_procedures(&epkk_text()).unwrap();
    let text = toml::to_string(&p).unwrap();
    let again = parse_procedures(&text).unwrap();
    assert_eq!(p, again);
}

#[test]
fn override_replaces_scalar_and_array() {
    let base = table("preferred_runway = \"07\"\nvfr_holds = [\"A\"]");
    let over = table("preferred_runway = \"25\"\nvfr_holds = [\"KILO\", \"ZULU\"]");
    let merged = merge_overrides(base, &over);
    assert_eq!(merged["preferred_runway"].as_str(), Some("25"));
    assert_eq!(
        merged["vfr_holds"],
        toml::Value::Array(vec!["KILO".into(), "ZULU".into()])
    );
}

#[test]
fn override_merges_nested_table() {
    let base = table("ifr = { cruise_ft = 5000 }\ntma = { radius_nm = 30.0, ceiling_ft = 9500 }");
    let over = table("[tma]\nradius_nm = 40.0");
    let merged = merge_overrides(base, &over);
    assert_eq!(
        merged,
        table("ifr = { cruise_ft = 5000 }\ntma = { radius_nm = 40.0, ceiling_ft = 9500 }")
    );
}

#[test]
fn sources_table_is_not_merged() {
    let base = table("preferred_runway = \"07\"");
    let over = table("[sources]\npreferred_runway = \"user ruling\"");
    let merged = merge_overrides(base, &over);
    assert!(!merged.contains_key("sources"));
}

#[test]
fn load_station_prefers_station_toml() {
    let dir = unique_dir("prefers");
    let mut station = table(&epkk_text());
    station.insert("preferred_runway".into(), "07".into());
    std::fs::write(dir.join("station.toml"), station.to_string()).unwrap();
    std::fs::write(dir.join("procedures.toml"), epkk_text()).unwrap();
    std::fs::write(dir.join("overrides.toml"), "preferred_runway = \"25\"\n").unwrap();
    let p = load_station(&dir).unwrap();
    assert_eq!(p.preferred_runway, "25");
    std::fs::remove_file(dir.join("overrides.toml")).unwrap();
    assert_eq!(load_station(&dir).unwrap().preferred_runway, "07");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_station_falls_back_to_procedures_toml() {
    let dir = unique_dir("fallback");
    std::fs::write(dir.join("procedures.toml"), epkk_text()).unwrap();
    let p = load_station(&dir).unwrap();
    assert_eq!(p, parse_procedures(&epkk_text()).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_station_error_names_file_and_line() {
    let dir = unique_dir("error");
    std::fs::write(dir.join("station.toml"), "icao = \"X\"\nbad line\n").unwrap();
    let err = load_station(&dir).unwrap_err().to_string();
    assert!(err.contains("station.toml"), "{err}");
    assert!(err.contains("line 2"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn airspaces_default_empty() {
    let p = parse_procedures(&epkk_text()).unwrap();
    assert!(p.airspaces.is_empty());
}

#[test]
fn overrides_type_error_names_overrides_file() {
    let dir = unique_dir("over-type");
    std::fs::write(dir.join("station.toml"), epkk_text()).unwrap();
    std::fs::write(dir.join("overrides.toml"), "preferred_runway = 25\n").unwrap();
    let err = load_station(&dir).unwrap_err().to_string();
    assert!(err.contains("overrides.toml"), "{err}");
    assert!(!err.contains("line 0"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn station_type_error_names_station_file_and_line() {
    let dir = unique_dir("station-type");
    std::fs::write(dir.join("station.toml"), "icao = 5\n").unwrap();
    let err = load_station(&dir).unwrap_err().to_string();
    assert!(err.contains("station.toml"), "{err}");
    assert!(err.contains("line 1"), "{err}");
    assert!(!err.contains("line 0"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn round_trip_fully_populated() {
    use atc_navdata::procedures::{AirspaceDef, AirspaceKind, DeliveryHours, IfrDefaults, Tma};
    let mut p = parse_procedures(&epkk_text()).unwrap();
    p.tma = Some(Tma {
        radius_nm: 30.0,
        ceiling_ft: 9500,
    });
    p.ifr = Some(IfrDefaults { cruise_ft: 5000 });
    p.delivery_hours = Some(DeliveryHours {
        summer_utc: [5, 20],
        winter_utc: [6, 21],
    });
    p.approach_alt_khz = vec![118_000, 119_000];
    p.airspaces = vec![AirspaceDef {
        kind: AirspaceKind::Ctr,
        name: "TEST CTR".into(),
        class: Some('C'),
        lower_ft: 0,
        upper_ft: 3500,
        polygon: vec![[50.0, 19.0], [50.1, 19.2], [49.9, 19.3]],
    }];
    let again = parse_procedures(&toml::to_string(&p).unwrap()).unwrap();
    assert_eq!(p, again);
}
