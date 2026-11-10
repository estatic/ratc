# Simulator Adapter Boundary (traffic phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every simulator-specific part of today's code sits behind one adapter boundary. On the client that is a reader trait plus a display trait. On the server it is a data-source trait for layout, procedures and airports. Nothing changes for the user, and every existing test stays green.

**Architecture:**
- **Server side (`atc-navdata`).** A new `source` module defines the `SimDataSource` trait: airport layout, procedure set, airport list and cache inputs. It also defines a serde `SimDataConfig` that opens a source. The X-Plane implementation (`XPlaneSource`) wraps the existing `atc_navdata::xplane` functions. `atc-server` and the wizard (`atc-stations`) hold an `Option<Arc<dyn SimDataSource>>` and never name a simulator. A test guards that.
- **Client side (`atc-client`).** A new `sim` module defines `SimUpdate`, `SimInfo`, `SimReader` (own-aircraft state, radios, transponder, type, weather, sim clock, draw range) and `SimDisplay` (on-screen text, follow-me car). Today's RREF reader, datarefs and overlay socket move to `sim/xplane/` and implement the traits. `App` uses only the traits, and console lines name the simulator through `SimInfo`.
- **Wire and config.** Nothing changes on the wire. The `xpdr_mode` codes keep their values but are documented as protocol codes. The server config gains a `[sim_data]` section, and the old top-level `xplane_root` key still loads.

**Tech Stack:** Rust 2024 workspace, serde/toml, tokio. Tests: `cargo test`.

**Spec:** `docs/superpowers/specs/2026-11-10-traffic-design.md`. Read goal 6 (clock), goal 12 (multi-simulator), the Phases section (phase 1 "Adapter boundary") and the anti-goals ("Server code that names a simulator").

## Global Constraints

- English code, comments and docs. Russian text only under `docs/user-guide/ru/`.
- Work on a feature branch `refactor/sim-adapter-boundary`, created from `docs/traffic-design`, or from `main` once the design PR is merged. Never push to `main`; GitHub changes go through a PR only.
- Never commit `config/server.toml`, `config/client.toml`, `track.csv`, `docs/.obsidian/`, or wizard station data (anything under `data/airports/*` except the tracked EPKK files).
- Tests must not depend on git-ignored station data or on a local X-Plane install. Use the fixtures in `crates/atc-navdata/tests/fixtures/{xp,xplane}` and `crates/atc-stations/tests/fixtures/xp`.
- **No behaviour change.** Console and log lines the user sees stay byte-identical for X-Plane, except the server log lines named in Task 3. The wire format (`atc-proto`) does not change, and `PROTOCOL_VERSION` stays 2.
- Existing tests stay green. Tests that use a renamed API are adapted mechanically, and their assertions keep the same values. Each adapted test is named in the task report.
- Verification is `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --features atc-core/testkit`.
- Two `atc-replay` tests fail locally only because of the git-ignored `data/airports/EPKK/station.toml`. They pass in a clean worktree; report them as known. Goldens (`scenarios/*.expected.txt`) must NOT change in this phase. Never run `UPDATE_GOLDEN=1` outside a clean worktree. `git diff --exit-code scenarios/` must be empty at the end of every task.
- Windows and macOS CI builds must keep working. Add no platform-only dependency.
- The X-Plane plugin crate builds as before: `cargo build -p atc-xplane-plugin`.

## Decisions (author's calls; the user should check those marked ★)

1. ★ **New server config key, old one still accepted.** The new form is the section below. The old top-level `xplane_root = "<dir>"` loads exactly as `sim = "xplane"`, `root = "<dir>"`. Setting both is a config error that names both keys. `config/server.example.toml`, README and RUNNING.md show the new form and mention the old key. The user guides (EN + RU) and `data/airports/EPKK/SOURCE.md` still say `xplane_root`, which keeps working; they are not edited in this phase.
   ```toml
   [sim_data]
   sim = "xplane"
   root = "/path/to/X-Plane 11"
   ```
