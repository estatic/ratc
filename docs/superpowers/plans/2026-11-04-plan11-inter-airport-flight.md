# Inter-airport IFR Flight Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An IFR flight between two loaded stations (EPKK -> EPKT) flies cleanly. The departure Approach hands the flight to the destination's Approach. A Tower-only destination accepts the flight. The clearance respects the SID's floors. "Request taxi" after landing goes to a stand. Stand names with spaces can be requested.

**Architecture:**
- **Network (`atc-server`)**
  - Gives each station's engine the other stations' Approach positions as extra adjacent units (`AdjacentKind::Approach`, airport ICAO).
  - An acknowledged hand-off that names an airport moves the flight to that airport's station, even on a frequency both stations work.
- **Engine (`atc-core`)**
  - Hands an IFR flight to the destination Approach within 30 NM.
  - A destination Tower identifies a handed-in IFR inbound and keeps it.
  - `ifr::initial_climb_ft` applies `AtOrAbove` floors.
  - Ground treats "request taxi" in `TaxiIn`/`Landed` as a taxi-in.
  - Stands are resolved with a normalised name match (nearest free one among duplicates).
- **Phraseology**
  - The stand parser keeps the keyword and more tokens.
  - Two new items, `Station` and `ExpectVisualApproach`.

**Tech Stack:** Rust 2024 workspace (`atc-core`, `atc-server`, `atc-phraseology`, `atc-navdata`), serde. Tests: `cargo test`.

**Spec:** `docs/superpowers/specs/2026-11-04-inter-airport-flight-design.md`. Read it first. Its "Decisions after the builder check" section is binding.

## Global Constraints

