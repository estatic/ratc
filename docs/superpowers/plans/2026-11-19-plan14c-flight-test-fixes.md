# Flight test fixes 14c Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the radio-exchange findings C1–C12 of the 2026-11-19 EPKK→EPKT flight.

**Architecture:** Point fixes in the client PTT source (debounce), the server pipeline (short
audio, superseded replies), the English parser (`atc-phraseology`), the engine
(`atc-core/src/engine`) and the compliance monitor. Each finding gets a unit/engine test that
reproduces the exchange from the flight log.

**Tech Stack:** Rust workspace (cargo test, clippy), gilrs, tokio.

**Spec:** `docs/superpowers/specs/2026-11-19-flight-test-fixes-14c-design.md`

## Global Constraints

- English only in code and docs, except `docs/user-guide/ru/`.
- Simulator names stay out of server, core and the client's main part (the sim-neutral guard test).
- No user station data under `data/airports` other than EPKK's tracked files; engine tests use the testkit/EPKK fixtures.
- Every fixed exchange gets a test; new engine tests go in `crates/atc-core/tests/engine.rs` as a `mod` per finding (pattern: `mod near_callsign`).
- Before each commit: `cargo test --workspace -q` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
- Branch `fix/flight-test-14c`; PR into `main`, never push to `main`.

## Review Focus

- A joystick held steadily for a long transmission (no bounce) still ends ~300 ms after release, once — Task 1 test `steady_hold_releases_once_after_window`.
- A deliberate quick double press (release, press again after 0.5 s) is two transmissions — Task 1 test `press_after_window_is_new_transmission`.
- A reply already on the air is never cut by the pilot's next transmission (only queued items are dropped) — Task 2 test `drop_keeps_other_aircraft_and_own_transmissions`.
- "heading" followed by a Break and a non-heading number ("heading, one zero miles") is not a heading — Task 3 test `heading_break_needs_three_digits`.
- A departing aircraft at its stand saying "stand nine, request start-up" is still a start-up request, not a taxi-to-stand — Task 3 test `stand_with_request_keeps_request`.

---

### Task 1: PTT release debounce (C1, client)

**Files:**
- Modify: `crates/atc-client/src/ptt.rs` (new `ReleaseDebounce`, used by `spawn_joystick`)
- Modify: `crates/atc-client/src/app.rs:345-358` (`ptt_release` message)

**Interfaces:**
- Produces: `pub struct ReleaseDebounce`, `pub const RELEASE_DEBOUNCE: Duration = Duration::from_millis(300)`,
  `impl ReleaseDebounce { pub fn new() -> Self; pub fn on_edge(&mut self, down: bool, now: Instant) -> Option<bool>; pub fn deadline(&self) -> Option<Instant>; pub fn poll(&mut self, now: Instant) -> Option<bool> }`
  — `on_edge` returns the edge to forward (`Some(true)` press, `Some(false)` never; a release is only armed); a press while a release is armed cancels it and returns `None`; `poll` returns `Some(false)` once `now >= deadline`.
- Produces: `pub const MIN_TRANSMISSION: Duration = Duration::from_millis(500)` in `ptt.rs` (used by Task 2 too, via the same value on the server: `MIN_PILOT_SAMPLES = PILOT_RATE / 2`).

- [ ] **Step 1: Write failing tests in `ptt.rs` `mod tests`**

```rust
#[test]
fn bounce_inside_window_is_one_transmission() {
    let t0 = Instant::now(); let ms = Duration::from_millis;
    let mut d = ReleaseDebounce::new();
    assert_eq!(d.on_edge(true, t0), Some(true));
    assert_eq!(d.on_edge(false, t0 + ms(2000)), None);
    assert_eq!(d.poll(t0 + ms(2100)), None);
    assert_eq!(d.on_edge(true, t0 + ms(2150)), None); // bounce: same transmission
    assert_eq!(d.on_edge(false, t0 + ms(5000)), None);
    assert_eq!(d.poll(t0 + ms(5299)), None);
    assert_eq!(d.poll(t0 + ms(5300)), Some(false));
    assert_eq!(d.poll(t0 + ms(6000)), None);
}
#[test]
fn steady_hold_releases_once_after_window() { /* press, release at 10 s, deadline() == 10.3 s, poll → Some(false) once */ }
#[test]
fn press_after_window_is_new_transmission() { /* release at 1 s, poll(1.3 s) Some(false), press at 1.5 s → Some(true) */ }
```

