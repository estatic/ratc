# Follow-me car and readback reminders — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix what the 2026-10-29 in-sim circuit at EPKK showed: the follow-me car's wheels, corners, end manoeuvre and missing markings; a readback of "follow-me car is on the way" taken for a new request; the follow-me instruction without a destination; and no reminder when a readback is missing.

**Architecture:** The engine builds a smoothed follow-me path (corner arcs) and names the destination; the parser stops treating that readback as a request; the strip records when a readback became due and the engine reminds once (10 s, or at once when the aircraft moves with a taxi readback pending). The plugin passes its own steering angle to the car instance, leaves the path along an arc instead of sliding sideways, and draws a second instance — our own OBJ8 with a FOLLOW ME sign, an amber beacon and turn signals — on top of the car.

**Tech Stack:** Rust 2024 (atc-core, atc-phraseology, atc-xplane-plugin); X-Plane 11 XPLM instancing API; OBJ8 text format.

**Spec:** this plan (user-approved list in chat, 2026-10-29); root causes recorded below.

## Root causes (investigated 2026-10-29)

- Wheels: `crew_car.obj` animates `sim/graphics/animation/ground_traffic/tire_steer_deg[0]`; the plugin creates the instance with no datarefs (`crates/atc-xplane-plugin/src/plugin.rs:519`), so X-Plane's own ground traffic drives the steering.
- 90° turns: the path is the taxi-network nodes joined by straight segments (`crates/atc-core/src/engine/follow_me.rs:78`).
- Sideways slide: `Phase::Parking` interpolates the position 12 m to the right while turning separately (`crates/atc-xplane-plugin/src/follow_me.rs:214`).
- No sign/beacon: the X-Plane 11 library has no follow-me vehicle.
- Duplicate "follow the follow-me car": `parse_pilot("hold position, follow-me car is on the way, Sierra Bravo Charlie")` → `RequestFollowMe` (verified).
- Silent controller: readbacks are checked only when the pilot transmits; nothing reminds.

## Global Constraints

- Corner arcs: radius 25 m, reduced so an arc never uses more than half of either adjacent segment; corners turning less than 10° are left as they are; arcs sampled every 2 m.
- Reminder: 10 s of sim time after a clearance that needs a readback, `<callsign>, read back <items>` (the existing `AtcItem::ReadBack(items)` with the still-missing items); at once when the aircraft's ground speed exceeds 3 kt while a taxi clearance's readback is pending; at most once per clearance (a new clearance re-arms it).
- Follow-me phrase: `follow the follow-me car to stand <name>` / `follow the follow-me car to holding point runway <rwy>` (Polish: `kołuj za samochodem follow me do stanowiska <name>` / `do punktu oczekiwania drogi startowej <rwy>`).
- Turn signal: on from 40 m before a path corner turning more than 30° until the corner is passed, side by the turn direction; blinking 1.5 Hz. Beacon: amber, 1 Hz flash. Both via instance datarefs set every frame.
- End manoeuvre: the car leaves the path along an arc forward to a point 12 m to the right of the end, ends facing the end heading, never moves sideways (position change always along its heading).
- The plugin never panics across `extern "C"`; missing object file → log once and draw the car without the roof object.
- English code/comments/docs; fmt, clippy `-D warnings`, `cargo test --workspace` green before every commit; one commit per task. Don't commit `config/*.toml` (personal), `track.csv`, `docs/.obsidian/`, wizard stations.

## Review Focus

1. **Readback of the follow-me "on the way" with extra words** ("roger, hold position, follow me car on the way"): still a readback, not a request (Task 1 `on_the_way_readback_is_not_a_request`, plus a real request "request follow me" still works).
2. **Pilot reads back right at 9.9 s**: no reminder after it (Task 2 `readback_before_10s_cancels_reminder`).
3. **Short taxi segments** (two nodes 8 m apart at a 90° corner): arc radius shrinks, path stays continuous, no loops (Task 3 `short_segments_shrink_the_arc`).
4. **End manoeuvre with a stand heading opposite to the path's last leg**: no sideways move, finishes facing the stand heading (Task 4 `parking_never_moves_sideways`).
5. **Plugin installed without the roof object file**: car still drawn, one log line (Task 5 manual check + unit test of the path resolution helper).

---

### Task 1: Parser and follow-me phrase

**Files:**
- Modify: `crates/atc-phraseology/src/parse.rs` (`follow_me()` ~1022), `crates/atc-phraseology/src/types.rs` (`AtcItem::FollowTheFollowMe`), `crates/atc-phraseology/src/render.rs`, `crates/atc-phraseology/src/pl/render.rs`, readback label/required-readback code if it matches on the variant, `crates/atc-core/src/engine/follow_me.rs` (pass the destination)
- Test: `crates/atc-phraseology/tests/` (existing parse/render test files), `crates/atc-core/tests/engine.rs`