2. ★ **Client config is unchanged** (`xplane = "host:port"`, `overlay = ...`). These are the X-Plane adapter's settings. A `sim = ...` selector arrives with the second adapter (phase 10/11), when it has something to select.
3. ★ **Wizard texts stay byte-identical** ("No X-Plane layout for …; check xplane_root"). The wizard switches to the trait for data access only. Its messages become simulator-neutral in phase 11, when a second source exists. The station's layout file stays in apt.dat format, which is the repo's storage format for layouts (see the fit check for how MSFS writes it).
4. **The trait lives in `atc-navdata`** (`atc_navdata::source`), next to the model types it returns. The free functions in `atc_navdata::xplane` stay public and unchanged. Tests in `atc-core`, `atc-navdata` and `atc-replay` keep using them. `atc-replay` keeps its `navdata = "xplane"` scenario key and `ATC_XPLANE_ROOT`: it is a test tool, not server code.
5. **Transponder wire codes.** `TelemetryFrame::xpdr_mode` keeps its numbers, now documented as protocol codes `0 off, 1 standby, 2 on, 3 test`. Those are X-Plane 11's numbers, which `TransponderMode` already decodes. The proto doc comment that says "3 alt, 4 test" is wrong and gets fixed. `TransponderMode::from_xplane` becomes `TransponderMode::from_code`. Every adapter maps its own sim's values to these codes.
6. **Time (goal 6).** The only change now is documentation, because this phase is a pure refactor. `utc_s` and `day_of_year` are declared UTC on the wire, and converting to UTC is the adapter's job. ★ Known gap: X-Plane's `sim/time/local_date_days` is the *local* date, so near midnight UTC `day_of_year` can be off by one. It only affects the delivery opening hours. It is not fixed here, because fixing it changes behaviour. Phase 2 (world clock) fixes it in the X-Plane reader.
7. **Draw range (visibility).** `SimUpdate::DrawRange { metres }` is defined now. X-Plane does not produce it before phase 5, and `App` ignores it. Weather visibility (`WeatherFrame::visibility_m`) is unrelated and stays as it is.
8. **Not defined now; added in phase 5.** These need the bot-state type from phase 2: `SimDisplay::draw_traffic(&mut self, bots: &[TrafficState])`, `SimControl::hold_real_time(&mut self, on: bool)` (undo pause and acceleration), `SimControl::set_utc(&mut self, utc: WorldUtc)`. Phase 5 adds a `SimControl` trait next to `SimDisplay`. The names are reserved here.
9. **Guard tests** enforce the anti-goal. Lowercased lines must not contain `xplane`, `x-plane`, `msfs`, `simconnect` or `prepar3d`. The scan covers:
   - Server: `crates/atc-server/src/**` and `crates/atc-core/src/**`.
   - Client: `crates/atc-client/src/**` except `src/sim/xplane/**`, `src/main.rs` and `src/config.rs`.
   - Scanning a file stops at its first `#[cfg(test)]` line, so unit-test literals may still say "X-Plane".
   - A line containing the marker `sim-neutral:` is allowed. The marker has exactly two uses: the legacy config key (`// sim-neutral: legacy config key`) and the adapter list `pub mod xplane; // sim-neutral: adapter list` in `crates/atc-client/src/sim/mod.rs`.

## Adapter interface

### Server: `crates/atc-navdata/src/source.rs`

```rust
/// One airport's layout in apt.dat text (the repo's layout format), from a simulator's data.
pub struct AirportLayout { pub origin: String, pub ident: String, pub apt_dat: String }
impl AirportLayout { pub fn parse(&self, icao: &str) -> Result<Airport, NavdataError> } // same as ResolvedAirport::parse

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// The source has no such airport / no procedures for it.
    #[error("{0}")] NotFound(String),
    /// The data exists but cannot be read or used.
    #[error("{0}")] Unusable(String),
}

pub trait SimDataSource: Send + Sync {
    /// For logs: "X-Plane scenery at <root>".
    fn label(&self) -> String;
    fn airport_layout(&self, icao: &str) -> Result<AirportLayout, SourceError>;
    fn procedure_set(&self, icao: &str) -> Result<ProcedureSet, SourceError>;
    /// Airports within `radius_nm` of `around` (f64::INFINITY = all), sorted by ICAO.
    fn airports(&self, around: LatLon, radius_nm: f64) -> Result<Vec<Destination>, SourceError>;
    /// Files whose size/mtime change makes a cached airport scan stale.
    fn cache_inputs(&self) -> Vec<PathBuf>;
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "sim", rename_all = "snake_case", deny_unknown_fields)]
pub enum SimDataConfig { Xplane { root: PathBuf } }
impl SimDataConfig {
    pub fn from_legacy_root(root: PathBuf) -> Self;     // the old top-level key
    pub fn open(&self) -> Arc<dyn SimDataSource>;
}
```

`ResolveError` maps to `SourceError` as follows: `NotFound` and `CifpNotFound` become `SourceError::NotFound(e.to_string())`, and every other variant becomes `Unusable(e.to_string())`.

### Client: `crates/atc-client/src/sim/mod.rs`

```rust
pub enum SimUpdate {                        // replaces XpUpdate
    /// Contract: `utc_s`/`day_of_year` in UTC; `xpdr_mode` in protocol codes; `t` advances only
    /// while the sim runs (see SimClock); `listen_khz` per TelemetryFrame.
    Telemetry(TelemetryFrame),
    Weather(WeatherFrame),
    /// Type or tail changed (also: first reading).
    Aircraft(SimAircraft),
    /// How far the sim draws other aircraft. Not produced before phase 5.
    DrawRange { metres: f64 },
}
pub struct SimInfo { pub name: &'static str, pub address: String, pub config_key: &'static str }
pub trait SimReader: Send + 'static {
    fn info(&self) -> SimInfo;
    /// Starts reading on the tokio runtime; updates go to `tx` until it closes; errors are logged here.
    fn spawn(self: Box<Self>, tx: mpsc::Sender<SimUpdate>);
}
pub trait SimDisplay: Send {
    fn show_text(&mut self, line: &str);
    /// Only `ServerMsg::FollowMe` / `FollowMeCancel` are passed.
    fn follow_me(&mut self, msg: &ServerMsg);
}
pub fn is_follow_me(msg: &ServerMsg) -> bool;
pub struct SimClock { .. }                  // moved unchanged from xplane.rs
```

## Fit check: other simulators behind the same interfaces, with no server change

