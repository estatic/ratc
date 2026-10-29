use std::path::PathBuf;

use atc_navdata::NavdataError;
use atc_navdata::cifp::{AltConstraint, ApproachKind, PathTerm, parse_cifp};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xplane")
}

fn cifp_path(root: &std::path::Path, icao: &str) -> PathBuf {
    root.join("Resources/default data/CIFP")
        .join(format!("{icao}.dat"))
}

fn fixture(icao: &str) -> String {
    std::fs::read_to_string(cifp_path(&fixture_root(), icao)).unwrap()
}

#[test]
fn parses_fixture_procedures() {
    let c = parse_cifp(&fixture("ZZZZ"), "ZZZZ").unwrap();
    assert_eq!(c.runways.len(), 2);
    assert_eq!(c.runways[1].ident, "27");
    assert_eq!(c.runways[1].elevation_ft, 100);
    assert!((c.runways[1].threshold.lat - 10.0).abs() < 1e-9);
    assert!((c.runways[1].threshold.lon - (10.0 + 1.0 / 60.0)).abs() < 1e-9);
    let sids: Vec<_> = c.sids("27").iter().map(|p| p.ident.as_str()).collect();
    assert_eq!(sids, ["ALFA1A", "BRAV1A"]);
    let sids: Vec<_> = c.sids("09").iter().map(|p| p.ident.as_str()).collect();
    assert_eq!(sids, ["CHAR1A"]);
    let alfa = c.sids("27")[0];
    assert_eq!(alfa.legs[0].path, PathTerm::Cf);
    assert_eq!(alfa.legs[0].alt, AltConstraint::AtOrAbove(3000));
    assert_eq!(alfa.legs[0].course_deg, Some(270.0));
    assert_eq!(alfa.legs[1].alt, AltConstraint::AtOrAbove(8000));
    assert_eq!(alfa.legs[2].fix.as_ref().unwrap().ident, "ALFAX");
    assert_eq!(alfa.legs[2].fix.as_ref().unwrap().section, 'E');
    assert!(c.stars("27")[0].legs.last().unwrap().iaf);
}

#[test]
fn approach_marks_faf_map_and_missed() {
    let c = parse_cifp(&fixture("ZZZZ"), "ZZZZ").unwrap();
    let i27 = c
        .approaches("27")
        .into_iter()
        .find(|p| p.transition.is_empty())
        .unwrap();
    assert_eq!(i27.approach, Some(ApproachKind::Ils));
    assert!(i27.legs[1].faf && i27.legs[2].map);
    assert!(i27.legs[3].missed && i27.legs[4].missed);
    assert!(!i27.legs[2].missed);
    assert_eq!(i27.legs[4].path, PathTerm::Vm);
    assert!(i27.legs[4].fix.is_none());
    assert_eq!(i27.legs[0].alt, AltConstraint::At(3000));
}

#[test]
fn approaches_list_finals_before_transitions() {
    let c = parse_cifp(&fixture("ZZZZ"), "ZZZZ").unwrap();
    let a27 = c.approaches("27");
    assert_eq!(a27.len(), 2);
    assert!(a27[0].transition.is_empty());
    assert_eq!(a27[1].transition, "ZZ011");
    assert_eq!(a27[1].approach, Some(ApproachKind::Ils));
    assert!(a27[1].legs[0].iaf);
    let a09 = c.approaches("09");
    assert_eq!(a09.len(), 1);
    assert_eq!(a09[0].approach, Some(ApproachKind::Rnav));
}

#[test]
fn unknown_leg_type_is_an_error() {
    let text = fixture("ZZZZ");
    let n = text.lines().position(|l| l.starts_with("SID:020")).unwrap() + 1;
    let bad = text.replacen(
        "SID:020,5,ALFA1A,RW27,ZZ002,ZZ,P,C,E   , ,   ,TF",
        "SID:020,5,ALFA1A,RW27,ZZ002,ZZ,P,C,E   , ,   ,RF",
        1,
    );
    match parse_cifp(&bad, "ZZZZ") {
        Err(NavdataError::Malformed { line, .. }) => assert_eq!(line, n),
        other => panic!("{other:?}"),
    }
}

fn xp_root() -> PathBuf {
    std::env::var_os("ATC_XPLANE_ROOT").map_or_else(
        || {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local/share/Steam/steamapps/common/X-Plane 11")
        },
        PathBuf::from,
    )
}

#[test]
fn epkk_cifp_loads_all_procedures() {
    let path = cifp_path(&xp_root(), "EPKK");
    let Ok(text) = std::fs::read_to_string(&path) else {
        println!("skipped: no X-Plane CIFP at {}", path.display());
        return;
    };
    let c = parse_cifp(&text, "EPKK").unwrap();
    let mut idents: Vec<&str> = Vec::new();
    for p in c
        .procedures
        .iter()
        .filter(|p| p.kind == atc_navdata::cifp::ProcKind::Sid)
    {
        if !idents.contains(&p.ident.as_str()) {
            idents.push(&p.ident);
        }
    }
    assert_eq!(idents.len(), 20);
    assert_eq!(idents.iter().filter(|i| i.ends_with("1F")).count(), 10);
    assert_eq!(idents.iter().filter(|i| i.ends_with("2J")).count(), 10);
    let mut stars: Vec<&str> = Vec::new();
    for p in c
        .procedures
        .iter()
        .filter(|p| p.kind == atc_navdata::cifp::ProcKind::Star)
    {
        if !stars.contains(&p.ident.as_str()) {
            stars.push(&p.ident);
        }
    }
    assert_eq!(stars.len(), 16);
    for (rwy, ident) in [
        ("25", "I25"),
        ("07", "D07"),
        ("25", "D25"),
        ("07", "R07"),
        ("25", "R25"),
    ] {
        assert!(
            c.approaches(rwy).iter().any(|p| p.ident == ident),
            "{ident}"
        );
    }
    let sudo = c
        .sids("25")
        .into_iter()
        .find(|p| p.ident == "SUDO2J")
        .unwrap();
    let first = &sudo.legs[0];
    assert_eq!(first.fix.as_ref().unwrap().ident, "KK599");
    assert_eq!(first.alt, AltConstraint::AtOrAbove(3000));
}