**Interfaces:**
- Produces: `AtcItem::FollowTheFollowMe { to: TaxiTo }` (the existing `TaxiTo`); render per Global Constraints. `follow_me()` in the parser returns false when "follow me" is part of "follow-me car is on the way" / "follow me car on the way" (a readback), and when preceded by "the".
- [ ] **Step 1: Tests:** `on_the_way_readback_is_not_a_request` (the logged phrase and "roger, hold position, follow me car on the way, SBC" → not `RequestFollowMe`); `request_follow_me_still_parsed` ("request follow me car", "request taxi to stand with follow me car" → `RequestFollowMe`); render tests EN and PL for both destinations; engine test `follow_me_phrase_names_the_stand` (taxi-in follow-me → "follow the follow-me car to stand one seven" rendered).
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `fix(phraseology): follow-me readback is not a request; the instruction names its destination`

---

### Task 2: Readback reminders

**Files:**
- Modify: `crates/atc-core/src/strip.rs` (`expect_readback`, new fields with `#[serde(default)]` for the snapshot), `crates/atc-core/src/engine/mod.rs` (`on_telemetry`)
- Test: `crates/atc-core/tests/engine.rs`

**Interfaces:**
- Produces: `Strip.readback_due_s: Option<f64>` (sim time the pending readback became due), `Strip.reminded: bool`; `expect_readback` takes the sim time (`expect_readback(&mut self, items, strictness, t: f64)` — update all call sites) and re-arms; `readback_complete` clears both. In `on_telemetry`, after the existing expiry check: if a readback is pending, not yet reminded, and (`t - due >= 10.0` or (pending items contain `AtcItem::Taxi` and `gs_kt > 3.0`)) → transmit `AtcItem::ReadBack(missing items)` from the strip's owner role, set `reminded`.
- [ ] **Step 1: Tests:** `silent_pilot_is_reminded_after_10s`; `moving_without_taxi_readback_is_reminded_at_once` (taxi clearance, next telemetry 2 s later at 6 kt → "read back …"); `reminder_only_once`; `readback_before_10s_cancels_reminder`; `new_clearance_rearms_reminder`; snapshot of an old state file (no new fields) still restores.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat(core): remind a missing readback once`

---

### Task 3: Smoothed follow-me path

**Files:**
- Modify: `crates/atc-core/src/engine/follow_me.rs`
- Test: unit tests in that file

**Interfaces:**
- Produces: `fn smooth_corners(path: &[LatLon], radius_m: f64) -> Vec<LatLon>` (private, used where the `VehicleCommand::FollowMe` path is built, before `trim_end`) implementing the arc rule of Global Constraints (tangent points at `r·tan(θ/2)` from the corner, arc sampled every 2 m). The stand's final point is appended after smoothing the route, and the last corner (route → stand) is smoothed too.
- [ ] **Step 1: Tests:** `right_angle_becomes_an_arc` (100 m east then 100 m north → no point-to-point heading change above 10° between consecutive 2 m steps; all points within 25 m + 0.5 of the original corner's legs); `straight_path_unchanged`; `small_turn_unchanged` (8°); `short_segments_shrink_the_arc` (8 m legs: tangent offset ≤ 4 m, monotonic arc length, no self-intersection); `endpoints_kept`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat(core): follow-me path with rounded corners`

---

### Task 4: Car driving — steering, end arc, signals

**Files:**
- Modify: `crates/atc-xplane-plugin/src/follow_me.rs`
- Test: unit tests in that file (existing style, no X-Plane)

