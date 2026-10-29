//! Scripted wizard sessions over synthetic OpenAIP data (`FakeApi`) and a synthetic X-Plane
//! install (`tests/fixtures/xp`).

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use atc_navdata::procedures::Procedures;
use atc_stations::openaip::{
    ApiError, Endpoint, FakeApi, OaAirport, OaAirspace, OaFrequency, OaReportingPoint, OpenAipApi,
};
use atc_stations::store::{Store, save_key_in};
use atc_stations::wizard::{Prompt, WizardEnv, run};

const KEY: &str = "secret-key-7f3a9";
const NOW: &str = "2026-10-29T12:00:00Z";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn airports() -> Vec<OaAirport> {
    serde_json::from_str(&fixture("zzzz_airports.json")).unwrap()
}

fn points() -> Vec<OaReportingPoint> {
    serde_json::from_str(&fixture("zzzz_points.json")).unwrap()
}

fn airspaces() -> Vec<OaAirspace> {
    serde_json::from_str(&fixture("zzzz_airspaces.json")).unwrap()
}

/// Hands the wizard a box around the test's shared fake.
struct Shared(Rc<FakeApi>);

impl OpenAipApi for Shared {
    fn airports(&self, c: &str, u: Option<&str>) -> Result<Vec<OaAirport>, ApiError> {
        self.0.airports(c, u)
    }
    fn airspaces(&self, c: &str, u: Option<&str>) -> Result<Vec<OaAirspace>, ApiError> {
        self.0.airspaces(c, u)
    }
    fn reporting_points(
        &self,
        c: &str,
        u: Option<&str>,
    ) -> Result<Vec<OaReportingPoint>, ApiError> {
        self.0.reporting_points(c, u)
    }
    fn changed_count(&self, e: Endpoint, c: &str, u: &str) -> Result<u64, ApiError> {
        self.0.changed_count(e, c, u)
    }
    fn search_airports(&self, q: &str) -> Result<Vec<OaAirport>, ApiError> {
        self.0.search_airports(q)
    }
}

struct Ctx {
    root: PathBuf,
    env: WizardEnv,
    api: Rc<FakeApi>,
}

fn now() -> String {
    NOW.to_string()
}

fn ctx(tag: &str) -> Ctx {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("vatc-wiz-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let env = WizardEnv {
        data_dir: root.join("data"),
        xplane_root: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xp")),
        store: Store::at(root.join("store")),
        key_dir: root.join("config"),
        now_utc: now,
    };
    let api = Rc::new(FakeApi::new());
    api.set_airports("ZZ", airports());
    api.set_airspaces("ZZ", airspaces());
    api.set_reporting_points("ZZ", points());
    Ctx { root, env, api }
}

impl Ctx {
    fn with_key(self) -> Self {
        save_key_in(&self.env.key_dir, KEY).unwrap();
        self
    }

    fn run(&self, input: &str) -> String {
        let mut prompt = Prompt::new(Cursor::new(input.as_bytes().to_vec()), Vec::new());
        let api = self.api.clone();
        run(
            &self.env,
            move |_key: &str| -> Box<dyn OpenAipApi> { Box::new(Shared(api.clone())) },
            &mut prompt,
        )
        .unwrap();
        String::from_utf8(prompt.into_output()).unwrap()
    }

    fn station(&self, id: &str) -> PathBuf {
        self.env.data_dir.join(id)
    }

    fn procs(&self, id: &str) -> Procedures {
        toml::from_str(&std::fs::read_to_string(self.station(id).join("station.toml")).unwrap())
            .unwrap()
    }
}

impl Drop for Ctx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn khz_of(p: &Procedures) -> Vec<u32> {
    p.positions.iter().map(|x| x.khz).collect()
}

/// Main menu 1, search "ZZZZ", pick it, no neighbours, keep ZZZZ ticked in the area list.
const CREATE_ZZZZ: &str = "1\nZZZZ\n1\n\n\n";

