//! The `[sim_data]` section and the old top-level key it replaces.

use atc_navdata::source::SimDataConfig;
use atc_server::config::load_config;

const EXAMPLE: &str = include_str!("../../../config/server.example.toml");
const LEGACY: &str = "xplane_root";

/// The example without any simulator data lines (commented or not).
fn base() -> String {
    EXAMPLE
        .lines()
        .filter(|l| !l.contains("sim_data") && !l.contains(LEGACY))
        .filter(|l| {
            let t = l.trim_start_matches(['#', ' ']);
            !t.starts_with("sim =") && !t.starts_with("root =")
        })
        .map(|l| format!("{l}\n"))
        .collect()
}

/// `base()` with `line` added at the top level, before the first table.
fn with_top_level(line: &str) -> String {
    let text = base();
    let at = text.find("\n[").expect("a table") + 1;
    format!("{}{line}\n{}", &text[..at], &text[at..])
}

const SECTION: &str = "[sim_data]\nsim = \"xplane\"\nroot = \"/x\"\n";

fn xplane_x() -> Option<SimDataConfig> {
    Some(SimDataConfig::from_legacy_root("/x".into()))
}

#[test]
fn sim_data_section_loads() {
    let cfg = load_config(&format!("{}{SECTION}", base())).unwrap();
    assert_eq!(cfg.sim_data, xplane_x());
}

#[test]
fn legacy_root_key_loads_as_xplane_source() {
    let cfg = load_config(&with_top_level(&format!("{LEGACY} = \"/x\""))).unwrap();
    assert_eq!(cfg.sim_data, xplane_x());
}

#[test]
fn both_sim_data_and_legacy_root_is_an_error() {
    let text = format!("{}{SECTION}", with_top_level(&format!("{LEGACY} = \"/y\"")));
    let e = load_config(&text).unwrap_err();
    let msg = format!("{e:#}");
    assert!(msg.contains("[sim_data]"), "{msg}");
    assert!(msg.contains(LEGACY), "{msg}");
}

#[test]
fn no_source_is_none() {
    assert_eq!(load_config(&base()).unwrap().sim_data, None);
}
