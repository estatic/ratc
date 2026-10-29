use std::path::{Path, PathBuf};

use atc_navdata::xplane::{ResolveError, apt_dat_candidates, resolve_airport};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn root(name: &str) -> PathBuf {
    Path::new(FIX).join(name)
}

fn pack_of(source: &Path) -> String {
    // <pack>/Earth nav data/apt.dat
    let pack = source.parent().unwrap().parent().unwrap();
    pack.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn candidates_follow_x_plane_priority() {
    let packs: Vec<String> = apt_dat_candidates(&root("xp"))
        .iter()
        .map(|p| pack_of(p))
        .collect();
    // Unlisted pack first (X-Plane adds it on top), disabled pack never, Global Airports once.
    assert_eq!(
        packs,
        [
            "New Pack",
            "Listed Pack",
            "Global Airports",
            "default apt dat"
        ]
    );
}

#[test]
fn global_airports_wins_over_default_and_disabled_pack() {
    let r = resolve_airport(&root("xp"), "EPKK").unwrap();
    assert_eq!(pack_of(&r.source), "Global Airports");
    assert_eq!(r.ident, "EPKK");
    let ap = r.parse("EPKK").unwrap();
    assert_eq!(ap.name, "Global Krakow");
    assert_eq!(ap.runways.len(), 1);
    assert_eq!(ap.taxi.edges.len(), 2);
    assert_eq!(ap.stands.len(), 1);
    // The block stops at the next airport header.
    assert!(!r.text.contains("EPWA"), "{}", r.text);
}

#[test]
fn listed_pack_matches_by_icao_code_row() {
    let r = resolve_airport(&root("xp"), "EPKO").unwrap();
    assert_eq!(pack_of(&r.source), "Listed Pack");
    assert_eq!(r.ident, "XEPKO");
    let ap = r.parse("EPKO").unwrap();
    assert_eq!(ap.icao, "EPKO");
    assert_eq!(ap.name, "Listed Katowice");
}

#[test]
fn unlisted_pack_comes_first() {
    let r = resolve_airport(&root("xp"), "EPWA").unwrap();
    assert_eq!(pack_of(&r.source), "New Pack");
    assert_eq!(r.parse("EPWA").unwrap().name, "New Warsaw");
}

#[test]
fn default_apt_dat_is_the_last_resort() {
    let r = resolve_airport(&root("xp"), "EPGD").unwrap();
    assert_eq!(pack_of(&r.source), "default apt dat");
    assert_eq!(r.parse("EPGD").unwrap().name, "Default Gdansk");
}

#[test]
fn unknown_airport_is_not_found() {
    match resolve_airport(&root("xp"), "ZZZZ") {
        Err(ResolveError::NotFound { icao, searched }) => {
            assert_eq!(icao, "ZZZZ");
            assert_eq!(searched, 4);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn without_ini_global_airports_then_default() {
    let r = resolve_airport(&root("xp-noini"), "EPKK").unwrap();
    assert_eq!(pack_of(&r.source), "Global Airports");
    let r = resolve_airport(&root("xp-noini"), "EPGD").unwrap();
    assert_eq!(pack_of(&r.source), "default apt dat");
}

#[test]
fn absolute_pack_path_in_ini() {
    let tmp = std::env::temp_dir().join(format!("atc-navdata-xp-{}", std::process::id()));
    let scenery = tmp.join("Custom Scenery");
    std::fs::create_dir_all(&scenery).unwrap();
    let listed = root("xp").join("Custom Scenery/Listed Pack");
    std::fs::write(
        scenery.join("scenery_packs.ini"),
        format!(
            "I\n1000 Version\nSCENERY\n\nSCENERY_PACK {}/\n",
            listed.display()
        ),
    )
    .unwrap();
    let r = resolve_airport(&tmp, "EPKO");
    std::fs::remove_dir_all(&tmp).unwrap();
    assert_eq!(r.unwrap().source, listed.join("Earth nav data/apt.dat"));
}
