# Plan 14a: Flight Test Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix what the first in-sim EPKK→EPKT flight exposed: radio after the sim is gone,
position routing, same-frequency hand-off, recognition tolerance, stands, Tower on final, small
UX and the guide.

**Architecture:** Point fixes in the existing layers: the client sim adapter and app (B4,
B11), the protocol (one new `ClientMsg`), the server session/hub (untune) and STT prompt, the
engine (`atc-core`: routing, hand-off, readback, Tower, stands), the phraseology parser and
normaliser (`atc-phraseology`), navdata (stand attributes, nav DB check), the wizard
(`atc-stations`) and docs.

**Tech Stack:** Rust 2024 workspace; tokio; whisper-rs prompt strings; TOML replay scenarios.

**Spec:** `docs/superpowers/specs/2026-11-11-flight-test-fixes-design.md`

## Global Constraints

- Full test run: `cargo test --workspace --features atc-core/testkit --no-fail-fast`; plus
  `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --features atc-core/testkit -- -D warnings`.
- Two `atc-replay` tests fail locally only because `data/airports/EPKK/station.toml` is
  git-ignored: run replay tests and regenerate goldens (`UPDATE_GOLDEN=1`) in a clean worktree
  (`git worktree add`), never commit goldens produced with the local station.toml.
- Replay scenarios exist only for EPKK (the only station tracked in git); EPKT exchanges are
  reproduced as engine tests with fixture airports.
- English in code, comments and docs; Russian only under `docs/user-guide/ru/`, kept in sync
  with `docs/user-guide/en/`.
- No simulator names in `atc-core`, `atc-server`, `atc-proto` or the client outside
  `crates/atc-client/src/sim/` (guard test must stay green).
- Never commit `config/server.toml`, `config/client.toml`, `track.csv`, `docs/.obsidian/`,
  station data under `data/airports` other than EPKK's tracked files, or secrets.
- Match surrounding code: doc comments on every pub item, the existing comment density and
  naming; new `AtcItem`s get English rendering and Polish rendering (or the existing English
  fallback group in `pl/render.rs`).
- Protocol: adding a `ClientMsg` variant bumps `PROTOCOL_VERSION` from 2 to 3.

## Review Focus

1. Sim lost while the pilot holds PTT, or while a controller transmission is playing: playback
   stops, the PTT is released on the server (no frequency stuck "busy"). Test in Task 1.
2. A request on the right frequency addressed to a position that is closed (Delivery outside
   its hours, the request then served by Ground): no redirect loop between two positions. Test
   in Task 2.
3. Fuzzy callsign with two aircraft on the frequency: never guessed. Test in Task 7.
4. "Any stand" when every suitable stand is taken: "unable, no stand available, say stand"
   rather than a de-icing stand or a heavy gate. Test in Task 10.
5. Tower's unprompted landing clearance when the runway is occupied: no clearance, and no
   repeated "go around" every telemetry tick. Test in Task 11.

---

### Task 1: Simulator lost (B4)

**Files:**
- Modify: `crates/atc-client/src/sim/mod.rs` (`SimUpdate`)
- Modify: `crates/atc-client/src/sim/xplane/mod.rs:~315-345` (`run_xplane` silence branch)
- Modify: `crates/atc-client/src/app.rs:~249-283` (`App::sim`), `crates/atc-client/src/playback.rs`
- Modify: `crates/atc-proto/src/lib.rs` (`ClientMsg`, `PROTOCOL_VERSION`)
- Modify: `crates/atc-server/src/session.rs` (`on_msg` dispatch, new `on_sim_lost`)

**Interfaces:**
- Produces: `SimUpdate::Lost` (unit variant, doc: "The sim stopped sending data after it had sent some; the next `Telemetry` means it is back."); `ClientMsg::SimLost` (serde tag `"sim_lost"`); `Playback::stop_all(&mut self)`; `PROTOCOL_VERSION = 3`.

