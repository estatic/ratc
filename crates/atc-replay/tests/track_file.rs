//! A `track_file` CSV of the synthetic circuit replays to the same transcript as the
//! synthetic track itself.

use std::fmt::Write;
use std::path::PathBuf;

use atc_core::testkit::circuit_track;
use atc_navdata::parse_apt_dat;
use atc_navdata::procedures::parse_procedures;
use atc_replay::run_scenario;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn track_file_matches_synthetic_track() {
    let read = |p: &str| std::fs::read_to_string(root().join(p)).unwrap();
    let ap = parse_apt_dat(&read("data/airports/EPKK/apt.dat"), "EPKK").unwrap();
    let procs = parse_procedures(&read("data/airports/EPKK/procedures.toml")).unwrap();
    let mut csv = String::from("t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz\n");
    for s in circuit_track(&ap, &procs, "25", 1.0) {
        writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{}",
            s.t.0,
            s.pos.lat,
            s.pos.lon,
            s.alt_ft_msl,
            s.heading_deg,
            s.ias_kt,
            s.gs_kt,
            s.vs_fpm,
            u8::from(s.on_ground),
            s.com1_khz
        )
        .unwrap();
    }
    let dir = std::env::temp_dir().join(format!("atc-replay-track-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("circuit.csv"), csv).unwrap();
    let synthetic = read("scenarios/epkk-circuit-vmc.toml");
    assert!(synthetic.contains("track = \"synthetic-circuit:25\""));
    let from_file = synthetic.replace(
        "track = \"synthetic-circuit:25\"",
        "track_file = \"circuit.csv\"",
    );
    std::fs::write(dir.join("from-file.toml"), from_file).unwrap();

    let expected = run_scenario(&root().join("scenarios/epkk-circuit-vmc.toml")).unwrap();
    let actual = run_scenario(&dir.join("from-file.toml")).unwrap();
    assert_eq!(actual, expected);
}
