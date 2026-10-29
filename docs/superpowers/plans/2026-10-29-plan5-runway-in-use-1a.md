# Roadmap 1a (rest): runway in use, departure-runway request, amended clearance, flight rules, airborne IFR clearance — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish roadmap item 1a (parts a, b, c, f and "say flight rules"); (d) and (e) are already done on this branch (09b519e, 45b5253).

**Architecture:** All changes are in `atc-phraseology` (new intents, items and their EN/PL renders) and `atc-core` (ATIS runway choice, strip/plan fields, Delivery/Ground/Approach handlers). One strip-level notion is added: the flight's **departure runway** = the runway the pilot was granted on request, else the runway in use. Every place that today reads `runway_in_use()` for a departing flight reads the departure runway instead.

**Tech Stack:** Rust workspace (edition as in `Cargo.toml`), `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt`.

**Spec:** `docs/superpowers/specs/2026-10-25-roadmap.md`, item 1a.

## Global Constraints

- Branch `insim-fixes`; one commit per task; `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` green before each commit.
- Tailwind limit **5 kt** (PANS-ATM ch. 7.2): `pub const MAX_TAILWIND_KT: f64 = 5.0;` in `crates/atc-core/src/atis.rs`.
- Every new `AtcItem` / `UnableReason` is rendered in English (`crates/atc-phraseology/src/render.rs`) **and** Polish (`crates/atc-phraseology/src/pl/render.rs`), with the existing runway/procedure word helpers of each file.
- New `Strip` / `FlightPlan` fields are `#[serde(default)]` so snapshots written by 45b5253 (`data/cache/state-<ICAO>.json`) still restore.
- Code and comments in English; doc comments in the style of the surrounding code.

## Review Focus

1. **Calm wind after a westerly** — runway 25 in use, wind goes calm or 3 kt variable: runway stays 25, ATIS letter does not change for the runway (Task 1 test `calm_keeps_runway_in_use`).
2. **Request for the runway already in use** — "request departure runway two five" while 25 is in use: approved, no amended clearance, no second SID (Task 3 test `request_runway_in_use_is_approved_without_change`).
3. **Runway request before any clearance** — the later IFR clearance uses the requested runway's SID (Task 3 test `clearance_after_runway_request_uses_its_sid`).
4. **Snapshot from before this plan** — a state file without the new fields restores (Task 3 test `old_snapshot_without_requested_runway_restores`).
5. **Airborne clearance request from an aircraft that has a plan** — not re-cleared, nothing changes (Task 5 test `airborne_clearance_with_plan_is_ignored`).

---

### Task 1: Runway in use stays until its tailwind exceeds 5 kt (1a a)

**Files:**
- Modify: `crates/atc-core/src/atis.rs` (`select_runway` ~l.67-85, `AtisService::new` / `update` ~l.135-165, tests module ~l.418+)

**Interfaces:**
- Produces: `pub const MAX_TAILWIND_KT: f64 = 5.0;` and `pub fn tailwind_kt(wind: &Wind, airport: &Airport, runway: &str) -> Option<f64>` (positive = tailwind, `Some(0.0)` when calm, `None` for an unknown runway ident) in `atis.rs`. Used by Task 3.

