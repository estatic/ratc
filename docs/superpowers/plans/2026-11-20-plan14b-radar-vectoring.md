# Radar vectoring with a planned path (plan 14b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Approach vectors an arrival along a planned path to the final approach course, stays silent while it is followed, and corrects heading/altitude from the radar picture when it is not.

**Architecture:** A new pure module `crates/atc-core/src/vectoring.rs` holds the geometry (planned path, turn lead, radar track/drift, judging the aircraft against the plan). The plan lives on the strip's `FlightPlan` (replacing `pattern: Option<PatternLeg>`). `engine/approach.rs` `vector_items` is rewritten around it, plus the after-clearance watch, radar-checked "established" and break-off. Compliance's heading/level calls are suppressed while a plan is active. New spoken items in `atc-phraseology`.

**Tech Stack:** Rust workspace, serde, existing `atc-geo` (`RunwayFrame`, `bearing_deg`, `destination`, `distance_m`).

**Spec:** `docs/superpowers/specs/2026-11-20-radar-vectoring-design.md`

## Global Constraints

- Simulator-neutral: core and server use only `AircraftState` (position, heading, ground speed, altitude, vertical speed); nothing simulator-specific is added.
- Approach and adjacent units always speak English; new items go in the Polish renderer's English-only list.
- English only in code and docs, except `docs/user-guide/ru/`.
- Engine tests in `crates/atc-core/tests/engine.rs` (run with `cargo test -p atc-core --features testkit`); geometries reproduced at EPKK ILS 25, the only tracked station.
- Before each commit: `cargo fmt --all -- --check`, `cargo test --workspace -q`, `cargo clippy --workspace --all-targets -- -D warnings`. Known local-only failure to ignore: atc-replay `tests::path_and_frequency_errors`.
- Old snapshots must still load (`#[serde(default)]` on new strip fields; the removed `pattern` field is ignored on load).
- Branch `feat/radar-vectoring-14b`; never push to main.

Values from the spec used across tasks (one source: constants in `vectoring.rs`):
- intercept angle 30° nominal (20–30), re-intercept up to 45°; join at least 2 NM outside the FAF
- turn lead: standard-rate radius `gs_kt / 188.5` NM (half rate, i.e. double radius, above 250 kt) × `tan(turn/2)` plus 10 s at ground speed
- descent gradient 300 ft per NM
- tolerances: 0.5 NM off the leg (1 NM on downwind); 15° off the assigned heading; 300 ft off the planned altitude — each only when not converging
- correction transmitted only when the heading changes by ≥ 10°, or out of tolerance and not converging; at most one unsolicited correction per 20 s per aircraft; nothing while a heading/level readback is pending or for 10 s after (14c rule)
- established: within 0.3 NM of the course and track within 15° of it
- after clearance: crossing the course by > 0.3 NM, or converging at > 45° within 1 NM of it → re-intercept
- drift: radar track (from positions) minus heading, smoothed over the last ~30 s

## Review Focus

- A strong crosswind (20 kt across the final) must not make the controller correct every 20 s — Task 5 test `crosswind_leg_is_flown_without_repeated_corrections`.
- An aircraft that turns early (overshoots the turn point the other way, inside) gets no correction while converging — Task 5 test `early_turn_converging_is_silent`.
- A missed approach after this change still vectors a new approach from the missed-approach altitude — Task 6 test `missed_approach_is_vectored_again`.
- A snapshot taken before this change (strip with `pattern`) restores and vectoring continues — Task 3 test `old_snapshot_with_pattern_loads`.
- A fast aircraft (180 kt) gets its intercept heading earlier than a C172 (100 kt) at the same geometry — Task 4 test `intercept_given_earlier_for_faster_aircraft`.

---

### Task 1: Vectoring geometry module

**Files:**
- Create: `crates/atc-core/src/vectoring.rs` (+ `pub mod vectoring;` in `crates/atc-core/src/lib.rs`)
- Test: unit tests in `vectoring.rs`

