# Plan 12: Small Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the ten small engine/parser defects and the doc wording left over from plan 11 and the EPKK→EPKT run.

**Architecture:** Local changes in `atc-core` (approach, tower, delivery, ifr, ground/stand matching, engine dispatch), `atc-phraseology` (one new item, the stand capture, the Polish normaliser) and the docs. No new modules. Each task can be built and tested on its own; they touch different functions, so the order only matters for the `epkk-ifr-vor07-missed` golden (Tasks 1 and 2).

**Tech Stack:** Rust workspace (atc-core, atc-phraseology, atc-navdata, atc-replay), serde, cargo test.

**Spec:** `docs/superpowers/specs/2026-11-10-small-fixes-design.md` (item numbers below refer to its "Fixes" list).

## Global Constraints

- Code, comments, commit messages and docs are in English. Russian appears only under `docs/user-guide/ru/`. When a guide changes, change the EN and RU guides together (same facts, same transcript lines).
- Never commit `config/server.toml`, `config/client.toml`, `track.csv`, `docs/.obsidian/`, or wizard data under `data/airports/` (for example `data/airports/EPKK/station.toml`, `wizard.toml`). Stage files by name, never `git add -A` / `git add .`.
- Tests must not depend on git-ignored station data. Engine tests use the ZZZZ fixtures (`crates/atc-navdata/tests/fixtures/xplane`).
- Engine tests need the testkit feature: per crate `cargo test -p atc-core --features testkit --test engine <filter>`; whole workspace `cargo test --workspace --features atc-core/testkit --no-fail-fast`.
- Known local-only failures: 2 `atc-replay` tests (`path_and_frequency_errors`, `golden_transcripts_match`) fail in the main checkout only because of the git-ignored `data/airports/EPKK/station.toml`. They are not regressions. Check them in a clean worktree (below).
- Goldens are regenerated only in a clean worktree with `UPDATE_GOLDEN=1`. The X-Plane install (default Steam path or `ATC_XPLANE_ROOT`) must be present, because `epkk-ifr-vor07-missed` uses `navdata = "xplane"`. Procedure:
  ```bash
  git worktree add /tmp/claude-1000/atc-clean HEAD   # after committing the code change
  cd /tmp/claude-1000/atc-clean
  CARGO_TARGET_DIR=/home/estatic/Documents/Projects/atc/target cargo test -p atc-replay            # see what differs
  UPDATE_GOLDEN=1 CARGO_TARGET_DIR=/home/estatic/Documents/Projects/atc/target cargo test -p atc-replay golden_transcripts_match
  git diff --stat; cp scenarios/<changed>.expected.txt /home/estatic/Documents/Projects/atc/scenarios/
  cd /home/estatic/Documents/Projects/atc && git worktree remove --force /tmp/claude-1000/atc-clean
  ```
  Every changed golden line must be explained by the task's fix. Any other diff is a regression: stop and report it.
- Before each commit: `cargo fmt --all` and `cargo clippy --workspace --all-targets --features atc-core/testkit -- -D warnings` are clean.
- Do not push. One branch: `docs/plan12-small-fixes` (already checked out).
- New serde fields on `Strip` get `#[serde(default)]` so old state files (`data/cache/state-*.json`) still load.

## Review Focus

- A missed approach flown by a pilot who never reaches the missed altitude (stays low on vectors). The intercept clearance must still bring the cleared level down to the intercept altitude, not leave "4000" standing. Task 1 test `descent_after_missed_approach_waits_for_the_missed_altitude` covers it.
- An approach without published missed-approach legs. It must keep today's wording, with no "follow published missed approach" that names nothing. Task 2 test `going_around_without_published_missed_legs_is_unchanged`.
- A stand request where the stand is unknown and the pilot's callsign follows it. The refusal must name only the stand. Task 5 test `unknown_stand_with_trailing_callsign_names_only_the_stand`.
- Polish "przechodzę na drugi krąg" must still mean going around after "przechodzę" gains a meaning. Task 6 unit test `przechodze_is_passing_only_before_a_number`.
- A garbled transmission followed by a clean first call from a handed-in flight. The clean call must still get the first-contact acknowledgement. Task 7 test `garbled_transmission_does_not_use_up_the_first_call`.

---

### Task 1: Missed-approach altitude from the procedure (spec item 1)