- [ ] **Step 1: Write the failing tests** in the `atis.rs` tests module (use the module's existing airport/procs helpers and runway idents of `runway_by_headwind`):
  - `calm_keeps_runway_in_use`: service created with wind 250/10 (→ 25); `update` with calm wind → `None` (no new letter) and `current().runway == "25"`; same with 070/03.
  - `light_tailwind_keeps_runway_in_use`: in use 25, update with 070/05 (5 kt tailwind on 25) → still `"25"`.
  - `tailwind_above_limit_switches_runway`: in use 25, update with 070/06 → `"07"`, a new letter.
  - `first_atis_unchanged`: `AtisService::new` with calm → preferred runway; with 070/10 → `"07"` (today's rule).
  - `tailwind_kt_signs`: `tailwind_kt(250/10, "07") ≈ 10.0`, `tailwind_kt(250/10, "25") ≈ -10.0`, `tailwind_kt(calm, "25") == Some(0.0)`, unknown ident → `None`.

- [ ] **Step 2: Run** `cargo test -p atc-core --lib atis` — Expected: the new tests FAIL (calm currently falls back to the preferred runway; `tailwind_kt` undefined).

- [ ] **Step 3: Implement.** `select_runway(wind, airport, procs, current: Option<&str>) -> String`: when `current` is `Some(r)` and `tailwind_kt(wind, airport, r)` is `Some(t)` with `t <= MAX_TAILWIND_KT`, return `r`; otherwise today's rule (best headwind ≥ `MIN_HEADWIND_KT`, else preferred). `new` passes `None`, `update` passes `Some(&self.current.runway)`. `tailwind_kt` reuses `headwind_kt` with the end's heading computed as in `select_runway` (negated).

- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS (fix any engine test that relied on calm → preferred after a switch; such a test is now wrong by the spec — adjust its expectation, say so in the commit body).

- [ ] **Step 5: Commit** `feat(core): runway in use stays until its tailwind exceeds 5 kt`

---

### Task 2: Delivery asks "say flight rules" when a request names neither IFR nor VFR (1a)

**Files:**
- Modify: `crates/atc-phraseology/src/types.rs` (`PilotIntent::RequestClearance` l.96-102; `AtcItem` add `SayFlightRules`), `crates/atc-phraseology/src/parse.rs` (`is_ifr` l.1038, l.1063), `render.rs`, `pl/render.rs`
- Modify: `crates/atc-core/src/engine/delivery.rs` (`delivery_request` l.77-95), `crates/atc-core/src/engine/ground.rs` (l.38-45)
- Test: `crates/atc-phraseology/tests/parse.rs` (l.530-560), `crates/atc-phraseology/tests/parse_pl.rs` (l.426), `crates/atc-core/tests/engine.rs` (`mod delivery`)

**Interfaces:**
- Produces: `pub enum FlightRules { Ifr, Vfr }` (derive `Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize`) in `types.rs`; `PilotIntent::RequestClearance { destination: Option<String>, rules: Option<FlightRules>, level_ft: Option<i32> }` (replaces `ifr: bool`); `AtcItem::SayFlightRules`.
- `clearance(...)` / `clearance_elsewhere(...)` keep their `ifr: bool` parameter.

- [ ] **Step 1: Write the failing tests.**
  - parse: existing cases become `rules: Some(FlightRules::Ifr)` / `Some(FlightRules::Vfr)`; new `clearance_request_without_rules`: `"Kraków Delivery, SP-TST, request clearance to Katowice"` → `rules: None`. Same for one Polish phrase in `parse_pl.rs` (`rules: None` when neither "IFR" nor "VFR" is said).
  - engine (`mod delivery`): `request_without_rules_gets_say_flight_rules` — the request above on `DEL` → `items == [AtcItem::SayFlightRules]`, strip has no plan, no pending readback; then the same request with "VFR" → a normal VFR clearance.
  - render: `AtcItem::SayFlightRules` → `"say flight rules"`; PL → `"podaj przepisy lotu"`.

- [ ] **Step 2: Run** `cargo test -p atc-phraseology -p atc-core` — Expected: compile errors / FAIL.

- [ ] **Step 3: Implement.** Parser: `rules` = `Ifr` when `is_ifr()`, `Vfr` when a new `is_vfr()` sees `"vfr"`, `"visual"` (only as "visual flight rules") or the letters `v f r`, else `None`. Delivery and Ground: `rules == None` → `vec![AtcItem::SayFlightRules]`; otherwise `ifr = rules == Some(Ifr)`. `ground.rs:35` (IFR clearance needed before taxi) is unchanged.

- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.

- [ ] **Step 5: Commit** `feat: Delivery asks 'say flight rules' when a request names neither IFR nor VFR`

---

### Task 3: Pilot requests a departure runway (1a b)

**Files:**
- Modify: `crates/atc-phraseology/src/types.rs` (`PilotIntent::RequestDepartureRunway { runway: String }`, `AtcItem::RunwayApproved(String)`, `UnableReason::RunwayInUse { runway: String }`), `parse.rs`, `render.rs`, `pl/render.rs`
- Modify: `crates/atc-core/src/strip.rs` (`Strip.requested_runway`), `crates/atc-core/src/engine/mod.rs` (`refresh_runway` l.1135, new `departure_runway`, intent routing ~l.1265), `delivery.rs` (`ifr_departure` l.191, `delivery_request`), `ground.rs` (request match l.37+)
- Modify fixture: `crates/atc-navdata/tests/fixtures/xplane/Resources/default data/CIFP/ZZZZ.dat` — add one SID `CHAR1A` for `RW09` (copy an `ALFA1A` line set, mirrored), so ZZZZ has a SID per runway end. Update any fixture test that counts SIDs.
- Test: `crates/atc-phraseology/tests/parse.rs`, `crates/atc-core/tests/engine.rs` (`mod delivery`), snapshot test next to the existing ones in `crates/atc-core/src/engine/snapshot.rs`

**Interfaces:**
- Consumes: `tailwind_kt`, `MAX_TAILWIND_KT` (Task 1).
- Produces: `Strip.requested_runway: Option<String>` (`#[serde(default)]`, cleared with the other per-flight state in the per-flight strip reset of 82786b8); `AtcEngine::departure_runway(&self, i: usize) -> String` (`pub(super)`, in `engine/mod.rs`) = `requested_runway` else `runway_in_use()`. Used by Task 4.

- [ ] **Step 1: Write the failing tests.**
  - parse: `"request departure runway zero seven"`, `"request runway zero seven for departure"` → `RequestDepartureRunway { runway: "07" }`; a Polish equivalent (`"prośba o pas zero siedem do startu"`) → same.
  - render EN: `RunwayApproved("07")` → `"runway zero seven approved"`; `Unable(RunwayInUse { runway: "25" })` → `"unable, runway in use two five"` (follow the existing `Unable` prefix convention of `render.rs`). PL: `"pas zero siedem, zgoda"`, `"niemożliwe, pas w użyciu dwa pięć"`.
  - engine (`mod delivery`, ZZZZ, runway in use 27):
    - `runway_request_within_tailwind_is_approved`: wind 270/03, request 09 on `DEL` → `[RunwayApproved("09")]`, `strip.requested_runway == Some("09")`.
    - `clearance_after_runway_request_uses_its_sid`: then the IFR request → SID is `CHAR1A` (spoken `CHAR one Alfa` per `spoken_designator`), `plan` records runway 09 (Task 4 field — here assert via the SID only).
    - `runway_request_above_tailwind_is_unable`: wind 270/08, request 09 → `[Unable(RunwayInUse { runway: "27" })]`, `requested_runway == None`.
    - `request_runway_in_use_is_approved_without_change`: request 27 → `[RunwayApproved("27")]`, nothing else.
    - `ground_taxis_to_requested_runway`: after the approval, taxi request on `GND` → taxi to the holding point of 09.
  - snapshot: `old_snapshot_without_requested_runway_restores` — a snapshot JSON with the field removed deserialises, `requested_runway == None`.

- [ ] **Step 2: Run** `cargo test --workspace` — Expected: FAIL.

- [ ] **Step 3: Implement.** Accepted on Delivery and Ground while the aircraft is `Parked` or `TaxiOut`; elsewhere ignored like other out-of-phase requests. Unknown runway ident → `Unable(RunwayInUse { runway: runway_in_use })`. Granted when `tailwind_kt(wind, airport, r) <= MAX_TAILWIND_KT` (no ATIS/wind yet → standby, as Tower does). `refresh_runway` and `ifr_departure` use `departure_runway(i)`. If a clearance was already issued for another runway, the approval also carries the amended clearance — that is Task 4; here only store the request.

- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.

- [ ] **Step 5: Commit** `feat: pilot may request a departure runway within 5 kt tailwind`

---

### Task 4: A runway change after the clearance is announced and read back (1a c)

**Files:**
- Modify: `crates/atc-core/src/strip.rs` (`FlightPlan.dep_runway: Option<String>`, `#[serde(default)]`), `crates/atc-phraseology/src/types.rs` (`AtcItem::RunwayChanged(String)`), `render.rs`, `pl/render.rs`, `crates/atc-core/src/readback.rs` (`item_readback`, doc table at the top), `delivery.rs` (`ifr_departure` sets `dep_runway`), `engine/mod.rs` (`refresh_runway`), `ground.rs` (`taxi_to_holding_point` l.99+), the Task 3 approval path
- Test: `crates/atc-core/tests/engine.rs` (`mod delivery`), `crates/atc-phraseology/tests/render.rs`, readback unit tests in `readback.rs`

**Interfaces:**
- Consumes: `departure_runway` (Task 3).
- Produces: `AtcItem::RunwayChanged(String)`; readback requires `ReadbackItem::Runway(r)` for it (all strictness levels).

- [ ] **Step 1: Write the failing tests.**
  - render: `[RunwayChanged("07"), Sid { fix: "SUDOL", number: 1, letter: 'F' }]` → `"runway changed, runway zero seven, SUDOL one Foxtrot departure"`; PL `RunwayChanged("07")` → `"zmiana pasa, pas zero siedem"`.
  - readback: `required_readback(&[RunwayChanged("07"), Sid{..}], Standard) == [Runway("07"), Sid(..)]`.
  - engine:
    - `runway_change_before_taxi_is_announced`: IFR clearance on 27 (SID ALFA1A/BRAV1A), read back; wind becomes 090/10 (tailwind 10 on 27 → 09 in use); taxi request → items start with `RunwayChanged("09")`, `Sid` = `CHAR1A`, then the taxi to the holding point of 09; pending readback contains `Runway("09")` and the SID; `plan.sid == "CHAR1A"`, `plan.dep_runway == Some("09")`.
    - `runway_request_after_clearance_amends_it`: IFR clearance on 27, wind 270/03, request 09 → `[RunwayApproved("09"), RunwayChanged("09"), Sid(CHAR1A)]` with readback.
    - `vfr_runway_change_has_no_sid`: VFR flight, same wind change → `RunwayChanged("09")` + taxi, no `Sid`.
    - `no_change_no_announcement`: clearance and taxi on the same runway → no `RunwayChanged` (existing flows untouched).

- [ ] **Step 2: Run** `cargo test --workspace` — Expected: FAIL.

- [ ] **Step 3: Implement.** In `refresh_runway(i)` return whether the strip's runway changed *and* the flight has a plan whose `dep_runway` (IFR) — or, for VFR, the strip runway at clearance — differs. On change with an IFR plan: re-run `choose_sid` for the new runway (same destination, same cruise), update `plan.sid`, `plan.dep_runway`, `plan.cleared_ft`; prepend `RunwayChanged(new)` + `Sid` + `ClimbTo` when the initial climb changed. No SID for the new runway → `RunwayChanged` + the existing `Unable(NoIfrData)` handling is **not** used; instead keep the old runway for this flight (log it) — the flight taxis to the runway it was cleared for. `taxi_to_holding_point` prepends the items to its clearance; `expect_readback` covers the whole transmission.
  Tower (`tower.rs::departure`) is unchanged: once a flight has taxied, it departs from the runway on its strip (my call, see the end of this plan).

- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.

- [ ] **Step 5: Commit** `feat: a runway change after the clearance is announced with the new SID and read back`

---

### Task 5: Approach — "no flight plan, say intentions" and an IFR clearance in the air (1a f)

**Files:**
- Modify: `crates/atc-phraseology/src/types.rs` (`AtcItem::NoFlightPlan`), `render.rs`, `pl/render.rs`, `crates/atc-core/src/engine/approach.rs` (`approach_request` l.136-165, `identify_departure` l.841, new `airborne_clearance`)
- Modify docs: `docs/superpowers/specs/2026-10-25-roadmap.md` (mark 1a done, one line per part with its commit), `docs/user-guide/ru/scenario-ifr-departure.md` (add "request departure runway …" and "say flight rules" to the kneeboard)
- Test: `crates/atc-core/tests/engine.rs` (the module with Approach departure tests — search `ReportDeparture`/`identify_departure` usages)

**Interfaces:**
- Consumes: `clearance` building blocks of `delivery.rs` (`ClearedTo`, `level`, `squawk`).
- Produces: `AtcItem::NoFlightPlan`; `fn airborne_clearance(&mut self, i: usize, destination: Option<&str>, level_ft: Option<i32>) -> Vec<AtcItem>` in `approach.rs`.

- [ ] **Step 1: Write the failing tests.**
  - render: `[NoFlightPlan, SayIntentions]` → `"no flight plan, say intentions"`; PL → `"brak planu lotu, podaj zamiary"`.
  - engine:
    - `departure_report_without_plan_gets_say_intentions`: airborne aircraft without plan (circuit-free, e.g. took off VFR without clearance and left the circuit), `ReportDeparture` on Approach → `[NoFlightPlan, SayIntentions]`.
    - `airborne_ifr_clearance`: then `"request IFR clearance to Katowice, flight level eight zero"` on Approach → `[Identified, ClearedTo { icao: "EPKT" or the fixture destination }, ClimbTo(level(8000)), Squawk(code)]`; after the readback the strip has an IFR plan with `sid: None`, `cleared_ft == cruise_ft`, `identified == true`; flying on, the flight is handed off by `approach_update` (destination tower or adjacent unit) like a SID departure.
    - `airborne_vfr_request_goes_to_tower_rules`: same request with "VFR" → today's VFR handling (no plan from Approach; `redirect` / nothing as today — assert unchanged behaviour).
    - `airborne_clearance_with_plan_is_ignored`: an aircraft with an IFR plan asks again → no new clearance, plan unchanged.

- [ ] **Step 2: Run** `cargo test --workspace` — Expected: FAIL.

- [ ] **Step 3: Implement.** `identify_departure`: plan `None` (and not handed off, airborne) → `[NoFlightPlan, SayIntentions]`. `approach_request`: `RequestClearance { rules: Some(Ifr), .. }` while airborne and plan `None` → `airborne_clearance`: unknown destination → `Unable(UnknownDestination)`; cruise = requested level, else `procs.ifr.cruise_ft`, else `Unable(NoIfrData)`; plan with `sid: None`, `dep_runway: None`, `cleared_ft = cruise_ft`; `identified = true`; squawk as in `clearance`. `rules: None` → `[SayFlightRules]`. Check that `direct_after_turn` returns `None` for `sid: None` without panicking.

- [ ] **Step 4: Run** `cargo test --workspace`, then `cargo clippy --workspace --all-targets -- -D warnings` — Expected: PASS, no warnings.

- [ ] **Step 5: Commit** `feat(core): Approach answers a flight without a plan and clears it IFR in the air`

---

**Calls made in this plan (not from the spec):**
- Tower does not re-route a flight that has already taxied; the amended clearance is given by Ground at the taxi clearance or by Delivery/Ground on a runway request. With the 5 kt rule the runway in use changes rarely; a change during the last minutes of taxi keeps the strip's runway.
- The amended clearance reads "runway changed, runway zero seven, SUDOL one Foxtrot departure" — the SID in its usual wording rather than the spec's "new departure SUDOL one Foxtrot".
- A runway change with no SID on the new runway keeps the old runway for that flight.
- The airborne IFR clearance has no SID or route: cleared to the destination, climb to the cruise level, squawk.
