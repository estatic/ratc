use std::path::{Path, PathBuf};

use atc_geo::{LatLon, distance_m};
use atc_navdata::cifp::{ApproachKind, PathTerm};
use atc_navdata::parse_apt_dat;
use atc_navdata::xplane::{ResolveError, load_destinations, load_procedure_set, resolve_airport};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xplane")
}

fn xp_install() -> Option<PathBuf> {
    let root = std::env::var_os("ATC_XPLANE_ROOT").map_or_else(
        || {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local/share/Steam/steamapps/common/X-Plane 11")
        },
        PathBuf::from,
    );
    if root.join("Resources/default data/CIFP/EPKK.dat").is_file() {
        Some(root)
    } else {
        println!(
            "skipped: no X-Plane install with CIFP/EPKK.dat at {}",
            root.display()
        );
        None
    }
}

fn near(p: LatLon, lat: f64, lon: f64) -> bool {
    distance_m(p, LatLon { lat, lon }) < 50.0
}

/// A scratch copy of the fixture's `Resources` so a test can add `Custom Data`.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("atc-navdata-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixture_root(), &dir);
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

const HOME: LatLon = LatLon {
    lat: 10.0,
    lon: 10.0,
};

#[test]
fn destinations_within_radius() {
    let all = load_destinations(&fixture_root(), HOME, 100.0, &[]).unwrap();
    let icaos: Vec<&str> = all.iter().map(|d| d.icao.as_str()).collect();
    assert_eq!(icaos, ["ZZZY", "ZZZZ"]);
    assert_eq!(all[0].name, "Testville");
    assert_eq!(all[1].name, "Zeta Zulu International");
    assert!(near(all[1].pos, 10.0, 10.0));
    assert!(near(all[0].pos, 10.5, 10.0), "{:?}", all[0].pos);
    assert_eq!(all[0].elevation_ft, Some(50));
    assert_eq!(all[1].elevation_ft, Some(100));

    let wide = load_destinations(&fixture_root(), HOME, 200.0, &[]).unwrap();
    let far = wide.iter().find(|d| d.icao == "ZZZX").unwrap();
    assert_eq!(far.name, "Faréville Intl");
}

#[test]
fn preferred_name_wins() {
    let names = [("ZZZY".to_string(), "Test Town".to_string())];
    let got = load_destinations(&fixture_root(), HOME, 100.0, &names).unwrap();
    assert_eq!(got[0].name, "Test Town");
    assert_eq!(got[1].name, "Zeta Zulu International");
}

#[test]
fn zzzz_airport_block_parses() {
    let block = resolve_airport(&fixture_root(), "ZZZZ").unwrap();
    let ap = block.parse("ZZZZ").unwrap();
    assert_eq!(ap.runways.len(), 1);
    assert_eq!(ap.runways[0].ends.len(), 2);
    assert!(!ap.stands.is_empty());
    assert_eq!(ap.frequencies.len(), 5);
    // Thresholds are the CIFP RWY positions.
    let set = load_procedure_set(&fixture_root(), "ZZZZ").unwrap();
    for end in &ap.runways[0].ends {
        let rw = set.runways.iter().find(|r| r.ident == end.ident).unwrap();
        assert!(
            distance_m(rw.threshold, end.threshold) < 1.0,
            "{}",
            end.ident
        );
    }
    // The parsed block is also a block parse_apt_dat takes from the raw text.
    assert!(parse_apt_dat(&block.text, "ZZZZ").is_ok());
}

#[test]
fn zzzz_set_resolves_every_leg() {
    let set = load_procedure_set(&fixture_root(), "ZZZZ").unwrap();
    assert_eq!(set.icao, "ZZZZ");
    assert_eq!(set.cycle, "1802");
    for p in &set.procedures {
        for l in &p.legs {
            assert_eq!(
                l.pos.is_none(),
                l.leg.fix.is_none(),
                "{} {:?}",
                p.procedure.ident,
                l.leg.path
            );
            if l.leg.path == PathTerm::Vm {
                assert!(l.pos.is_none());
            }
        }
    }
    // earth_fix value; the same ident in region XX is not used.
    let alfax = set.fix("ALFAX").unwrap();
    assert!(near(alfax, 10.1, 9.8), "{alfax:?}");
    // Own-airport entry wins over the ZZZY one of the same ident.
    assert!(near(set.fix("ZZ011").unwrap(), 10.05, 9.88));
    // Runway fixes are the CIFP thresholds.
    let rw27 = set.runways.iter().find(|r| r.ident == "27").unwrap();
    assert_eq!(set.fix("RW27"), Some(rw27.threshold));
    assert!(set.fix("NOPE").is_none());

    assert_eq!(set.sids("27").len(), 2);
    assert_eq!(set.stars("27").len(), 1);
    let ils = set.approach("27", ApproachKind::Ils).unwrap();
    assert_eq!(ils.procedure.ident, "I27");
    assert_eq!(ils.legs.len(), 5);
    assert!(set.approach("09", ApproachKind::Rnav).is_some());
    assert!(set.approach("27", ApproachKind::VorDme).is_none());
}

