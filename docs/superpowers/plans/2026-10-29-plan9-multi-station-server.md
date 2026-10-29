# Airport wizard, phase 2: multi-station server — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The server runs every station in `data/airports/` at once, routes each transmission to the station or en-route unit owning its frequency, hands flights between stations and FIS/ACC units with their flight data, releases them outside coverage, and accepts any apt.dat airfield in the world as destination.

**Architecture:** One `AtcEngine` per station (unchanged airport-centric engine) under a server-side `Network` dispatcher. Reason: the engine is built around one airport — taxi graph, runway resource, procedures, ATIS, phase detection, persistence are all per airport — and generalising it to many airports would rewrite most of `atc-core`; the dispatcher only has to decide which engine hears a transmission and when a flight moves. En-route FIS/ACC units are a new small controller (`atc-core::enroute`) driven by sector polygons the wizard writes to `data/airports/enroute.toml`. Flights move between owners as `FlightHandover` values (callsign, type, plan, squawk, cleared level).

**Tech Stack:** Rust 2024 workspace (atc-core, atc-navdata, atc-stations, atc-server).

**Spec:** `docs/superpowers/specs/2026-10-29-airport-wizard-design.md` (§2 goals 6–8, §5.4, §5.5 silent stations, §5.6, §7 phase 2).

## Global Constraints

- Stations: every `data_dir/<ID>/` that `load_station` accepts (station.toml + overrides.toml, or procedures.toml); `airport` in `config/server.toml` becomes optional — set: run only that station (today's behaviour); absent: run all.
- A frequency owned by several stations with the same callsign is **one unit**: a flight already owned by one of those stations stays with it on that frequency (no hand-off on that leg); a pilot calling it without an owner goes to the station whose airport reference point is nearest the aircraft.
- Telemetry goes to the owning engine and to every engine whose airport reference point is within 30 NM of the aircraft (traffic and runway awareness); a pilot is registered in an engine the first time that engine receives its telemetry.
- Hand-off detection: an `AtcOutput` whose items contain `AtcItem::Contact { khz, .. }` where `khz` belongs to another station or an en-route unit moves the flight to it after the pilot's readback completes (the engine's existing handoff state), carrying a `FlightHandover`.
- En-route units (user decision): answer the first call with `<callsign>, <unit>, identified, QNH <qnh>`; at sector exit (polygon or level limits) hand to the next unit containing the aircraft (`contact <unit> <freq>`), or to a station when the aircraft enters its CTR/TMA (`contact <station Approach, else Tower> <freq>`), else release with `frequency change approved`; keep the level last assigned (from the handover); no IFR route management.
- Release outside coverage from a station: where the engine today needs an adjacent ACC/FIS and finds none, it says `frequency change approved` (`AtcItem::FrequencyChangeApproved`) and ends its service.
- Silent stations (no positions): a flight handed to one hears `change to <name> <freq>` when the station has a published frequency (wizard `wizard.toml`/OpenAIP), else `frequency change approved`; nothing answers there.
- Destinations: every airport of the apt.dat files X-Plane loads (no radius); scan cached at `data/cache/destinations.json`, rebuilt when an apt.dat's size/mtime changes.
- Persistence: one snapshot per station (`data/cache/state-<ID>.json`), all saved and restored.
- STT prompt: the prompt of the station (or unit) owning the transmit frequency; unowned → the nearest station's.
- No OpenAIP data in the repo: tests use synthetic stations under `crates/atc-server/tests/fixtures/` (procedures.toml + apt.dat built from the navdata X-Plane fixture airports ZZZZ/ZZZY) and a synthetic `enroute.toml`.
- English code/comments/docs; fmt, clippy `-D warnings`, `cargo test --workspace` green before every commit; one commit per task. Don't commit `config/*.toml`, `track.csv`, `docs/.obsidian/`, wizard stations.

## Review Focus

1. **Single-station users** (config with `airport = "EPKK"`, no other station): behaviour and every existing test unchanged (Task 2 `single_station_config_unchanged`, whole suite).
2. **Two stations sharing Approach** (Kraków Approach at EPKK and EPKT): a flight EPKK → EPKT on 121.075 is not handed off on that frequency and is answered by one engine only (Task 3 `shared_frequency_answered_once`).
3. **Pilot calls a frequency nobody owns**: silence, no panic, no answer from an unrelated engine (Task 3 `unowned_frequency_is_silent`).
4. **Aircraft leaves the last sector**: exactly one `frequency change approved`, then silence until the pilot calls a station (Task 5 `last_sector_releases_once`).
5. **Restart with several stations mid-flight**: each station's snapshot restored, the flight's owner restored (Task 6 `restart_restores_every_station_and_owner`).