- English code, comments and docs. The Russian user guide stays under `docs/user-guide/ru/`.
- GitHub only via PR. Work on the current branch `fix/inter-airport-flight` and never push to `main`.
- Never commit `config/server.toml`, `config/client.toml`, `track.csv`, `docs/.obsidian/`, or wizard station data (anything under `data/airports/*` except the tracked EPKK files).
- Tests must not depend on git-ignored station data. Multi-station tests use `crates/atc-server/tests/fixtures/stations/{ZZZY,ZZZZ}` plus the `extra` TOML of `fixture_station`.
- Single-station behaviour stays as it is: an emulated adjacent tower, `adjacent_unit_inbound_is_silent`, `no_adjacent_unit_releases_at_boundary`, `real_unit_wins_over_emulated_adjacent`.
- Existing tests stay green. Only tests that assert the old behaviour are updated, and each one is named in the task report.
- New `Strip` fields carry `#[serde(default)]`, so old snapshots still load.
- Verification is `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. Two `atc-replay` tests are known to fail locally because of the git-ignored `data/airports/EPKK/station.toml`. They pass in a clean worktree. Report them as known, not as regressions.
- New constant: `DEST_APPROACH_M = 30.0 * M_PER_NM` (destination Approach hand-off). `DEST_TOWER_M` (10 NM) stays for destinations without one.

## Rulings made while writing this plan (author's calls)

1. The extra units live in a new engine field `station_units`. They are not merged into `procs.adjacent`, so the emulated-frequency map, `StationMap`, the layout and `is_ifr_inbound` stay as they are. `AdjacentKind` gets an `Approach` variant for them.
2. The destination Approach's callsign can equal the departure's ("Kraków Approach" on both). The network therefore picks the target station by the airport stored on the strip (`Strip::handed_off_airport`), not by callsign.
3. The departure Approach skips its 20 NM `descent_before_tower` when the destination has a loaded Approach. That Approach does the descent.
4. `plan.cruise_ft` becomes `max(cruise, initial climb)`. This stops Approach treating FL100 as "above the cruise level" after a 5000 ft default.
5. A floor is rounded up to a whole 1000 ft. At a ceiling no lower than the target, the first such ceiling caps it, as today. `At(a)` counts as a ceiling only.
6. Tower phrasing for a handed-in IFR inbound:
   - With an approach: `[Station{tower}, ExpectApproach{kind, runway}, ReportEstablished]`.
   - Without one: `[Station{tower}, ExpectVisualApproach{runway}, Report(CircuitLeg::Final)]`.
   - Polish renders `Station` through `station_name(.., Pl)`. `ExpectVisualApproach` falls back to English, like the other IFR items.
7. When the first call of a handed-in flight gets no items:
   - A `Content::Message` gets `[Station{position callsign}, Roger]` instead of `SayAgain`.
   - A `Content::Call` keeps `PassYourMessage`.
8. A stand counts as "free" when no other tracked aircraft is within `ON_STAND_M` (25 m) of it and it is not the taxi target or home stand of another strip. Duplicate-name stands are told apart by a position on `TaxiTarget::Stand { name, at: Option<LatLon> }`.
9. Taxi-in covers `FlightPhase::TaxiIn` and `FlightPhase::Landed`.
10. The core ZZZZ CIFP fixture (ALFA1A/CHAR1A +FL080, BRAV1A +FL060) is not changed. The engine tests and replay goldens that asserted "climb 5000" are updated.
11. The RU scenario `docs/user-guide/ru/scenario-ifr-departure.md` is not edited on this branch. It is updated after this PR and `docs/epkk-epkt-full-flight` are both merged.

## Review Focus

1. **Shared Approach unit, flight not bound for the other station.**
   - Setup: two stations list the same Approach unit, and an IFR flight is not bound for the other station's airport.
   - Expected: it is not handed over, even within 30 NM of that airport.
   - Test: Task 4, `approach_hand_off_only_for_the_flights_destination`.
2. **Destination with both an emulated `[[adjacent]]` Tower and a loaded Approach.**
   - Expected: the Approach wins, and no 20 NM "descend ... QNH" comes from the departure station.
   - Test: Task 4 (every new test runs with `ZZZZ_EXTRA`, which lists Testville Tower). Each test asserts the contact frequency is not `TESTVILLE_TWR`.
3. **Hand-off read back with the wrong frequency, or not read back.**
   - Expected: the flight stays with the departure station and is not moved early.
   - Test: Task 4 asserts the owner before the readback.
4. **A heard stand that matches nothing.**
   - Expected: "unknown stand 99", not "unknown stand stand 99", and no taxi.
   - Test: Task 3.
5. **"Request taxi" in `TaxiIn` with no stand named and no home stand.**
   - Expected: `SayStand`, never a taxi to the runway.
   - Test: Task 2.

---

### Task 1: SID floors in the initial climb

**Files:**
- Modify: `crates/atc-core/src/ifr.rs:124-139` (`initial_climb_ft` and its doc)
- Modify: `crates/atc-core/src/engine/delivery.rs:~205-246` (`ifr_departure`: `plan.cruise_ft`)
- Test: `crates/atc-core/tests/ifr.rs` (`initial_climb_stops_only_at_ceilings`, renamed)
- Test: `crates/atc-core/tests/engine.rs` (departure-clearance expectations)
- Test: `crates/atc-core/tests/xplane_install.rs` (`installed_epkk_clearance_to_katowice`)
- Test data: `scenarios/epkk-*.toml` pilot readbacks, then the regenerated `scenarios/*.expected.txt`

**Interfaces:**
- Produces: `pub fn initial_climb_ft(path: &Path, cruise_ft: i32) -> i32` (signature unchanged).
  - New rule: `target = max(cruise_ft, round_up_1000(highest AtOrAbove(a) / Between{above} on the path))`.
  - The result is the first ceiling (`At`, `AtOrBelow`, `Between.below`) on the path that is `<= target`, else `target`.

- [ ] **Step 1: Update and add the failing tests**

In `tests/ifr.rs`, rename `initial_climb_stops_only_at_ceilings` to `initial_climb_respects_floors_and_ceilings`. The fixture ALFA1A has +3000 and +FL080.

```rust
assert_eq!(initial_climb_ft(&path, 5000), 8000);   // floor FL080 beats 5000
assert_eq!(initial_climb_ft(&path, 10000), 10000); // planned level above the floor
path.constraints[1].1 = AltConstraint::AtOrAbove(6100);
assert_eq!(initial_climb_ft(&path, 5000), 7000);   // rounded up to 1000 ft
path.constraints[0].1 = AltConstraint::AtOrBelow(4000);
assert_eq!(initial_climb_ft(&path, 5000), 4000);   // the ceiling still wins
// Keep the existing Between / At assertions (3000, 2500).
```

In `tests/xplane_install.rs` `installed_epkk_clearance_to_katowice`:
- If `items[1]` is the SUDOL SID with number 2 and letter 'J', assert `items[2] == AtcItem::ClimbTo(Level::FlightLevel(100))` and that the text contains `"climb flight level one zero zero"`.
- Otherwise assert a `ClimbTo` level of at least 5000 ft.
- Update the doc comment.

- [ ] **Step 2: Run** `cargo test -p atc-core --test ifr`. Expected: FAIL (5000 != 8000).

- [ ] **Step 3: Implement**
  - In `initial_climb_ft`, use `i32::div_ceil(a, 1000) * 1000` for the rounding.
  - In `ifr_departure`, set `plan.cruise_ft = cruise_ft.max(climb_ft)`.

- [ ] **Step 4: Fix the ripple in the engine tests**

Run `cargo test -p atc-core`.
- Tests in `mod delivery` and `mod approach_departure` (`tests/engine.rs`) that expect `ClimbTo(Level::Altitude(5000))` from a ZZZZ SID clearance now get the SID floor: FL080 for ALFA1A/CHAR1A, 6000 for BRAV1A (transition altitude 6500).
- Update those expectations and the pilot readback strings in the helpers (`departed`, `departed_at` and their "five thousand" readbacks).
- Do not touch tests where 5000 comes from somewhere else (arrival levels, plans with an explicit level).
- List every updated test in the report.

Expected: PASS.

- [ ] **Step 5: Replay goldens**
  - In `scenarios/epkk-ifr-*.toml` and `scenarios/epkk-bad-readback.toml`, change the pilot readbacks and reports of the SID clearance level ("climb five thousand feet", "climbing five thousand feet") to the new level. That is "flight level one zero zero" for SUDOL 2J. For other SIDs, use the level printed by the run.
  - Regenerate the goldens in a clean worktree so the git-ignored station data does not leak in:
    `git worktree add ../atc-golden HEAD && (cd ../atc-golden && UPDATE_GOLDEN=1 cargo test -p atc-replay)`
  - Copy the changed `scenarios/*.expected.txt` back and remove the worktree.
  - Review the diff: only the climb lines and what follows from them may change.

- [ ] **Step 6: Run** `cargo test --workspace`. Expected: PASS, apart from the 2 known local `atc-replay` failures.

- [ ] **Step 7: Commit** `git commit -m "fix: initial climb respects SID at-or-above floors"`

---

### Task 2: Taxi after landing goes to a stand

**Files:**
- Modify: `crates/atc-core/src/engine/ground.rs:63-65` (the `RequestTaxi` arm), `:126-130` (`taxi_to_holding_point` no longer accepts `TaxiIn`)
- Test: `crates/atc-core/tests/engine.rs`, `mod approach_departure`, next to `start_up_after_a_flight_away_from_a_stand`

**Interfaces:**
- Consumes: `fn taxi_to_stand(&mut self, i: usize, stand: Option<String>) -> Vec<AtcItem>` (existing).

- [ ] **Step 1: Write the failing tests**

Set up as `short_follow_me_fallback_sets_the_taxi_target`: first seen on Stand 1, then `departed_at`, `fly_to_testville_tower`, repositioned 55 m north of Stand 1, phase `TaxiIn`.

- `request_taxi_after_landing_goes_to_the_home_stand`:
  - Send `"{CS}, request taxi"` on `GND`.
  - Assert `matches!(&out[0], AtcItem::Taxi { to: TaxiTo::Stand { name }, .. } if name == "Stand 1")`.
  - Assert `out` contains no `Qnh(_)`, `Squawk(_)` or `AtisCurrent(_)`.
- `request_taxi_after_landing_without_a_stand_says_stand`:
  - Same flow without the first parked sample, so there is no home stand.
  - Assert `items == vec![AtcItem::SayStand]`.
- The existing `start_up_after_a_flight_away_from_a_stand` must stay green: a departure request from `TaxiIn` still works.

- [ ] **Step 2: Run** `cargo test -p atc-core --test engine request_taxi_after_landing`. Expected: FAIL (taxi to the holding point).

- [ ] **Step 3: Implement**
  - In the `RequestTaxi` arm: when the phase is `TaxiIn | Landed`, call `self.taxi_to_stand(i, None)`; otherwise call `taxi_to_holding_point`.
  - Remove `TaxiIn` from the `taxi_to_holding_point` phase guard.
  - Add a comment that a new departure starts from `Parked` ("request start up").

- [ ] **Step 4: Run** `cargo test --workspace`. Expected: PASS.

- [ ] **Step 5: Commit** `git commit -m "fix: request taxi after landing is a taxi to the stand"`

---

### Task 3: Loose stand-name matching and duplicate stands

**Files:**
- Modify: `crates/atc-phraseology/src/parse.rs:1286-1296` (stand capture)
- Modify: `crates/atc-core/src/taxi.rs:10-13, 300-307` (`TaxiTarget::Stand { name, at }`)
- Modify: every `TaxiTarget::Stand { name }` pattern or constructor (`engine/stand.rs`, `engine/follow_me.rs`, `engine/ground.rs`, `testkit.rs`, the tests in `taxi.rs`): add `..` or `at: None`
- Modify: `crates/atc-core/src/engine/ground.rs:216-245` (`taxi_to_stand` uses the resolver)
- Create: `crates/atc-core/src/engine/stand_match.rs` (normalisation + resolver, `mod stand_match;` in `engine/mod.rs`)
- Test: `crates/atc-phraseology/tests/parse.rs`, `tests/parse_pl.rs`
- Test: unit tests in `stand_match.rs`
- Test: `crates/atc-core/tests/engine.rs` (`mod approach_departure`)

**Interfaces:**
- Produces (phraseology):
  - `PilotIntent::RequestTaxiToStand { stand: Option<String> }` now carries the keyword and the heard tokens joined by spaces, e.g. `"apron 4 G A"`, `"stand 5"`, `"parking apron4 ga"`.
  - Tokens taken after the keyword: `Letter`, `Number`, and `Word`s that are a stand keyword (`stand|parking|apron`) or are alphanumeric and contain a digit, or are at most 2 letters and not in `["to","on","at","via","and","for"]`.
  - At most 6 tokens. Stop at any other word or a `Break`.
- Produces (core):
  - `pub enum TaxiTarget { HoldingPoint { runway: String }, Stand { name: String, #[serde(default, skip_serializing_if = "Option::is_none")] at: Option<LatLon> } }`.
    - With `at: Some(p)`, `route` picks the stand named `name` nearest `p`.
    - With `None`, it picks the first stand of that name, as today.
  - `pub(crate) fn normalize_stand(s: &str) -> String`: uppercase ASCII alphanumerics only.
  - `pub(crate) fn strip_stand_keyword(n: &str) -> &str`: drops one leading `STAND` / `PARKING` / `APRON` when something follows it.
  - `pub(super) fn resolve_stand(&self, i: usize, heard: &str) -> Option<usize>`: an index into `self.airport.stands`.
    - Candidates are exact normalised matches. If there are none, they are matches of `strip(heard) == strip(stored)`, or of either side stripped against the other.
    - Among the candidates: the nearest free one to the aircraft, else the nearest (ruling 8).

- [ ] **Step 1: Write the failing tests**

Parser tests (`tests/parse.rs`):
- Update `request_taxi_to_stand` to expect `Some("stand 5".into())`, and the PL twin in `parse_pl.rs` likewise.
- `request_taxi_to_apron_stand_with_letters`: "request taxi to apron four golf alpha" gives `Some("apron 4 G A")`. Use the letter case the tokenizer actually yields, then assert on `normalize`-equivalent text.
- `request_taxi_to_parking_with_name`: "request taxi to parking apron4 ga" gives a string whose normalisation is `"PARKINGAPRON4GA"`.

Resolver unit tests in `stand_match.rs`:
- `normalize_stand("APRON4 GA") == "APRON4GA"`.
- `strip_stand_keyword("STAND5") == "5"`.
- `strip_stand_keyword("APRON") == "APRON"`.

Engine tests (`mod approach_departure`): build the airport with extra stands by pushing `Stand`s onto the ZZZZ `Airport` before `AtcEngine::new`. Add a helper next to `engine()`.
- `stand_with_spaces_is_matched_loosely`:
  - Stand "APRON4 GA" exists.
  - "apron four golf alpha", "apron 4 GA" and "parking APRON4 GA" each give `Taxi { to: TaxiTo::Stand { name: "APRON4 GA" }, .. }`.
- `duplicate_stand_names_pick_the_nearest_free`:
  - Two stands "GA" 100 m and 300 m away. A second aircraft is stopped on the nearer one.
  - Assert `strip.taxi_target == Some(TaxiTarget::Stand { name: "GA", at: Some(far.pos) })`.
- `unknown_stand_names_the_heard_stand`:
  - "request taxi to stand nine nine" gives `AtcItem::Unable(UnableReason::UnknownStand { stand })` with `stand == "99"`.
  - If the existing `unable(&e)` mapping produces a different variant, assert on that variant. The point is that the stand is named with the keyword stripped (Review Focus 4).

- [ ] **Step 2: Run** `cargo test -p atc-phraseology -p atc-core`. Expected: the new tests FAIL.

- [ ] **Step 3: Implement**
  - The parser capture.
  - `TaxiTarget::Stand.at` and the pattern updates.
  - `stand_match.rs`.
  - In `taxi_to_stand`:
    - Resolve a heard stand to `(name, Some(pos))` of the chosen stand.
    - An unresolved heard stand goes to the route with its stripped display form, so the error names it.
    - The home stand keeps `at: None`.
    - `report_on_stand`, `stand_of` and `stand_hint_at` prefer `at` when present.

- [ ] **Step 4: Run** `cargo test --workspace`. Expected: PASS.

- [ ] **Step 5: Commit** `git commit -m "fix: stand names matched loosely, nearest free stand among duplicates"`

---

### Task 4: Hand-off to the destination station's Approach

**Files:**
- Modify: `crates/atc-navdata/src/procedures.rs:120-128` (`AdjacentKind::Approach`, doc: "Approach unit of another loaded station; never in procedures.toml in practice")
- Modify: `crates/atc-core/src/engine/mod.rs` (field `station_units: Vec<AdjacentUnit>`, setter, `handoff_airport`)
- Modify: `crates/atc-core/src/strip.rs` (field `handed_off_airport`, reset next to `handed_off_khz` ~363)
- Modify: `crates/atc-core/src/engine/approach.rs`:
  - `hand_off` 1059-1095 and its caller ~1010.
  - `descent_before_tower` ~1038.
  - `direct_after_turn` ~1133.
  - New `destination_approach`.
  - The `DEST_APPROACH_M` constant next to `DEST_TOWER_M`.
- Modify: `crates/atc-server/src/network.rs`:
  - `Network::new` 164-212: inject the units.
  - `check_handoff` ~760-800: target by airport.
  - New helper `station_of_airport`.
  - Update the module docs (routing rule 1 and the hand-off paragraph).
- Test: `crates/atc-server/tests/network.rs`

**Interfaces:**
- Produces:
  - `AdjacentKind::Approach` (serde name `"Approach"`).
  - `pub fn set_station_units(&mut self, units: Vec<AdjacentUnit>)` on `AtcEngine`: the other loaded stations' Approach positions, `kind: Approach`, `airport: Some(icao)`, `min_level_ft: None`.
  - `pub fn handoff_airport(&self, cs: &Callsign) -> Option<String>`: the strip's `handed_off_airport`, under the same conditions as `handoff_target`.
  - `Strip::handed_off_airport: Option<String>` (`#[serde(default)]`). It is set for a destination Approach or Tower hand-off and cleared with `handed_off_khz`.
  - `fn hand_off(&self, plan, pos) -> Option<HandOff>`, with `struct HandOff { unit: String, airport: Option<String>, items: Vec<AtcItem> }` (private to approach.rs).
  - Order inside `hand_off`:
    1. `destination_approach(plan)` within `DEST_APPROACH_M` of `destination_pos(plan)`. Farther out, `None` (no TMA release).
    2. Else the existing tower branch, then the existing ACC/FIS logic.
  - `descent_before_tower` returns `None` when `destination_approach(plan)` is `Some`.
  - `direct_after_turn` treats a destination Approach like a destination tower.
- Network:
  - In `check_handoff` for `Owner::Station(i)`, when `handoff_airport` names the airport of station `j != i` and `Owner::Station(j)` works `khz` (`on_channel(&self.freq_index, khz)` contains it), the flight moves to `Station(j)`. This holds even when `at` contains `from`.
  - Otherwise the logic stays as today.
  - `fn station_of_airport(&self, icao: &str) -> Option<usize>`: case-insensitive on `stations[j].airport.icao`.

Test fixtures (in `tests/network.rs`; no new station directories needed):

```rust
/// Testville with an Approach of the same callsign and frequency as Zeta's (the EPKK/EPKT case).
const TESTVILLE_SAME_APP: &str = "\n[[positions]]\nrole = \"Approach\"\ncallsign = \"Zeta Approach\"\nkhz = 119100\n";
const TESTVILLE_APP: u32 = 120_400;
const TESTVILLE_OWN_APP: &str = "\n[[positions]]\nrole = \"Approach\"\ncallsign = \"Testville Approach\"\nkhz = 120400\n";
```

Build the network with `Network::new(vec![fixture_station("ZZZY", <extra>), zeta_with_testville()])`. Import `ifr_to_testville` into ZZZZ, and drive the identified call as in `handoff_moves_flight_with_plan`.

- [ ] **Step 1: Write the failing tests**

`destination_approach_same_frequency_takes_flight`:
- Start at lat 10.2, which is inside 30 NM.
- Fly north until an output has `Contact { station: "Zeta Approach", khz: ZETA_APP }`. Its `station` field is "Zeta Approach", from ZZZZ.
- `owner == Station(ZZZZ)` before the readback.
- Read back "contact one one niner decimal one, Sierra Papa Tango Sierra Tango" on `ZETA_APP`. Then `owner == Station(ZZZY)` and `!registered(ZZZZ)`.
- First call "Zeta Approach, Sierra Papa Tango Sierra Tango, two thousand feet" on `ZETA_APP`. The output is non-empty, contains no `SayAgain`, and `owner` stays `Station(ZZZY)`.
- 5 more telemetry samples: no output carries a `Contact` (no bounce).

`destination_approach_other_frequency_takes_flight`:
- Same flow with `TESTVILLE_OWN_APP`.
- The contact is `{ station: "Testville Approach", khz: TESTVILLE_APP }`.
- The readback on `ZETA_APP` moves the flight to ZZZY.
- The first call on `TESTVILLE_APP` is answered by ZZZY without `SayAgain`.

`destination_approach_waits_for_30_nm`:
- Start at lat 9.9 (36 NM out), northbound at 150 kt, 2 s steps.
- No `Contact` while `lat < 10.0` (more than 30 NM from Testville's reference point). A contact by lat 10.02.

`approach_hand_off_only_for_the_flights_destination`:
- Use `TESTVILLE_SAME_APP`, with the plan's destination set to `"ZZZF"` (not loaded).
- Fly from lat 10.2 to 10.45: no `Contact` naming `ZETA_APP`, and `owner == Station(ZZZZ)` throughout.

Existing `handoff_moves_flight_with_plan` (no ZZZY Approach) stays green: Tower hand-off at 10 NM.

- [ ] **Step 2: Run** `cargo test -p atc-server --test network destination_approach`. Expected: FAIL (the contact is Testville Tower 118.5 at 10 NM).

- [ ] **Step 3: Implement** the interfaces above.
  - Fill `Network::new` after the index loop: for each `i`, the units of every `j != i` with a `FreqKind::Approach` position. Do not add them to `emulated`.
  - Update the doc comment of `handoff_target` and the RUNNING-facing module docs.

- [ ] **Step 4: Run** `cargo test --workspace`. Expected: PASS. In particular, `shared_frequency_answered_once`, `real_unit_wins_over_emulated_adjacent` and all `approach_departure` single-station tests stay green.

- [ ] **Step 5: Commit** `git commit -m "fix: hand an IFR flight to the destination station's Approach"`

---

### Task 5: Destination Tower accepts a handed-in IFR inbound; no "say again" on a first handed-in call

**Files:**
- Modify: `crates/atc-phraseology/src/types.rs`: `AtcItem::Station { station: String }` ("<station>": the controller names itself on a first contact) and `AtcItem::ExpectVisualApproach { runway: String }`
- Modify: `crates/atc-phraseology/src/render.rs`:
  - `Station`: `out.push(station.clone())`.
  - `ExpectVisualApproach`: `"expect visual approach runway {runway_words}"`.
- Modify: `crates/atc-phraseology/src/pl/render.rs`:
  - `Station`: `station_name(station, Lang::Pl)`.
  - `ExpectVisualApproach`: in the English fallback list.
- Modify: `crates/atc-core/src/strip.rs`: `tower_inbound: bool` (`#[serde(default)]`), "Tower keeps this IFR arrival; never handed to Approach"
- Modify: `crates/atc-core/src/engine/mod.rs`:
  - `answer()` ~815-820: capture `was_handed_in` before the clear, and dispatch the Tower case.
  - `answer()` ~890-896: the fallback of ruling 7.
  - `handover()` 1572-1579: the arm is skipped when `strip.tower_inbound`.
- Modify: `crates/atc-core/src/engine/tower.rs`: new `handed_in_ifr_inbound`
- Modify: `crates/atc-core/src/engine/approach.rs:459`: `default_approach` becomes `pub(super)`
- Test: render unit tests in both render files
- Test: `crates/atc-core/tests/engine.rs` (`mod handover`)
- Test: `crates/atc-server/tests/network.rs`

**Interfaces:**
- Consumes: Task 4 (a flight handed to a Tower-only destination arrives with `owner = Approach`, `handed_in`, `plan.approach = None`).
- Produces: `pub(super) fn handed_in_ifr_inbound(&mut self, i: usize) -> Vec<AtcItem>`.
  - Called from `answer()` when `role == Tower`, `was_handed_in`, the strip is airborne, and its plan is IFR, bound here, with `approach == None`.
  - Runway: `self.strips[i].runway`.
  - Kind: `default_approach(runway)` (ILS, else RNAV, else VOR/DME).
  - Items as in ruling 6.
  - With a kind, it sets `plan.approach = Some((kind, runway))`.
  - In both cases it sets `owner = Tower`, `identified = true` and `tower_inbound = true`.
  - The answer goes through the normal readback expectation (`expect_readback`).

- [ ] **Step 1: Write the failing tests**

Render tests:
- `Station { station: "Katowice Tower" }` followed by `ExpectApproach { Ils, "27" }` and `ReportEstablished` renders "SPTST, Katowice Tower, expect ILS approach runway two seven, report established". Use the existing callsign test pattern of the file.
- The PL twin keeps the localised station name.

Engine tests (`mod handover`; `engine()` has the ZZZZ procedure set):

`handed_in_ifr_inbound_on_tower_expects_the_approach`:
- `import_flight(handover(ifr_plan("ZZZZ", "Zeta", 3000)))`, airborne 8 NM west at 3000 ft.
- Say "Zeta Tower, Sierra Papa Tango Sierra Tango, three thousand feet" on `TWR`.
- Assert:
  - `items[0] == Station { station: "Zeta Tower" }`.
  - `matches!(items[1], ExpectApproach { kind: ApproachKind::Ils, .. })`.
  - `items[2] == ReportEstablished`.
  - `strip.owner == Role::Tower`.
- Then 5 telemetry steps: no output `Contact` naming `APP` (no bounce).
- "established ILS runway two seven" gives `ClearedLand { .. }` in the answer.

`handed_in_first_call_is_never_say_again`:
- An imported VFR flight's ground call that has no handler, e.g. "Zeta Ground, Sierra Papa Tango Sierra Tango, two thousand feet" on `GND`.
- Its answer contains no `SayAgain`. It is `[Station { "Zeta Ground" }, Roger]`.

Network test `destination_tower_accepts_handed_in_ifr`:
- Flow of `handoff_moves_flight_with_plan` (ZZZY Tower only). After the move, call "Testville Tower, Sierra Papa Tango Sierra Tango, two thousand feet" on `TESTVILLE_TWR`.
- The output's station is "Testville Tower", and the items are `[Station{"Testville Tower"}, ExpectVisualApproach{runway: "18"}, Report(CircuitLeg::Final)]`. ZZZY has no procedure set; if the active runway differs, assert on the variant and on Testville's runway in use.
- Then 5 telemetry samples: no `Contact` in any output, and `owner == Station(ZZZY)`.

- [ ] **Step 2: Run** `cargo test -p atc-phraseology -p atc-core -p atc-server`. Expected: the new tests FAIL (`SayAgain`).

- [ ] **Step 3: Implement** the interfaces above. The `Station`/`ExpectVisualApproach` items need no readback (check `readback.rs` lists of required items; add them to the "no readback" side if that list is exhaustive).

- [ ] **Step 4: Run** `cargo test --workspace`. Expected: PASS.

- [ ] **Step 5: Commit** `git commit -m "fix: destination Tower accepts a handed-in IFR inbound"`

---

### Task 6: Docs and final verification

**Files:**
- Modify: `docs/RUNNING.md`, section "Several stations":
  - The "Frequencies" bullet: the same callsign and frequency at two stations is no longer one unit for a flight bound for the other airport.
  - The "Hand-offs between stations" bullet: a flight to a loaded destination is told to contact the destination's Approach within 30 NM, even on the same frequency, and reads it back. With no Approach there, the Tower within 10 NM, which gives the expected approach.
  - Add one line each on the SID floor in the clearance and on taxi-in / loose stand names.
- Modify: `docs/user-guide/ru/ifr-and-vfr-points.md`:
  - The SUDOL 2J clearance and readbacks (lines ~81, 85, 107): "climb flight level one zero zero".
  - Section 3 "Approach 121.075 — после взлёта": with EPKT loaded, "contact Kraków Approach 121.075" (the EPKT controller) instead of Katowice Tower.
  - Every "stand three five" (lines ~77, 103, 115, 299-311, 356): a stand present in the default scenery, e.g. "stand two zero".
  - Section 4 "Ground 118.105 — на стоянку": add that "request taxi" without a stand after landing leads to your own stand.
- Do NOT edit `docs/user-guide/ru/scenario-ifr-departure.md` (ruling 11).

- [ ] **Step 1: Edit** the docs above. RU text in Russian, phraseology quotes in English as the file does.
- [ ] **Step 2: Run** `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. Expected: clean. Only the 2 known local `atc-replay` failures (git-ignored `data/airports/EPKK/station.toml`) remain. Confirm them in a clean worktree if in doubt.
- [ ] **Step 3: Check** `git status` shows no `config/*.toml`, `track.csv`, `docs/.obsidian/` or wizard station data staged.
- [ ] **Step 4: Commit** `git commit -m "docs: inter-airport flight hand-off, SID level, taxi-in and stand names"`
- [ ] **Step 5:** The branch goes to GitHub as a PR into `main`, never a direct push. After both this PR and `docs/epkk-epkt-full-flight` are merged, `scenario-ifr-departure.md` is updated to the new behaviour (separate follow-up).