**Interfaces:**
- Produces (all `pub`, true degrees, metres unless named `_nm`/`_ft`):
```rust
pub struct FinalCourse { pub thr: LatLon, pub course_deg: f64, pub faf_m: f64, pub intercept_alt_ft: i32, pub circuit_side: f64 /* +1 right, -1 left */ }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum LegKind { Downwind, Base, Intercept, Final }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct VectorLeg { pub kind: LegKind, pub track_deg: f64, pub from: LatLon, pub to: LatLon, pub alt_ft: i32 }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct VectorPlan { pub legs: Vec<VectorLeg>, pub current: usize }
pub fn plan_path(pos: LatLon, track_deg: f64, alt_ft: f64, f: &FinalCourse) -> VectorPlan;
impl VectorPlan { pub fn leg(&self) -> &VectorLeg; pub fn joins_in_time(&self, f: &FinalCourse) -> bool; }
pub fn turn_lead_m(gs_kt: f64, turn_deg: f64) -> f64;
pub fn descent_start_m(from_ft: f64, to_ft: i32) -> f64; // distance before the point where the descent must begin, at 300 ft/NM
#[derive(Clone, Debug, Default, Serialize, Deserialize)] pub struct RadarTrack { /* last ~30 s of (t, pos, heading) */ }
impl RadarTrack { pub fn push(&mut self, t: f64, pos: LatLon, heading_deg: f64); pub fn track_deg(&self) -> Option<f64>; pub fn drift_deg(&self) -> f64 /* 0 without data */; }
pub fn heading_for_track(track_deg: f64, drift_deg: f64) -> f64;
pub struct Radar { pub pos: LatLon, pub track_deg: f64, pub gs_kt: f64, pub alt_ft: f64, pub vs_fpm: f64 }
#[derive(Debug, PartialEq)] pub enum Verdict { OnPlan, LegDue /* turn lead to the next leg reached */, OffTrack { right_nm: f64 }, OffAltitude { ft: f64 } }
pub fn judge(plan: &VectorPlan, r: &Radar) -> Verdict;
pub fn established(r: &Radar, f: &FinalCourse) -> bool;
pub fn through_course(r: &Radar, f: &FinalCourse) -> bool; // crossed by > 0.3 NM, or > 45° within 1 NM
```
- `plan_path` keeps today's geometry rules from `engine/approach.rs` (`steer`, `vector_to_final`, constants `INTERCEPT_NM` 8, `DOWNWIND_Y_NM` 4, `BASE_BEYOND_NM` 3, `PATTERN_GAP_DEG` 90): direct join when the turn onto the intercept is ≤ 90°, else downwind–base–intercept on the aircraft's side (circuit side on the centreline). Every leg gets `alt_ft` descending at 300 ft/NM to `intercept_alt_ft` before the Intercept leg; Final is the course from the join point to the FAF.

- [ ] **Step 1: Write failing unit tests:** `direct_join_has_intercept_and_final` (aircraft 15 NM out, 4 NM south of the final heading parallel inbound → legs `[Intercept, Final]`, intercept track = course ∓ 30°, join ≥ 2 NM outside FAF); `far_side_gets_pattern` (aircraft beyond the threshold → `[Downwind, Base, Intercept, Final]`); `turn_lead_scales_with_speed` (`turn_lead_m(100.0, 90.0) < turn_lead_m(180.0, 90.0)`; at 120 kt, 90° ≈ (0.64 NM + 10 s × 120 kt) within 5 %); `half_rate_above_250` (radius doubles); `drift_from_positions` (positions moving due north while heading 350 → `drift_deg ≈ +10`); `judge_off_track_and_converging` (0.6 NM right of an Intercept leg but track converging → `OnPlan`; same with track parallel → `OffTrack`); `established_needs_position_and_track`; `through_course_detects_overshoot`; `joins_in_time_false_inside_faf_margin`.
- [ ] **Step 2: Run** `cargo test -p atc-core vectoring` — FAIL (module missing).
- [ ] **Step 3: Implement** the module (geometry via `RunwayFrame`; `RadarTrack` keeps samples ≤ 30 s old, track from first-to-last position).
- [ ] **Step 4: Run** — PASS; fmt/clippy.
- [ ] **Step 5: Commit** `core: vectoring geometry - planned path, turn lead, radar track and judging`

### Task 2: Spoken items for corrections

**Files:**
- Modify: `crates/atc-phraseology/src/types.rs`, `render.rs`, `pl/render.rs` (English-only list)
- Test: `crates/atc-phraseology/tests/render.rs`

**Interfaces:**
- Produces:
```rust
pub enum CourseName { Localizer, FinalApproachCourse }
AtcItem::OffCourse { tenths_nm: u16, side: CompassPoint /* existing 8-point type if present, else new N/NE/.../NW */, course: CourseName }
AtcItem::ReIntercept(CourseName)          // appended after a Heading: "to re-intercept the localizer"
AtcItem::Negative                         // "negative"
AtcItem::NewApproach                      // "I will vector you for a new approach"
```
- [ ] **Step 1: Failing render tests:** `OffCourse{tenths_nm:5, side:N, course:Localizer}` → `"you are half a mile north of the localizer"`; `tenths_nm:10` → `"one mile"`; `15` → `"one and a half miles"`, other values `"two point three miles"`; `[Negative, OffCourse{..}, Heading{deg:260, turn:Some(Left)}, ReIntercept(Localizer)]` → `"negative, you are half a mile north of the localizer, turn left heading two six zero to re-intercept the localizer"`; `NewApproach` → `"I will vector you for a new approach"`; `FinalApproachCourse` → `"final approach course"`.
- [ ] **Step 2: Run** `cargo test -p atc-phraseology --test render` — FAIL.
- [ ] **Step 3: Implement**; Polish render of the four items uses the English text (as `CheckHeading`).
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `phraseology: vectoring corrections - off course, re-intercept, new approach`