---

### Task 1: Engine — handing a flight out and in

**Files:**
- Modify: `crates/atc-core/src/engine/mod.rs`, `crates/atc-core/src/strip.rs`, `crates/atc-core/src/engine/approach.rs` (release when no ACC/FIS)
- Create: `crates/atc-core/src/handover.rs` (+ `pub mod handover;`)
- Test: `crates/atc-core/tests/engine.rs`

**Interfaces:**
- Produces:
  - `pub struct FlightHandover { pub callsign: Callsign, pub aircraft_type: String, pub plan: Option<FlightPlan>, pub squawk: Option<u16>, pub cleared_ft: Option<i32>, pub rules: Option<FlightRules> }` (Serialize/Deserialize; `FlightPlan`/`FlightRules` are the existing strip types).
  - `AtcEngine::export_flight(&mut self, cs: &Callsign) -> Option<FlightHandover>` — snapshot of the flight data, then the flight is removed completely (strip, phase, clearances, frequencies, snapshot entry). If plan 7 already added `remove_pilot`, build on it; otherwise add `remove_pilot(&mut self, cs: &Callsign) -> bool` here with the same contract.
  - `AtcEngine::import_flight(&mut self, h: FlightHandover)` — registers the pilot with that data and marks the strip `handed_in = true` (new `Strip` field, `#[serde(default)]`): its first call on this engine's frequency is answered as a flight handed over (Approach: `<callsign>, <station>, identified` + the existing arrival continuation; Tower: the existing inbound/circuit handling), not as a first contact needing a flight plan.
  - `AtcEngine::handoff_target(&self, cs: &Callsign) -> Option<u32>` — the frequency the flight was told to contact and has read back (the existing `handoff_to` state resolved to kHz), cleared once taken.
  - Approach boundary with neither ACC nor FIS adjacent: `RadarServiceTerminated` + `FrequencyChangeApproved` and the strip leaves Approach's service.
- [ ] **Step 1: Tests:** `export_then_import_keeps_plan_and_squawk`; `export_removes_flight_and_snapshot_entry`; `imported_flight_first_call_is_identified` (IFR flight imported into an engine, call on its Approach → reply contains "identified", no "say intentions"/"no flight plan"); `handoff_target_after_readback` (contact Warszawa Radar read back → `Some(127660)`); `no_adjacent_unit_releases_at_boundary` (procs without ACC/FIS, departure leaves TMA → "radar service terminated, frequency change approved").
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat(core): flights handed out and in between engines`

---

### Task 2: Loading every station

**Files:**
- Create: `crates/atc-server/src/stations.rs`
- Modify: `crates/atc-server/src/config.rs` (`airport: Option<String>`), `crates/atc-server/src/main.rs` (`build_state`), `crates/atc-server/src/layout.rs`, `config/server.example.toml` (comment: omit `airport` to run every station)
- Test: `crates/atc-server/tests/stations.rs`, fixtures `crates/atc-server/tests/fixtures/stations/{ZZZZ,ZZZY}/` (procedures.toml + apt.dat)

**Interfaces:**
- Produces:
  - `pub struct LoadedStation { pub id: String, pub dir: PathBuf, pub procs: Procedures, pub airport: Airport, pub engine: Mutex<AtcEngine> }` and `pub fn load_stations(data_dir: &Path, only: Option<&str>, xplane_root: Option<&Path>, cfg: &EngineConfig) -> anyhow::Result<Vec<LoadedStation>>` — skips (with a warning naming the dir and the error) any directory that does not load; error only when none loads. `enroute.toml` and non-directories are not stations.
  - `build_state` builds `Vec<LoadedStation>`; with one station the server behaves exactly as today.
- [ ] **Step 1: Tests:** `loads_every_station_dir`; `only_loads_the_configured_station`; `broken_station_is_skipped_with_warning`; `single_station_config_unchanged` (EPKK config → one station, same procs as `load_procedures`).
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS (existing server tests still green; adapt `AppState` construction minimally — the dispatcher comes in Task 3).
- [ ] **Step 5: Commit** `feat(server): load every station in data_dir`

---

### Task 3: The network dispatcher

**Files:**
- Create: `crates/atc-server/src/network.rs`
- Modify: `crates/atc-server/src/app.rs` (`AppState.engine` → `AppState.network: Arc<Network>`, `StationMap` built from all stations + en-route units), `crates/atc-server/src/session.rs`, `crates/atc-server/src/pipeline.rs`, `crates/atc-server/src/atis.rs` (one ATIS per station), `crates/atc-server/src/persist.rs` (save loop over stations)
- Test: `crates/atc-server/tests/network.rs` (+ existing ws tests)

**Interfaces:**
- Consumes: Task 1 export/import/handoff_target, Task 2 `LoadedStation`.
- Produces:
  - `pub enum Owner { Station(usize), Unit(usize) }`; `pub struct Network { stations: Vec<LoadedStation>, units: Vec<Mutex<EnrouteUnit>> /* empty until Task 5 */, owners: Mutex<HashMap<Callsign, Owner>>, freq_index: HashMap<u32, Vec<Owner>> }`.
  - `Network::register_pilot(&self, p: PilotProfile)`, `on_telemetry(&self, s: AircraftState) -> Vec<AtcOutput>` (fan-out rule of Global Constraints, then hand-off check: for the owner, `handoff_target` → owner of that kHz → `export_flight` / `import_flight` (or unit import), update `owners`), `on_transmission(&self, from: &Callsign, freq_khz, text, lang, t) -> Vec<AtcOutput>` (routing rule of Global Constraints; unowned frequency → `Vec::new()`), `on_weather(&self, w)` (to every engine), `stt_prompt_owner(&self, freq_khz, pos) -> Option<Owner>`, `snapshots(&self) -> Vec<(String, EngineSnapshot)>`, `take_vehicle_orders(&self)`.
  - The first owner of a pilot: the station that receives its first transmission; before any transmission, none.
- [ ] **Step 1: Tests:** `transmission_routed_by_frequency` (ZZZZ Ground freq → only ZZZZ answers); `shared_frequency_answered_once` (both stations list the same Approach callsign+kHz; owned flight answered by its owner only; unowned → nearest); `unowned_frequency_is_silent`; `handoff_moves_flight_with_plan` (ZZZZ Approach hands to ZZZY Tower → after readback the flight is owned by ZZZY and its strip has the plan's destination); `telemetry_fans_out_within_30nm`; ws test `two_clients_two_stations`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat(server): network dispatcher — routing by frequency and hand-offs between stations`