| Interface item | X-Plane 11 (this plan) | X-Plane 12 (phase 10) | MSFS 2020/2024, P3D (SimConnect; phases 11–12) |
|---|---|---|---|
| Telemetry position, speeds, on-ground | RREF datarefs | Same datarefs | SimVars `PLANE LATITUDE/LONGITUDE`, `PLANE ALTITUDE`, `AIRSPEED INDICATED`, `GROUND VELOCITY`, `VERTICAL SPEED`, `SIM ON GROUND` via `RequestDataOnSimObject` (period SECOND or VISUAL_FRAME, throttled to 5 Hz) |
| Radios (tx + listened) | `com1/2_frequency_hz_833`, audio panel selectors | Same | `COM ACTIVE FREQUENCY:1/2`, `COM TRANSMIT:1/2`, `COM RECEIVE:1/2` |
| Transponder code / mode | `transponder_code`, `transponder_mode` (0–3 = protocol codes) | Mode values differ (0 off, 1 stby, 2 on, 3 alt, 4 test, 5 gnd): adapter maps 2/3/5→2, 4→3 | `TRANSPONDER CODE:1` (BCO16, adapter decodes), `TRANSPONDER STATE:1` (0 off, 1 stby, 2 test, 3 on, 4 alt): maps 3/4→2, 2→3 |
| Aircraft type / tail | `acf_ICAO`, `acf_tailnum` byte datarefs | Same | `ATC MODEL` / `ATC TYPE` (adapter extracts the ICAO designator), `ATC ID` |
| Weather | `sim/weather/*`, cloud layers | XP12 weather datarefs (`sim/weather/aircraft/...`); clouds best-effort | `AMBIENT WIND DIRECTION/VELOCITY`, `AMBIENT VISIBILITY`, `AMBIENT TEMPERATURE`, `SEA LEVEL PRESSURE`. There is no cloud-layer SimVar, so `clouds` is empty, which the protocol allows. |
| Sim clock (UTC) | `zulu_time_sec`, `local_date_days` (known local-date gap, phase 2) | Same plus fix | `ZULU TIME`, `ZULU DAY OF YEAR` (already UTC) |
| `t` and pause | `total_running_time_sec` + `paused` → `SimClock` | Same | `SIMULATION TIME` + system events `Pause`/`Sim` → `SimClock` |
| Draw range | phase 5 (render distance dataref) | phase 10 | Fixed estimate per sim setting (SimConnect exposes none); adapter constant |
| On-screen text | Overlay plugin over UDP (`XPlaneDisplay`) | Same plugin rebuilt for XP12 SDK | No text API in MSFS (`SimConnect_Text` is gone). `show_text` is a no-op, so console only, until an in-sim panel exists. P3D: `SimConnect_Text`. |
| Follow-me car | Plugin draws the object, fed by `overlay_command` datagrams | Same | `AICreateSimulatedObject` (ground vehicle title), positioned each frame by the client via `SetDataOnSimObject`, removed with `AIRemoveObject` |
| Draw bots (phase 5) | Plugin AI slots, `XPLMDisableAIForPlane` | Same | `AICreateNonATCAircraft` → `AIReleaseControl` → positioned by the client (the same "client positions it" model) |
| Pause/accel prevention, set clock (phase 5) | Plugin resets `sim/time/paused`, `sim_speed`; writes `zulu_time_sec` | Same | Events `PAUSE_OFF`, `SIM_RATE_SET`; `ZULU_HOURS_SET` etc. where allowed |
| **Server** layout | `XPlaneSource::airport_layout`: apt.dat block from scenery packs | Same type, XP12 global-airport path added to the candidate list | `MsfsSource` reads a Little Navmap DB (SQLite: `airport`, `runway`, `taxi_path`, `parking`) or BGL. It renders an `AirportLayout` in apt.dat text, which the wizard copies as is. |
| **Server** procedures | CIFP + `earth_fix/nav.dat` → `ProcedureSet` | Same | Little Navmap `approach`/`approach_leg`/`transition` tables → `ProcedureSet` |
| **Server** airports + cache | apt.dat scan; `cache_inputs` = apt.dat files | Same | `airport` table; `cache_inputs` = the DB file |

SimConnect is a Windows DLL. The MSFS reader is built only on Windows (`cfg(windows)`), and Linux/macOS CI is unaffected. Everything above stays behind `SimReader`, `SimDisplay` or `SimDataSource`, so `atc-server`, `atc-core` and `atc-proto` need no change for any of these simulators.

## Review Focus

1. A user's existing `config/server.toml` has only `xplane_root`. Layout, CIFP, destinations and the wizard must behave exactly as before. Pinned in Task 3 (`legacy_root_key_loads_as_xplane_source`) and Task 2 (the wizard tests keep the X-Plane fixture).
2. Both `[sim_data]` and `xplane_root` set: a clear config error naming both, not a silent pick. Pinned in Task 3 (`both_sim_data_and_legacy_root_is_an_error`).
3. Source configured but the root does not exist, or the airport or CIFP is missing. The server falls back to the repo apt.dat and answers "unable IFR", as today. Pinned by the adapted `layout_falls_back_to_the_repo` and `procedure_set_is_none_without_cifp_or_install`.
4. The destination cache written before the upgrade is still reused: the same files, the same JSON format. Pinned in Task 1 (`cache_inputs_are_the_apt_dat_candidates`) and the existing cache tests in `crates/atc-server/tests/stations.rs`.
5. The overlay address does not resolve, or the plugin is not running. The client keeps working, prints to the console only, and drops follow-me without a panic. Pinned in Task 5 (`display_without_overlay_drops_everything`).

