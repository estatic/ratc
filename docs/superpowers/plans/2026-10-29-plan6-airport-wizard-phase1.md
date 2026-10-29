# Airport wizard, phase 1: wizard and data — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A terminal wizard (`atc-server airports`) that creates, updates, edits and removes stations from OpenAIP + X-Plane data with the user's own key, writes a generated `station.toml` plus the user's `overrides.toml`, and a server that runs from them; EPKK regenerated and proven equal to the hand-made data.

**Architecture:** New crate `atc-stations` holds everything OpenAIP- and wizard-related: API types and client (behind a trait, so tests use a fake), the local store, the pure station generator, overrides and their merge, the comparison report, and the interactive wizard over `BufRead`/`Write`. `atc-navdata::Procedures` stays the engine's input; it gains `Serialize`, an `airspaces` field (carried, unused by the engine until phase 2) and a loader that merges `station.toml` with `overrides.toml`. `atc-server` gets the `airports` subcommand and loads stations through that loader.

**Tech Stack:** Rust 2024 workspace; new deps `ureq` 3 (`json` feature, rustls), `directories` 6, `toml_edit` (comments in written TOML) — add them to `[workspace.dependencies]`.

**Spec:** `docs/superpowers/specs/2026-10-29-airport-wizard-design.md` (phase 1 of §7).

## Global Constraints

- OpenAIP: base `https://api.core.openaip.net/api`, header `x-openaip-api-key`; list endpoints `/airports`, `/airspaces`, `/reporting-points` with `country`, `page`, `limit` (use 1000), `updatedAfter` (ISO 8601 UTC); response `{ items, totalCount, totalPages, page }`.
- OpenAIP codes (from its response schemas): frequency `type` 0 Approach, 5 Delivery, 7 FIS, 9 Ground, 10 Information, 12 Unicom, 13 Radar, 14 Tower, 15 ATIS, 16 Radio, 22 AFIS, 26 Clearance Delivery; airspace `type` 4 CTR, 7 TMA, 10 FIR, 13 ATZ, 26 CTA, 27 ACC sector, 33 FIS sector; vertical limit `unit` 0 m, 1 ft, 6 FL, `referenceDatum` 0 GND, 1 MSL, 2 STD; airport `type` 4 and 7 heliports, 8 closed; elevation in metres MSL; frequency `value` is an MHz string.
- **No OpenAIP data in the repository**, not even in test fixtures: fixtures are synthetic or built from the project's own AIP-sourced values (`data/airports/EPKK/SOURCE.md`, `procedures.toml`).
- **The API key is a secret**: stored only in the user config dir (`directories::ProjectDirs::from("", "", "vatc")` → `config_dir()/openaip.toml`), never logged, never in an error message, never in the repo.
- Downloaded data lives in `ProjectDirs.data_dir()/openaip/<CC>/` (CC = ISO country code, upper case), outside the repo; a failed download leaves the previous files intact (write to `*.tmp`, rename).
- Generated `station.toml` is git-ignored (`data/airports/*/station.toml` in `.gitignore`); `overrides.toml` is the user's and may be committed.
- Stations live in the server's `data_dir` (`data/airports/<ICAO>/`); a station without an ICAO code uses its X-Plane apt.dat ident as `<ICAO>`.
- The wizard works in a plain Windows console: numbered menus, no ANSI codes, UTF-8 names printed as-is (`KRAKÓW`).
- English code, comments and docs; fmt, clippy `-D warnings`, `cargo test --workspace` green before every commit; one commit per task.

## Review Focus

1. **Update keeps the user's choices** — re-running update after OpenAIP changed a frequency keeps every override and reports an override whose target vanished instead of dropping it (Task 6 test `update_reports_orphaned_override`).
2. **Key never leaks** — an HTTP 401/403 error message names the sign-up URL but not the key (Task 2 test `auth_error_hides_key`).
3. **Interrupted download** — a fetch that fails on page 2 leaves the previous country files and sync time untouched (Task 3 test `failed_fetch_keeps_previous_store`).
4. **Airfield without ICAO code or without X-Plane layout** — listed with "—"; choosing one with no apt.dat match explains why and creates nothing (Task 6 test `no_xplane_layout_creates_nothing`).
5. **Fresh clone without a key** — the server still runs EPKK from the shipped `procedures.toml` (Task 7 test `server_falls_back_to_procedures_toml`).