---

### Task 4: En-route data from the wizard

**Files:**
- Create: `crates/atc-navdata/src/enroute.rs` (+ `pub mod enroute;`), `crates/atc-stations/src/enroute.rs`
- Modify: `crates/atc-stations/src/wizard/{create,update}.rs` (write after every create/update), `crates/atc-stations/src/lib.rs`, `.gitignore` (`/data/airports/enroute.toml` is already covered by `/data/airports/*` — verify)
- Test: in those files + `crates/atc-stations/tests/wizard.rs`

**Interfaces:**
- Produces:
  - `pub struct EnrouteUnitDef { pub callsign: String, pub khz: u32, pub kind: EnrouteKind /* Fis | Acc */, pub sectors: Vec<SectorDef> }`, `pub struct SectorDef { pub name: String, pub lower_ft: i32, pub upper_ft: i32, pub polygon: Vec<[f64; 2]> }`, `pub fn load_enroute(path: &Path) -> Result<Vec<EnrouteUnitDef>, StationError>` (absent file → empty).
  - `pub fn build_enroute(countries: &[CountryData]) -> Vec<EnrouteUnitDef>` — every OpenAIP airspace of type 33 (FIS sector) or 27 (ACC sector) with a frequency; callsign by the phase-1 rules (FIS → "<Name> Information", ACC → frequency name title-cased); sectors with the same callsign + kHz merge into one unit; limits converted as in phase 1 (GND → 0, FL × 100, m × 3.28084).
  - The wizard writes `data_dir/enroute.toml` (atomic) after create and update; header comment says it is generated from OpenAIP and git-ignored.
