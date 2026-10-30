# Aircraft identity from the simulator — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The client takes the aircraft type and (unless `callsign` is set in the config) the callsign from X-Plane, reports aircraft changes to the server as new flights, and shows how the server identified the pilot; `aircraft_type` and `field_elev_ft` leave `client.toml`.

**Architecture:** Protocol 2 adds `callsign_source` to `Hello`, a client `AircraftChanged` and server `Identified` / `AircraftChangeRefused` messages, and sends cloud bases in feet MSL (the server converts to AGL with its station elevation). The engine gains `remove_pilot`, the server session swaps flights on `AircraftChanged`. The client reads two string datarefs over RREF byte by byte, debounces changes, resolves the identity (config callsign wins) and drives the connection with the current identity.

**Tech Stack:** Rust 2024 workspace (atc-proto, atc-core, atc-server, atc-client); tokio; existing RREF reader.

**Spec:** `docs/superpowers/specs/2026-10-29-aircraft-from-sim-design.md`

## Global Constraints

- `PROTOCOL_VERSION` = 2; no protocol-1 compatibility.
- Datarefs: `sim/aircraft/view/acf_ICAO` and `sim/aircraft/view/acf_tailnum`, byte arrays read over RREF as `name[i]` (one byte per float), string ends at the first NUL; trim; empty = unknown.
- Callsign rule: `callsign` in `client.toml` set → always used; absent → the X-Plane tail number.
- Unknown type → `ZZZZ`.
- A change counts after the new type/tail number has been stable for 2 s; with a config callsign only the type matters.
- A new flight is a full reset of the old one (strip, phase, clearances, frequencies, its snapshot entry), also with the same callsign.
- Identification line, printed through the client's existing line sink (console + overlay): `Aircraft: <TYPE>, callsign <CALLSIGN> (from config)` or `Aircraft: <TYPE>, callsign <CALLSIGN> (X-Plane tail number)`.
- Refused change: `Aircraft change refused: <reason>; still flying as <CALLSIGN> (<TYPE>)`, printed once; no retry until the sim identity changes again.
- Missing callsign: `X-Plane has no tail number for this aircraft; set callsign in config/client.toml` (once), no connection until one exists.
- Waiting for the sim at start: every 10 s `Waiting for X-Plane at <addr> for the aircraft type…` (with config callsign in `--text` mode: no wait, `ZZZZ` until X-Plane reports).
- Old `aircraft_type` / `field_elev_ft` keys in `client.toml` load with a warning `config/client.toml: '<key>' is no longer used (read from X-Plane); remove the line`.
- English code/comments/docs; fmt, clippy `-D warnings`, `cargo test --workspace` green before every commit; one commit per task. Don't commit `config/client.toml`, `config/server.toml`, `track.csv`, `docs/.obsidian/`, or anything under `data/airports/` except EPKK's tracked files.

## Review Focus

1. **Same callsign, new type** (config callsign, pilot swaps C172 → B738): the old strip is gone, the new flight starts at the stand from scratch, and a server restart does not resurrect the old flight (Task 2 `remove_pilot_drops_snapshot_entry`, Task 3 `aircraft_change_same_callsign_is_new_flight`).
2. **Aircraft loading flicker** (X-Plane passes through an empty or intermediate type while loading): no `AircraftChanged` until stable for 2 s (Task 4 `flicker_shorter_than_2s_is_ignored`).
3. **Tail number taken by another client**: old flight stays, message once, no retry loop (Task 3 `aircraft_change_to_taken_callsign_is_refused`, Task 5 `refusal_printed_once_and_not_retried`).
4. **Cloud bases at an airfield other than EPKK**: AGL computed from the server's station elevation (Task 1 `weather_converts_msl_to_agl_with_station_elevation`).
5. **Old client.toml with `aircraft_type`/`field_elev_ft`**: loads with a warning, not an error (Task 5 `old_keys_load_with_warning`).

---

### Task 1: Protocol 2 and cloud bases in MSL

**Files:**
- Modify: `crates/atc-proto/src/lib.rs`, `crates/atc-server/src/convert.rs`, `crates/atc-server/src/session.rs` (weather call), `crates/atc-client/src/xplane.rs` (`weather()`), `crates/atc-client/src/connection.rs` (Hello), every test/fixture building `CloudLayer` or `Hello`
- Test: `crates/atc-proto/src/lib.rs` tests, `crates/atc-server/src/convert.rs` tests, `crates/atc-client/src/xplane.rs` tests