### Task 3: Plan on the strip, radar track on the track state

**Files:**
- Modify: `crates/atc-core/src/strip.rs` (`FlightPlan.pattern` → `vector_plan: Option<VectorPlan>`, `#[serde(default)]`; remove `PatternLeg`; `last_correction_s: Option<f64>` on the plan or strip)
- Modify: engine track state (where `tracks` entries hold `last: AircraftState`, `engine/mod.rs`) to hold a `RadarTrack` pushed every telemetry sample
- Modify: every use of `PatternLeg` / `plan.pattern` (compile-driven; behaviour is Task 4)
- Test: `crates/atc-core/tests/engine.rs` (snapshot test)

**Interfaces:**
- Consumes: Task 1 `VectorPlan`, `RadarTrack`.
- Produces: `FlightPlan.vector_plan: Option<VectorPlan>`, `FlightPlan.last_correction_s: Option<f64>` (`#[serde(default)]`), `AtcEngine::radar(&self, i: usize) -> Option<vectoring::Radar>` (track from `RadarTrack`, else heading).

- [ ] **Step 1: Failing test** `old_snapshot_with_pattern_loads`: a snapshot JSON with `"pattern":"Base"` in the plan restores; `vector_plan` is `None`; the next telemetry sample does not panic.
- [ ] **Step 2: Run** — FAIL (field missing / compile).
- [ ] **Step 3: Implement**: temporarily keep today's `steer` behaviour by mapping it to a plan-less path (Task 4 replaces it) so all existing tests pass.
- [ ] **Step 4: Run** `cargo test -p atc-core --features testkit` and the workspace — PASS, goldens unchanged.
- [ ] **Step 5: Commit** `core: vector plan on the strip, radar track per aircraft`

### Task 4: Normal vectoring along the plan

**Files:**
- Modify: `crates/atc-core/src/engine/approach.rs` (`vector_items`, `steer`, `vector_to_final` callers; module doc section on vectors), `crates/atc-core/src/ifr.rs` (remove `vector_to_final`/`lead_point` when unused)
- Modify: test pilot `approach_arrival::Pilot` in `crates/atc-core/tests/engine.rs`: add `reaction_s: f64` (delay before turning/descending after an instruction) and `wind: Option<(f64 /*from deg*/, f64 /*kt*/)>` (ground track = heading + drift)
- Test: `crates/atc-core/tests/engine.rs` new `mod planned_vectors`

**Interfaces:**
- Consumes: Tasks 1–3.
- Behaviour: plan built when vectoring starts (`vector_plan` is None) from `radar(i)`; the current leg's heading = `heading_for_track(leg.track_deg, drift)` (magnetic, 5° rounding via existing `vector_on`); the next leg's heading is given when `judge` returns `LegDue` (turn lead); descent to a leg's `alt_ft` when `descent_start_m` says so; on the Intercept leg: Heading + `DistanceToGo` + `ClearedApproach` + `ReportEstablished` (as today). Replaces the fixed 2 NM, 15 NM and 20° rules.

- [ ] **Step 1: Failing tests:** `direct_join_flown_to_established` (pilot reaction 5 s, no wind, starting south-east of EPKK final 25 → instructions are one heading, descent, intercept with clearance; ends established on the final outside the FAF; never more than 0.5 NM off the course after joining); `pattern_from_far_side` (starting west of EPKK → downwind, base, intercept in that order); `intercept_given_earlier_for_faster_aircraft` (same geometry at 100 kt and 180 kt: distance to the course when the intercept heading is given is larger at 180 kt); `descent_reaches_intercept_altitude_before_intercept` (from 6000 ft: at the intercept heading the cleared level is the intercept altitude and the pilot is within 300 ft of it).
- [ ] **Step 2: Run** `cargo test -p atc-core --features testkit planned_vectors` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the crate tests; update existing `approach_arrival` tests whose instruction timing changes only where the new timing is the spec's (record each in the report); regenerate replay goldens that change (`scenarios/epkk-ifr-arrival-vectors.*` etc.) and check each changed line.
- [ ] **Step 5: Commit** `core: Approach vectors along a planned path with turn lead and planned descent`

### Task 5: Watching and correcting on the vector legs