---

### Task 1: `SimDataSource` trait and the X-Plane source (atc-navdata)

**Files:**
- Create: `crates/atc-navdata/src/source.rs`
- Modify: `crates/atc-navdata/src/lib.rs` (add `pub mod source;`; rewrite the crate doc so it describes the model, parsers and sources), `crates/atc-navdata/src/xplane.rs` (add `XPlaneSource`)
- Test: `crates/atc-navdata/tests/source.rs`

**Interfaces:**
- Produces: everything in "Adapter interface / Server" above, plus `atc_navdata::xplane::XPlaneSource { pub fn new(root: impl Into<PathBuf>) -> Self }`, which implements `SimDataSource`. `label()` returns `format!("X-Plane scenery at {}", root.display())`. `airport_layout` uses `resolve_airport`, with `origin = resolved.source.display().to_string()`. `procedure_set` uses `load_procedure_set`. `airports` uses `load_destinations(root, around, radius_nm, &[])`. `cache_inputs` uses `apt_dat_candidates(root)`. `SimDataConfig::open` for `Xplane { root }` returns `Arc::new(XPlaneSource::new(root))`.
- `serde` already depends on derive. Add `thiserror` only if it is not already a dependency (it is: `atc-navdata/Cargo.toml`).

- [ ] **Step 1: Write the failing tests** in `crates/atc-navdata/tests/source.rs`. Use the existing fixtures `tests/fixtures/xp` (EPKK layout "Global Krakow") and `tests/fixtures/xplane` (ZZZZ with CIFP):
  - `xplane_source_layout_matches_resolve_airport`: `XPlaneSource::new(XP).airport_layout("EPKK")`. Assert `parse("EPKK")` equals `resolve_airport(XP,"EPKK").unwrap().parse("EPKK")`, and `apt_dat == resolved.text`, `ident == resolved.ident`, `origin == resolved.source.display().to_string()`.
  - `missing_airport_is_not_found`: `airport_layout("QQQQ")` is `Err(SourceError::NotFound(_))`.
  - `procedures_match_load_procedure_set`: on the `xplane` fixture, `procedure_set("ZZZZ")` equals `load_procedure_set(XPLANE,"ZZZZ")` (compare `icao`, `cycle`, `procedures.len()`, `fixes.len()`). `procedure_set("EPKK")` is `Err(SourceError::NotFound(_))`.
  - `airports_match_load_destinations`: `airports(LatLon{lat:0.0,lon:0.0}, f64::INFINITY)` equals `load_destinations(XPLANE, same, INFINITY, &[])` (ICAO list).
  - `cache_inputs_are_the_apt_dat_candidates`: `cache_inputs() == apt_dat_candidates(XPLANE)`.
  - `nonexistent_root_is_not_found_not_panic`: `XPlaneSource::new("/nonexistent/X")`: the layout is `Err(NotFound)`, the procedure set is `Err(NotFound)`, and `cache_inputs()` is empty.
  - `sim_data_config_parses`: TOML `sim = "xplane"\nroot = "/a"` deserializes to `SimDataConfig::Xplane{root:"/a".into()}`. `sim = "fs98"` fails. An extra key `rooot = "/a"` fails. `SimDataConfig::from_legacy_root("/a".into())` equals the parsed value.
  - `open_gives_the_xplane_source`: `SimDataConfig::from_legacy_root(XP.into()).open().label()` is `format!("X-Plane scenery at {XP}")`.
- [ ] **Step 2: Run** `cargo test -p atc-navdata --test source`. Expected: FAIL (module `source` not found).
- [ ] **Step 3: Implement** `source.rs` and `XPlaneSource` with the signatures above. `AirportLayout::parse` has the same body as `ResolvedAirport::parse`; make `ResolvedAirport::parse` delegate to it or keep both, the builder's choice. If `deny_unknown_fields` does not combine with the internal tag in serde, put the field on the variant's struct instead; the "extra key fails" assertion decides.
- [ ] **Step 4: Run** `cargo test -p atc-navdata`. Expected: all PASS (the existing `xplane_resolve`, `procedure_set` and `cifp_parse` tests unchanged).
- [ ] **Step 5: Commit** `git add crates/atc-navdata && git commit -m "navdata: SimDataSource trait with the X-Plane source"`

### Task 2: Airport wizard reads through `SimDataSource` (atc-stations)

**Files:**
- Modify: `crates/atc-stations/src/wizard/mod.rs` (`WizardEnv`), `crates/atc-stations/src/wizard/create.rs` (`match_layout`, the write path, `apt_dat_text`), `crates/atc-stations/src/wizard/update.rs` (`layout_for_update`), `crates/atc-server/src/main.rs` (interim: build the source for the wizard)
- Test: `crates/atc-stations/tests/wizard.rs` (adapt `ctx` and `no_xplane_layout_creates_nothing` mechanically)