- [ ] **Step 2: Run** `cargo test -p atc-client ptt::tests` — Expected: FAIL (`ReleaseDebounce` not found).
- [ ] **Step 3: Implement `ReleaseDebounce`; in `spawn_joystick` feed button edges through it, use `next_event_blocking(Some(min(200 ms, deadline - now)))` and send `PttInput::Joystick(false)` when `poll` fires.** Keyboard toggle unchanged.
- [ ] **Step 4: In `ptt_release`, when `0 < samples < PILOT_RATE / 2` print `[ptt] off - too short, nothing sent`** (the server drops it, Task 2). Other messages unchanged.
- [ ] **Step 5: Run** `cargo test -p atc-client` — Expected: PASS.
- [ ] **Step 6: Commit** `client: one transmission across a PTT button bounce; too-short press not sent`

### Task 2: Server drops short audio and superseded replies (C1 server, C2)

**Files:**
- Modify: `crates/atc-server/src/pipeline.rs` (`voice`, `transmission`)
- Modify: `crates/atc-server/src/app.rs` (new `Server::drop_queued_replies`)

**Interfaces:**
- Produces: `pub(crate) fn drop_queued_replies(&self, freq_khz: u32, to: &Callsign) -> usize` — removes from `freq_khz`'s queue every `Queued` with `reply == true` and `out.message.to == Some(to)`; returns how many. Items on the air were already popped (`take_on_air`), so they are never affected.
- Produces: `const MIN_PILOT_SAMPLES: usize = (PILOT_RATE / 2) as usize` in `pipeline.rs`.

- [ ] **Step 1: Write failing unit tests in `app.rs` tests:** `drop_removes_queued_replies_to_aircraft` (enqueue_front two replies to N172SP and one to SPTST on 121805 → drop returns 2, `queue_front` is the SPTST item) and `drop_keeps_other_aircraft_and_own_transmissions` (an `enqueue_back` item to N172SP stays).
- [ ] **Step 2: Run** `cargo test -p atc-server drop_` — Expected: FAIL.
- [ ] **Step 3: Implement `drop_queued_replies`; call it in `voice` right after the empty-transcript check (before the say-again branch) and at the start of `transmission`** (log `tracing::info!(dropped, "superseded replies dropped")` when > 0).
- [ ] **Step 4: In `voice`, return before STT with `tracing::info!("transmission too short: dropped")` when `pcm.len() < MIN_PILOT_SAMPLES`.** Add a test if `voice` has a test seam with a fake STT (`grep -n "FakeStt\|impl SpeechToText" crates/atc-server`); else the constant's use is covered by review.
- [ ] **Step 5: Run** `cargo test -p atc-server` — PASS.
- [ ] **Step 6: Commit** `server: a newer transmission supersedes queued replies; sub-0.5 s audio dropped`

### Task 3: Parser fixes (C3, C4, C9 parser side)

**Files:**
- Modify: `crates/atc-phraseology/src/normalize.rs` (teens)
- Modify: `crates/atc-phraseology/src/parse.rs` (`"heading"`/`"altitude"` arms ~750, `find_callsign` ~1171, stand handling ~1406, intent selection)
- Modify: `crates/atc-phraseology/src/types.rs` (`PilotIntent::ReportStand`, `PilotMessage::short_callsign`)
- Test: `crates/atc-phraseology/tests/parse.rs`

**Interfaces:**
- Produces: `PilotIntent::ReportStand { stand: String }` — the message's only content besides station/callsign/greeting is "stand <designator>" (stand string in the same form as `RequestTaxiToStand.stand`, e.g. `"stand 11"`).
- Produces: `PilotMessage.short_callsign: Option<String>` — when `callsign` is `None`, an unused run of exactly 2 letters/digits after the addressee (e.g. `"SP"`).

- [ ] **Step 1: Write failing tests** (exact transcripts from the flight):
  - `heading_after_comma`: `"Turn left heading, two nine five, one zero miles from touchdown, cleared ILS approach, runway two seven, report established, November Sierra Papa"` → `items.headings == [295]`, `approaches == [(Ils, "27")]`, `intent == Readback`.
  - `heading_break_needs_three_digits`: `"turn left heading, one zero miles"` → `headings` empty.
  - `teens_are_numbers`: `normalize("stand eleven")` ends `[Word("stand"), Number("11")]`; `"nineteen"` → `Number("19")`; `"ten"` → `Number("10")`.
  - `stand_number_is_not_callsign`: `"Katowice Ground, runway vacated, request taxi, stand 11, November Sierra Papa."` → `callsign == Some("NSP")`, `intent == RequestTaxiToStand { stand: Some("stand 11") }`.
  - `stand_alone_is_report_stand`: `"Katowice Ground, stand eleven, November Sierra Papa."` and `"November Sierra Papa, stand eleven."` → `ReportStand { stand: "stand 11" }`, callsign `NSP`.
  - `stand_with_request_keeps_request`: `"Krakow Ground, SPABC, stand nine, request start-up"` → `RequestStartUp { .. }`.
  - `two_letter_callsign_is_short`: `"Katowice Tower, Sierra Papa."` → `callsign == None`, `short_callsign == Some("SP")`.