---

### Task 1: `Procedures` can be written, carries airspaces, and loads from station + overrides

**Files:**
- Modify: `crates/atc-navdata/src/procedures.rs`, `crates/atc-navdata/src/model.rs` (`FreqKind`)
- Create: `crates/atc-navdata/src/station.rs` (+ `pub mod station;` in `lib.rs`)
- Test: `crates/atc-navdata/tests/station.rs`

**Interfaces:**
- Produces:
  - `Serialize` on `Procedures` and every type inside it (`CircuitDef`, `PositionDef`, `VfrPoint`, `AdjacentUnit`, `AdjacentKind`, `Tma`, `DeliveryHours`, `IfrDefaults`).
  - `FreqKind::Afis` (new variant; the engine serves no AFIS yet — it is data).
  - `Procedures.airspaces: Vec<AirspaceDef>` (`#[serde(default)]`), `pub struct AirspaceDef { pub kind: AirspaceKind, pub name: String, pub class: Option<char>, pub lower_ft: i32, pub upper_ft: i32, pub polygon: Vec<[f64; 2]> /* [lat, lon] */ }`, `pub enum AirspaceKind { Ctr, Atz, Tma, Cta, Fis, Acc, Fir }`. Limits converted to feet: FL × 100, metres × 3.28084 rounded, GND = 0 (AGL limits are stored as AMSL by adding field elevation — the generator does that, the type only holds feet AMSL).
  - `pub fn merge_overrides(base: toml::Table, overrides: &toml::Table) -> toml::Table` — keys of `overrides` replace those of `base`; tables merge recursively; arrays and scalars are replaced whole; the reserved table `[sources]` of overrides is not merged.
  - `pub fn load_station(dir: &Path) -> Result<Procedures, StationError>` — `station.toml` merged with `overrides.toml` (absent = empty) when `station.toml` exists, else `procedures.toml` alone; `StationError` names the file and the TOML line.

- [ ] **Step 1: Write the failing tests** in `tests/station.rs`:
  - `round_trip`: `parse_procedures(EPKK procedures.toml)` → `toml::to_string` → parse again → equal.
  - `override_replaces_scalar_and_array`: base `preferred_runway = "07"`, `vfr_holds = ["A"]`; overrides `preferred_runway = "25"`, `vfr_holds = ["KILO","ZULU"]` → merged has `"25"` and `["KILO","ZULU"]`.
  - `override_merges_nested_table`: base `ifr = { cruise_ft = 5000 }`, `tma = { radius_nm = 30.0, ceiling_ft = 9500 }`; overrides `[tma] radius_nm = 40.0` → `tma = { radius_nm = 40.0, ceiling_ft = 9500 }`, `ifr` unchanged.
  - `sources_table_is_not_merged`: overrides with `[sources] preferred_runway = "user ruling"` → merged table has no `sources` key.
  - `load_station_prefers_station_toml` / `load_station_falls_back_to_procedures_toml` (temp dirs).
  - `airspaces_default_empty`: today's EPKK `procedures.toml` parses with `airspaces == []`.

- [ ] **Step 2: Run** `cargo test -p atc-navdata --test station` — Expected: FAIL (no `station` module).

- [ ] **Step 3: Implement** the derives, the variant, the types and `station.rs` as specified; `merge_overrides` is plain recursion over `toml::Table`.

- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS (fix any exhaustive `match` on `FreqKind` with an arm that ignores `Afis`, saying so in a comment).

- [ ] **Step 5: Commit** `feat(navdata): stations load from station.toml + overrides.toml; Procedures serialisable, carries airspaces`

---

### Task 2: OpenAIP types and client

**Files:**
- Create: `crates/atc-stations/Cargo.toml` (workspace member; deps `atc-navdata`, `atc-geo`, `serde`, `serde_json`, `toml`, `toml_edit`, `ureq`, `directories`, `thiserror`, `tracing`), `src/lib.rs`, `src/openaip.rs`
- Modify: root `Cargo.toml` (member + workspace deps)
- Test: `crates/atc-stations/src/openaip.rs` (unit tests with JSON literals)

