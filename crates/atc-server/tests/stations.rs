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

mod destinations {
    use std::path::{Path, PathBuf};

    use atc_server::layout::load_all_destinations;

    fn tree(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("atc-server-dest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn airport(icao: &str, name: &str, lat: f64, lon: f64) -> String {
        format!(
            "1    100 0 0 {icao} {name}\n1302 datum_lat {lat:.9}\n1302 datum_lon {lon:.9}\n\
             100 45.00 1 0 0.25 1 0 1 09 {lat:.8} {lon:.8} 0.00 0.00 3 0 0 0 \
             27 {lat:.8} {lon2:.8} 0.00 0.00 3 0 0 0\n",
            lon2 = lon + 0.03
        )
    }

    /// An X-Plane-like tree whose default apt.dat holds `airports`; returns the file.
    fn xplane(root: &Path, airports: &[(&str, &str, f64, f64)]) -> PathBuf {
        let dir = root.join("Resources/default scenery/default apt dat/Earth nav data");
        std::fs::create_dir_all(&dir).unwrap();
        let mut text = String::from("I\n1100 Version - synthetic test airports\n");
        for (icao, name, lat, lon) in airports {
            text += &airport(icao, name, *lat, *lon);
        }
        text += "99\n";
        let apt = dir.join("apt.dat");
        std::fs::write(&apt, text).unwrap();
        apt
    }

    const HOME: (&str, &str, f64, f64) = ("ZZZZ", "ZETA INTERNATIONAL", 10.0, 10.0);
    const FAR: (&str, &str, f64, f64) = ("ZZZF", "FARAWAY FIELD", 62.5, 120.25);

    #[test]
    fn destination_far_away_is_known() {
        let root = tree("far");
        xplane(&root, &[HOME, FAR]);
        let list = load_all_destinations(&root, &root.join("cache/destinations.json"), &[]);
        let far = list.iter().find(|d| d.icao == "ZZZF").expect("far airport");
        assert_eq!(far.name, "Faraway Field");
        assert!((far.pos.lat - 62.5).abs() < 0.01 && (far.pos.lon - 120.25).abs() < 0.1);
        assert!(list.iter().any(|d| d.icao == "ZZZZ"));
        let names = [("ZZZF".to_string(), "Farland".to_string())];
        let list = load_all_destinations(&root, &root.join("cache/destinations.json"), &names);
        assert!(list.iter().any(|d| d.icao == "ZZZF" && d.name == "Farland"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn destinations_cache_rebuilt_when_apt_dat_changes() {
        let root = tree("cache");
        let apt = xplane(&root, &[HOME]);
        let cache = root.join("cache/destinations.json");
        assert_eq!(load_all_destinations(&root, &cache, &[]).len(), 1);
        assert!(cache.is_file());

        // Unchanged apt.dat: the scan is not repeated (a marker planted in the cache shows).
        let text = std::fs::read_to_string(&cache).unwrap();
        std::fs::write(&cache, text.replace("Zeta International", "Cached Name")).unwrap();
        let list = load_all_destinations(&root, &cache, &[]);
        assert_eq!(list[0].name, "Cached Name");

        // Changed apt.dat (size differs): rebuilt.
        xplane(&root, &[HOME, FAR]);
        assert_ne!(std::fs::metadata(&apt).unwrap().len(), 0);
        let list = load_all_destinations(&root, &cache, &[]);
        assert_eq!(list.len(), 2);
        let home = list.iter().find(|d| d.icao == "ZZZZ").unwrap();
        assert_eq!(home.name, "Zeta International");

        // An unreadable cache is rebuilt too.
        std::fs::write(&cache, "{not json").unwrap();
        assert_eq!(load_all_destinations(&root, &cache, &[]).len(), 2);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