**Files:**
- Modify: `crates/atc-core/src/engine/approach.rs` (correction logic in `vector_items`/`arrival_update`)
- Modify: `crates/atc-core/src/engine/mod.rs` (compliance: no heading/level deviation calls while `vector_plan.is_some()`)
- Test: `crates/atc-core/tests/engine.rs` `mod planned_vectors`

**Interfaces:**
- Consumes: Tasks 1–4; 14c's readback hold (`holds_clearance_checks` / `readback_done_at` in `engine/mod.rs`).
- Behaviour: when `judge` says `OffTrack`/`OffAltitude` (not converging) and no readback hold and ≥ 20 s since `last_correction_s`: re-plan from the present position; transmit `[OffCourse{..}?, Heading, level?]` only if the heading differs by ≥ 10° from the last given or still out of tolerance; an aircraft still on its previous heading after the reaction time gets `CheckHeading`-form wording (existing `AtcItem::CheckHeading` followed by the turn); set `last_correction_s`.

- [ ] **Step 1: Failing tests:** `late_turn_on_base_gets_correction` (C13 geometry at EPKK 25: pilot reaction 40 s on the intercept heading → one correction naming the side and a new heading, then established; total corrections ≤ 2); `wrong_heading_gets_correction` (pilot flies 20° off the given heading → correction after the hold, not `CheckHeading` from compliance twice); `early_turn_converging_is_silent`; `crosswind_leg_is_flown_without_repeated_corrections` (20 kt wind across the final: ≤ 1 correction per leg); `no_correction_during_readback` (pilot slow to read back → nothing until readback + 10 s); `compliance_silent_while_planned` (no `Deviation`-driven `CheckHeading`/`CheckLevel` on an aircraft with a plan).
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS; goldens.
- [ ] **Step 5: Commit** `core: Approach corrects heading and altitude when the aircraft leaves the planned path`

### Task 6: After the approach clearance: radar-checked established, re-intercept, break-off

**Files:**
- Modify: `crates/atc-core/src/engine/approach.rs` (`arrival_update` approach-cleared branch, `PilotIntent::ReportEstablished` arm ~203, `hand_over` to Tower)
- Test: `crates/atc-core/tests/engine.rs` `mod planned_vectors`

**Interfaces:**
- Consumes: `established`, `through_course`, `plan_path`, `joins_in_time` (Task 1); items (Task 2).
- Behaviour (spec "Decisions" 1, 3, 5): "established" report or final-inside-FAF transfers to Tower only when `established(radar)`; otherwise `[Negative, OffCourse, Heading, ReIntercept]` and keep the flight. `through_course` after clearance → `[OffCourse, Heading (≤45° onto course), ReIntercept]`. If the re-plan `!joins_in_time` → break-off: `[Heading (away, towards the new pattern side), MaintainLevel(max(present rounded 100, intercept alt)) or ClimbTo, NewApproach]`, `approach_cleared = false`, new pattern plan, not a missed approach. Course name: `Localizer` for ILS, `FinalApproachCourse` otherwise. Only flights Approach vectored onto the final (full procedure from the IAF: unchanged).

- [ ] **Step 1: Failing tests:** `overshoot_gets_reintercept` (B8 geometry at EPKK 25: pilot overshoots by 1 NM → re-intercept correction, then established, then Tower); `early_established_report_is_checked` (pilot reports established 0.6 NM off → `Negative` + correction, flight stays with Approach; later report when on course → Tower); `late_overshoot_breaks_off` (overshoot near the FAF → `NewApproach`, approach clearance cancelled, then a new pattern and a new intercept); `missed_approach_is_vectored_again` (existing missed-approach flow still vectors from the published altitude); `rnav_uses_final_approach_course_wording`.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the workspace — PASS; goldens regenerated and checked.
- [ ] **Step 5: Commit** `core: established checked by radar; re-intercept and break-off after the approach clearance`

### Task 7: Replay scenarios and docs

**Files:**
- Create: `scenarios/epkk-vectoring-late-turn.toml` (+ `.expected.txt`) — the C13-style late intercept at EPKK 25 with a scripted late readback
- Modify: `crates/atc-core/src/engine/approach.rs` module doc (vectoring paragraph), `docs/RUNNING.md` / user guides EN+RU only if they describe the old vector behaviour (grep "intercept" in `docs/`)
- [ ] **Step 1:** Write the scenario (path track flies the given headings late, see `scenarios/epkk-ifr-arrival-vectors.toml`), generate the expected output, review each controller line against the spec.
- [ ] **Step 2: Run** `cargo test --workspace -q`, fmt check, clippy — PASS.
- [ ] **Step 3: Commit** `replay, docs: planned-path vectoring with a late intercept at EPKK`