**Interfaces:**
- Produces: `Pose` gains `steer_deg: f64` (front-wheel angle, + right, from the heading rate: `atan(wheelbase · yaw_rate / speed)` with wheelbase 2.7 m, clamped ±35°, 0 when stopped), `turn_signal: Signal` (`pub enum Signal { Off, Left, Right }`), `beacon_on: bool` (true while the car exists). `Phase::Parking` replaced by driving an arc: a quarter-circle-like curve (cubic Bézier from the end point along the end heading to the aside point with the end heading) followed at ≤ 3 m/s, heading = curve tangent. Turn signal per Global Constraints (corners detected on the received path, >30°), and Right during the end arc.
- [ ] **Step 1: Tests:** `steer_follows_turn_direction` (left corner → negative steer, straight → ~0); `steer_zero_when_stopped`; `parking_never_moves_sideways` (each frame's displacement is within 5° of the heading or zero; final heading = end heading ±2°; final position 12 m ±0.5 right of the end); `signal_before_corner` (left corner → `Left` from 40 m before until passed); `signal_off_on_straight`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `fix(plugin): follow-me car steers its wheels, leaves the path on an arc, signals turns`

---

### Task 5: Instance datarefs and the roof object

**Files:**
- Modify: `crates/atc-xplane-plugin/src/plugin.rs`, `tools/install-xplane-plugin.sh`
- Create: `crates/atc-xplane-plugin/assets/follow_me_roof.obj`, `crates/atc-xplane-plugin/assets/follow_me_roof.png` (generated by `tools/make-follow-me-texture.py`, committed), `tools/make-follow-me-texture.py`
- Test: unit tests for pure helpers in `plugin.rs` / `lib.rs`; manual check in X-Plane

**Interfaces:**
- Consumes: Task 4 `Pose { steer_deg, turn_signal, beacon_on }`.
- Produces:
  - Car instance created with datarefs `["sim/graphics/animation/ground_traffic/tire_steer_deg[0]", "sim/graphics/animation/ground_traffic/door_open[0]", "sim/graphics/animation/ground_traffic/door_open[1]"]`; per frame `[steer_deg, 0, 0]`.
  - Roof object `follow_me_roof.obj` (OBJ8, `TEXTURE follow_me_roof.png`): a sign box about 1.2 × 0.35 × 0.3 m on the roof (crew_car roof at y ≈ 1.45 m) with "FOLLOW ME" on both long sides (black on yellow checker texture), an amber beacon (`LIGHT_CUSTOM` or `LIGHT_PARAM` halo, shown via `ANIM_show/ANIM_hide` on `atc/follow_me/beacon`), and four amber turn lights at the car's corners (front ±0.76 m, z −2.34; rear ±0.69 m, z +2.28; y ≈ 0.9 m) shown via `atc/follow_me/left` / `atc/follow_me/right`. Its instance gets datarefs `["atc/follow_me/beacon", "atc/follow_me/left", "atc/follow_me/right"]` with 0/1 values computed from the pose and the blink phases (beacon 1 Hz, signals 1.5 Hz); same position/heading as the car.
  - Object path: the plugin's own folder (`XPLMGetPluginInfo` / `XPLMGetMyID` path → `…/atc-overlay/objects/follow_me_roof.obj`); `fn roof_object_path(plugin_xpl: &Path) -> PathBuf` (pure, tested). If the instancing API refuses custom dataref names, register them with `XPLMRegisterDataAccessor` (read-only ints) — check the XPLMInstance docs and record which was needed in a code comment.
  - Installer copies `assets/follow_me_roof.{obj,png}` to `Resources/plugins/atc-overlay/objects/`.
- [ ] **Step 1: Tests:** `roof_object_path_is_next_to_the_plugin`; `blink_phases` (beacon on/off at 1 Hz, signals at 1.5 Hz, signals off when `Signal::Off`); a parser-free sanity test that the committed OBJ starts with `I\n800\nOBJ\n` and references the PNG.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS; `tools/install-xplane-plugin.sh` runs.
- [ ] **Step 5: Docs:** README plugin section — the car has a FOLLOW ME sign, beacon and turn signals; reinstall the plugin to get the object.
- [ ] **Step 6: Commit** `feat(plugin): follow-me sign, beacon and turn signals; car wheels steered by the plugin`
- [ ] **Step 7: Manual acceptance (user):** EPKK, request taxi to stand with follow-me — wheels steady on straights, smooth corners, signals before corners, beacon flashing, sign readable, car pulls aside forward and stops facing the stand heading.

---

**Calls made in this plan (not from the user):**
- Arc radius 25 m, 10° minimum corner, 2 m sampling; signal 40 m before corners over 30°; blink 1 Hz / 1.5 Hz.
- Reminder uses the existing "read back <items>" phrase; movement threshold 3 kt.
- The sign/beacon/signals are a separate roof object instance over `crew_car.obj` rather than a new car model.
- Polish phrases for the destination as listed in Global Constraints.
- Departure case (user confirmed 2026-10-29 the aircraft had already reached the holding point): "follow the follow-me car" fires a fixed `READY_S` after "on the way" regardless of where the aircraft is. Added to Task 3: no car when the remaining route is under 150 m (answer with the normal taxi instruction instead), and `follow_me_update` drops the car silently (cancel order, no "follow…") when the aircraft is within 30 m of the path's end. Tests `no_car_for_a_short_route`, `no_follow_call_when_already_at_the_end`.