**Files:**
- Modify: `crates/atc-core/src/strip.rs` (Strip field + `Strip::new` + `end_flight`)
- Modify: `crates/atc-core/src/engine/approach.rs` (`set_missed_approach` ~l.860, `vector_items` ~l.641; module docs if they describe the missed approach)
- Test: `crates/atc-core/tests/engine.rs` (module `approach_arrival`: `missed_approach_goes_back_to_approach` ~l.5859, new test next to it)
- Modify: `scenarios/epkk-ifr-vor07-missed.toml`, `scenarios/epkk-ifr-vor07-missed.expected.txt` (regenerated)
- Modify: `docs/user-guide/en/ifr-and-vfr-points.md` and `docs/user-guide/ru/ifr-and-vfr-points.md` section 6 (~l.462-480), `docs/phraseology/en.md` row "Missed approach (controller)" (~l.69)

**Interfaces:**
- Produces: `pub missed_climb: bool` on `Strip` (`#[serde(default)]`), true from `set_missed_approach` until the aircraft is within `LEVEL_TOLERANCE_FT` of `plan.cleared_ft` or the intercept is given. Cleared by `end_flight`.

- [ ] **Step 1: Fix the existing test's expectation and add the descent test**

In `missed_approach_goes_back_to_approach`, the ZZZZ I27 missed approach is published to 4000 ft (fixture `ZZZZ.dat`, VM leg `04000`). Change the call to "... missed approach, climbing four thousand feet" and the expected items to `[AtcItem::Heading { .. }, AtcItem::ClimbTo(Level::Altitude(4000))]`. Also assert `e.strip(&cs()).unwrap().plan.as_ref().unwrap().cleared_ft == 4000`.

New test `descent_after_missed_approach_waits_for_the_missed_altitude` (same set-up as above, up to the Approach call after the missed approach):
- read back the heading and climb; keep `p.target = 3000.0` and fly on the vectors (`p.until(&mut e, 300, ...)`) until a `ClearedApproach` is heard;
- assert that no `DescendTo` was heard before that `ClearedApproach`;
- assert that the transmission carrying `ClearedApproach` also has `DescendTo(Level::Altitude(3000))` or `MaintainLevel(Level::Altitude(3000))` (the intercept brings the level down), and that `plan.cleared_ft == 3000` afterwards.

Then a second scenario in the same test (fresh engine): after the missed approach call, set `p.target = 4000.0`, fly until a `DescendTo(Level::Altitude(3000))` is heard, and assert it comes before any `ClearedApproach`.

- [ ] **Step 2: Run, verify both fail**

Run: `cargo test -p atc-core --features testkit --test engine missed_approach`
Expected: FAIL. The first test gets `ClimbTo(Altitude(3000))`. The new test hears `DescendTo`/`ClimbTo(3000)` right after the missed approach.

- [ ] **Step 3: Implement**

- `Strip`: add `missed_climb: bool` with a doc comment and `#[serde(default)]`; `false` in the constructor; reset in `end_flight`.
- `set_missed_approach`: set `strip.missed_climb = true` next to `missed_approach = true`.
- `vector_items`: compute `descend` as `!self.strips[i].missed_climb && plan.cleared_ft > intercept_ft && to_point <= DESCENT_M`. Before computing it, clear `missed_climb` when `(s.alt_ft_msl - f64::from(plan.cleared_ft)).abs() <= LEVEL_TOLERANCE_FT`. Clear it also when `intercept` is true. The intercept branch (`intercept && plan.cleared_ft > intercept_ft`) stays as is.
- Update the doc comment of `after_missed_approach` / `vector_items`: the first vectors after a missed approach keep the published missed-approach altitude, and the descent to the intercept altitude waits until it is reached.

- [ ] **Step 4: Run the engine tests**

Run: `cargo test -p atc-core --features testkit --test engine`
Expected: all pass.

- [ ] **Step 5: Update the VOR/DME 07 scenario**

In `scenarios/epkk-ifr-vor07-missed.toml`:
- the pilot's readback `at = "heard:heading two seven zero"` becomes "Left heading two seven zero, climb four thousand feet, Sierra Bravo Charlie";
- the two path legs after the go-around (`{ hdg = 270, nm = 3, ... }`, `{ hdg = 270, nm = 7.9, ... }`) fly `alt = 4000`; the later legs stay at 3000;
- add a `[[say]]` readback "Descend three thousand feet, Sierra Bravo Charlie" for the second descent. First check how `at = "heard:..."` triggers repeat in `crates/atc-replay/src/lib.rs`. If one entry fires only once, add a second entry for this descent.

