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
fn initial_climb_stops_only_at_ceilings() {
    let set = zzzz();
    let sid = choose_sid(&set, "27", AIRPORT, ll(20.0, 10.0)).unwrap();
    let mut path = procedure_path(sid);
    // +3000 and +FL080 are minimums: climb to the cruise level.
    assert_eq!(initial_climb_ft(&path, 5000), 5000);
    path.constraints[0].1 = AltConstraint::AtOrBelow(4000);
    assert_eq!(initial_climb_ft(&path, 5000), 4000);
    // A ceiling above the cruise level does not apply.
    assert_eq!(initial_climb_ft(&path, 3500), 3500);
    path.constraints[0].1 = AltConstraint::Between {
        above: 2000,
        below: 3000,
    };
    assert_eq!(initial_climb_ft(&path, 5000), 3000);
    path.constraints[0].1 = AltConstraint::At(2500);
    assert_eq!(initial_climb_ft(&path, 5000), 2500);
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

/// The lead point 3 NM back along the 30-degree intercept track from the south (060).
fn lead_south(point: LatLon) -> LatLon {
    destination(point, 240.0, 3.0 * M_PER_NM)
}

#[test]
fn vector_far_heads_for_the_lead_point() {
    let (thr, point) = setup();
    // 15 NM south of the intercept point, heading north: towards the lead point SW of it.
    let ac = destination(point, 180.0, 15.0 * M_PER_NM);
    let want = (bearing_deg(ac, lead_south(point)) / 5.0).round() as u16 * 5;
    let (v, intercept) = vector_to_final(ac, 0.0, thr, 90.0, 8.0, 3000, 0.0);
    assert_eq!(v.heading_deg, want, "{v:?}");
    assert_eq!(want, 350);
    assert!(!intercept);
    assert_eq!(v.alt_ft, 3000);
    // heading east, needs to turn left
    let (v, _) = vector_to_final(ac, 90.0, thr, 90.0, 8.0, 3000, 0.0);
    assert_eq!(v.turn, Turn::Left);
}

#[test]
fn vector_from_the_side_turns_in_at_the_lead_point() {
    let (thr, point) = setup();
    // Coming from the south at right angles to the final: short of the lead point still
    // towards it (the 30-degree track would join inside the intercept point) ...
    let short = destination(lead_south(point), 180.0, 0.5 * M_PER_NM);
    assert!(!vector_to_final(short, 0.0, thr, 90.0, 8.0, 3000, 0.0).1);
    // ... just past it the intercept heading: the track joins at the intercept point.
    let ac = destination(lead_south(point), 0.0, 0.2 * M_PER_NM);
    assert!(distance_m(ac, point) > 2.0 * M_PER_NM);
    let (v, intercept) = vector_to_final(ac, 0.0, thr, 90.0, 8.0, 3000, 0.0);
    assert!(intercept);
    assert_eq!((v.heading_deg, v.turn), (60, Turn::Right));
}

#[test]
fn vector_near_intercepts_at_thirty_degrees() {
    let (thr, point) = setup();
    // 2 NM before the point, south of the line
    let ac = destination(point, 270.0, 1.0 * M_PER_NM);
    let ac = destination(ac, 180.0, 1.5 * M_PER_NM);
    assert!(distance_m(ac, point) < 3.0 * M_PER_NM);
    let (v, intercept) = vector_to_final(ac, 45.0, thr, 90.0, 8.0, 3000, 0.0);
    assert!(intercept);
    assert_eq!(v.heading_deg, 60);
    assert_eq!(v.turn, Turn::Right);
    // north of the line: 120
    let ac_n = destination(point, 0.0, 1.8 * M_PER_NM);
    let (v, _) = vector_to_final(ac_n, 200.0, thr, 90.0, 8.0, 3000, 0.0);
    assert_eq!(v.heading_deg, 120);
    assert_eq!(v.turn, Turn::Left);
}

#[test]
fn vector_heading_is_magnetic() {
    let (thr, point) = setup();
    let ac = destination(point, 180.0, 15.0 * M_PER_NM);
    // true heading 349 to the lead point; variation 6E -> magnetic 343 -> 345
    let (v, _) = vector_to_final(ac, 10.0, thr, 90.0, 8.0, 3000, 6.0);
    assert_eq!(v.heading_deg, 345);
    assert_eq!(v.turn, Turn::Left);
    // 090 true with 90E: wrap to 360 rather than 0
    let ac_w = destination(lead_south(point), 270.0, 15.0 * M_PER_NM);
    let (v, _) = vector_to_final(ac_w, 0.0, thr, 90.0, 8.0, 3000, 90.0);
    assert_eq!(v.heading_deg, 360);
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