**Interfaces:**
- Produces (in `atc_stations::openaip`):
  - Serde types for the fields we use, unknown fields ignored: `OaAirport { id: String /* _id */, name, icao_code: Option<String>, kind: u8 /* type */, country, geometry: Point, elevation: Value, magnetic_declination: Option<f64>, frequencies: Vec<OaFrequency>, runways: Vec<OaRunway>, updated_at: String }`, `OaFrequency { value: String, kind: u8, name: Option<String>, primary: bool }`, `OaRunway { designator: String, true_heading: Option<f64> }`, `OaAirspace { id, name, kind: u8, icao_class: Option<u8>, lower_limit: Limit, upper_limit: Limit, frequencies: Vec<OaFrequency>, geometry: Polygon, country }`, `Limit { value: f64, unit: u8, reference_datum: u8 }`, `OaReportingPoint { id, name, compulsory: bool, geometry: Point, airports: Vec<String> }` (camelCase rename).
  - `pub fn khz(mhz: &str) -> Option<u32>` — `"121.980"` → `121980`, `"119.95"` → `119950`.
  - `pub trait OpenAipApi { fn list<T: DeserializeOwned>(&self, endpoint: Endpoint, country: &str, updated_after: Option<&str>) -> Result<Vec<T>, ApiError>; fn changed_count(&self, endpoint: Endpoint, country: &str, updated_after: &str) -> Result<u64, ApiError>; fn search_airports(&self, query: &str) -> Result<Vec<OaAirport>, ApiError>; }` with `pub enum Endpoint { Airports, Airspaces, ReportingPoints }`.
  - `pub struct HttpApi { key: String, base: String }` implementing it with `ureq`: all pages (`limit=1000`) for `list`; `limit=1` + `totalCount` for `changed_count`; `search=<query>` for `search_airports`; 429/5xx retried 3 times with 2 s, 4 s, 8 s waits.
  - `pub enum ApiError { Auth, Network(String), Status(u16), Decode(String) }`; `Display` of `Auth` = `"OpenAIP rejected the API key; get one at https://www.openaip.net (Profile → API clients)"`.
- Test-only: `pub struct FakeApi` (behind `#[cfg(any(test, feature = "fake"))]`, feature `fake` in Cargo.toml) serving canned items per endpoint/country and recording calls.

- [ ] **Step 1: Write the failing tests:** decode one synthetic airport / airspace / reporting point JSON (field names as in the API: `_id`, `icaoCode`, `type`, `magneticDeclination`, `lowerLimit { value, unit, referenceDatum }`, …); `khz` cases above plus `"abc"` → `None`; `auth_error_hides_key` — `ApiError::Auth.to_string()` contains `openaip.net` and not the key string used to build the `HttpApi`; `HttpApi`'s `Debug` does not print the key.
- [ ] **Step 2: Run** `cargo test -p atc-stations` — Expected: FAIL.
- [ ] **Step 3: Implement** as specified (no live network call in tests).
- [ ] **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `feat(stations): OpenAIP types and REST client`

---

### Task 3: Local store and key

**Files:**
- Create: `crates/atc-stations/src/store.rs`
- Test: in `store.rs` (temp dirs)