- [ ] **Step 2: Run** `cargo test -p atc-phraseology --test parse` — FAIL.
- [ ] **Step 3: Implement:** "ten".."nineteen" → "10".."19" in `normalize` (as whole numbers, not digit runs: "one one" stays "11" by the existing digit join, "eleven" joins nothing after it); `heading`/`altitude` arms read `self.num(self.skip_break(i + 1))` and mark through the number; a Number token right after a stand keyword belongs to the stand (marked used before `find_callsign`); `ReportStand` when no other intent is found and a stand was heard; `short_callsign` as defined. Polish parser untouched except where the new field/variant must be filled (`None` / not produced).
- [ ] **Step 4: Run** `cargo test -p atc-phraseology` — PASS (existing tests unchanged).
- [ ] **Step 5: Commit** `phraseology: heading after a comma, teen numbers, stand number outside the callsign, stand-only report`

### Task 4: Ground stand for arrivals (C4, C5 engine side)

**Files:**
- Modify: `crates/atc-core/src/engine/ground.rs` (`taxi_to_stand` ~250-270, `ground_request`)
- Test: `crates/atc-core/tests/engine.rs` new `mod arrival_stand`

**Interfaces:**
- Consumes: `PilotIntent::ReportStand { stand }` (Task 3); existing `taxi_to_any_stand(i)`, `taxi_to_stand(i, Some(stand))`.

- [ ] **Step 1: Write failing tests** reusing the B6 helpers (`engine_with_ga_stands`, `taxiing_in_near_stand_1`):
  - `vacated_request_taxi_assigns_a_stand`: `"{CS}, runway vacated, request taxi"` → first item `AtcItem::Taxi { to: TaxiTo::Stand { name }, .. }` with `name == "GA 1"` (no `SayStand`).
  - `stand_report_after_say_stand_is_taxi_request`: aircraft taxiing in, `"{CS}, stand golf alpha two"` → `Taxi { to: Stand { name: "GA 2" } }`.
  - `stand_report_at_departure_stand_is_not_taxi`: parked departure, `"Krakow Ground, {CS}, stand five"` → no `Taxi` item.
- [ ] **Step 2: Run** `cargo test -p atc-core --test engine arrival_stand` — FAIL.
- [ ] **Step 3: Implement:** in `taxi_to_stand`, `None` stand with no `home_stand` → `self.taxi_to_any_stand(i)`; in `ground_request`, `ReportStand { stand }` for an aircraft in `Landed | TaxiIn` (or with a vacated/arrival strip) → `self.taxi_to_stand(i, Some(stand.clone()))`, otherwise empty (falls to the normal content answer). Delivery/Tower redirect lists: `ReportStand` behaves like `RequestTaxiToStand`.
- [ ] **Step 4: Run** `cargo test -p atc-core` — PASS.
- [ ] **Step 5: Commit** `core: Ground assigns a stand to an arrival that names none; a stand-only call answers`

### Task 5: Acknowledgements and follow-me readback (C6, C7, C8)