- [ ] **Step 1: Tests:** `fis_and_acc_sectors_become_units`; `same_callsign_and_freq_merge`; `sector_without_frequency_skipped`; `enroute_round_trips`; wizard test `create_writes_enroute_file`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat(stations): en-route FIS/ACC units written by the wizard`

---

### Task 5: En-route controller and release

**Files:**
- Create: `crates/atc-core/src/enroute.rs` (+ `pub mod enroute;`)
- Modify: `crates/atc-server/src/network.rs` (units as owners, frequency index, hand-offs unit ↔ station), `crates/atc-server/src/main.rs` (load `enroute.toml`), `crates/atc-server/src/app.rs` (Radar voice for ACC, FIS voice = Radar voice unless `config/voices.toml` defines `[fis]` — then add it)
- Test: unit tests in `enroute.rs`, `crates/atc-server/tests/network.rs`

**Interfaces:**
- Consumes: `EnrouteUnitDef` (Task 4), `FlightHandover` (Task 1), stations' `procs.airspaces` (phase 1, CTR/TMA polygons) for entry detection.
- Produces:
  - `pub struct EnrouteUnit { def: EnrouteUnitDef, flights: HashMap<Callsign, EnrouteFlight> }` with `import_flight(&mut self, h: FlightHandover)`, `export_flight(&mut self, cs) -> Option<FlightHandover>`, `on_transmission(&mut self, from: &Callsign, text: &str, qnh: Option<u16>) -> Vec<AtcItem>` (first call → `Identified` + QNH; later calls → `Roger`-style acknowledgement or `SayAgain`; never clearances), `on_telemetry(&mut self, cs, pos, alt_ft) -> Option<EnrouteEvent>` where `pub enum EnrouteEvent { LeftSector }` (outside every sector of the unit: polygon via the phase-1 point-in-polygon helper moved to `atc-geo` if needed, or altitude outside `lower..=upper`).
  - In `Network`: on `LeftSector` choose next: a unit with a sector containing the aircraft (position + altitude) → `Contact`; else a station whose CTR/TMA contains it → its Approach, else Tower → `Contact`; else `FrequencyChangeApproved`. Spoken from the unit's callsign/frequency; ownership moves after readback (units use the same readback rule: the pilot's next transmission on the unit's frequency that repeats the frequency, or 30 s timeout → moves anyway). Released flights have no owner.
  - Silent stations per Global Constraints.
  - New `AtcItem` variants only if needed (`Identified` exists in phraseology? reuse; add `ChangeTo { station, khz }` for silent stations if absent, EN + PL render).
- [ ] **Step 1: Tests:** `first_call_identified_with_qnh`; `left_sector_to_next_unit`; `left_sector_into_station_tma_contacts_approach`; `last_sector_releases_once`; `level_kept_from_handover`; `silent_station_change_to_or_release`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat: en-route FIS/ACC units hand flights on and release them outside coverage`

---

### Task 6: Destinations worldwide, persistence, prompts, docs

**Files:**
- Modify: `crates/atc-server/src/layout.rs` (`load_destinations_near` → `load_all_destinations` with cache), `crates/atc-navdata/src/xplane.rs` (scan without radius if needed), `crates/atc-server/src/persist.rs`, `crates/atc-server/src/main.rs` (restore all; save owners map alongside: `data/cache/network.json`), `crates/atc-server/src/app.rs` (STT prompts per station), `README.md`, `docs/RUNNING.md`
- Test: `crates/atc-server/tests/stations.rs`, `crates/atc-server/src/persist.rs` tests

**Interfaces:**
- Produces:
  - `pub fn load_all_destinations(xp_root: &Path, cache: &Path, names: &[(String, String)]) -> Vec<Destination>` — cached JSON `{ sources: [{path, size, mtime}], destinations }`, rebuilt on mismatch; shared by all engines as `Arc<[Destination]>` (change `with_destinations` to accept it if it copies today).
  - Snapshots: one per station via `persist::state_path(id)`; `network.json` holds `owners` (callsign → station id / unit callsign); restore ignores entries naming a station or unit that no longer exists.
  - STT prompts built per station from its own names (`SttPrompts` per station, chosen per Global Constraints).
  - Docs: README/RUNNING "Several stations" — omit `airport` to run all, how hand-offs work, `enroute.toml`, release outside coverage, any airfield as destination.
- [ ] **Step 1: Tests:** `destination_far_away_is_known` (fixture apt.dat with a distant airport); `destinations_cache_rebuilt_when_apt_dat_changes`; `restart_restores_every_station_and_owner`; `stt_prompt_follows_frequency_owner`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat(server): worldwide destinations, per-station persistence and prompts; docs`
- [ ] **Step 6: Manual acceptance (user):** with EPKK and EPKT created by the wizard and `airport` removed from `config/server.toml`, fly EPKK → EPKT: Kraków Ground/Tower/Approach, Approach keeps you on 121.075 (shared unit), Katowice Tower lands you; then a departure out of Poland's data: hand-offs to FIS/Radar and "frequency change approved" at the edge. Phase 3 (EPKT scenario and RU guide) follows.

---

**Calls made in this plan (not from the spec):**
- One engine per station plus a dispatcher (reason in Architecture).
- 30 NM telemetry fan-out radius; nearest-station rule for unowned shared frequencies; first owner = first station called.
- En-route units read back by repeating the frequency, or move after 30 s regardless.
- `enroute.toml` lives next to the stations in `data_dir` and is regenerated by every create/update.
- Owners persisted in `data/cache/network.json`.
- `remove_pilot` is defined here if plan 7 has not landed yet (both plans share the contract).