**Interfaces:**
- Consumes: `OpenAipApi`, `Endpoint`, the `Oa*` types (Task 2).
- Produces:
  - `pub struct Store { root: PathBuf }` — `Store::default_location() -> Option<Store>` (`ProjectDirs` data dir + `openaip`), `Store::at(root)` for tests.
  - `pub struct CountryData { pub airports: Vec<OaAirport>, pub airspaces: Vec<OaAirspace>, pub points: Vec<OaReportingPoint>, pub synced_at: String /* ISO UTC */ }`.
  - `fn fetch_country(&self, api: &dyn OpenAipApi /* or generic */, cc: &str, now_utc: &str) -> Result<CountryData, StoreError>` — downloads all three endpoints, writes `<root>/<CC>/{airports,airspaces,reporting-points}.json` and `sync.toml { synced_at }` atomically (all `*.tmp` first, then renames), returns the data.
  - `fn load_country(&self, cc) -> Result<Option<CountryData>, StoreError>`, `fn countries(&self) -> Vec<String>`.
  - `fn changes(&self, api, cc) -> Result<ChangeCount, StoreError>` — `ChangeCount { airports: u64, airspaces: u64, points: u64 }` via `changed_count(.., synced_at)`.
  - `pub fn load_key() -> Option<String>`, `pub fn save_key(key: &str) -> io::Result<PathBuf>` in the config dir file `openaip.toml` (`key = "…"`); on Unix the file is created with mode 0600. Test variants take the directory.
- [ ] **Step 1: Write the failing tests:** `fetch_then_load_round_trips`; `failed_fetch_keeps_previous_store` (FakeApi fails the airspaces call on the second fetch → previous files and `synced_at` unchanged); `changes_uses_synced_at` (FakeApi records `updated_after == synced_at`); `key_file_round_trip_and_mode` (0600 on Unix).
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `feat(stations): local OpenAIP store per country and the API key file`

---

### Task 4: Station generator

**Files:**
- Create: `crates/atc-stations/src/generate.rs`, `crates/atc-stations/src/defaults.rs`
- Test: `crates/atc-stations/tests/generate.rs` with synthetic fixtures in `crates/atc-stations/tests/fixtures/`

**Interfaces:**
- Consumes: `CountryData` (Task 3), `atc_navdata::model::Airport` (X-Plane layout via `atc_navdata::xplane::resolve_airport`), `Procedures`/`AirspaceDef` (Task 1).
- Produces:
  - `pub struct Choice { pub positions: Vec<PositionDef> /* the ticked ones */ }`.
  - `pub fn generate(ap: &OaAirport, data: &CountryData, layout: &Airport, choice: &Choice, fetched_on: &str) -> StationDoc` where `pub struct StationDoc { pub procs: Procedures, pub provenance: BTreeMap<String, String> /* field → "OpenAIP" | "X-Plane apt.dat" | "default: <rule>" */ }`.
  - `pub fn offered_positions(ap: &OaAirport) -> Vec<PositionDef>` — one per OpenAIP frequency whose type maps to a role (table below), callsign = the OpenAIP name title-cased with "APP"/"GND"/"TWR"/"DEL" expanded (`"KRAKÓW APP"` → `"Kraków Approach"`), sorted ATIS, Delivery, Ground, Tower, Approach, Afis, Unicom.
  - `pub fn write_station(doc: &StationDoc, path: &Path) -> io::Result<()>` — TOML via `toml_edit`, each top-level key preceded by `# source: <provenance>`.
- Mapping (document it as a table in `defaults.rs` doc comment):
  - Frequency type → role: 15 ATIS→`Atis`; 5, 26 → `Delivery`; 9, 1 (Apron) → `Ground`; 14 → `Tower`; 0, 2, 6, 13 (Approach/Arrival/Departure/Radar on an airport) → `Approach` (the first is `khz`, the others go to `approach_alt_khz`; first = `primary`, else the lowest frequency); 22, 10 → `Afis`; 12, 16, 4, 11 → `Unicom`.
  - `transition_altitude_ft`: country table in `defaults.rs` with `PL` = 6500 (AIP Polska ENR 1.7, cited in a comment) — add another country only with its AIP citation; every other country falls back to **5000** with provenance `"default: no national value known"`.
  - `circuits`: one per runway end of `layout`, `Left`, `altitude_ft_msl` = field elevation + 1000 rounded **up** to the next 100 ft (EPKK 791 → 1800).
  - `preferred_runway`: the runway end whose true heading is closest to 270° (prevailing westerlies; provenance `"default: closest to 270°"`).
  - `vfr_points`: reporting points whose `airports` contains `ap.id`, `south = false`.
  - `airspaces`: CTR/ATZ/TMA/CTA polygons that contain the airport reference point, plus FIS/ACC/FIR containing it; GND/AGL limits + field elevation.
  - `tma`: from the smallest TMA (else CTR) containing the ARP: `radius_nm` = mean ARP→vertex distance rounded to 1 NM, `ceiling_ft` = its upper limit; none → `None`.
  - `adjacent`: each FIS sector containing the ARP → `Fis` with its frequency and title-cased name ("FIS KRAKÓW-ZACHOD" → "Kraków Information" is **not** guessed: callsign = `"<Name> Information"` from the sector name without the `FIS ` prefix and the `-ZACHOD`/`-WSCHOD`/directional suffix, title-cased); ACC sectors containing the ARP → `Acc` with `min_level_ft` = lower limit; Tower of every other airport within 100 km with a type-14 frequency → `Tower` with `airport`.
  - `magnetic_variation_deg` = `magneticDeclination` (0.0 when absent); `ifr = { cruise_ft = 5000 }` (design spec 3.2 default); `delivery_hours = None`; `source = "OpenAIP <fetched_on>, X-Plane apt.dat"`; `airac = ""`.