**Interfaces:**
- Consumes: `SimDataSource`, `AirportLayout`, `XPlaneSource`, `SimDataConfig::from_legacy_root(..).open()` (Task 1).
- Produces:
  - `WizardEnv { data_dir, pub source: Option<Arc<dyn SimDataSource>>, store, key_dir, now_utc }`, which replaces `xplane_root`.
  - `WizardEnv::new(data_dir: PathBuf, source: Option<Arc<dyn SimDataSource>>, store: Store, key_dir: PathBuf) -> Self`.
  - `fn match_layout(source: &dyn SimDataSource, ap: &OaAirport) -> Option<(String, AirportLayout)>`: the same rule (by ICAO, else nearest within 300 m via `source.airports(at, MATCH_M / M_PER_NM)`, then `airport_layout(&nearest.icao)`, id = `layout.ident`).
  - `apt_dat_text(layout: &AirportLayout, id: &str)` reads `layout.apt_dat`.
  - The "Cannot read the … layout" message prints `layout.origin`, which is byte-identical to the old `resolved.source.display()`.

- [ ] **Step 1: Adapt the tests.** In `ctx()`, set `source: Some(Arc::new(XPlaneSource::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xp"))))`. In `no_xplane_layout_creates_nothing`, use `c.env.source = None`. Do not change any expected string.
- [ ] **Step 2: Run** `cargo test -p atc-stations --test wizard`. Expected: FAIL to compile (`source` field missing).
- [ ] **Step 3: Implement** the interfaces above in `wizard/mod.rs`, `create.rs` and `update.rs`. `layout_for_update` tries `env.source` → `airport_layout(id)` → `parse(id)` first, then the station's own `apt.dat`, as today. Every user-facing string stays byte-identical (decision 3). In `crates/atc-server/src/main.rs::run_wizard`, pass `cfg.xplane_root.clone().map(|r| SimDataConfig::from_legacy_root(r).open())`. This is an interim line that Task 3 replaces.
- [ ] **Step 4: Run** `cargo test -p atc-stations && cargo build -p atc-server`. Expected: all PASS, and the build succeeds.
- [ ] **Step 5: Commit** `git commit -am "stations: wizard reads the layout through SimDataSource"`

### Task 3: Server loads layout, procedures and destinations through the source; `[sim_data]` config

**Files:**
- Modify: `crates/atc-server/src/config.rs`, `crates/atc-server/src/layout.rs`, `crates/atc-server/src/stations.rs`, `crates/atc-server/src/main.rs`, `config/server.example.toml`, `README.md` (config table row and the troubleshooting rows that name `xplane_root`), `docs/RUNNING.md` (the "Airport layout from X-Plane" section and the other `xplane_root` mentions found with `grep -n xplane_root docs/RUNNING.md`)
- Test: `crates/atc-server/tests/config_sim_data.rs` (new); adapt `crates/atc-server/tests/layout.rs` and `crates/atc-server/tests/stations.rs` mechanically

**Interfaces:**
- Consumes: Task 1 types.
- Produces:
  - `ServerConfig { …, pub sim_data: Option<SimDataConfig>, … }`. A private raw field takes the old key. Its attribute line carries the marker, because rustfmt keeps the attribute on its own line: `#[serde(default, rename = "xplane_root")] // sim-neutral: legacy config key`, then `legacy_root: Option<PathBuf>,`.
  - `load_config` merges `legacy_root` into `sim_data` via `SimDataConfig::from_legacy_root`. When both are set it returns an error whose text contains `[sim_data]` and `xplane_root`. Build the text from a `const LEGACY_ROOT_KEY: &str = "xplane_root"; // sim-neutral: legacy config key`.
  - `pub fn load_airport(source: Option<&dyn SimDataSource>, repo_dir: &Path, icao: &str) -> Result<Airport>`
  - `pub fn load_procedure_set_for(source: Option<&dyn SimDataSource>, icao: &str) -> Option<ProcedureSet>`. Results map to logs: `Ok` logs info, `Err(NotFound)` and `Err(Unusable)` each keep today's warning with the "unable IFR" wording, and `None` source logs a warning.
  - `pub fn load_all_destinations(source: &dyn SimDataSource, cache: &Path, names: &[(String, String)]) -> Vec<Destination>`. `sources_of` takes `source.cache_inputs()`. The scan calls `source.airports(LatLon{lat:0.0,lon:0.0}, f64::INFINITY)`. The cache JSON format is unchanged.
  - `pub fn load_stations(data_dir: &Path, only: Option<&str>, source: Option<&dyn SimDataSource>, cfg: &EngineConfig) -> Result<Vec<LoadedStation>>`, and `shared_destinations` takes the same `Option<&dyn SimDataSource>`.
  - `main.rs` opens `cfg.sim_data.as_ref().map(SimDataConfig::open)` once. It passes `.as_deref()` to `load_stations` and the `Arc` to `WizardEnv::new`.
- Log lines that change, and only these:
  - `airport layout: <file> (<label>)…`: the label is the source's `label()` where it was "X-Plane scenery".
  - Warnings that said `xplane_root not set` now say `no simulator data source configured ([sim_data] in config/server.toml)`.
  - `procedures: … from <label>: cycle …`.
  - No server source line may name a simulator except the two marked legacy lines.