#[test]
fn create_station_writes_station_and_overrides() {
    let c = ctx("create").with_key();
    let out = c.run(&format!("{CREATE_ZZZZ}\n1\n6\n"));
    let dir = c.station("ZZZZ");
    assert!(dir.join("station.toml").is_file(), "{out}");
    assert!(dir.join("overrides.toml").is_file());
    assert!(dir.join("wizard.toml").is_file());
    let p = c.procs("ZZZZ");
    assert_eq!(p.icao, "ZZZZ");
    assert_eq!(khz_of(&p), [127_000, 121_900, 121_700, 118_300, 119_900]);
    // The resolved layout is copied so the station also runs without xplane_root.
    let apt = std::fs::read_to_string(dir.join("apt.dat")).unwrap();
    assert!(atc_navdata::apt::parse_apt_dat(&apt, "ZZZZ").is_ok());
    assert!(!apt.contains("ZZZY"), "only the station's own block");
    // Area list: distance, code, name, kind, main frequency; no code -> em dash.
    assert!(
        out.contains("0 km  ZZZZ  ZETA INTERNATIONAL  airport  TWR 118.300"),
        "{out}"
    );
    assert!(out.contains("22 km  —  NEARBY STRIP  airport"), "{out}");
    assert!(!out.contains("DISTANT FIELD"), "beyond 100 km: {out}");
    assert!(out.contains("[OpenAIP]"), "{out}");
    let wiz: toml::Table = std::fs::read_to_string(dir.join("wizard.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(wiz["openaip_id"].as_str(), Some("ap1"));
    assert_eq!(wiz["country"].as_str(), Some("ZZ"));
}

#[test]
fn value_change_is_stored_as_user_override() {
    let c = ctx("change").with_key();
    // Change value 6 (preferred_runway), first an invalid literal, then "27"; then keep.
    let out = c.run(&format!("{CREATE_ZZZZ}\n2\n6\n27x\n\"27\"\n1\n6\n"));
    assert!(out.contains("Not a valid TOML value"), "{out}");
    assert!(out.contains("preferred_runway = \"27\"   [user]"), "{out}");
    let ov = atc_stations::overrides::Overrides::load(&c.station("ZZZZ")).unwrap();
    assert_eq!(ov.table["preferred_runway"].as_str(), Some("27"));
    assert_eq!(ov.sources["preferred_runway"], "user");
}

#[test]
fn unticked_positions_make_silent_station() {
    let c = ctx("silent").with_key();
    c.run(&format!("{CREATE_ZZZZ}none\n1\n6\n"));
    let text = std::fs::read_to_string(c.station("ZZZZ").join("station.toml")).unwrap();
    assert!(text.contains("positions = []"), "{text}");
    assert!(c.procs("ZZZZ").positions.is_empty());
}

#[test]
fn no_xplane_layout_creates_nothing() {
    let c = ctx("nolayout").with_key();
    // Area pick 3 = YONDER FIELD, which has no apt.dat airport.
    let out = c.run("1\nZZZZ\n1\n\n3\n6\n");
    assert!(
        out.contains(
            "No X-Plane layout for YONDER FIELD: install its scenery or check xplane_root; \
             nothing created."
        ),
        "{out}"
    );
    assert!(!c.station("YYYY").exists());

    // Without an X-Plane install nothing can be created at all.
    let mut c = c;
    c.env.xplane_root = None;
    let out = c.run(&format!("n\n{CREATE_ZZZZ}6\n"));
    assert!(
        out.contains("No X-Plane layout for ZETA INTERNATIONAL"),
        "{out}"
    );
    assert!(!c.station("ZZZZ").exists());
}

#[test]
fn closed_airfield_is_listed_but_not_selectable() {
    let c = ctx("closed").with_key();
    let mut aps = airports();
    let mut closed = aps[2].clone();
    closed.id = "ap9".into();
    closed.name = "OLD FIELD".into();
    closed.kind = 8;
    closed.geometry.coordinates = [10.1, 10.0];
    aps.push(closed);
    c.api.set_airports("ZZ", aps);
    // Area list: 1 ZZZZ, 2 OLD FIELD (11 km), ...; picking 2 is refused, then "none".
    let out = c.run("1\nZZZZ\n1\n\n2\nnone\n6\n");
    assert!(out.contains("OLD FIELD  aerodrome (closed)"), "{out}");
    assert!(out.contains("2 cannot be picked"), "{out}");
}

#[test]
fn airfield_without_icao_uses_xplane_ident() {
    let c = ctx("ident").with_key();
    // Area pick 2 = NEARBY STRIP (no ICAO), matched to apt.dat XZ01 55 m away.
    let out = c.run("1\nZZZZ\n1\n\n2\n\n1\n6\n");
    let dir = c.station("XZ01");
    assert!(dir.join("station.toml").is_file(), "{out}");
    assert_eq!(c.procs("XZ01").icao, "XZ01");
    let apt = std::fs::read_to_string(dir.join("apt.dat")).unwrap();
    assert!(atc_navdata::apt::parse_apt_dat(&apt, "XZ01").is_ok());
}

#[test]
fn existing_apt_dat_is_not_overwritten() {
    let c = ctx("keepapt").with_key();
    let dir = c.station("ZZZZ");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("apt.dat"), "shipped").unwrap();
    c.run(&format!("{CREATE_ZZZZ}\n1\n6\n"));
    assert_eq!(
        std::fs::read_to_string(dir.join("apt.dat")).unwrap(),
        "shipped"
    );
    assert!(dir.join("station.toml").is_file());
}

#[test]
fn update_reports_orphaned_override() {
    let c = ctx("orphan").with_key();
    // Create with an override that holds at NOVEMBER (value 9 = vfr_holds).
    c.run(&format!("{CREATE_ZZZZ}\n2\n9\n[\"NOVEMBER\"]\n1\n6\n"));
    // OpenAIP drops NOVEMBER; the startup check offers the update.
    let pts: Vec<_> = points()
        .into_iter()
        .filter(|p| p.name != "NOVEMBER")
        .collect();
    c.api.set_reporting_points("ZZ", pts);
    let out = c.run("y\n6\n");
    assert!(
        out.contains("ZZZZ: vfr_holds: NOVEMBER is not in OpenAIP any more"),
        "{out}"
    );
    let ov = atc_stations::overrides::Overrides::load(&c.station("ZZZZ")).unwrap();
    assert_eq!(
        ov.table["vfr_holds"],
        toml::Value::Array(vec!["NOVEMBER".into()])
    );
    let p = atc_navdata::station::load_station(&c.station("ZZZZ")).unwrap();
    assert!(p.vfr_point("NOVEMBER").is_none());
}

#[test]
fn update_keeps_position_choice() {
    let c = ctx("keep").with_key();
    // Tick Delivery, Tower and Approach only.
    c.run(&format!("{CREATE_ZZZZ}2 4 5\n1\n6\n"));
    assert_eq!(khz_of(&c.procs("ZZZZ")), [121_900, 118_300, 119_900]);
    // OpenAIP drops Delivery 121.900 and adds a second Ground frequency.
    let mut aps = airports();
    aps[0].frequencies.retain(|f| f.value != "121.900");
    aps[0].frequencies.push(OaFrequency {
        value: "121.800".into(),
        kind: 9,
        name: Some("ZETA GND".into()),
        primary: false,
    });
    c.api.set_airports("ZZ", aps);
    // Decline the startup offer, then menu 2.
    let out = c.run("n\n2\n6\n");
    assert_eq!(khz_of(&c.procs("ZZZZ")), [118_300, 119_900]);
    assert!(
        out.contains("ZZZZ: positions: 121.900 (Zeta Delivery) is not in OpenAIP any more"),
        "{out}"
    );
}

#[test]
fn update_skips_stations_not_made_by_the_wizard() {
    let c = ctx("handmade").with_key();
    c.run(&format!("{CREATE_ZZZZ}\n1\n6\n"));
    let hand = c.station("EPZZ");
    std::fs::create_dir_all(&hand).unwrap();
    std::fs::write(hand.join("procedures.toml"), "").unwrap();
    let out = c.run("n\n2\n6\n");
    assert!(out.contains("Skipping EPZZ"), "{out}");
    assert!(out.contains("Updated ZZZZ"), "{out}");
}

#[test]
fn startup_offers_update_when_changed() {
    let c = ctx("startup").with_key();
    Store::at(c.root.join("store"))
        .fetch_country(&*c.api, "ZZ", "2026-10-01T00:00:00Z")
        .unwrap();
    let out = c.run("n\n6\n");
    assert!(
        out.contains(
            "Updates available for ZZ: 4 airports, 6 airspaces, 3 reporting points. \
             Update now? [Y/n]"
        ),
        "{out}"
    );
    // Declined: the store keeps its old sync time.
    let data = c.env.store.load_country("ZZ").unwrap().unwrap();
    assert_eq!(data.synced_at, "2026-10-01T00:00:00Z");

    let out = c.run("y\n6\n");
    assert!(out.contains("Updates available for ZZ"), "{out}");
    let data = c.env.store.load_country("ZZ").unwrap().unwrap();
    assert_eq!(data.synced_at, NOW);
}

#[test]
fn remove_asks_before_overrides() {
    let c = ctx("remove").with_key();
    c.run(&format!("{CREATE_ZZZZ}\n1\n6\n"));
    let dir = c.station("ZZZZ");
    // Remove, pick ZZZZ, yes to station.toml, no to overrides.toml.
    let out = c.run("n\n4\n1\ny\nn\n6\n");
    assert!(!dir.join("station.toml").exists(), "{out}");
    assert!(dir.join("overrides.toml").exists());
    let a = out.find("Delete station.toml of ZZZZ?").unwrap();
    let b = out.find("Also delete overrides.toml of ZZZZ").unwrap();
    assert!(a < b, "{out}");
}

#[test]
fn bad_menu_input_reasks() {
    let c = ctx("badmenu");
    let out = c.run("9\nabc\n1\n6\n");
    assert_eq!(
        out.matches("Please enter a number from 1 to 6.").count(),
        2,
        "{out}"
    );
    assert!(
        out.contains("Set the OpenAIP API key first (menu 5)."),
        "{out}"
    );
    for item in [
        "1 Create stations in an area",
        "2 Update data",
        "3 Edit a station",
        "4 Remove a station",
        "5 Set the OpenAIP API key",
        "6 Quit",
    ] {
        assert!(out.contains(item), "{item}: {out}");
    }
}

#[test]
fn end_of_input_quits() {
    let c = ctx("eof").with_key();
    let out = c.run("1\nZZ");
    assert!(!c.station("ZZZZ").exists(), "{out}");
}

#[test]
fn invalid_key_is_reported_without_echoing_it() {
    let c = ctx("badkey");
    c.api.fail(Endpoint::Airports, ApiError::Auth);
    let typed = "my-typed-key-0042";
    let out = c.run(&format!("5\n{typed}\nn\n6\n"));
    assert!(out.contains("https://www.openaip.net"), "{out}");
    assert!(!out.contains(typed), "{out}");
    assert!(atc_stations::store::load_key_from(&c.env.key_dir).is_none());

    // A good key is saved.
    c.api.clear_failures();
    let out = c.run(&format!("5\n{typed}\n6\n"));
    assert!(!out.contains(typed), "{out}");
    assert_eq!(
        atc_stations::store::load_key_from(&c.env.key_dir).as_deref(),
        Some(typed)
    );
}

#[test]
fn edit_changes_only_overrides() {
    let c = ctx("edit").with_key();
    c.run(&format!("{CREATE_ZZZZ}\n1\n6\n"));
    let before = std::fs::read_to_string(c.station("ZZZZ").join("station.toml")).unwrap();
    // Edit, pick ZZZZ, change value 4 (transition_altitude_ft) to 7000, keep.
    let out = c.run("n\n3\n1\n2\n4\n7000\n1\n6\n");
    assert!(
        out.contains("[default"),
        "provenance read back from station.toml: {out}"
    );
    let after = std::fs::read_to_string(c.station("ZZZZ").join("station.toml")).unwrap();
    assert_eq!(before, after);
    let p = atc_navdata::station::load_station(&c.station("ZZZZ")).unwrap();
    assert_eq!(p.transition_altitude_ft, 7000);
}