- [ ] **Step 1: Write the failing tests** with a synthetic fixture airport "ZZZZ" (two runway ends 09/27, frequencies of types 15/5/9/14/0/0, one CTR, one TMA, one FIS sector, two reporting points of which one belongs to another airport) and the existing X-Plane fixture layout `crates/atc-navdata/tests/fixtures/xplane` (`ZZZZ`): each mapping rule above gets one assertion (`positions_follow_type_table`, `approach_primary_first_rest_alternate`, `circuit_height_rounds_up`, `preferred_runway_closest_to_270`, `vfr_points_of_this_airport_only`, `tma_from_smallest_containing`, `agl_limits_become_amsl`, `fis_callsign_from_sector_name`, `unknown_country_ta_default`), plus `provenance_for_every_top_level_field` and `write_station_has_source_comments`.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `feat(stations): generate a station from OpenAIP and X-Plane data`

---

### Task 5: Overrides, comparison report

**Files:**
- Create: `crates/atc-stations/src/overrides.rs`, `crates/atc-stations/src/compare.rs`
- Test: in those files

**Interfaces:**
- Consumes: `merge_overrides`, `load_station` (Task 1), `StationDoc` (Task 4).
- Produces:
  - `pub struct Overrides { pub table: toml::Table /* without [sources] */, pub sources: BTreeMap<String, String> }`; `Overrides::load(dir) -> Result<Overrides, ..>` (absent = empty), `save(&self, dir)` (keys sorted, `[sources]` last), `set(&mut self, key: &str, value: toml::Value, source: &str)`, `remove(&mut self, key)`.
  - `pub fn orphans(generated: &Procedures, ov: &Overrides) -> Vec<String>` — overrides that name a position frequency, a VFR point or an adjacent unit (by callsign or name) absent from `generated`, as human-readable lines (`"vfr_points: ECHO is not in OpenAIP any more"`).
  - `pub fn compare(a: &Procedures, b: &Procedures) -> Vec<Difference>`, `pub struct Difference { pub field: String, pub left: String, pub right: String }` — field-by-field, lists compared as sets keyed by name/callsign/runway, floats within 1e-6.