- [ ] **Step 1: Write the failing tests** in `crates/atc-server/tests/config_sim_data.rs`. Build each config text from `include_str!("../../../config/server.example.toml")`: drop any `sim_data`/`xplane_root` lines, then append the variant.
  - `sim_data_section_loads`: append `[sim_data]\nsim = "xplane"\nroot = "/x"`. Then `cfg.sim_data == Some(SimDataConfig::from_legacy_root("/x".into()))`. Put the appended section at the end of the file, after `[piper]`, or before the first table; the builder's choice, as long as TOML parses.
  - `legacy_root_key_loads_as_xplane_source`: append the top-level line `xplane_root = "/x"` before the first `[` table. Expect the same value.
  - `both_sim_data_and_legacy_root_is_an_error`: `load_config` is `Err`, and the `{e:#}` text contains `[sim_data]` and `xplane_root`.
  - `no_source_is_none`: neither key, so `sim_data == None`.
- [ ] **Step 2: Adapt the existing tests mechanically.**
  - `tests/layout.rs`: `load_airport(Some(&XPlaneSource::new(XP)), …)`. `None` stays `None`. `/nonexistent/X-Plane 11` becomes `Some(&XPlaneSource::new("/nonexistent/X-Plane 11"))`. `load_procedure_set_for` and `load_all_destinations` are wrapped the same way.
  - `tests/stations.rs`: `load_all_destinations(&XPlaneSource::new(&root), …)`. The `load_stations(…, None, …)` calls are unchanged.
  - `src/config.rs` unit tests: `c.xplane_root` becomes `c.sim_data`. The filter on `"xplane_root"` lines uses `LEGACY_ROOT_KEY`.
  - All assertion values stay the same.
- [ ] **Step 3: Run** `cargo test -p atc-server --features atc-core/testkit`. Expected: FAIL to compile.
- [ ] **Step 4: Implement** the interfaces above. Rewrite the module docs of `layout.rs`: "from the configured simulator data source when given, else the repo copy". Replace the commented example in `config/server.example.toml` with the `[sim_data]` form plus one comment line: "the old top-level `xplane_root = "<dir>"` is still accepted". Keep the Linux/Windows/macOS path hints. Update README and RUNNING.md: the new form first, then one sentence that `xplane_root` still works.
- [ ] **Step 5: Run** `cargo test -p atc-server --features atc-core/testkit && cargo test -p atc-stations`. Expected: PASS.
- [ ] **Step 6: Commit** `git commit -am "server: layout, procedures and destinations through SimDataSource; [sim_data] config"`

### Task 4: Simulator-neutral server, engine and protocol, guarded by a test

**Files:**
- Modify:
  - `crates/atc-core/src/state.rs`: rename `from_xplane` to `from_code` and rewrite the docs of `utc_s`, `day_of_year` and `TransponderMode::On` neutrally.
  - `crates/atc-server/src/convert.rs`: the call site.
  - `crates/atc-replay/src/lib.rs`: the call sites `TransponderMode::from_xplane` → `from_code`; nothing else.
  - `crates/atc-proto/src/lib.rs`: docs of `xpdr_mode` (protocol codes 0 off, 1 standby, 2 on, 3 test; "each simulator adapter maps its own values"), `utc_s` (UTC seconds of the day, converted by the client), `day_of_year` (UTC day of year, 0 = 1 January) and the `OVERLAY_COMMAND_PREFIX` doc ("the X-Plane adapter's plugin channel"). Docs only, so no wire change.
  - Any remaining simulator name in `crates/atc-server/src/**` or `crates/atc-core/src/**` above the first `#[cfg(test)]` line.
- Test: `crates/atc-server/tests/sim_neutral.rs` (new)

**Interfaces:**
- Produces: `TransponderMode::from_code(code: u8) -> Option<TransponderMode>`, with the same mapping as before (0 Off, 1 Standby, 2 On, 3 Test, else None).

- [ ] **Step 1: Write the failing test** `server_and_engine_name_no_simulator` in `crates/atc-server/tests/sim_neutral.rs`:
  - Walk `concat!(env!("CARGO_MANIFEST_DIR"), "/src")` and `concat!(env!("CARGO_MANIFEST_DIR"), "/../atc-core/src")` recursively over `*.rs`.
  - For each file, scan the lines up to the first one whose trim is `#[cfg(test)]`.
  - Collect `"{path}:{n}: {line}"` for lowercased lines that contain any of `["xplane", "x-plane", "msfs", "simconnect", "prepar3d"]` and not `sim-neutral:`.
  - `assert!(hits.is_empty(), "{}", hits.join("\n"))`.
  - Also add `transponder_codes_decode_as_before` to the existing `atc-core` state tests, or a new `crates/atc-core/tests/transponder.rs`: `from_code(0..=3)` gives `[Off, Standby, On, Test]`, and `from_code(4)` is `None`.
