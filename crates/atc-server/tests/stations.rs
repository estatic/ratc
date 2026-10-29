use std::path::Path;

use atc_navdata::procedures::Procedures;
use atc_server::layout::load_procedures;

const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/airports/EPKK");

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("atc-server-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn server_falls_back_to_procedures_toml() {
    let dir = temp_dir("fallback");
    for f in ["procedures.toml", "apt.dat"] {
        std::fs::copy(Path::new(REPO).join(f), dir.join(f)).unwrap();
    }
    let procs = load_procedures(&dir).unwrap();
    assert_eq!(procs.preferred_runway, "25");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn server_prefers_station_toml() {
    let dir = temp_dir("station");
    std::fs::copy(
        Path::new(REPO).join("procedures.toml"),
        dir.join("procedures.toml"),
    )
    .unwrap();
    let text = std::fs::read_to_string(dir.join("procedures.toml")).unwrap();
    let mut procs: Procedures = toml::from_str(&text).unwrap();
    procs.preferred_runway = "07".into();
    std::fs::write(dir.join("station.toml"), toml::to_string(&procs).unwrap()).unwrap();
    assert_eq!(load_procedures(&dir).unwrap().preferred_runway, "07");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn missing_station_files_name_the_file() {
    let err = load_procedures(Path::new("/nonexistent/EPKK")).unwrap_err();
    assert!(format!("{err:#}").contains("procedures.toml"));
}

mod multi {
    use std::path::{Path, PathBuf};

    use atc_core::engine::EngineConfig;
    use atc_core::readback::Strictness;
    use atc_server::layout::load_procedures;
    use atc_server::stations::load_stations;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/stations");
    const EPKK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/airports/EPKK");

    fn cfg() -> EngineConfig {
        EngineConfig::new(Strictness::Standard)
    }

    fn data_dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("atc-server-multi-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn copy_station(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for f in ["procedures.toml", "apt.dat"] {
            std::fs::copy(from.join(f), to.join(f)).unwrap();
        }
    }

    #[test]
    fn loads_every_station_dir() {
        let list = load_stations(Path::new(FIXTURES), None, None, &cfg()).unwrap();
        let ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["ZZZY", "ZZZZ"]);
        assert_eq!(list[1].airport.icao, "ZZZZ");
        assert_eq!(list[1].procs.icao, "ZZZZ");
        assert_eq!(list[0].dir, Path::new(FIXTURES).join("ZZZY"));
    }

    #[test]
    fn only_loads_the_configured_station() {
        let list = load_stations(Path::new(FIXTURES), Some("ZZZY"), None, &cfg()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "ZZZY");
        assert!(load_stations(Path::new(FIXTURES), Some("NOPE"), None, &cfg()).is_err());
    }

    #[test]
    fn broken_station_is_skipped_with_warning() {
        let dir = data_dir("broken");
        copy_station(&Path::new(FIXTURES).join("ZZZZ"), &dir.join("ZZZZ"));
        std::fs::create_dir_all(dir.join("BAD")).unwrap();
        std::fs::write(dir.join("BAD/procedures.toml"), "not = [valid").unwrap();
        std::fs::write(dir.join("enroute.toml"), "").unwrap();
        let list = load_stations(&dir, None, None, &cfg()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "ZZZZ");
        // Nothing loads: an error.
        std::fs::remove_dir_all(dir.join("ZZZZ")).unwrap();
        assert!(load_stations(&dir, None, None, &cfg()).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn single_station_config_unchanged() {
        let dir = data_dir("single");
        copy_station(Path::new(EPKK), &dir.join("EPKK"));
        let list = load_stations(&dir, Some("EPKK"), None, &cfg()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].procs, load_procedures(&dir.join("EPKK")).unwrap());
        assert_eq!(list[0].airport.icao, "EPKK");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