- [ ] **Step 1: Write the failing tests:** `save_load_round_trip_keeps_sources`; `set_then_merge_wins`; `orphaned_vfr_point_reported`; `orphaned_position_reported`; `compare_equal_is_empty`; `compare_lists_missing_point_by_name`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat(stations): user overrides, orphan detection and station comparison`

---

### Task 6: The wizard

**Files:**
- Create: `crates/atc-stations/src/wizard.rs`, `crates/atc-stations/src/prompt.rs`
- Test: `crates/atc-stations/tests/wizard.rs` (scripted input, `FakeApi`, temp store and data dir, the X-Plane fixture)

**Interfaces:**
- Consumes: Tasks 2–5; `atc_navdata::xplane::{resolve_airport, load_destinations}` for matching OpenAIP airfields to apt.dat (ICAO match, else the nearest apt.dat airport within 300 m of the OpenAIP point, whose ident becomes the station id).
- Produces:
  - `pub struct Prompt<R: BufRead, W: Write>` with `menu(title, &[&str]) -> usize` (numbered from 1, re-asks on bad input), `ask(question, default: Option<&str>) -> String`, `confirm(question, default: bool) -> bool`, `pick_many(title, items, preselected) -> Vec<usize>` (input like `1 3 5`, `all`, `none`, Enter = keep).
  - `pub struct WizardEnv { pub data_dir: PathBuf, pub xplane_root: Option<PathBuf>, pub store: Store, pub key_dir: PathBuf }` and `pub fn run<R, W>(env: &WizardEnv, api_factory: impl Fn(&str) -> Box<dyn OpenAipApi>, prompt: &mut Prompt<R, W>) -> Result<(), WizardError>`.
  - Main menu (exact items): `1 Create stations in an area`, `2 Update data`, `3 Edit a station`, `4 Remove a station`, `5 Set the OpenAIP API key`, `6 Quit`. At start, when a key and a store exist, the update check runs first and prints `Updates available for PL: 3 airports, 0 airspaces, 1 reporting points. Update now? [Y/n]`.
  - **Create**: ask ICAO or name → `search_airports` → pick → fetch the country (and every neighbour country whose airfields fall within the 100 km circle, by `country` field of nearby results) → list airfields within 100 km (`  12 km  EPKC  KRAKÓW-CZYŻYNY  airfield  TWR 122.330`; no code → `—`; heliports and closed (types 4, 7, 8) are listed, closed ones marked `(closed)` and not selectable) → `pick_many` → per airfield: `pick_many` of `offered_positions` (all preselected) → show every generated value with its provenance, `menu` "Keep / Change a value" (change = `ask` new value for one top-level field, stored as an override with source "user") → write `station.toml` + `overrides.toml`. No positions ticked → silent station (`positions = []`). No apt.dat match → print `No X-Plane layout for <name>: install its scenery or check xplane_root; nothing created.` and continue with the next.
  - **Update**: per country with changes → `fetch_country` → regenerate every station of that country keeping its position choice (the `positions` currently in its `station.toml`, matched by frequency) → print `orphans` lines → write.
  - **Edit**: pick station → same value screen as create.
  - **Remove**: pick station → confirm → delete `station.toml` (and `overrides.toml` only after a second confirm).
  - **Set key**: ask, save, verify with one `changed_count` call (`Auth` → print the error, keep asking or cancel).
- [ ] **Step 1: Write the failing tests** (each a scripted session asserting files and output): `create_station_writes_station_and_overrides`, `unticked_positions_make_silent_station`, `no_xplane_layout_creates_nothing`, `airfield_without_icao_uses_xplane_ident`, `update_reports_orphaned_override` (FakeApi's second dataset drops a reporting point the overrides add), `update_keeps_position_choice`, `startup_offers_update_when_changed`, `remove_asks_before_overrides`, `bad_menu_input_reasks`, `invalid_key_is_reported_without_echoing_it`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat(stations): interactive airport wizard`

---

### Task 7: Server integration

**Files:**
- Modify: `crates/atc-server/Cargo.toml` (dep `atc-stations`), `crates/atc-server/src/main.rs` (subcommand, `build_state`), `.gitignore`
- Test: `crates/atc-server/tests/` (existing test style) and a unit test in `main.rs` for argument parsing

