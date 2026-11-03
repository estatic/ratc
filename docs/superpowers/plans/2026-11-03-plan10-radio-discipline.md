# Radio Discipline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One speaker per frequency (pilot or controller, first to talk wins), controller speech queued and revalidated, and readbacks/calls without a callsign accepted for an unambiguous aircraft with a callsign reminder from the 3rd time.

**Architecture:**
- **Engine (`atc-core`)**
  - Infers the aircraft for a call without a callsign and counts those calls per strip.
  - Tags readback reminders so the server can check them again just before air time.
  - Takes a per-frequency busy/free input that holds the 10 s reminder wait.
- **Server (`atc-server`)**
  - A pure-logic `Floor` per frequency decides who may talk.
  - Each frequency gets one speaker task that drains a queue of controller outputs. Replies go to the front.
  - PTT and typed text are granted or refused by the floor.

**Tech Stack:** Rust 2024, tokio, serde. Tests: `cargo test`, using the `atc-voice` fakes (FakeTts makes 0.1 s of audio per word).

**Spec:** `docs/superpowers/specs/2026-11-03-radio-discipline-design.md`. Read it first; its "Decisions after the builder check" section is binding.

## Global Constraints

- One speaker per frequency. A pilot's PTT or text is refused (`ServerMsg::FrequencyBlocked`) while the controller's audio is playing, or while another pilot holds or is being processed on that frequency.
- A refused press is never transcribed or answered. Only a new `PttStart` after a `PttEnd` can be granted.
- The pilot floor runs from the granted `PttStart` until the transcript is handled. On the normal path that is once the engine's answer is queued at the front. On an STT error, an empty transcript, a low-confidence say-again (queued first) or a dropped job, it is released at once.
- The controller floor runs for audio duration + margin, 300 ms. Without audio, the duration is the word count / 2.5 words per second.
- The next controller transmission starts no sooner than a 1 s gap after any transmission ends.
- The floor is per (canonical) frequency, not per position. ATIS is excluded. Vehicle orders are not queued.
- The readback reminder fires 10 s after the frequency became free. While the frequency is busy it cannot fall due. The "aircraft started moving" trigger also waits for a free frequency.
- Revalidation drops only readback reminders: a reminder whose readback is no longer pending, or whose aircraft has transmitted since the reminder was generated. A dropped reminder can fall due again later.
- Callsign inference: exactly one aircraft is worked by that position on the frequency, and it is the one the position last spoke to (today's `repeat()` rule). An inferred call is handled as that aircraft's.
  - **1st and 2nd inferred call in a flight:** a correct readback (otherwise silent) gets `"<callsign>, roger"`.
  - **3rd and later:** the answer ends with `"use your callsign in every transmission"`. Polish: `"używaj znaku wywoławczego w każdej korespondencji"`.
- The miss counter counts every inferred transmission, a bare "roger" included. It lives on the strip, goes into `FlightHandover` and snapshots, and has serde default 0.
- English code and docs. The Russian guide stays under `docs/user-guide/ru/`. Never commit `config/server.toml`, `config/client.toml`, `track.csv` or `docs/.obsidian/`.

## Review Focus

1. A pilot who keys again within the processing window of their own previous transmission (broken-off call) is granted. Test in Task 3.
2. A client disconnecting while holding or processing the floor frees it. Without that, ATC is silent forever on that frequency. Test in Task 3.
3. Telemetry outputs queued during a pilot transmission still go out after the reply, in order. A deviation call must not be lost. Test in Task 4.
4. Two inferred-callsign candidates (two aircraft on Ground) still get "say again your callsign". Test in Task 1.
5. A reminder dropped at revalidation does not fire twice in a row: it re-arms 10 s later, and only if the readback is still pending. Test in Task 2.

---

### Task 1: Callsign inference, miss counter, callsign reminder phrase

**Files:**
- Modify:
  - `crates/atc-phraseology/src/types.rs` (new `AtcItem::UseCallsign`)
  - `crates/atc-phraseology/src/render.rs` and `crates/atc-phraseology/src/pl/render.rs`
  - `crates/atc-core/src/strip.rs` (field)
  - `crates/atc-core/src/handover.rs` (field)
  - `crates/atc-core/src/engine/mod.rs` (`on_transmission`'s `_ =>` arm, a helper extracted from `repeat()`)
- Test: `crates/atc-core/tests/engine.rs`, plus render unit tests in each render file.

**Interfaces:**
- Produces:
  - `AtcItem::UseCallsign` (unit variant).
  - `Strip::callsign_misses: u32` with `#[serde(default)]`.
  - `FlightHandover::callsign_misses: u32` with `#[serde(default)]`, exported and imported with the strip.
  - Private `fn only_addressed(&self, role: Role) -> Option<usize>`, the existing `repeat()` rule, which `repeat()` now calls.

- [ ] **Step 1: Write the failing tests** in `crates/atc-core/tests/engine.rs`. Use the existing helpers (`engine()`, `say`, `items`, `taxi_cleared`, `readback`).
  - `readback_without_callsign_is_accepted_with_roger`:
    - Given: taxi clearance issued to SP-ABC (the only aircraft on Ground). The pilot reads it back correctly without a callsign.
    - Assert: `items(&out) == vec![AtcItem::Roger]`, the output's `message.to` is SP-ABC's callsign, and the readback is no longer pending, so a second identical readback gets no reminder.
  - `third_call_without_callsign_adds_use_callsign`: three callsign-less transmissions handled for the same aircraft. The 1st and 2nd outputs contain no `UseCallsign`. The 3rd output's last item is `AtcItem::UseCallsign`, in one transmission (one `AtcOutput`).
  - `callsign_less_call_with_two_aircraft_says_again_callsign`: two aircraft are on Ground and the last addressed is one of them, but two are worked there. Output: `[AtcItem::SayAgainCallsign { .. }]`, counters unchanged.
  - `fragment_with_callsign_still_joins`: the existing broken-off-transmission tests keep passing unchanged. Run them, do not edit them.
  - Render tests: EN `"November Sierra Papa, use your callsign in every transmission"` (via the normal callsign prefix). PL item text `"używaj znaku wywoławczego w każdej korespondencji"`.
  - Handover round-trip: `callsign_misses == 3` survives export → import. An old JSON without the field deserializes to 0.
- [ ] **Step 2:** Run `cargo test -p atc-core -p atc-phraseology`. Expected: the new tests FAIL. The existing tests that assert `SayAgainCallsign` for a single-aircraft callsign-less readback (around lines 1335, 1371, 1414, 1596) may need updating. Update only those whose scenario now meets the inference rule, and note each one in the report.
- [ ] **Step 3: Implement.**
  - In the `_ =>` arm of `on_transmission`, try `only_addressed(role)` before storing the fragment.
  - On a hit:
    - hear the text as that aircraft's;
    - increment `callsign_misses`;
    - run the normal handling;
    - if the result is empty, answer `[Roger]`;
    - if `callsign_misses >= 3`, append `UseCallsign` to the last output's items and re-render it, or to the `[Roger]`.
  - On a miss, behaviour is unchanged.
- [ ] **Step 4:** Run `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(engine): calls without a callsign are taken for the one aircraft in the exchange; reminder from the 3rd time`.

### Task 2: Engine frequency busy input and revalidated readback reminders

**Files:**
- Modify:
  - `crates/atc-core/src/engine/mod.rs` (`AtcOutput`, `readback_reminder`, new methods)
  - `crates/atc-core/src/strip.rs` (counter of heard transmissions)
  - the 7 `AtcOutput { .. }` literals (engine, approach, `network.rs`, tests)
  - `crates/atc-server/src/network.rs` (fan-out methods)
- Test: `crates/atc-core/tests/engine.rs`, `crates/atc-server/tests/network.rs`.

**Interfaces:**
- Produces:
  - `pub struct Reminder { pub callsign: Callsign, pub heard: u64 }` (Clone, Debug, PartialEq).
  - `AtcOutput::reminder: Option<Reminder>`: `Some` only on readback reminders.
  - `Strip::heard: u64`: incremented for every transmission attributed to the aircraft. Skipped in snapshots (`#[serde(skip)]`).
  - `AtcEngine::set_frequency_busy(&mut self, freq_khz: u32, busy: bool)`.
  - `AtcEngine::reminder_still_due(&self, r: &Reminder) -> bool`.
  - `AtcEngine::reminder_dropped(&mut self, r: &Reminder)`.
  - `Network::set_frequency_busy(&self, freq_khz: u32, busy: bool)`: every station and unit engine.
  - `Network::reminder_still_due(&self, r: &Reminder) -> bool` and `Network::reminder_dropped(&self, r: &Reminder)`: routed to the flight's owner engine.

- [ ] **Step 1: Write the failing tests.**
  - `reminder_waits_while_frequency_busy`: taxi clearance at t=0, `set_frequency_busy(ground, true)`. Telemetry at t=25 gives no reminder. `set_frequency_busy(ground, false)` at t=25. No reminder at t=30. Reminder at t=36: 10 s after free plus the existing 10 s rule, so assert it fires at ≥ t=35 and not before.
  - `moving_trigger_waits_for_free_frequency`: the aircraft moves at 5 kt with the taxi readback pending while busy, and there is no reminder. Once free, the reminder comes at the next sample.
  - `reminder_output_is_tagged`: the reminder `AtcOutput` has `reminder == Some(Reminder { callsign, heard })`. Other outputs have `None`.
  - `reminder_not_due_after_readback`: `reminder_still_due` is false once the readback was received.
  - `reminder_not_due_after_any_transmission`: false after any transmission from that aircraft, e.g. "say again".
  - `dropped_reminder_rearms_once`: after `reminder_dropped`, no reminder for 10 s. Then exactly one, and only if the readback is still pending.
  - Network test: `set_frequency_busy` reaches the owning station's engine. Use a two-station fixture from `tests/network.rs`.
- [ ] **Step 2:** Run `cargo test -p atc-core -p atc-server`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - The engine keeps `busy: HashSet<u32>`.
  - In `readback_reminder`, a strip whose `readback_role` frequency is busy gets `None`.
  - On busy→free, every strip with a pending readback on that role gets `readback_due_s = max(due, now)` and `readback_issued_s = now`, so the 10 s wait counts from the free moment.
  - `reminder_dropped` clears `reminded` and sets `readback_due_s = Some(now)`.
- [ ] **Step 4:** Run `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(engine): readback reminders wait for a free frequency and are revalidated before air`.

### Task 3: Floor (pure logic) and radio timing

**Files:**
- Create: `crates/atc-server/src/floor.rs` (module doc, unit tests inside).
- Modify:
  - `crates/atc-server/src/lib.rs` (mod)
  - `crates/atc-server/src/app.rs` (`AppState::radio: RadioTiming`, `Server::floor: Mutex<Floor>`)
  - the three `AppState { .. }` sites: `src/main.rs`, `tests/common/mod.rs`, `crates/atc-client/tests/common/server.rs`

**Interfaces:**
- Produces:
  - `pub struct RadioTiming { pub gap: Duration, pub margin: Duration, pub audio_scale: f64 }`.
    - `Default`: 1 s, 300 ms, 1.0.
    - `RadioTiming::instant()`: 0, 0, 0.0, for tests, so existing ws tests keep their timing.
  - `pub struct Floor`:
    - `pilot_press(&mut self, freq: u32, id: ClientId, now: Instant) -> PttOutcome`
    - `pilot_release(&mut self, freq: u32, id: ClientId)`: the floor stays the pilot's, now processing.
    - `pilot_done(&mut self, freq: u32, id: ClientId, now: Instant) -> bool`: true when it freed the floor.
    - `atc_may_start(&self, freq: u32, now: Instant) -> Option<Instant>`: `None` means start now; `Some(t)` means not before `t`; pilot-held is `Some(far future)`, and the caller waits for a change notification.
    - `atc_start(&mut self, freq: u32, now: Instant, airtime: Duration, timing: &RadioTiming)`
    - `atc_end_at(&self, freq: u32) -> Option<Instant>`
    - `leave(&mut self, id: ClientId) -> Vec<u32>`: frequencies freed.
    - `is_busy(&self, freq: u32, now: Instant) -> bool`

- [ ] **Step 1: Write the failing unit tests** in `floor.rs`, with explicit `Instant`s:
  - `pilot_press_on_free_frequency_is_granted`
  - `pilot_press_while_atc_talks_is_blocked`: ATC started at t0 with 2 s airtime and 300 ms margin; a press at t0+1 s is blocked, a press at t0+2.4 s is granted.
  - `pilot_press_during_gap_is_granted`
  - `other_pilot_blocked_while_first_is_processing`
  - `same_pilot_rekeys_while_processing`
  - `atc_waits_for_pilot_then_gap`: `pilot_done` at t → `atc_may_start(t)` is `Some(t+1 s)`, and at t+1 s it is `None`.
  - `leave_frees_held_and_processing_floor`
  - `atis_frequency_untouched`: nothing to test if ATIS never calls the floor; instead assert floors are independent per frequency.
- [ ] **Step 2:** Run `cargo test -p atc-server floor`. Expected: FAIL (module missing).
- [ ] **Step 3: Implement** `Floor` with a `HashMap<u32, FreqState>` and `RadioTiming`. Add the `AppState` field at all three construction sites: production `RadioTiming::default()`, both test commons `RadioTiming::instant()`.
- [ ] **Step 4:** Run `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(server): per-frequency floor - one speaker at a time`.

### Task 4: Speaker queue and wiring PTT, text and the pipeline to the floor

**Files:**
- Modify:
  - `crates/atc-server/src/pipeline.rs`
    - replace `emit_all`/`emit` with queueing;
    - `voice()` and `transmission()` release the pilot floor on every exit, through a drop guard.
  - `crates/atc-server/src/app.rs`
    - speaker queues: one `VecDeque<AtcOutput>` per frequency;
    - one speaker task per frequency, started on first use;
    - a `tokio::sync::Notify` for floor changes.
  - `crates/atc-server/src/session.rs`
    - `on_ptt_start` asks the floor first, then the hub;
    - `on_ptt_end` → `pilot_release`;
    - `TextTransmission` is refused when busy;
    - `leave` → `floor.leave`;
    - telemetry outputs → `enqueue_back`.
  - `crates/atc-client/src/app.rs`: the blocked message becomes `"[ptt] frequency {freq} busy - wait, then press again"`, for text too.
- Test: `crates/atc-server/tests/ws.rs` (new tests use `RadioTiming::default()` with a FakeTts, so 0.1 s/word gives real airtime).

**Interfaces:**
- Consumes:
  - Task 3: `Floor`, `RadioTiming`.
  - Task 2: `AtcOutput::reminder`, `Network::{set_frequency_busy, reminder_still_due, reminder_dropped}`.
- Produces:
  - `Server::enqueue_front(&self, outputs: Vec<AtcOutput>)`: replies, batch order kept, ahead of queued items.
  - `Server::enqueue_back(&self, outputs: Vec<AtcOutput>)`
- Speaker loop per frequency:
  1. Pop the front item and synthesize it (TTS may run while waiting).
  2. Wait until `atc_may_start` is `None`.
  3. If `reminder.is_some()` and `!reminder_still_due`, call `reminder_dropped` and skip the item.
  4. Otherwise `atc_start` with the audio duration × `audio_scale` (no audio: words / 2.5 s), then `remember_said`, `AtcText`, the audio, and `set_frequency_busy(freq, true)`.
  5. Sleep until `atc_end_at`, then `set_frequency_busy(freq, false)`.
- Pilot side:
  - a granted press → `set_frequency_busy(freq, true)`;
  - `pilot_done` returning true → `set_frequency_busy(freq, false)`, plus a notify.

- [ ] **Step 1: Write the failing ws tests.**
  - `ptt_while_atc_speaks_is_blocked_and_not_transcribed`: text request → while Ground's taxi clearance airtime runs, PTT → client gets `FrequencyBlocked`. The FakeStt's queued reply is not consumed: assert `prompts` is empty.
  - `atc_waits_for_pilot_transmission`: client A holds PTT. A telemetry deviation output for that frequency is not sent until after `PttEnd` + processing. It is never received while the PTT is held.
  - `reply_goes_before_queued_telemetry_output` (Review Focus 3): both are received, the reply first.
  - `reminder_dropped_when_readback_arrives_first`: clearance, a pilot readback started before the 10 s → no `"read back"` text is ever received.
  - `typed_text_while_atc_speaks_is_blocked`
  - `disconnect_frees_floor` (Review Focus 2): client A presses PTT and disconnects. Client B's request is answered.
  - Existing ws and client tests stay green with `RadioTiming::instant()`.
- [ ] **Step 2:** Run `cargo test -p atc-server --test ws`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement** as in the Interfaces block. `emit_all` callers:
  - the transcript path → `enqueue_front`, then `pilot_done`;
  - the say-again path → `enqueue_front`, then `pilot_done`;
  - telemetry → `enqueue_back`.
- [ ] **Step 4:** Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`. Expected: PASS, no warnings.
- [ ] **Step 5:** Commit: `feat(server): controller speech waits its turn; PTT and text respect the floor`.

### Task 5: Docs

**Files:** `docs/RUNNING.md`, `README.md` (if it describes radio behaviour), `docs/user-guide/ru/scenario-circuit.md` and `polish-circuit.md` where they cover readback and PTT.

- [ ] **Step 1:** Document the rules.
  - One speaker per frequency.
  - "frequency busy": wait, then press again.
  - The controller answers after you finish.
  - A readback without a callsign is accepted twice, then you are reminded.

  Write it in English in RUNNING.md and in Russian in the RU guide.
- [ ] **Step 2:** Commit: `docs: one speaker per frequency; callsign reminders`.