- [ ] **Step 2: Run** `cargo test -p atc-server --test sim_neutral`. Expected: FAIL, listing `state.rs`, `convert.rs` and any leftover doc lines.
- [ ] **Step 3: Rename and reword** until the guard passes. Change no logic.
- [ ] **Step 4: Run** the full verification: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --features atc-core/testkit && cargo build -p atc-xplane-plugin && git diff --exit-code scenarios/`. Expected: all pass, except the two known local `atc-replay` failures.
- [ ] **Step 5: Commit** `git commit -am "core, server, proto: simulator-neutral names, guarded by a test"`

### Task 5: Client `sim` module; the X-Plane reader and overlay move behind it

**Files:**
- Create: `crates/atc-client/src/sim/mod.rs`
- Move with `git mv`, then edit:
  - `crates/atc-client/src/xplane.rs` → `crates/atc-client/src/sim/xplane/mod.rs`
  - `crates/atc-client/src/datarefs.rs` → `crates/atc-client/src/sim/xplane/datarefs.rs`
  - `crates/atc-client/src/overlay.rs` → `crates/atc-client/src/sim/xplane/overlay.rs`
- Modify:
  - `crates/atc-client/src/lib.rs`: modules become `pub mod sim;`. Drop `datarefs`, `overlay` and `xplane`, and drop the root re-exports of X-Plane items. Rewrite the crate doc neutrally.
  - `crates/atc-client/src/app.rs`: the type rename only; `Input::Xp(XpUpdate)` becomes `Input::Sim(SimUpdate)`. Add a `SimUpdate::DrawRange { .. } => {}` arm with the comment `// used from phase 5 (traffic drawing range)`.
  - `crates/atc-client/src/main.rs`: imports only.
  - `crates/atc-client/src/config.rs`: the doc link `crate::sim::xplane::overlay::DEFAULT_ADDR`.
- Test: adapt `crates/atc-client/tests/xplane_fake.rs`, `tests/app_events.rs` and `tests/client_server.rs`. The imports become `atc_client::sim::{SimUpdate}` and `atc_client::sim::xplane::{XpConfig, run_xplane}`, and `Input::Xp` becomes `Input::Sim`. Add new unit tests in `sim/xplane/mod.rs`.

**Interfaces:**
- Consumes: nothing from earlier tasks (independent of Tasks 1–4).
- Produces:
  - The "Adapter interface / Client" types above.
  - `run_xplane(cfg: XpConfig, tx: mpsc::Sender<SimUpdate>) -> anyhow::Result<()>` (otherwise unchanged).
  - `sim::xplane::{NAME: &str = "X-Plane", CONFIG_KEY: &str = "xplane", fn info(address: &str) -> SimInfo}`.
  - `sim::xplane::XPlaneReader { pub fn new(cfg: XpConfig, address: String) -> Self }`, which implements `SimReader`. `spawn` runs `run_xplane` on `tokio::spawn` and logs `X-Plane reader stopped: {e:#}` at error level, as `spawn_links` does today.
  - `sim::xplane::XPlaneDisplay { pub fn new(overlay: Option<Overlay>) -> Self; pub fn open(addr: &str) -> Self }`, which implements `SimDisplay`. `open` moves the `overlay()` helper out of `main.rs`, with both warnings verbatim. `show_text` calls `overlay.send(line)`. `follow_me` sends `overlay_command(msg)` when it is `Some`.
  - `sim::xplane::overlay::{Overlay, DEFAULT_ADDR}` (moved unchanged).
  - `SimClock` moves to `sim/mod.rs` unchanged, together with its tests.
  - `sim/mod.rs` declares `pub mod xplane; // sim-neutral: adapter list`. This is the only simulator name in that file.

- [ ] **Step 1: Write the failing tests** in `sim/xplane/mod.rs` `#[cfg(test)]`:
  - `display_sends_text_and_follow_me`:
    - Bind a `std::net::UdpSocket` on `127.0.0.1:0` with a 2 s read timeout, then use `XPlaneDisplay::new(Some(Overlay::new(addr)?))`.
    - `show_text("hello")` arrives as `b"hello"`.
    - `follow_me(&FollowMe{id:3, path: vec![[50.0,19.0],[50.001,19.0]], end_heading_deg:0.0, aside_left:true})` arrives as a datagram for which `atc_proto::parse_overlay_command` returns the same message.
  - `display_without_overlay_drops_everything`: `XPlaneDisplay::new(None)`. Both calls return without panic.
  - `info_names_the_configured_address`: `info("127.0.0.1:49000") == SimInfo{name:"X-Plane", address:"127.0.0.1:49000".into(), config_key:"xplane"}`, and `XPlaneReader::new(XpConfig::new(addr), "host:49000".into()).info().address == "host:49000"`.
  - `is_follow_me_matches_only_vehicle_commands`, in `sim/mod.rs`: true for `FollowMe` and `FollowMeCancel`, false for `AtcText`.
- [ ] **Step 2: Run** `cargo test -p atc-client`. Expected: FAIL to compile.
- [ ] **Step 3: Implement.** Move the files, add the traits and the X-Plane implementations, and fix the paths. `spawn_links` in `app.rs` still takes `XpConfig` in this task; Task 6 changes it.
- [ ] **Step 4: Run** `cargo test -p atc-client --features atc-core/testkit && cargo clippy -p atc-client --all-targets -- -D warnings`. Expected: PASS, with the existing `xplane_fake`, `app_events` and `client_server` assertions unchanged.
- [ ] **Step 5: Commit** `git add -A crates/atc-client && git commit -m "client: sim module; X-Plane reader and overlay behind SimReader/SimDisplay"`

### Task 6: `App` and identity use only the traits; client guard test