**Interfaces:**
- Consumes: `load_station` (Task 1), `atc_stations::wizard::run` + `HttpApi` (Tasks 2, 6).
- Produces: `atc-server airports [--config <file>]` runs the wizard with `data_dir` and `xplane_root` from the server config and `HttpApi` built from the saved key; plain `atc-server` unchanged except that `build_state` uses `load_station(&dir)` instead of reading `procedures.toml` directly.
- [ ] **Step 1: Write the failing tests:** `airports_subcommand_parses` (`["airports"]`, `["airports","--config","x.toml"]`); `server_falls_back_to_procedures_toml` (a data dir with only EPKK's `procedures.toml` + `apt.dat` builds the state); `server_prefers_station_toml` (a temp dir with a generated `station.toml` whose `preferred_runway` differs is the one used).
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS. Add `data/airports/*/station.toml` to `.gitignore`.
- [ ] **Step 5: Commit** `feat(server): 'atc-server airports' runs the wizard; stations load from station.toml + overrides`

---

### Task 8: EPKK regenerated, overrides committed, docs

**Files:**
- Create: `data/airports/EPKK/overrides.toml`, `crates/atc-stations/tests/fixtures/epkk/{airport,airspaces,reporting-points}.json` (synthetic: built **only** from values in `data/airports/EPKK/procedures.toml` and `SOURCE.md` — AIP — never copied from OpenAIP), `crates/atc-stations/tests/epkk.rs`
- Modify: `README.md` (section "Adding airports"), `docs/RUNNING.md` (section 2 or a new section "Stations and the airport wizard"), `crates/atc-server/src/main.rs` only if Task 7 left something
- Keep: `data/airports/EPKK/procedures.toml` (the shipped fallback for users without an OpenAIP key — my call, see below)

**Interfaces:**
- Consumes: everything above.
- [ ] **Step 1: Write the failing test** `epkk_regenerated_equals_hand_made`: `generate(fixture EPKK, layout = parse_apt_dat(data/airports/EPKK/apt.dat), Choice = the five EPKK positions of procedures.toml)` → `merge_overrides` with `data/airports/EPKK/overrides.toml` → `compare` with `parse_procedures(procedures.toml)` → `[]`. Print the differences on failure.
- [ ] **Step 2: Run** — FAIL (no overrides yet); the failure output is the list of what OpenAIP-shaped data cannot give.
- [ ] **Step 3: Write `overrides.toml`** with exactly the differences, each with a `[sources]` line citing AIP (`"AIP AD 2.22 3.1"`, `"AIP AD 2.20"`, …) or `"user ruling 2026-10-19"` (preferred runway 25, circuits 1800 ft): expected at least `preferred_runway`, `vfr_points` (ECHO, HOTEL, LIMA and the `south` flags), `vfr_holds`, `vfr_hold_max_ft`, `delivery_hours`, `approach_alt_khz` / primary Approach frequency, `tma`, `adjacent` (Warszawa Radar 127.660 `min_level_ft = 9500`, Kraków Information 119.275, Katowice Tower 129.255), `source`, `airac`. No change to `procedures.toml`.
- [ ] **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Docs.** README "Adding airports": get a free OpenAIP key, `target/release/atc-server airports`, what the wizard asks, where files go, that OpenAIP data stays on the user's machine (CC BY-NC 4.0). RUNNING.md: the same in more detail plus the file layout (`station.toml` generated and git-ignored, `overrides.toml` yours, `procedures.toml` fallback) and the update check.
- [ ] **Step 6: Manual acceptance (user, after merge):** with a real key, `atc-server airports` → Create → `EPKK` → keep the five positions → the value screen shows sources; then `cargo test -p atc-stations --test epkk` still green and a circuit at EPKK flies as before.
- [ ] **Step 7: Commit** `feat: EPKK as a wizard station — overrides from AIP and rulings; wizard docs`

---

**Calls made in this plan (not from the spec):**
- Stations stay in the server's `data_dir` (`data/airports/<ICAO>/`); the OpenAIP cache and the key go to the per-user OS directories (`directories` crate).
- Overrides are a partial TOML table merged over the generated one (arrays replaced whole), with a `[sources]` table for provenance; this lets an override set any `Procedures` field.
- EPKK's hand-made `procedures.toml` stays in the repo as the fallback for users without a key; the loader prefers `station.toml` when it exists.
- `preferred_runway` default = the runway end closest to 270°; transition altitude fallback 5000 ft; circuit 1000 ft above field rounded up to 100 ft, left-hand; IFR cruise default 5000 ft.
- AFIS / Information frequencies become `FreqKind::Afis` positions as data only; the engine gets AFIS behaviour in a later phase.
- FIS callsigns are built from the sector name ("<Name> Information"); a wrong guess is fixed with an override.
- The EPKK acceptance test uses a synthetic OpenAIP-shaped fixture made from the project's own AIP values, so no OpenAIP data is committed.
