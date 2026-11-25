use std::path::PathBuf;

use atc_core::ifr::*;
use atc_geo::{LatLon, M_PER_NM, bearing_deg, destination, distance_m};
use atc_navdata::cifp::{AltConstraint, ApproachKind};
use atc_navdata::fixes::ProcedureSet;
use atc_navdata::xplane::load_procedure_set;
use atc_phraseology::Turn;

fn zzzz() -> ProcedureSet {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../atc-navdata/tests/fixtures/xplane");
    load_procedure_set(&root, "ZZZZ").expect("fixture loads")
}

fn ll(lat: f64, lon: f64) -> LatLon {
    LatLon { lat, lon }
}

const AIRPORT: LatLon = LatLon {
    lat: 10.0,
    lon: 10.0,
};

#[test]
fn sid_follows_destination_bearing() {
    let set = zzzz();
    let north = choose_sid(&set, "27", AIRPORT, ll(20.0, 10.0)).unwrap();
    assert_eq!(north.procedure.ident, "ALFA1A");
    let south = choose_sid(&set, "27", AIRPORT, ll(0.0, 10.0)).unwrap();
    assert_eq!(south.procedure.ident, "BRAV1A");
    let east = choose_sid(&set, "09", AIRPORT, ll(20.0, 10.0)).unwrap();
    assert_eq!(east.procedure.ident, "CHAR1A");
    assert!(choose_sid(&set, "18", AIRPORT, ll(20.0, 10.0)).is_none());
}

#[test]
fn star_needs_the_start_fix_ahead() {
    let set = zzzz();
    let star = choose_star(&set, "27", ll(10.2, 9.6), AIRPORT).unwrap();
    assert_eq!(star.procedure.ident, "ALFA1B");
    assert!(choose_star(&set, "27", ll(10.01, 9.98), AIRPORT).is_none());
}

/// A STAR whose start fix is ahead but on the far side of the airport (more than 90 degrees
/// from the aircraft, seen from the airport) is not given: the aircraft gets vectors.
#[test]
fn star_on_the_far_side_is_not_chosen() {
    let set = zzzz();
    // ALFAX (start of ALFA1B) is 13 NM north-west; the aircraft 17 NM south-east.
    let se = ll(9.8, 10.2);
    let alfax = set.fix("ALFAX").unwrap();
    assert!(distance_m(AIRPORT, alfax) < distance_m(AIRPORT, se));
    assert!(choose_star(&set, "27", se, AIRPORT).is_none());
    // Just within 90 degrees of the start fix's bearing (south-west, beyond it): still given.
    let sw = destination(AIRPORT, bearing_deg(AIRPORT, alfax) - 85.0, 20.0 * M_PER_NM);
    assert_eq!(
        choose_star(&set, "27", sw, AIRPORT)
            .unwrap()
            .procedure
            .ident,
        "ALFA1B"
    );
    // Just beyond 90 degrees: not.
    let s = destination(AIRPORT, bearing_deg(AIRPORT, alfax) - 95.0, 20.0 * M_PER_NM);
    assert!(choose_star(&set, "27", s, AIRPORT).is_none());
}

#[test]
fn constraints_above_cleared_level_dropped() {
    let set = zzzz();
    let sid = choose_sid(&set, "27", AIRPORT, ll(20.0, 10.0)).unwrap();
    let path = procedure_path(sid);
    assert_eq!(
        path.points.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(),
        ["ZZ001", "ZZ002", "ALFAX"]
    );
    assert_eq!(path.constraints.len(), 2);
    let kept = applicable_constraints(&path, 5000);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].1, AltConstraint::AtOrAbove(3000));
    assert_eq!(applicable_constraints(&path, 9000).len(), 2);
}

#[test]
fn initial_climb_respects_floors_and_ceilings() {
    let set = zzzz();
    let sid = choose_sid(&set, "27", AIRPORT, ll(20.0, 10.0)).unwrap();
    let mut path = procedure_path(sid);
    assert_eq!(initial_climb_ft(&path, 5000), 8000); // floor FL080 beats 5000
    assert_eq!(initial_climb_ft(&path, 10000), 10000); // planned level above the floor
    path.constraints[1].1 = AltConstraint::AtOrAbove(6100);
    assert_eq!(initial_climb_ft(&path, 5000), 7000); // rounded up to 1000 ft
    path.constraints[0].1 = AltConstraint::AtOrBelow(4000);
    assert_eq!(initial_climb_ft(&path, 5000), 4000); // the ceiling still wins
    // A ceiling above the target does not apply.
    path.constraints[0].1 = AltConstraint::AtOrBelow(9000);
    assert_eq!(initial_climb_ft(&path, 5000), 7000);
    // The lower bound of Between is a floor, the upper bound a ceiling.
    path.constraints[0].1 = AltConstraint::Between {
        above: 9200,
        below: 11000,
    };
    assert_eq!(initial_climb_ft(&path, 5000), 10000);
    path.constraints[0].1 = AltConstraint::Between {
        above: 2000,
        below: 3000,
    };
    assert_eq!(initial_climb_ft(&path, 5000), 3000);
    path.constraints[0].1 = AltConstraint::At(2500);
    assert_eq!(initial_climb_ft(&path, 5000), 2500);
}