- [ ] **Step 1: Write failing tests**
  - `atc-proto`: `sim_lost_round_trips` — `serde_json::to_string(&ClientMsg::SimLost)` is `{"type":"sim_lost"}` and parses back.
  - `atc-client` xplane: `silence_after_data_reports_lost_once` — with a fake UDP peer that sends one RREF packet then nothing, and `silence_timeout` 50 ms, the receiver gets `Telemetry`… then exactly one `SimUpdate::Lost` within 200 ms (no second `Lost` after a further 200 ms); `silence_before_any_data_reports_nothing`.
  - `atc-client` playback: `stop_all_silences_a_looping_clip` — after `stop_all`, mixing yields silence and a looping ATIS clip does not restart.
  - `atc-client` app: `lost_sim_turns_radios_off` — feeding `SimUpdate::Lost` sends `ClientMsg::SimLost`, prints `X-Plane lost - radios off until it is back` (the line uses `self.sim.name`), clears `heard`/`com1`; the next `Telemetry` prints `X-Plane connected, ...` again.
  - `atc-server` session: `sim_lost_untunes_and_releases_ptt` — a client tuned to the ATIS frequency and holding PTT sends `SimLost`: `hub.listeners(atis_khz)` no longer contains it, its PTT is ended (the frequency is free for another client), and a later `Telemetry` re-tunes it and restarts the ATIS for it.

- [ ] **Step 2: Run them, verify they fail** — `cargo test -p atc-proto -p atc-client -p atc-server sim_lost silence_ stop_all lost_sim` → compile errors / FAIL.

- [ ] **Step 3: Implement**
  - `run_xplane`: track `had_data: bool` (set on a decoded packet) and `lost_sent: bool`; on timeout with `had_data && !lost_sent` send `SimUpdate::Lost` and set `lost_sent`; reset `lost_sent` on the next decoded packet. Keep resubscribing.
  - `App::sim(Lost)`: `playback.stop_all()`, `set_heard(&[])`, `self.com1 = None`, `self.heard.clear()`, print the line, `send(ClientMsg::SimLost)`.
  - Server `on_sim_lost`: `hub.tune(id, &[])`, `self.tuned.clear()`, end this client's PTT if held (the same path as `PttEnd` without a transcription job).
  - Bump `PROTOCOL_VERSION` to 3; update any test asserting `2`.

- [ ] **Step 4: Run the tests, verify they pass**, then the full suite (Global Constraints).

- [ ] **Step 5: Commit** — `git commit -m "client, server: stop the radio when the simulator is lost"`

---

### Task 2: Position routing (B1, B2)

**Files:**
- Modify: `crates/atc-core/src/engine/mod.rs:815-825` (`answer`, `called_elsewhere` at ~1689), `:871-906` (pending readback block)
- Test: `crates/atc-core/tests/engine.rs` (new `mod position_routing`), `scenarios/epkk-wrong-addressee.toml` + `.expected.txt`

**Interfaces:**
- Consumes: `is_request(&PilotIntent) -> bool`, `AtcEngine::redirect`, `AtcItem::ThisIs`.
- Produces: `called_elsewhere` returns `Some` only for a message with no request (`!is_request(&msg.intent)`).

- [ ] **Step 1: Write failing tests** (EPKK fixtures, Delivery open):
  - `request_to_wrong_position_is_served_here`: on the Delivery frequency, "Krakow Ground, SPABC, Cessna 172, stand 9R, information Alpha, IFR destination Katowice, request clearance" → first items `[ThisIs { station: "Kraków Delivery" }, ClearedTo { .. }, ..]`, no `Contact`.
  - `bare_call_to_other_position_is_still_redirected`: "Krakow Ground, SPABC" on Delivery → `[ThisIs{..}, Contact { station: "Kraków Ground", khz: 118105 }]` (unchanged behaviour).
  - `request_after_redirect_is_answered`: bare call redirected as above, then (without reading back) "Krakow Delivery, SPABC, IFR destination Katowice, request clearance" → a clearance, not `ReadBack([Frequency])`; `handoff_to` cleared.
  - `closed_delivery_no_redirect_loop` (Review Focus 2): Delivery closed, the clearance request "Krakow Delivery, ..." on Ground → Ground issues the clearance (existing behaviour), and the same request on Ground addressed "Krakow Ground" → the same; never a `Contact` to Delivery.
  - Replay `scenarios/epkk-wrong-addressee.toml`: the flight's own three Delivery transmissions (B1/B2) at EPKK, golden shows the clearance on the first.