**Interfaces:**
- Produces:
  - `pub const PROTOCOL_VERSION: u16 = 2;`
  - `#[serde(rename_all = "snake_case")] pub enum CallsignSource { Config, Sim }` (Copy, Eq).
  - `ClientMsg::Hello { protocol, callsign, aircraft_type, callsign_source: CallsignSource }`
  - `ClientMsg::AircraftChanged { callsign: String, aircraft_type: String, callsign_source: CallsignSource }`
  - `ServerMsg::Identified { callsign: String, aircraft_type: String, callsign_source: CallsignSource }`
  - `ServerMsg::AircraftChangeRefused { reason: String }`
  - `CloudLayer { cover, base_ft_msl: u32 }` (renamed from `base_ft_agl`).
  - `convert::weather(f: &WeatherFrame, field_elev_ft: i32) -> Weather` — base AGL = `base_ft_msl - field_elev_ft`, clamped at 0.
  - `XPlaneState::weather(&self) -> Option<WeatherFrame>` (no elevation parameter).
- [ ] **Step 1: Tests:** proto JSON round-trip for the four messages (e.g. `{"type":"aircraft_changed","callsign":"SPABC","aircraft_type":"B738","callsign_source":"config"}`); `weather_converts_msl_to_agl_with_station_elevation` (base 2791 MSL, elevation 791 → 2000; base 500 MSL, elevation 791 → 0); client `weather()` reports MSL (`base_ft_msl == round(base_m × 3.28084)`).
- [ ] **Step 2: Run** `cargo test -p atc-proto -p atc-server -p atc-client` — FAIL.
- [ ] **Step 3: Implement.** The server passes `engine.airport().elevation_ft` (or the equivalent accessor; add `pub fn field_elevation_ft(&self) -> i32` to `AtcEngine` if none). The client sends `callsign_source: Config` with its current config values for now (Task 5 changes that). The server accepts `AircraftChanged` by logging and ignoring it for now (Task 3 implements it) and sends nothing new yet.
- [ ] **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `feat(proto): protocol 2 — aircraft identity messages, cloud bases in MSL`

---

### Task 2: Engine forgets a flight

**Files:**
- Modify: `crates/atc-core/src/engine/mod.rs` (+ whatever per-flight state lives elsewhere in `engine/`)
- Test: `crates/atc-core/tests/engine.rs`

**Interfaces:**
- Produces: `pub fn remove_pilot(&mut self, cs: &Callsign) -> bool` on `AtcEngine` — drops the strip and every per-callsign state the engine keeps (pending transmissions, held frequencies/handoffs, last-heard language, timers — read the engine for the full list); returns whether a flight existed. After it, `register_pilot` with the same callsign creates a fresh strip.
- [ ] **Step 1: Tests:** `remove_pilot_then_register_starts_fresh` (a departure taxiing with a clearance; remove; register same callsign with type `B738` → strip phase is the initial one, no clearance, type `B738`); `remove_pilot_drops_snapshot_entry` (`snapshot()` after removal has no strip for that callsign); `remove_unknown_pilot_is_false`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat(core): remove_pilot ends a flight completely`

---

### Task 3: Server identifies and swaps flights

**Files:**
- Modify: `crates/atc-server/src/session.rs`, `crates/atc-server/src/hub.rs`
- Test: `crates/atc-server/tests/ws.rs` (existing WebSocket test style)

**Interfaces:**
- Consumes: Task 1 messages, Task 2 `remove_pilot`.
- Produces:
  - After `Welcome` the server sends `Identified` with the registered callsign (normalised as the hub/engine store it), type and source.
  - `Hub::rename(&mut self, id: ClientId, callsign: String) -> Result<(), HubError>` (`DuplicateCallsign` when another client has it; renaming to the own callsign is Ok).
  - On `AircraftChanged`: validate the callsign like the handshake (empty → refused); `hub.rename`; on error → `AircraftChangeRefused { reason }` (e.g. `"callsign SPXYZ is in use"`), session and old flight unchanged; on success → end the old flight as on disconnect (end relay / PTT lock, engine `remove_pilot`), register the new `PilotProfile`, keep the weather-source role, send `Identified`.
- [ ] **Step 1: Tests:** `identified_follows_welcome`; `aircraft_change_same_callsign_is_new_flight` (taxi clearance given → `AircraftChanged` same callsign, type `B738` → `Identified` with B738, engine strip fresh); `aircraft_change_new_callsign_renames` (a second client can then join with the old callsign); `aircraft_change_to_taken_callsign_is_refused` (two clients; A changes to B's callsign → `AircraftChangeRefused`, A still flies under its old callsign); `weather_source_kept_after_change`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat(server): Identified, and aircraft changes start a new flight`

---

### Task 4: Reading the aircraft from X-Plane

**Files:**
- Modify: `crates/atc-client/src/datarefs.rs`, `crates/atc-client/src/xplane.rs`
- Create: `crates/atc-client/src/aircraft.rs` (+ `pub mod aircraft;` in lib.rs)
- Test: in those files