**Files:**
- Modify: `crates/atc-core/src/engine/mod.rs` (`content` ~1422, fragment join ~745-790 if C8's cause is there)
- Test: `crates/atc-core/tests/engine.rs` new `mod acknowledgements`

**Interfaces:**
- Produces: `fn homophone(w: &str) -> &str` in `engine/mod.rs` — `"for" → "four"`, `"to" → "two"`, `"card" → "car"`, else `w`; `was_told` compares `homophone(w)` too (both sides).

- [ ] **Step 1: Write failing tests** (follow-me helpers: see `scenarios/epkk-follow-me.toml` / existing follow-me engine tests):
  - `station_and_roger_is_acknowledgement`: after "follow the follow-me car to stand X", `"Krakow Ground, Roger, {CS}"` → no output (no `PassYourMessage`).
  - `station_and_greeting_is_still_a_call`: `"Krakow Ground, good morning, {CS}"` with nothing told → `PassYourMessage` (unchanged behaviour).
  - `follow_me_readback_with_homophones`: `"Krakow Ground, follow the follow me card to stand for {CS}"` after "follow the follow-me car to stand 4"-equivalent at EPKK → no `SayAgain`.
  - `check_heading_ack_after_garble`: Approach says `CheckHeading(340)`; pilot `"check heading, now we've missed your papa."` (→ `SayAgain`), then `"check heading November Sierra Papa."` → no `SayAgain` (callsign N172SP, abbreviated NSP; use `taxi_cleared_as`-style registration of N172SP airborne on Approach — follow the `heading_deviation` tests around engine.rs:7066).
- [ ] **Step 2: Run** `cargo test -p atc-core --test engine acknowledgements` — FAIL. For `check_heading_ack_after_garble`, add a `dbg!` of the joined text in the fragment path to confirm the cause before fixing; remove it after.
- [ ] **Step 3: Implement:** in `content`, `called && acked && !greeted_only` → `Acknowledgement` (rule: station named + only ack/told words → acknowledgement; station + greeting without ack → call); `homophone` in `was_told`; for C8 the fix the reproduction points to (expected: a joined fragment is used only when `joined_says_more`, and an acknowledgement alone is never replaced by a joined message).
- [ ] **Step 4: Run** `cargo test -p atc-core` — PASS.
- [ ] **Step 5: Commit** `core: "Ground, roger" acknowledges; follow-me readback homophones; check-heading ack after a garble`

### Task 6: Callsign without its first letter (C9 engine side)

**Files:**
- Modify: `crates/atc-core/src/engine/mod.rs` (`near_callsign` ~2159, the `near` lookup ~752)
- Test: `crates/atc-core/tests/engine.rs` `mod near_callsign`

**Interfaces:**
- Consumes: `PilotMessage.short_callsign` (Task 3).

- [ ] **Step 1: Write failing tests** in `mod near_callsign`:
  - `first_letter_dropped_is_the_only_aircraft`: N172SP working Tower (not last addressed, `n172sp_not_last_addressed` pattern on Tower), `"Krakow Tower, Sierra Papa."` → `out[0].message.to == Some(N172SP)`, no `SayAgainCallsign`.
  - `first_letter_dropped_ambiguous_asks`: two aircraft ending "SP" on the frequency → `SayAgainCallsign`.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement:** `near_callsign` also true when `heard` (≥ 2 chars) equals the full or abbreviated callsign without its first character; the `near` lookup uses `callsign.or(short_callsign)`.
- [ ] **Step 4: Run** `cargo test -p atc-core` — PASS.
- [ ] **Step 5: Commit** `core: a callsign heard without its first letter is the only aircraft it fits`

### Task 7: No heading/level check during the readback (C10)

**Files:**
- Modify: `crates/atc-core/src/compliance.rs` (new `hold_clearance_checks`)
- Modify: `crates/atc-core/src/engine/mod.rs` (~655-665 telemetry update; readback acceptance ~997)
- Modify: `crates/atc-core/src/strip.rs` (`readback_done_at: Option<f64>`)
- Test: `crates/atc-core/src/compliance.rs` tests; `crates/atc-core/tests/engine.rs` near line 7066

**Interfaces:**
- Produces: `pub fn hold_clearance_checks(&mut self)` on the monitor — resets the heading and level latches' `since` (they cannot fire this sample).
- Produces: `pub const REACTION_S: f64 = 10.0` in `compliance.rs`.
- Produces: `Strip.readback_done_at: Option<f64>`, set where the readback is accepted (`readback_complete` call site) to `self.now`, and to the transmit time for an instruction that asks no readback.

- [ ] **Step 1: Write failing tests:** compliance unit `held_checks_do_not_fire` (assigned 340, actual 262 mag for 60 s, `hold_clearance_checks()` every sample → no `HeadingDeviation`; then 30 s without hold → one); engine `heading_check_waits_for_readback`: "turn right heading 340" issued, aircraft keeps 262 for 35 s while the pilot's first readback has a wrong QNH ("read back QNH"), correct readback at 36 s → no `CheckHeading` before 36 + 10 + 30 s, one after if still off.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement:** in the telemetry update, call `sv.monitor.hold_clearance_checks()` when `busy` (readback pending) or `now < readback_done_at + REACTION_S`.
- [ ] **Step 4: Run** `cargo test -p atc-core` — PASS (existing heading/level tests may need the readback marked done; adjust their setup, not their expectations).
- [ ] **Step 5: Commit** `core: heading and level checks start after the readback and a reaction time`

### Task 8: "check altimeter setting" (C11)

**Files:**
- Modify: `crates/atc-phraseology/src/types.rs` (`AtcItem::CheckAltimeter { qnh_hpa: u16, level: Level }`), `render.rs` (~442), `pl/render.rs` (~405 English list)
- Modify: `crates/atc-core/src/engine/mod.rs` (deviation handler ~1807)
- Test: `crates/atc-phraseology/tests/render.rs`, `crates/atc-core/tests/engine.rs`

**Interfaces:**
- Produces: `pub fn altimeter_error_ft(qnh_hpa: f64) -> f64 { (qnh_hpa - 1013.25) * 27.3 }` in `compliance.rs` — the true-minus-indicated error of a pilot flying on standard pressure.

- [ ] **Step 1: Write failing tests:** render `CheckAltimeter { qnh_hpa: 992, level: Level::Altitude(3000) }` → `"check altimeter setting, QNH zero niner niner two, maintain altitude three thousand feet"`; engine `std_altimeter_gets_altimeter_check` (QNH 992 weather, cleared altitude 3000, aircraft level at 2455 ft for 30+ s off the circuit → `CheckAltimeter { qnh_hpa: 992, .. }`, no `CheckLevel`); `plain_level_bust_is_check_level` (QNH 992, level at 3400 → `CheckLevel`).
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement:** for an off-circuit `AltitudeDeviation` with `Level::Altitude` and `|altimeter_error_ft(qnh)| >= 200` and `|(actual - assigned) - altimeter_error_ft(qnh)| <= 100` → `CheckAltimeter`; QNH from the engine's current weather.
- [ ] **Step 4: Run** `cargo test --workspace -q` — PASS.
- [ ] **Step 5: Commit** `core: a level bust that matches standard pressure gets "check altimeter setting"`

### Task 9: Delivery corrects an old ATIS letter (C12)

**Files:**
- Modify: `crates/atc-phraseology/src/types.rs`, `parse.rs`, `pl/parse.rs` (`RequestClearance { atis: Option<char>, .. }`)
- Modify: `crates/atc-core/src/engine/delivery.rs` (`delivery_request` / `clearance`)
- Test: `crates/atc-phraseology/tests/parse.rs`, `crates/atc-core/tests/engine.rs`

- [ ] **Step 1: Write failing tests:** parse `"Kraków Delivery November one seven two Sierra Papa, Cessna one seven two stand nine right information Alpha IFR to Katowice request clearance."` → `RequestClearance { atis: Some('A'), destination: Some("EPKT"-form as today), .. }`; engine `delivery_corrects_old_atis`: current ATIS Bravo, clearance request with Alpha → items start `AtisCurrent('B'), Qnh(..)` then the clearance; with Bravo → no `AtisCurrent`.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement:** fill `atis` from the parser's `self.atis` (Polish: from "informacja X" if the Polish parser already reads it, else `None`); in Delivery, a heard letter different from the current one prepends `AtisCurrent` + `Qnh` (same rule as Ground's `stale`, but only when a letter was heard).
- [ ] **Step 4: Run** `cargo test --workspace -q` — PASS.
- [ ] **Step 5: Commit** `core: Delivery corrects an old ATIS letter`

### Task 10: Replay scenario and final check

**Files:**
- Create: `scenarios/epkk-flight-test-14c.toml`, `scenarios/epkk-flight-test-14c.expected.txt`

- [ ] **Step 1: Write an EPKK arrival scenario** (start from `scenarios/epkk-ifr-arrival-vectors.toml`) containing, after landing on 25: Tower's "vacate, contact Ground" read back with a comma heading-style transcript earlier on Approach (`"Turn left heading, two two zero, ..."`), "runway vacated, request taxi" (stand assigned), "Krakow Ground, Roger, ..." after follow-me request (no reply), and the follow-me readback with "card"/"for".
- [ ] **Step 2: Generate expected output** with the replay binary (`cargo run -q -p atc-replay -- scenarios/epkk-flight-test-14c.toml`, see how existing `.expected.txt` files are produced/compared in `crates/atc-replay`), review it line by line against the spec, save as `.expected.txt`.
- [ ] **Step 3: Run** `cargo test --workspace -q && cargo clippy --workspace --all-targets -- -D warnings` — PASS.
- [ ] **Step 4: Commit** `replay: EPKK arrival with the 14c exchanges`