- [ ] **Step 2: Run, verify FAIL** — `cargo test -p atc-core --features testkit position_routing`.

- [ ] **Step 3: Implement** — In `answer`: when the message is a request naming another open position, prepend `ThisIs` (this position) to whatever the normal handling returns (role handlers still redirect by intent where this role does not serve it); `called_elsewhere` keeps the redirect for non-requests only. In the pending-readback block: when `strip.handoff_to.is_some()`, the pending items are only `Contact`, and the message is a request → drop the pending readback and `handoff_to`, then continue normal handling.

- [ ] **Step 4: Run, verify PASS**; full suite; regenerate the new golden in a clean worktree.

- [ ] **Step 5: Commit** — `git commit -m "core: serve a request made to the wrong position; a request overrides a pending redirect"`

---

### Task 3: Silent hand-off within one frequency (B7)

**Files:**
- Modify: `crates/atc-core/src/engine/approach.rs:~1056-1072` (hand-off emission), `:1119` (`hand_off`), `handed_in_call` (~210)
- Modify: `crates/atc-core/src/handover.rs` (`FlightHandover`), `crates/atc-core/src/engine/mod.rs:359-418` (`export_flight`, `import_flight`)
- Modify: `crates/atc-core/src/strip.rs` (one flag)
- Test: `crates/atc-core/tests/engine.rs` (`mod same_frequency_handoff`), `crates/atc-server/src/network.rs` tests

**Interfaces:**
- Produces: `FlightHandover.radar_contact: bool` (`#[serde(default)]`; doc: "Handed over silently by the same unit on the same frequency: already identified, no first call expected."); `Strip.radar_contact_kept: bool`.

- [ ] **Step 1: Write failing tests**
  - Engine: `same_unit_handoff_is_silent` — departure station's Approach (`Kraków Approach`, 121075) and a destination Approach unit with the same callsign and khz: when the hand-off point is reached, no output is transmitted, `handoff_target(cs) == Some(121075)` immediately, `handoff_airport(cs) == Some("EPKT")`.
  - Engine: `different_unit_handoff_still_spoken` — destination Approach on another frequency → `Contact` as today.
  - Engine: `imported_with_radar_contact_continues_unprompted` — `import_flight` with `radar_contact: true` (destination = this airport, IFR): the next telemetry sample produces the arrival instruction (route / descent, same items `handed_in_call` gives) without `Identified`; a later pilot call is not answered with `Identified`.
  - Server network: `same_frequency_handoff_moves_flight_without_call` — two stations sharing Approach 121075; the flight moves to the destination station on the tick after the hand-off point with no transmission in between.

- [ ] **Step 2: Run, verify FAIL.**

- [ ] **Step 3: Implement** — In the Approach hand-off emission: when the target unit's `khz` equals this Approach position's `khz` and its callsign equals this position's callsign, set `handed_off`, `handed_off_khz`, `handed_off_airport`, `handoff_acked = true`, mark the strip `radar_contact_kept`, transmit nothing. `export_flight` carries `radar_contact = strip.radar_contact_kept`. `import_flight` with `radar_contact`: `identified = true`, `first_call_due = false`, `handed_in = false`, and the next `arrival_update` issues the handed-in arrival instruction without `Identified` (refactor `handed_in_call` so its instruction part can run without a `PilotMessage`, using the cleared level).