**Files:**
- Modify: `crates/atc-client/src/app.rs`, `crates/atc-client/src/aircraft.rs`, `crates/atc-client/src/main.rs`
- Test: adapt `crates/atc-client/tests/app_events.rs`, `tests/client_server.rs`, and the unit tests in `app.rs` and `aircraft.rs`. Create `crates/atc-client/tests/sim_neutral.rs`.

**Interfaces:**
- Consumes: `SimInfo`, `SimReader`, `SimDisplay`, `is_follow_me`, `XPlaneReader`, `XPlaneDisplay`, `sim::xplane::info` (Task 5).
- Produces:
  - `App::new(to_conn: mpsc::Sender<Outgoing>, print: LineSink, sim: SimInfo) -> App`.
  - `App::with_identity(self, tracker: IdentityTracker) -> App`. The address comes from `SimInfo`.
  - `App::with_sim_display(self, d: Box<dyn SimDisplay>) -> App`, which replaces `with_plugin`.
  - `spawn_links(conn: ConnConfig, identity: watch::Receiver<Option<Identity>>, sim: Box<dyn SimReader>, inputs: mpsc::Sender<Input>) -> mpsc::Sender<Outgoing>`.
  - `IdentityTracker::new(config_callsign: Option<String>, tx: watch::Sender<Option<Identity>>, sim_name: &'static str)`.
  - `pub fn missing_callsign(sim_name: &str) -> String`, which replaces `MISSING_CALLSIGN`. The text is `"{sim_name} has no tail number for this aircraft; set callsign in config/client.toml"`.
  - `pub fn still_flying(sim_name: &str, callsign: &str, aircraft_type: &str) -> String`.
- Behaviour:
  - `App::say` calls `print(line)` and then `display.show_text(line)`.
  - `server_msg` routes `is_follow_me(m)` to `display.follow_me(m)` only, and returns.
  - `main.rs::printer` becomes console-only.
  - Every console line is built from `SimInfo`: `"{name} connected, "`, `"no telemetry from {name} yet (is it running? check \`{config_key}\` in the config) - line dropped"`, `"no COM radio from {name} yet"`, `"Waiting for {name} at {address} for the aircraft type…"`, `"({name} tail number)"`. With X-Plane's `SimInfo` they are byte-identical to today's lines.

- [ ] **Step 1: Write the failing tests.**
  - In `app.rs` tests, rename `follow_me_goes_to_the_plugin_only` to `follow_me_goes_to_the_sim_display_only`. It uses a `FakeDisplay` that records `(kind, payload)`. The same two messages go in, and the asserts are: two follow-me records, the first equal to the `FollowMe` message, and nothing printed or shown as text.
  - New `lines_go_to_console_and_sim_text`: `server_msg(&ServerMsg::Rejected{reason:"x".into()})` puts the same line in `printed` and in the display's text records.
  - Create `crates/atc-client/tests/sim_neutral.rs`: `client_core_names_no_simulator` uses the same scan as Task 4, over `src/**` excluding `src/sim/xplane/`, `src/main.rs` and `src/config.rs`.
- [ ] **Step 2: Run** `cargo test -p atc-client --features atc-core/testkit`. Expected: FAIL (compile errors, and guard hits in `app.rs`/`aircraft.rs`).
- [ ] **Step 3: Implement** the interfaces. Mechanical test updates:
  - `App::new(…, atc_client::sim::xplane::info("127.0.0.1:49000"))` in `app_events.rs`, `client_server.rs` and the `app.rs` tests.
  - `with_identity(IdentityTracker::new(None, tx, "X-Plane"))`.
  - `spawn_links(…, Box::new(XPlaneReader::new(xp, addr)), …)`.
  - In the `aircraft.rs` tests, `MISSING_CALLSIGN` becomes `missing_callsign("X-Plane")`.
  - Every expected string stays byte-identical.
  - In `main.rs`, build `XPlaneReader::new(XpConfig::new(xplane), cfg.xplane.clone())`, take `info()` from it before spawning, use `XPlaneDisplay::open(cfg.overlay.as_deref().unwrap_or(DEFAULT_ADDR))`, and pass `IdentityTracker::new(cfg.callsign.clone(), identity_tx, info.name)`.
- [ ] **Step 4: Run the full verification:** `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --features atc-core/testkit && cargo build -p atc-xplane-plugin && git diff --exit-code scenarios/`. Expected: everything passes except the two known local `atc-replay` failures. Confirm those two pass in a clean worktree with `git worktree add /tmp/<scratch> HEAD`, then `cargo test -p atc-replay` there, then remove the worktree. Do not regenerate goldens.
- [ ] **Step 5: Commit** `git commit -am "client: App and identity use only the sim traits; guard test"`

## After the last task

- `git grep -n -i 'xplane\|x-plane' -- crates/atc-server/src crates/atc-core/src` shows only the two `sim-neutral:` lines of `config.rs` and test modules.
- Manual check, for the user and not for CI: an old `config/server.toml` with `xplane_root` starts the server with the same `airport layout:` taxiway list as before, and the client in X-Plane shows the same console and overlay lines and the follow-me car.
- Update the memory/roadmap note: phase 1 done; phase 2 must fix the X-Plane UTC date gap (decision 6).