#[test]
fn custom_data_overrides_default() {
    let scratch = Scratch::new("custom");
    let dir = scratch.0.join("Custom Data/CIFP");
    std::fs::create_dir_all(&dir).unwrap();
    let default = fixture_root().join("Resources/default data/CIFP/ZZZZ.dat");
    let text = std::fs::read_to_string(default).unwrap();
    let kept: Vec<&str> = text.lines().filter(|l| !l.contains("BRAV1A")).collect();
    std::fs::write(dir.join("ZZZZ.dat"), kept.join("\n")).unwrap();

    let set = load_procedure_set(&scratch.0, "ZZZZ").unwrap();
    assert_eq!(set.sids("27").len(), 1);

    // A Custom Data earth_fix.dat replaces the default one as a whole.
    std::fs::write(
        scratch.0.join("Custom Data/earth_fix.dat"),
        "I\n1101 Version - data cycle 2001, build 1\n\n 10.0 9.95 ZZ001 ZZZZ ZZ 1\n",
    )
    .unwrap();
    match load_procedure_set(&scratch.0, "ZZZZ") {
        Err(ResolveError::UnresolvedFix { icao, .. }) => assert_eq!(icao, "ZZZZ"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn unresolved_fix_is_an_error() {
    let scratch = Scratch::new("unresolved");
    let dir = scratch.0.join("Custom Data");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("earth_nav.dat"),
        "I\n1150 Version - data cycle 1802\n99\n",
    )
    .unwrap();
    let fix = std::fs::read_to_string(fixture_root().join("Resources/default data/earth_fix.dat"))
        .unwrap()
        .replace("ZZ032 ZZZZ", "ZZ099 ZZZZ");
    std::fs::write(dir.join("earth_fix.dat"), fix).unwrap();
    match load_procedure_set(&scratch.0, "ZZZZ") {
        Err(ResolveError::UnresolvedFix { icao, ident }) => {
            assert_eq!((icao.as_str(), ident.as_str()), ("ZZZZ", "ZZ032"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn navaid_fix_resolves_from_earth_nav() {
    let scratch = Scratch::new("navaid");
    let cifp_dir = scratch.0.join("Custom Data/CIFP");
    std::fs::create_dir_all(&cifp_dir).unwrap();
    std::fs::write(
        cifp_dir.join("ZZZZ.dat"),
        "RWY:RW27 ,     ,      ,00500, ,ZRW ,1,   ;N10000000,E010010000,0100;\n\
         STAR:010,5,VOR1A,RW27,ZZV,ZZ,D, ,V   , ,   ,IF, , , , , ,      ,    ,    ,    ,    , ,     ,     ,     , ,   ,    ,   , , , , , , , , ;\n",
    )
    .unwrap();
    let set = load_procedure_set(&scratch.0, "ZZZZ").unwrap();
    assert!(near(set.fix("ZZV").unwrap(), 10.05, 10.1));
}

#[test]
fn missing_cifp_is_not_found() {
    match load_procedure_set(&fixture_root(), "ZZZY") {
        Err(e @ ResolveError::CifpNotFound { .. }) => {
            let text = e.to_string();
            assert!(text.contains("no CIFP procedures found for ZZZY"), "{text}");
            assert!(!text.contains("apt.dat"), "{text}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn epkk_set_resolves() {
    let Some(root) = xp_install() else { return };
    let set = load_procedure_set(&root, "EPKK").unwrap();
    assert_eq!(set.cycle, "1802");
    let sudol = set.fix("SUDOL").unwrap();
    assert!(near(sudol, 50.6533, 18.4117), "{sudol:?}");
    let kk599 = set.fix("KK599").unwrap();
    assert!(near(kk599, 50.0571, 19.6378), "{kk599:?}");
    assert!(!set.sids("25").is_empty());
    assert!(set.approach("25", ApproachKind::Ils).is_some());
}