#[test]
fn sid_target_keeps_a_floor_after_a_ceiling() {
    let set = zzzz();
    let sid = choose_sid(&set, "27", AIRPORT, ll(20.0, 10.0)).unwrap();
    let mut path = procedure_path(sid);
    path.constraints[0].1 = AltConstraint::AtOrBelow(4000);
    path.constraints[1].1 = AltConstraint::AtOrAbove(6000);
    assert_eq!(initial_climb_ft(&path, 5000), 4000);
    assert_eq!(sid_target_ft(&path, 5000), 6000);
    assert_eq!(sid_target_ft(&path, 10000), 10000);
    path.constraints[1].1 = AltConstraint::AtOrAbove(6100);
    assert_eq!(sid_target_ft(&path, 5000), 7000);
}

#[test]
fn cross_track() {
    let set = zzzz();
    let sid = choose_sid(&set, "27", AIRPORT, ll(20.0, 10.0)).unwrap();
    let path = procedure_path(sid);
    assert!(cross_track_m(&path, path.points[1].1) < 1.0);
    // one mile abeam the segment ZZ001 -> ZZ002 (due west)
    let mid = destination(path.points[0].1, 270.0, 2000.0);
    let abeam = destination(mid, 0.0, M_PER_NM);
    let xt = cross_track_m(&path, abeam);
    assert!((xt - M_PER_NM).abs() < 20.0, "{xt}");
}

#[test]
fn iaf_and_designator() {
    let set = zzzz();
    let (name, pos) = iaf(&set, "27", ApproachKind::Ils).unwrap();
    assert_eq!(name, "ZZ011");
    assert_eq!(pos, set.fix("ZZ011").unwrap());
    assert_eq!(iaf(&set, "09", ApproachKind::Rnav).unwrap().0, "ZZ030");
    assert!(iaf(&set, "27", ApproachKind::Rnav).is_none());
    let sid = choose_sid(&set, "27", AIRPORT, ll(20.0, 10.0)).unwrap();
    assert_eq!(spoken_designator(sid), ("ALFAX".to_string(), 1, 'A'));
    let star = choose_star(&set, "27", ll(10.2, 9.6), AIRPORT).unwrap();
    assert_eq!(spoken_designator(star), ("ALFAX".to_string(), 1, 'B'));
}

#[test]
fn approach_kind_conversion_round_trips() {
    for k in [ApproachKind::Ils, ApproachKind::VorDme, ApproachKind::Rnav] {
        assert_eq!(navdata_approach_kind(spoken_approach_kind(k)), k);
    }
}

// Final course 090 (true) into a threshold at the airport; intercept point 8 NM west.
fn setup() -> (LatLon, LatLon) {
    let thr = AIRPORT;
    (thr, destination(thr, 270.0, 8.0 * M_PER_NM))
}

#[test]
fn vector_heading_is_magnetic() {
    // 349 true with variation 6E -> magnetic 343 -> 345, left from 010.
    let v = vector_on(349.0, 10.0, 3000, 6.0);
    assert_eq!((v.heading_deg, v.turn, v.alt_ft), (345, Turn::Left, 3000));
    // 090 true with 90E: wrap to 360 rather than 0.
    let v = vector_on(90.0, 0.0, 3000, 90.0);
    assert_eq!((v.heading_deg, v.turn), (360, Turn::Right));
}

#[test]
fn distance_to_go() {
    let (thr, point) = setup();
    assert_eq!(distance_to_go_nm(point, thr), 8);
}

#[test]
fn epkk_sid_towards_epkt() {
    let root = std::env::var_os("ATC_XPLANE_ROOT").map_or_else(
        || {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".local/share/Steam/steamapps/common/X-Plane 11")
        },
        PathBuf::from,
    );
    let Ok(set) = load_procedure_set(&root, "EPKK") else {
        println!("skipped: no EPKK CIFP under {}", root.display());
        return;
    };
    let epkk = ll(50.0777, 19.7848);
    let epkt = ll(50.4743, 19.0800);
    let sid = choose_sid(&set, "25", epkk, epkt).expect("a SID");
    assert!(
        ["SUDO2J", "NUMB2J", "EMBI2J"].contains(&sid.procedure.ident.as_str()),
        "{}",
        sid.procedure.ident
    );
}

/// ALFA1A: ZZ001, ZZ002 (the turn), ALFAX. ZZ002 is passed within 1 NM of it or once the
/// nearest leg is the one after it.
#[test]
fn passed_second_fix() {
    let set = zzzz();
    let sid = set
        .sids("27")
        .into_iter()
        .find(|p| p.procedure.ident == "ALFA1A")
        .unwrap();
    let path = procedure_path(sid);
    assert_eq!(path.points[1].0, "ZZ002");
    let nm = M_PER_NM;
    // Between ZZ001 and ZZ002, 1.5 NM short of ZZ002: not yet.
    assert!(!passed_fix(&path, 1, ll(10.0, 9.925), nm));
    // Within 1 NM of it.
    assert!(passed_fix(&path, 1, ll(10.0, 9.91), nm));
    // On the leg to ALFAX.
    assert!(passed_fix(&path, 1, ll(10.05, 9.85), nm));
    // Unknown fix.
    assert!(!passed_fix(&path, 7, ll(10.05, 9.85), nm));
}
