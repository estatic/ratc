//! EPKK regenerated from a synthetic OpenAIP-shaped fixture plus `overrides.toml` must equal
//! the hand-made `procedures.toml`.
//!
//! The fixture is built only from the project's own AIP values (`procedures.toml`, `SOURCE.md`);
//! it holds no OpenAIP data. It carries what OpenAIP would carry (frequencies, runways,
//! declination, airspaces, the VFR points it publishes, Katowice) and omits what it is known
//! not to carry; `overrides.toml` supplies exactly that.

use std::path::{Path, PathBuf};

use atc_navdata::apt::parse_apt_dat;
use atc_navdata::procedures::parse_procedures;
use atc_navdata::station::merge_overrides;
use atc_stations::compare::compare;
use atc_stations::generate::{Choice, generate, offered_positions};
use atc_stations::openaip::{OaAirport, OaAirspace, OaReportingPoint};
use atc_stations::overrides::{Overrides, orphans};
use atc_stations::store::CountryData;

fn fixture<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/epkk")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn epkk_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/airports/EPKK")
}

fn epkk_data() -> CountryData {
    CountryData {
        airports: fixture::<Vec<OaAirport>>("airport.json"),
        airspaces: fixture::<Vec<OaAirspace>>("airspaces.json"),
        points: fixture::<Vec<OaReportingPoint>>("reporting-points.json"),
        synced_at: "2026-10-29T00:00:00Z".into(),
    }
}

#[test]
fn epkk_regenerated_equals_hand_made() {
    let dir = epkk_dir();
    let hand =
        parse_procedures(&std::fs::read_to_string(dir.join("procedures.toml")).unwrap()).unwrap();
    let data = epkk_data();
    let layout = parse_apt_dat(
        &std::fs::read_to_string(dir.join("apt.dat")).unwrap(),
        "EPKK",
    )
    .unwrap();
    let ap = data.airports[0].clone();
    // Everything OpenAIP offers ticked, as a user accepting the defaults would.
    let choice = Choice {
        positions: offered_positions(&ap),
    };
    let generated = generate(&ap, &data, &layout, &choice, "2026-10-29");

    let base: toml::Table = toml::Table::try_from(&generated.procs).unwrap();
    let ov = Overrides::load(&dir).unwrap();
    let merged: atc_navdata::procedures::Procedures =
        merge_overrides(base, &ov.table).try_into().unwrap();

    // The hand-made file predates airspaces and has none (the engine does not use them yet);
    // the generated station has them, so they are copied over and left out of the equality.
    let mut hand = hand;
    hand.airspaces = merged.airspaces.clone();

    let diffs = compare(&merged, &hand);
    let text: Vec<String> = diffs.iter().map(|d| d.to_string()).collect();
    assert!(diffs.is_empty(), "differences:\n{}", text.join("\n"));
}

#[test]
fn epkk_regenerated_twice_has_no_orphans() {
    let dir = epkk_dir();
    let data = epkk_data();
    let layout = parse_apt_dat(
        &std::fs::read_to_string(dir.join("apt.dat")).unwrap(),
        "EPKK",
    )
    .unwrap();
    let ap = data.airports[0].clone();
    let choice = Choice {
        positions: offered_positions(&ap),
    };
    let first = generate(&ap, &data, &layout, &choice, "2026-10-01");
    let second = generate(&ap, &data, &layout, &choice, "2026-10-29");
    let ov = Overrides::load(&dir).unwrap();
    // The overrides add VFR points and holds OpenAIP never had: not orphans.
    assert!(orphans(&first.procs, &second.procs, &ov).is_empty());
}