- [ ] **Step 6: Commit the code, regenerate the golden in a clean worktree**

```bash
git add crates/atc-core/src/strip.rs crates/atc-core/src/engine/approach.rs crates/atc-core/tests/engine.rs scenarios/epkk-ifr-vor07-missed.toml
git commit -m "fix(approach): keep the published missed-approach altitude after a missed approach"
```
Then follow the clean-worktree procedure. Expected golden diff, `epkk-ifr-vor07-missed.expected.txt` only:
- "turn left heading two seven zero, climb altitude four thousand feet";
- a later "descend to altitude three thousand feet" plus its readback, before the second "cleared VOR D M E approach";
- timestamps may shift.

No other scenario changes. Copy the file back and commit with the docs (Step 7).

- [ ] **Step 7: Docs**

In section 6 of both guides (EN and RU), make the Approach line and the readback the new golden lines ("climb altitude four thousand feet"), and add the descent line. Add one sentence: after a missed approach Approach keeps you at the published missed-approach altitude (4000 ft on VOR/DME 07) and clears the descent to 3000 ft later. In `docs/phraseology/en.md` the "Missed approach (controller)" example becomes `turn left heading two seven zero, climb altitude four thousand feet`.

```bash
git add scenarios/epkk-ifr-vor07-missed.expected.txt docs/user-guide/en/ifr-and-vfr-points.md docs/user-guide/ru/ifr-and-vfr-points.md docs/phraseology/en.md
git commit -m "docs: missed approach climbs to the published altitude"
```

---

### Task 2: "Follow published missed approach" + Voices wording (spec items 2, 11)

**Files:**
- Modify: `crates/atc-phraseology/src/types.rs` (`AtcItem`), `crates/atc-phraseology/src/render.rs`, `crates/atc-phraseology/src/pl/render.rs`; any other exhaustive `match` on `AtcItem` the compiler points at
- Modify: `crates/atc-core/src/engine/approach.rs` (`missed_alt_ft` ~l.529), `crates/atc-core/src/engine/tower.rs` (`going_around` ~l.144, the `GoAround` branch of `landing` ~l.300-315)
- Test: `crates/atc-phraseology/tests/render.rs`, `crates/atc-phraseology/tests/render_pl.rs`, `crates/atc-core/tests/engine.rs` (`pilot_going_around_goes_back_to_approach` ~l.6491, `tower_go_around_goes_back_to_approach` ~l.6423, `handed_in_ifr_inbound_on_tower_expects_the_approach` ~l.7276 go-around part ~l.7319)
- Modify: `scenarios/epkk-ifr-vor07-missed.toml` + `.expected.txt`, `docs/phraseology/en.md`, `docs/phraseology/pl.md`, `docs/user-guide/{en,ru}/ifr-and-vfr-points.md` (section 6 and "Voices" ~l.11-13)

**Interfaces:**
- Consumes: `Strip::missed_climb` (Task 1); not touched here.
- Produces: `AtcItem::FollowMissedApproach` (unit variant). `fn published_missed_ft(&self, runway: &str, kind: ApproachKind) -> Option<i32>` in approach.rs gives the highest floor of the missed-approach legs, or `None`. `missed_alt_ft` becomes `published_missed_ft(..).unwrap_or_else(|| intercept_alt_ft(..))`.

- [ ] **Step 1: Failing render tests**

- `render.rs`: `follow_missed_approach_renders` — rendering `[AtcItem::FollowMissedApproach, AtcItem::Contact { station: "Kraków Approach".into(), khz: 121075 }]` to SBC (abbreviated) gives text containing `"follow published missed approach, contact Kraków Approach one two one decimal zero seven five"`. Use the file's existing render helper.
- `render_pl.rs`: `follow_missed_approach_renders_in_polish` — `one(AtcItem::FollowMissedApproach)` contains `"wykonuj opublikowaną procedurę nieudanego podejścia"`.

- [ ] **Step 2: Failing engine tests**

- `pilot_going_around_goes_back_to_approach`: assert `items(&out) == [AtcItem::FollowMissedApproach, AtcItem::Contact { station: "Zeta Approach".into(), khz: APP }]`.
- `tower_go_around_goes_back_to_approach`: `vec![AtcItem::GoAround]` becomes `vec![AtcItem::GoAround, AtcItem::FollowMissedApproach]`.
- `handed_in_ifr_inbound_on_tower_expects_the_approach`, go-around part:
  - the reply is `[AtcItem::FollowMissedApproach]`;
  - `plan.cleared_ft == 4000`;
  - the pilot's "follow published missed approach, Sierra Papa Tango Sierra Tango" gets no `SayAgain`;
  - set `f.alt = 4000.0` before `no_bounce`, in place of the old `ft` from `ClimbTo`.
