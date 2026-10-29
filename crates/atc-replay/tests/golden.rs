//! Every `scenarios/*.toml` must replay to exactly its `.expected.txt` transcript.
//! `UPDATE_GOLDEN=1 cargo test -p atc-replay` rewrites the expected files.

use std::path::PathBuf;

use atc_replay::{needs_xplane, run_scenario, xplane_root};

fn scenarios_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scenarios")
}

#[test]
fn golden_transcripts_match() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut scenarios: Vec<PathBuf> = std::fs::read_dir(scenarios_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    scenarios.sort();
    let (xplane, plain): (Vec<PathBuf>, Vec<PathBuf>) = scenarios
        .into_iter()
        .partition(|p| needs_xplane(p).unwrap_or_else(|e| panic!("{}: {e:#}", p.display())));
    assert_eq!((plain.len(), xplane.len()), (8, 7), "{plain:?} {xplane:?}");
    let run = if xplane_root().is_some() {
        plain.into_iter().chain(xplane).collect::<Vec<_>>()
    } else {
        println!(
            "X-Plane install not found (ATC_XPLANE_ROOT or the default Steam path): skipping {} \
             scenarios that need its navdata",
            xplane.len()
        );
        plain
    };
    for path in run {
        let actual = run_scenario(&path).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        let expected_path = path.with_extension("expected.txt");
        if update {
            std::fs::write(&expected_path, &actual).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("{}: {e}", expected_path.display()));
        assert_eq!(actual, expected, "transcript of {} changed", path.display());
    }
}

#[test]
fn transcript_is_deterministic() {
    let path = scenarios_dir().join("epkk-circuit-vmc.toml");
    assert_eq!(run_scenario(&path).unwrap(), run_scenario(&path).unwrap());
}

#[test]
fn unknown_trigger_is_an_error() {
    let dir = std::env::temp_dir().join("atc-replay-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.toml");
    let src = std::fs::read_to_string(scenarios_dir().join("epkk-circuit-vmc.toml")).unwrap();
    std::fs::write(&path, src.replace("phase:Parked", "phase:Nowhere")).unwrap();
    let err = run_scenario(&path).unwrap_err();
    assert!(format!("{err:#}").contains("Nowhere"), "{err:#}");
}

#[test]
fn bad_transponder_mode_is_an_error() {
    let dir = std::env::temp_dir().join("atc-replay-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad-xpdr.toml");
    let src = std::fs::read_to_string(scenarios_dir().join("epkk-wrong-squawk.toml")).unwrap();
    assert!(src.contains("mode = 2"));
    std::fs::write(&path, src.replace("mode = 2", "mode = 9")).unwrap();
    let err = run_scenario(&path).unwrap_err();
    assert!(format!("{err:#}").contains("transponder mode 9"), "{err:#}");
}