**Interfaces:**
- Produces:
  - String datarefs subscribed at 1 Hz: `acf_ICAO[0..8]` and `acf_tailnum[0..12]` (indices beyond `SLOT_COUNT`'s numeric slots — extend the subscription table cleanly, e.g. a second table of `(StrSlot, base name, len)` mapped to RREF indices after the numeric ones).
  - `pub struct SimAircraft { pub icao: Option<String>, pub tail: Option<String> }` (PartialEq) and `XPlaneState::aircraft(&self) -> SimAircraft` — bytes until NUL, ASCII only, trimmed, empty → None; nothing received yet → both None.
  - `XpUpdate::Aircraft(SimAircraft)`, emitted by `run_xplane` when the decoded value differs from the last one emitted.
  - In `aircraft.rs`: `pub struct ChangeFilter` with `new(stable: Duration)`, `fn offer(&mut self, now: Instant, a: SimAircraft) -> Option<SimAircraft>` — returns a value once it has been unchanged for `stable` (2 s in production) and differs from the last returned one; the first stable value is returned too.
- [ ] **Step 1: Tests:** `decodes_type_and_tail` (bytes `C 1 7 2 \0` and `S P - A B C \0` as RREF floats → `C172`, `SP-ABC`); `empty_strings_are_none`; `filter_returns_first_stable_value`; `flicker_shorter_than_2s_is_ignored` (C172 stable → B738 for 1 s → C172 → nothing new); `change_after_2s_is_returned`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat(client): read the aircraft type and tail number from X-Plane`

---

### Task 5: Client identity, config and messages

**Files:**
- Modify: `crates/atc-client/src/config.rs`, `crates/atc-client/src/main.rs`, `crates/atc-client/src/connection.rs`, `crates/atc-client/src/app.rs`, `config/client.example.toml`, `README.md` (client config table), `docs/RUNNING.md` (client settings)
- Test: in those files (existing test style)

**Interfaces:**
- Consumes: Task 1 messages, Task 4 `XpUpdate::Aircraft`, `ChangeFilter`.
- Produces:
  - `ClientConfig.callsign: Option<String>`; `aircraft_type`/`field_elev_ft` accepted as ignored keys with the warning from Global Constraints (keep `deny_unknown_fields` for everything else).
  - `pub struct Identity { pub callsign: String, pub aircraft_type: String, pub source: CallsignSource }` and `pub fn resolve(config_callsign: Option<&str>, sim: &SimAircraft) -> Result<Identity, MissingCallsign>` (type `ZZZZ` when unknown).
  - The connection gets the identity through a `tokio::sync::watch::Receiver<Option<Identity>>` instead of fixed fields in `ConnConfig`: it waits until `Some`, sends `Hello` with the latest value on every (re)connect, and sends `AircraftChanged` when the value changes while connected.
  - `App` prints the identification line on `Identified` and the refusal line on `AircraftChangeRefused` (once per refused identity; the client does not resend until the filtered sim identity changes).
  - Waiting / missing-callsign messages and the `--text` rule from Global Constraints.
- [ ] **Step 1: Tests:** `resolve_prefers_config_callsign`; `resolve_uses_tail_number`; `resolve_without_any_callsign_is_missing`; `unknown_type_is_zzzz`; `old_keys_load_with_warning` (example config plus `aircraft_type = "C172"` and `field_elev_ft = 791` parses); `example_config_parses` (callsign commented out); connection test with a fake server: `hello_waits_for_identity`, `identity_change_sends_aircraft_changed`, `reconnect_uses_latest_identity`; app test: `identified_prints_line` (exact text, config and sim variants), `refusal_printed_once_and_not_retried`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test --workspace` PASS.
- [ ] **Step 5: Docs:** `config/client.example.toml`: `# callsign = "SP-ABC"` with a comment (set it to always fly under this callsign; without it the X-Plane tail number is used); remove `aircraft_type` and `field_elev_ft`. README and RUNNING.md client tables updated the same way, plus one line on the identification message and that changing the aircraft in X-Plane starts a new flight.
- [ ] **Step 6: Commit** `feat(client): aircraft and callsign from X-Plane; identification line`

---

**Calls made in this plan (not from the spec):**
- Refusal is a dedicated `ServerMsg::AircraftChangeRefused { reason }` (the spec left the message to the builder).
- `Hub::rename` keeps the client id, so per-client server state (weather source, session) survives the change.
- String datarefs read as the first 8 (type) and 12 (tail number) bytes — enough for ICAO designators and registrations.
- The identity flows to the connection through a `watch` channel.
- Server elevation for AGL comes from the engine's airport layout.