- New `going_around_without_published_missed_legs_is_unchanged` (module `approach_arrival`). Use `super::approach_departure::engine_with(|set| for p in &mut set.procedures { p.legs.retain(|l| !l.leg.missed) })` and the flow of `pilot_going_around_goes_back_to_approach`. "going around" gets exactly `[Contact Zeta Approach]`, with no `FollowMissedApproach`.

Run: `cargo test -p atc-phraseology follow_missed && cargo test -p atc-core --features testkit --test engine going_around go_around`
Expected: compile errors or FAIL (no such variant / old items).

- [ ] **Step 3: Implement**

- `AtcItem::FollowMissedApproach` with a doc comment. EN render: `"follow published missed approach"`. PL render: `"wykonuj opublikowaną procedurę nieudanego podejścia"`. No `ReadBack` element (`required_readback` adds nothing for it). Content/acknowledgement handling needs no change.
- approach.rs: add `published_missed_ft` and rewrite `missed_alt_ft` on it (Interfaces).
- tower.rs `going_around`: after `set_missed_approach(i)`, `let follow = self.published_missed_ft(&runway, kind).is_some()` (runway and kind from `self.arrival(i)`'s `approach`).
  - Not `tower_inbound`: `FollowMissedApproach` goes in front of `self.redirect(i, Role::Approach)` when `follow`.
  - `tower_inbound` with `follow`: `[FollowMissedApproach]`, through `expect_readback` as today.
  - Without `follow`: unchanged.
- tower.rs landing: when `published && items == [AtcItem::GoAround]` and the approach has published missed legs, push `FollowMissedApproach` after `GoAround`. Do it before `expect_readback`, so `pending_items` holds both.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p atc-phraseology && cargo test -p atc-core --features testkit --test engine`
Expected: all pass.

- [ ] **Step 5: Scenario, commit, golden**

In `scenarios/epkk-ifr-vor07-missed.toml`, the pilot's answer `at = "heard:contact kraków approach"` (freq tower) becomes "Follow published missed approach, Approach one two one decimal zero seven five, Sierra Bravo Charlie".
```bash
git add crates/atc-phraseology/src/types.rs crates/atc-phraseology/src/render.rs crates/atc-phraseology/src/pl/render.rs crates/atc-phraseology/tests/render.rs crates/atc-phraseology/tests/render_pl.rs crates/atc-core/src/engine/approach.rs crates/atc-core/src/engine/tower.rs crates/atc-core/tests/engine.rs scenarios/epkk-ifr-vor07-missed.toml
git commit -m "feat(tower): follow published missed approach after a go-around"
```
Clean-worktree golden. Expected diff, `epkk-ifr-vor07-missed.expected.txt` only:
- Tower's go-around reply is "Sierra Bravo Charlie, follow published missed approach, contact Kraków Approach one two one decimal zero seven five";
- the new pilot line.

- [ ] **Step 6: Docs (both languages in sync)**

- `docs/phraseology/en.md`: a row `FollowMissedApproach` | `follow published missed approach` (after a go-around, before `contact ...`) | Doc 4444 ch. 12 (missed approach). Update the "Missed approach (controller)" row text to mention it.
- `docs/phraseology/pl.md`: a controller row `FollowMissedApproach` | `wykonuj opublikowaną procedurę nieudanego podejścia` | project wording.
- Both guides, section 6: Tower's go-around line and the pilot's answer as in the new golden.
- Both guides, "Voices" (~l.13). Replace the list with: Delivery, Approach and the units the EPKK station emulates (Warszawa Radar, Kraków Information, and Katowice Tower while the EPKT station is not loaded) use the three additional English voices. With EPKT loaded, Katowice Tower is a full station like Kraków Tower. RU: the same meaning.

```bash
git add scenarios/epkk-ifr-vor07-missed.expected.txt docs/phraseology/en.md docs/phraseology/pl.md docs/user-guide/en/ifr-and-vfr-points.md docs/user-guide/ru/ifr-and-vfr-points.md
git commit -m "docs: follow published missed approach; Katowice Tower in the voices list"
```

---

### Task 3: Adjacent Tower only for a neighbouring destination (spec item 3)

**Files:**
- Modify: `crates/atc-core/src/engine/approach.rs` (`destination_tower` ~l.1172, its doc and `hand_off`'s doc ~l.1100)
- Test: `crates/atc-core/tests/engine.rs` module `approach_departure` (next to `departure_beyond_tma_below_acc_goes_to_fis` ~l.4286)

**Interfaces:**
- Produces: `destination_tower(&self, plan: &FlightPlan) -> Option<&AdjacentUnit>`. It is `None` when `distance_m(self.airport_ref()?, self.destination_pos(plan)?) > NEIGHBOUR_DEST_M`, as in `destination_approach`. Factor the shared check into `fn neighbour_destination(&self, plan: &FlightPlan) -> Option<LatLon>` and use it in both.

- [ ] **Step 1: Failing test**

`far_adjacent_tower_does_not_block_the_en_route_hand_off`:
- engine from `engine_with_procs(|p| p.adjacent.push(<AdjacentUnit "Farland Tower", AdjacentKind::Tower, airport Some("ZZZF"), khz 118500, min None>), |_| {})`. Farland is 80 NM north, beyond the 60 NM neighbour radius;
- then the exact flow and assertions of `departure_beyond_tma_below_acc_goes_to_fis`: `DirectTo ALFAX` (not Farland), then at 39.5–41 NM `[RadarServiceTerminated, Contact Zeta Information]`.

Share the flow through a helper `fn fly_farland_departure(e: &mut AtcEngine)` called by both tests instead of copying it.

- [ ] **Step 2: Run, verify it fails**

Run: `cargo test -p atc-core --features testkit --test engine far_adjacent_tower`
Expected: FAIL (`DirectTo Farland`, or no hand-off at 40 NM).

- [ ] **Step 3: Implement** `neighbour_destination` and the guard in `destination_tower`, per Interfaces. Update the doc comments of `hand_off`, `destination_tower` and `NEIGHBOUR_DEST_M` (it now covers both units).

- [ ] **Step 4: Run** `cargo test -p atc-core --features testkit --test engine` — all pass (the Testville Tower tests, 30 NM, still hand to Testville Tower).

- [ ] **Step 5: Commit**

```bash
git add crates/atc-core/src/engine/approach.rs crates/atc-core/tests/engine.rs
git commit -m "fix(approach): an adjacent tower beyond 60 NM does not block the ACC/FIS hand-off"
```
Run the clean-worktree replay check (no UPDATE). Expected: no golden diff.

---

### Task 4: SID floor after an earlier ceiling (spec item 4)

**Files:**
- Modify: `crates/atc-core/src/ifr.rs` (`initial_climb_ft` ~l.124)
- Modify: `crates/atc-core/src/engine/delivery.rs` (`ifr_departure` ~l.206, `sid_from` ~l.240, `runway_change` ~l.263)
- Test: `crates/atc-core/tests/ifr.rs`, `crates/atc-core/tests/engine.rs` (module `delivery` or `approach_departure`)

**Interfaces:**
- Produces: `pub fn sid_target_ft(path: &Path, cruise_ft: i32) -> i32`, the cruise level raised to the highest `AtOrAbove`/`Between.above` floor on the path, rounded up to 1000 ft. `initial_climb_ft` uses it as its `target`. `sid_from` returns `(AtcItem, String, i32 /*climb_ft*/, i32 /*target_ft*/)`.

- [ ] **Step 1: Failing tests**

- `ifr.rs` `sid_target_keeps_a_floor_after_a_ceiling`: on the ZZZZ SID path from `initial_climb_respects_floors_and_ceilings`, set `constraints[0].1 = AtOrBelow(4000)` and `constraints[1].1 = AtOrAbove(6000)`. Assert:
  - `initial_climb_ft(&path, 5000) == 4000`;
  - `sid_target_ft(&path, 5000) == 6000`;
  - `sid_target_ft(&path, 10000) == 10000`;
  - `sid_target_ft(&path, 5000)` with `AtOrAbove(6100)` is `7000`.
- engine: `ifr_clearance_cruise_respects_a_floor_after_a_ceiling`. Use `super::approach_departure::engine_with(edit)`, where `edit` sets the first two constraints of the chosen RW27 SID's legs (`leg.alt`) to `AtOrBelow(4000)` and `AtOrAbove(6000)`. Request an IFR clearance with no level. Assert `ClimbTo(Altitude(4000))` in the clearance, and `plan.cruise_ft == 6000`, `plan.cleared_ft == 4000`.

- [ ] **Step 2: Run, verify they fail**

Run: `cargo test -p atc-core --features testkit --test ifr sid_target && cargo test -p atc-core --features testkit --test engine floor_after_a_ceiling`
Expected: FAIL (`sid_target_ft` not found / cruise 5000).

- [ ] **Step 3: Implement**

- Add `sid_target_ft` and use it inside `initial_climb_ft`. Update its doc: the clearance stops at the first ceiling; the target is what the flight is later climbed to.
- `sid_from` also returns `sid_target_ft(&procedure_path(sid), cruise_ft)`.
- `ifr_departure`: `cruise_ft = target_ft` (drop the `max(climb_ft)` line; `target_ft >= climb_ft` always holds).
- `runway_change`: `plan.cruise_ft = target_ft`.
- Update the module docs at the top of delivery.rs (l.7) to match.

- [ ] **Step 4: Run** `cargo test -p atc-core --features testkit` — all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/atc-core/src/ifr.rs crates/atc-core/src/engine/delivery.rs crates/atc-core/tests/ifr.rs crates/atc-core/tests/engine.rs
git commit -m "fix(delivery): the cruise level respects a SID floor after an earlier ceiling"
```
Clean-worktree replay check, no UPDATE. Expected: no golden diff (no EPKK SID has a ceiling before a higher floor). If an EPKK golden changes, stop and report.

---

### Task 5: Stand names: home stand position, trailing callsign, "nine right" (spec items 5, 6, 8)

**Files:**
- Modify: `crates/atc-core/src/strip.rs` (field, constructor)
- Modify: `crates/atc-core/src/engine/mod.rs` (~l.613-621, where `home_stand` is set), `crates/atc-core/src/engine/ground.rs` (`taxi_to_stand` ~l.239), `crates/atc-core/src/engine/follow_me.rs` (~l.92), `crates/atc-core/src/engine/stand.rs` (`stand_of` ~l.46), `crates/atc-core/src/engine/stand_match.rs` (`stand_is_free` ~l.100)
- Modify: `crates/atc-phraseology/src/parse.rs` (stand capture in `intent`, ~l.1312-1322)
- Test: `crates/atc-phraseology/tests/parse.rs`, `crates/atc-core/tests/engine.rs` module `approach_departure` (stand tests ~l.4408-4900)

**Interfaces:**
- Produces: `pub home_stand_at: Option<LatLon>` on `Strip` (`#[serde(default)]`), set together with `home_stand` from the stand found within `HOME_STAND_M`. All home-stand targets become `TaxiTarget::Stand { name: home, at: home_stand_at }`, and `stand_of` returns `(home, home_stand_at)`.

- [ ] **Step 1: Failing parser tests** (`tests/parse.rs`)

- `stand_said_with_right_is_the_letter`: `heard_stand("Ground, Sierra Bravo Charlie, request taxi to stand nine right") == "stand 9 R"`. Also `"... stand niner romeo"` gives `"stand 9 R"`. Also `"... stand two left"` gives `"stand 2 L"`.
- `stand_capture_stops_at_the_known_callsign`: `parse_pilot_known("Krakow Ground, request taxi to stand nine nine Sierra Papa Alpha Bravo Charlie", Lang::En, &[Callsign::new("SPABC")])` has intent `RequestTaxiToStand { stand: Some("stand 99") }`. If spaced digits do not merge, use the form the tokenizer gives for "nine nine" and assert that no letter follows.

- [ ] **Step 2: Failing engine tests** (module `approach_departure`)

- `home_stand_is_the_one_the_aircraft_started_from`. Build with `engine_airport(Strictness::Standard, |ap| ap.stands.insert(0, Stand { name: "Stand 1".into(), pos: <300 m east of STAND_1>, ..<as engine_with_stands builds it> }), |_| {}, |_| {})`. The duplicate comes first, so a name lookup finds it. Then `taxiing_in_near_stand_1(&mut e, true)`. Assert:
  - `strip.home_stand_at` is within 1 m of `STAND_1`;
  - after "request taxi" the strip's `taxi_target` is `TaxiTarget::Stand { name: "Stand 1", at: Some(p) }` with `p` within 1 m of `STAND_1`.
- `unknown_stand_with_trailing_callsign_names_only_the_stand`: SPTST parked as in `unknown_stand_names_the_heard_stand` (~l.4590). "Zeta Ground, request taxi to stand nine nine Sierra Papa Tango Sierra Tango" gives `Unable(UnknownStand { stand: "99" })`.
- `stand_nine_right_matches_stand_9r`: `engine_with_stands(&[("9R", <100 m east of STAND_1>)])`, parked on Stand 1. "Zeta Ground, Sierra Papa Tango Sierra Tango, request taxi to stand nine right" gives `Taxi { to: TaxiTo::Stand { name: "9R" }, .. }`.

Run: `cargo test -p atc-phraseology --test parse stand && cargo test -p atc-core --features testkit --test engine stand`
Expected: the four new tests FAIL (the first engine test may fail to compile until the field exists; that counts).

- [ ] **Step 3: Implement**

- parse.rs stand capture loop:
  - stop (`break`) when `self.used[k]` is true for the token's index, as in `designator_run` (iterate with indices);
  - `Token::Word("left" | "right" | "centre" | "center")` right after a `Token::Number` pushes `"L"` / `"R"` / `"C"`; anywhere else these words stop the capture as before.
- `Strip::home_stand_at` (doc, serde default, `None` in the constructor). Set it in mod.rs from the same `find(...)` as `home_stand` (keep the `Stand`, take `name` and `pos`).
- ground.rs, follow_me.rs, stand.rs: use `home_stand_at` as `at`.
- stand_match.rs `stand_is_free`: a strip's home stand claims stand `k` when `home_stand == st.name` and either `home_stand_at` is within 1 m of `st.pos`, or `home_stand_at` is `None` and `first_of_name`. Update the comment.

- [ ] **Step 4: Run** `cargo test -p atc-phraseology && cargo test -p atc-core --features testkit` — all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/atc-core/src/strip.rs crates/atc-core/src/engine/mod.rs crates/atc-core/src/engine/ground.rs crates/atc-core/src/engine/follow_me.rs crates/atc-core/src/engine/stand.rs crates/atc-core/src/engine/stand_match.rs crates/atc-phraseology/src/parse.rs crates/atc-phraseology/tests/parse.rs crates/atc-core/tests/engine.rs
git commit -m "fix(stands): home stand by position, callsign after the stand, 'nine right' is 9R"
```
Clean-worktree replay check, no UPDATE. Expected: no golden diff. If a golden changes because the trailing callsign is no longer captured, the change must be only in that refusal or taxi line; update it and say so in the report.

---

### Task 6: Polish "passing" and the Polish Tower-only inbound test (spec items 7, 10)

**Files:**
- Modify: `crates/atc-phraseology/src/pl/normalize.rs` (rule table, "Instrument procedures and levels" ~l.157-176; unit tests at the bottom)
- Modify: `docs/phraseology/pl.md` (pilot table ~l.86-124)
- Test: `crates/atc-core/tests/engine.rs` (module `approach_arrival` near `polish_inbound_climbing_reports_the_level_it_climbs_to` ~l.5596; module `handover` near `tower_only` ~l.7224)

**Interfaces:**
- Produces: rules `("mijam", "passing", Next::Any)` and `("przechodzę", "passing", Next::Number)`.

- [ ] **Step 1: Failing tests**

- normalize.rs unit tests (use the file's existing helpers for normalised words):
  - `mijam_is_passing`: "mijam cztery tysiące stóp" normalises to the words/numbers `passing 4000 feet`;
  - `przechodze_is_passing_only_before_a_number`: "przechodzę cztery tysiące" gives `passing 4000`, and "przechodzę na drugi krąg" still gives `going around`.
- engine `polish_passing_only_gets_the_arrival_altitude` (approach_arrival): same set-up as `polish_inbound_climbing_reports_the_level_it_climbs_to`, aircraft at 4000 ft. The call is "Zeta Approach, Sierra Papa Tango Sierra Tango, IFR, mijam cztery tysiące stóp, zbliżam się". Assert `ClimbTo(Level::Altitude(5000))` is in the reply and `MaintainLevel(Altitude(4000))` is not.
- engine `polish_tower_only_inbound_speaks_polish` (handover). Build like `tower_only` but with `engine_airport_cfg(EngineConfig { language: LanguageMode::Fixed(Lang::Pl), ..EngineConfig::new(Strictness::Standard) }, |_| {}, <same Approach-removing edit_procs>, |_| {})`. Import the flight and fly as `first_call_to_tower`, but say the call with `say_in(.., Lang::Pl)`: "Zeta Wieża, Sierra Papa Tango Sierra Tango, trzy tysiące stóp". Assert the reply's `text`:
  - contains `"spodziewaj się podejścia ILS, pas dwa siedem"` and `"zgłoś ustalenie na kursie podejścia końcowego"`;
  - does not contain `"expect"`;
  - and `message.lang == Lang::Pl`.

Run: `cargo test -p atc-phraseology --lib normalize && cargo test -p atc-core --features testkit --test engine polish`
Expected: the "mijam"/"przechodzę" and passing-only tests FAIL. `polish_tower_only_inbound_speaks_polish` may already pass: it is a missing-coverage test (spec item 10). Note in the report whether it passed before the change.

- [ ] **Step 2: Implement** the two rules in the levels group, with a comment: project wording, no Polish source in the repo; "przelatuję" deliberately not mapped (spec item 7).

- [ ] **Step 3: Run** `cargo test -p atc-phraseology && cargo test -p atc-core --features testkit` — all pass.

- [ ] **Step 4: Docs** — `docs/phraseology/pl.md` pilot table: a row `mijam`, `przechodzę` (before a number) | `passing`, marked project wording.

- [ ] **Step 5: Commit**

```bash
git add crates/atc-phraseology/src/pl/normalize.rs crates/atc-core/tests/engine.rs docs/phraseology/pl.md
git commit -m "fix(pl): 'mijam' / 'przechodzę <level>' mean passing; test the Polish Tower-only inbound"
```
Clean-worktree replay check, no UPDATE. Expected: no golden diff (`epkk-circuit-pl`, `epkk-language-switch` unchanged).

---

### Task 7: Garbled transmissions do not use up the handed-in first call (spec item 9)

**Files:**
- Modify: `crates/atc-core/src/strip.rs` (field, constructor)
- Modify: `crates/atc-core/src/engine/mod.rs` (`import_flight` ~l.391, `answer` ~l.803-850 and the `Content::Message if first_call` arm ~l.935)
- Test: `crates/atc-core/tests/engine.rs` module `handover` (next to `handed_in_first_call_is_never_say_again` ~l.7426)

**Interfaces:**
- Produces: `pub first_call_due: bool` on `Strip` (`#[serde(default)]`; `false` in the constructor; `true` in `import_flight`). `first_call` in `answer` becomes `strip.first_call_due && understood(&msg)`. Any understood transmission sets `first_call_due = false`. `heard` stays as it is for readback reminders.

- [ ] **Step 1: Failing test**

`garbled_transmission_does_not_use_up_the_first_call`: set up as in `handed_in_first_call_is_never_say_again` (VFR flight handed in, airborne west at 2000 ft), Tower frequency.
- "Zeta Tower, Sierra Papa Tango Sierra Tango, blah blah blah" gets exactly `[AtcItem::SayAgain]`;
- then "Zeta Tower, Sierra Papa Tango Sierra Tango, two thousand feet" gets `[Station { "Zeta Tower" }, Roger]`;
- a third identical call gets `[SayAgain]`.

Run: `cargo test -p atc-core --features testkit --test engine garbled_transmission`
Expected: FAIL (the garbled call gets `[Station, Roger]`).

- [ ] **Step 2: Implement** per Interfaces. Keep the comment on the literal first call, reworded: "the first understood call since the hand-in". `handed_in` handling is unchanged.

- [ ] **Step 3: Run** `cargo test -p atc-core --features testkit --test engine handed_in` and then the whole engine suite — all pass. In particular `handed_in_first_call_is_never_say_again` and `handed_in_second_call_on_approach_is_say_again` still pass.

- [ ] **Step 4: Commit**

```bash
git add crates/atc-core/src/strip.rs crates/atc-core/src/engine/mod.rs crates/atc-core/tests/engine.rs
git commit -m "fix(engine): a garbled call does not use up a handed-in flight's first call"
```

---

### Final check (after Task 7)

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --features atc-core/testkit -- -D warnings`, `cargo test --workspace --features atc-core/testkit --no-fail-fast`. Expected: everything passes except the 2 known local-only atc-replay failures.
- [ ] Clean worktree at HEAD: `cargo test -p atc-replay` passes 10/10, all goldens unchanged since Task 2.
- [ ] `git status` shows only the untracked `docs/.obsidian/` and `track.csv` (never staged).