- [ ] **Step 4: Run, verify PASS**; full suite.

- [ ] **Step 5: Commit** — `git commit -m "core: a hand-off to the same unit on the same frequency is silent"`

---

### Task 4: Destination after IFR/VFR (B3)

**Files:**
- Modify: `crates/atc-phraseology/src/parse.rs:~782` (keyword dispatch), `:837` (`destination_at`), `:874` (`place_at`)
- Test: `crates/atc-phraseology/src/parse.rs` tests; `scenarios/epkk-real-calls.toml` (append the flight's calls) + golden

**Interfaces:**
- Produces: no new pub API; `HeardItems.destination` is set in the cases below.

- [ ] **Step 1: Write failing tests** (vocab with places Katowice=EPKT):
  - `ifr_2_place_is_destination`: "IFR 2 Katowice, request clearance" → `destination == Some("EPKT")`.
  - `ifr_glued_to_place_is_destination`: "IFR Tokatowice, request clearance" → `Some("EPKT")`.
  - `ifr_two_place_is_destination`: "IFR two Katowice" → `Some("EPKT")`.
  - `clearance_with_one_place_takes_it`: "IFR Katowice, request clearance" → `Some("EPKT")`.
  - `stand_number_is_not_destination`: "stand 2, IFR to Katowice" → `Some("EPKT")` and the stand stays 2.
  - Replay: the flight's four Delivery calls ("IFR 2 Katowice" ×2, "IFR Katowice", "IFR, destination Katowice") each get the clearance.

- [ ] **Step 2: Run, verify FAIL** — `cargo test -p atc-phraseology destination`.

- [ ] **Step 3: Implement** — After `ifr`/`vfr`, skip one of `2` (number token), `two`, `to`, `do`, then `destination_at`; in `place_at`, a word starting with `to` whose remainder is a known place name matches that place; for a clearance request with no destination found, a single known place token not used by another item is the destination.

- [ ] **Step 4: Run, verify PASS**; full suite; golden in clean worktree.

- [ ] **Step 5: Commit** — `git commit -m "phraseology: destination after IFR/VFR despite 'to' mis-heard as 2 or glued"`

---

### Task 5: Normalising observed Whisper output (B13a)

**Files:**
- Modify: `crates/atc-phraseology/src/normalize.rs` (English)
- Test: same file's tests; `scenarios/epkk-real-calls.toml`

**Interfaces:** none new.

- [ ] **Step 1: Write failing tests** (assert on parsed `HeardItems` through `parse`):
  - "cleared ILS approach one way two seven" → approach runway `27`.
  - "turn left heading two nine-year five" and "two ninea five" → heading 295.
  - "Contact Katowice Tower 129, decimal 255" and "one two ninea decimal two five five" → `freqs_khz == [129255]`.
  - "establish ILS runway two seven" → `PilotIntent::ReportEstablished` (or the intent today's "established ILS runway two seven" yields — assert equality with that).
  - "Horshirt runway two five" → hold short `25`.
  - "Stand 9-Ride" and "stand nine ride" → stand `9R`.

- [ ] **Step 2: Run, verify FAIL.**
- [ ] **Step 3: Implement** — word-level rewrites before tokenising: `one way`→`runway` only when followed by a number; `ninea`, `nine-year`, `nineyear` → `nine`; `establish` → `established`; `horshirt`, `hold-short` → `hold short`; `ride` → `right` after a stand number or `9-ride` style token; frequency join: `<3 digits>[,] decimal <3 digits>` → one frequency.
- [ ] **Step 4: Run, verify PASS**; full suite.
- [ ] **Step 5: Commit** — `git commit -m "phraseology: normalise Whisper mis-hearings seen in flight"`

---

### Task 6: Clean readback answered "say again" (B9)

**Files:**
- Modify: as found (likely `crates/atc-core/src/engine/mod.rs:700-740` hearing/fragment join, or `readback.rs`)
- Test: `crates/atc-core/tests/engine.rs` (`mod readback_after_garble`)

**Interfaces:** none new.

- [ ] **Step 1: Write the reproduction test** `clean_readback_after_garbled_ones_is_accepted` — Approach hands N172SP (abbreviated "November Sierra Papa") to "Katowice Tower" 129255; then the pilot transmissions exactly as flown, in order: "Contact Katowice Tower one two ninea decimal two five five November Sierra Papa", "Contact Katowice Tower 129, decimal 255 November Sierra Papa", "Conta Katowice-Krácow Tower", "Contact Katowice Tower 129, decimal two five five, November Sierra Papa", "Contact Katowice Tower, one two nine decimal two five five, November Sierra Papa". Assert: no output of the run contains `AtcItem::SayAgain`, and the hand-off is answered (`handoff_target == Some(129255)`) by the end. Also `clean_readback_after_fragment_alone`: only the fragment then the clean readback → accepted.
- [ ] **Step 2: Run, verify FAIL**; use superpowers:systematic-debugging to find which transmission produces `SayAgain` and why (record the root cause in the commit message).
- [ ] **Step 3: Fix the root cause** — a transmission that alone is a correct readback is never answered "say again"; a garbled fragment never spoils a correct readback.
- [ ] **Step 4: Run, verify PASS**; full suite.
- [ ] **Step 5: Commit** — `git commit -m "core: a correct readback after garbled ones is accepted (<root cause>)"`

---

### Task 7: Near-miss callsign of the only aircraft (B13b)

**Files:**
- Modify: `crates/atc-core/src/engine/mod.rs` (hearing: `hear`/`only_addressed` path)
- Test: `crates/atc-core/tests/engine.rs` (`mod near_callsign`)

**Interfaces:**
- Produces: `fn near_callsign(heard: &str, known: &str) -> bool` (private, in the module that matches callsigns): true when the spoken forms differ by exactly one letter or digit (substitution), compared on the abbreviated and full forms.

- [ ] **Step 1: Write failing tests**
  - `one_letter_off_is_the_only_aircraft`: N172SP working Approach, "using my callsign in every transmission November Sierra Bravo" → answered as N172SP (not `SayAgainCallsign`).
  - `one_letter_off_with_two_aircraft_is_not_guessed` (Review Focus 3): N172SP and N172SR both working the frequency; "November Sierra Bravo" (one letter off both) → `SayAgainCallsign`.
  - `two_letters_off_is_not_taken`: "November Charlie Bravo" → `SayAgainCallsign`.
- [ ] **Step 2: Run, verify FAIL.**
- [ ] **Step 3: Implement** — when the heard callsign matches no aircraft, and exactly one aircraft on this frequency's exchange is a `near_callsign`, take it (counts like a callsign miss? no — it named a callsign; do not add "use your callsign").
- [ ] **Step 4: Run, verify PASS**; full suite.
- [ ] **Step 5: Commit** — `git commit -m "core: a callsign one letter off is the only aircraft it can be"`

---

### Task 8: Whisper prompt from expectations (B13c)

**Files:**
- Modify: `crates/atc-server/src/app.rs:~600-612` (`stt_prompt`)
- Modify: `crates/atc-server/src/network.rs` (new `stt_hints`), `crates/atc-core/src/engine/mod.rs` (new `stt_hints`)
- Test: tests in those files

**Interfaces:**
- Produces: `AtcEngine::stt_hints(&self, cs: &Callsign, freq_khz: u32) -> Vec<String>` — the speaker's callsign spoken full and abbreviated (e.g. "November one seven two Sierra Papa", "November Sierra Papa"), and when `freq_khz` is a Ground position, the airport's stand names in spoken form ("Apron four Golf Alpha"), at most 20 stands, nearest the aircraft first. `Network::stt_hints(&self, freq_khz: u32, from: &Callsign) -> Vec<String>` delegates to the owning station's engine (empty for units).
- Consumes: the spoken-form rendering already used for callsigns in `atc-phraseology`.

- [ ] **Step 1: Write failing tests**
  - Engine: `hints_carry_own_callsign` — N172SP on Approach → contains both spoken forms.
  - Engine: `ground_hints_carry_stands` — at EPKK on Ground, hints contain a stand spoken form; on Tower they do not.
  - Server: `prompt_includes_hints_and_stays_short` — `stt_prompt` contains "November Sierra Papa" and its whitespace-split word count is ≤ 170 (proxy for the 224-token window) with 60 stands loaded.
- [ ] **Step 2: Run, verify FAIL.**
- [ ] **Step 3: Implement** — `stt_prompt` = static prompt + hints joined with ", " + last controller transmission; trim hints (stands first) to keep within the word cap.
- [ ] **Step 4: Run, verify PASS**; full suite.
- [ ] **Step 5: Commit** — `git commit -m "server: Whisper prompt carries the speaker's callsign and the stands"`

---

### Task 9: Stand matching tolerance (B5)

**Files:**
- Modify: `crates/atc-core/src/engine/stand_match.rs`
- Test: same file's tests

**Interfaces:** none new (existing matcher entry point keeps its signature).

- [ ] **Step 1: Write failing tests** — stands `["1", "14", "APRON4 GA", "APRON6 GA", "40"]`; each of "a prone for Golf Alpha", "Upron for GALPHA", "Aparon 4, Gulf Alpha", "Apron for Golf Alpha", "apron four golf alpha" matches `APRON4 GA`; "stand 1-4" matches `14`; "stand for" alone does not match `APRON4 GA` (ambiguous); "apron six golf alpha" matches `APRON6 GA`.
- [ ] **Step 2: Run, verify FAIL** — `cargo test -p atc-core --features testkit stand_match`.
- [ ] **Step 3: Implement** — before comparing: phonetic words → letters (golf/gulf→G, alpha/alfa→A, all ICAO words), `galpha`→`GA`, apron mis-hearings (`a prone`, `upron`, `aparon`, `apron`) → `APRON`, `for`/`four` → `4` inside a stand phrase, `1-4` → `14`; then exact match; else the unique stand at edit distance 1 on the normalised string.
- [ ] **Step 4: Run, verify PASS**; full suite.
- [ ] **Step 5: Commit** — `git commit -m "core: stand names match through phonetic and Whisper mis-hearings"`

---

### Task 10: "Any stand" (B6)

**Files:**
- Modify: `crates/atc-navdata/src/model.rs:93` (`Stand`), `crates/atc-navdata/src/apt.rs:160` (row 1300)
- Modify: `crates/atc-phraseology/src/parse.rs` (intent/item for "any stand")
- Modify: `crates/atc-core/src/engine/ground.rs` (taxi-in request), new `crates/atc-core/src/engine/stand_pick.rs`
- Test: in those files; engine test with a fixture airport

**Interfaces:**
- Produces: `Stand.kind: StandKind` (`Gate | TieDown | Hangar | Misc`, `#[serde(default)]` = `Misc`) and `Stand.categories: Vec<StandCategory>` (`Heavy | Jets | Turboprops | Props | Helos | Fighters`; empty = all; `#[serde(default)]`), parsed from apt.dat 1300 fields 4 (type) and 5 (`|`-separated categories, `all` = empty); `HeardItems.any_stand: bool`; `fn pick_stand(&self, i: usize) -> Option<String>` in `stand_pick.rs`; `fn stand_category(aircraft_type: &str) -> StandCategory` (props for ICAO types starting `C1`, `C2`, `PA`, `SR2`, `DA4`, `DA2`, `BE3`, `BE9`… listed in a const; `Jets` otherwise).

- [ ] **Step 1: Write failing tests**
  - Navdata: `stand_row_keeps_type_and_categories` — `1300 50.47 19.08 270.0 tie_down props APRON4 GA` → kind `TieDown`, categories `[Props]`, name `APRON4 GA`; `... gate heavy|jets 81` → `[Heavy, Jets]`; `all` → `[]`.
  - Parser: "request taxi to any available spot", "any stand", "any parking" → `any_stand`.
  - Engine `any_stand_assigns_free_ga_stand` — C172 after landing asks any stand: taxi clearance to the nearest free stand accepting `Props`, preferring `TieDown`/`Misc` over `Gate`; never a stand whose name contains `DE-ICE`/`DEICE` (case-insensitive); never one occupied (an aircraft track within `ON_STAND_M`) or assigned to another strip.
  - Engine `any_stand_none_free` (Review Focus 4) — all suitable stands taken → `[Unable(..) , SayStand]` (reuse the existing unable reason for an unknown/unavailable stand).
- [ ] **Step 2: Run, verify FAIL.**
- [ ] **Step 3: Implement** as in Interfaces; Ground handles `any_stand` like a named stand once picked.
- [ ] **Step 4: Run, verify PASS**; full suite.
- [ ] **Step 5: Commit** — `git commit -m "navdata, core: stand type and categories; Ground assigns any free stand"`

---

### Task 11: Tower answers an IFR arrival before final (B10)

**Files:**
- Modify: `crates/atc-core/src/engine/tower.rs:~296-310` (`landing`), Tower telemetry path
- Modify: `crates/atc-phraseology/src/types.rs`, `render.rs`, `pl/render.rs` (`AtcItem::ContinueApproach`, `AtcItem::OffFinal`)
- Test: `crates/atc-core/tests/engine.rs` (`mod tower_ifr_final`); replay `scenarios/epkk-ifr-arrival-star-ils25.*` golden may change

**Interfaces:**
- Produces: `AtcItem::ContinueApproach` ("continue approach"); `AtcItem::OffFinal { nm: u16, side: Compass8 }` ("you are <nm> miles <north> of the final approach course") — use the existing compass/side type if one exists in `types.rs`, else add `Compass8` with eight points.

- [ ] **Step 1: Write failing tests**
  - `established_beyond_final_zone_continue_approach`: IFR arrival on the centreline 12 NM out calls "Katowice Tower, N172SP established ILS runway two seven" → `[ContinueApproach, DistanceToGo(12)]`, not `Report(Final)`.
  - `final_call_off_course_gets_position`: arrival 4 km north of the centreline, 6 NM out, says "final" → `[OffFinal { nm: 2, side: North }]` (nm rounded).
  - `ifr_arrival_cleared_unprompted_on_final`: once the phase becomes `Circuit(Final)` with the runway free, Tower transmits `ClearedLand` with no pilot call; exactly once.
  - `unprompted_clearance_not_repeated_when_runway_busy` (Review Focus 5): runway occupied when the arrival enters final → no transmission while it stays occupied (no `GoAround` per tick); once it frees with the arrival still on final → `ClearedLand` exactly once.
- [ ] **Step 2: Run, verify FAIL.**
- [ ] **Step 3: Implement** — in `landing` for a not-yet-final IFR arrival: on the centreline (within the final half-width) beyond the final zone → `ContinueApproach` + `DistanceToGo`; off it → `OffFinal`. Tower telemetry: on the transition into `Circuit(Final)` for an IFR arrival with no landing permission, issue the landing clearance (same items as `landing`) if the runway is free; if not, wait and issue it once it frees while still on final.
- [ ] **Step 4: Run, verify PASS**; full suite; regenerate affected goldens in a clean worktree and check the diff reads right.
- [ ] **Step 5: Commit** — `git commit -m "core: Tower tells an IFR arrival where it is and clears it on final unprompted"`

---

### Task 12: PTT message and navigation database warning (B11)

**Files:**
- Modify: `crates/atc-client/src/app.rs:~330-340` (PTT off line)
- Modify: `crates/atc-navdata/src/source.rs` (`SimDataSource`), the X-Plane source implementation in `atc-navdata`, `crates/atc-server/src/main.rs` (startup)
- Test: in those files

**Interfaces:**
- Produces: `SimDataSource::gps_warnings(&self, icaos: &[String]) -> Vec<String>` (default method returning empty); X-Plane impl: if `<root>/Custom Data/earth_424.dat` exists, for each ICAO without `<root>/Custom Data/CIFP/<ICAO>.dat`, one line: `X-Plane Custom Data replaces the default navigation data and has no procedures for <ICAO>: the aircraft GPS will list no departures or arrivals there. Move "Custom Data" aside or install a navigation database that covers <ICAO>.`

- [ ] **Step 1: Write failing tests**
  - Client: `refused_ptt_says_frequency_busy` — after a refused PTT start, PTT off prints `[ptt] off - nothing sent (frequency was busy)`; a normal press without audio still prints the microphone hint.
  - Navdata: `custom_data_without_cifp_warns` (temp dir with `Custom Data/earth_424.dat` and no CIFP) → one warning naming EPKK; `no_custom_data_no_warning`; `custom_data_with_cifp_no_warning`.
- [ ] **Step 2: Run, verify FAIL.**
- [ ] **Step 3: Implement**; the server logs each warning with `tracing::warn!` at start-up for the loaded stations.
- [ ] **Step 4: Run, verify PASS**; full suite (sim-neutral guard included).
- [ ] **Step 5: Commit** — `git commit -m "client: clear message for a refused PTT; server: warn when the sim GPS data lacks a station"`

---

### Task 13: Wizard UX (3 items)

**Files:**
- Modify: `crates/atc-stations/src/wizard/create.rs` (area list), `crates/atc-stations/src/wizard/mod.rs` (menu), `crates/atc-stations/src/wizard/update.rs` (countries)
- Test: in those files

**Interfaces:**
- Produces: menu entry `Remove a country` (placed before `Quit`); list suffix ` (station)`.

- [ ] **Step 1: Write failing tests**
  - `area_list_marks_existing_stations` — with a station dir for EPKT, its row ends with ` (station)` and is not preselected (`[ ]`).
  - `airfield_without_layout_is_not_selectable` — a heliport without X-Plane layout is shown like a closed airfield and choosing its number is refused with the existing "cannot be built" wording.
  - `remove_country_stops_downloading_it` — after removing `DE`, the stored country list no longer contains it and update does not fetch it; its cached data file is deleted; stations are untouched.
- [ ] **Step 2: Run, verify FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, verify PASS**; full suite.
- [ ] **Step 5: Commit** — `git commit -m "wizard: mark existing stations, grey out unbuildable airfields, remove a country"`

---

### Task 14: Guide — callsign, G1000, navigation database

**Files:**
- Modify: `docs/user-guide/en/scenario-ifr-departure.md`, `docs/user-guide/ru/scenario-ifr-departure.md`

- [ ] **Step 1: Write** in both languages, same structure:
  - "Your callsign": taken from the sim's tail number; override with `callsign = "..."` in `config/client.toml`; how it is abbreviated on the radio.
  - "Flying it with the G1000": FPL → enter EPKT; PROC → Select Departure → SUDO2J (runway 25) → Load; after take-off NAV (follows the SID), ALT/VS for levels with the altitude table already in the guide; above the transition altitude STD BARO when cleared to a flight level; on the ILS: tune the localizer, CDI to LOC, APR.
  - "Empty departure list": the Custom Data pitfall and the start-up warning text from Task 12.
  - Mention that a hand-off to the same Approach is silent (Task 3) and that Tower clears an IFR arrival to land on final without a "final" call (Task 11).
- [ ] **Step 2: Verify** — headings and steps correspond 1:1 between EN and RU (`diff <(grep '^#' en/...) <(grep '^#' ru/...)` shows only translated text, same count).
- [ ] **Step 3: Commit** — `git commit -m "docs: guide on callsign, G1000 and the sim navigation database"`
