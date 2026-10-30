# Aircraft identity from the simulator — design (2026-10-29)

## Intent

Today the pilot types the aircraft into `config/client.toml` (`callsign`, `aircraft_type`,
`field_elev_ft`). Switching aircraft in X-Plane means editing the file and restarting the
client, and `field_elev_ft` is EPKK's elevation even when flying from another airfield. The user
does not want to set any of this by hand: the client reads it from the simulator. Only the
callsign may still be set, as an explicit choice.

## User decisions

- `aircraft_type` and `field_elev_ft` are removed from `client.toml`.
- `callsign` becomes optional. Set → always used, also after an aircraft change. Not set → the
  aircraft's tail number from the simulator.
- Changing the aircraft in the simulator while the client runs updates the data automatically and
  is a **new flight** for the controller (a fresh strip).
- How the server identified the pilot is shown to the user — in the client console and in the
  X-Plane overlay window when the plugin is installed — on connect and after every change, e.g.
  `Aircraft: C172, callsign SP-ABC (from config)` /
  `Aircraft: B738, callsign N738AB (X-Plane tail number)`.

## Goals

1. The client reads the aircraft ICAO type and tail number from X-Plane over the existing UDP RREF
   channel (no plugin required; the plugin stays optional).
2. Callsign resolution: config value if present, else the tail number.
3. Aircraft changes are detected and sent to the server, which ends the old flight and starts a
   new one under the new identity.
4. Cloud bases leave the client as feet MSL; the server converts them to AGL with its station's
   field elevation (so `field_elev_ft` is no longer needed and is right at any airport).
5. The identification line in the console and the overlay.

## Non-goals

- MSFS / X-Plane 12 specifics (roadmap item 7); the design must not block them (identity comes
  from "the simulator", X-Plane is the first source).
- Choosing among several stations by aircraft position (airport wizard phase 2).
- Reading the airline/flight-number callsign from the sim (X-Plane has none; the tail number is
  what it has).

## Anti-goals

- Silently using a made-up default type or callsign while the sim has not answered — the client
  waits and says what it is waiting for.
- A config that used to work refusing to start: old `aircraft_type` / `field_elev_ft` lines are
  accepted with a warning that they are no longer used.

## Constraints and decided how

- X-Plane: `sim/aircraft/view/acf_ICAO` (type, e.g. `C172`) and `sim/aircraft/view/acf_tailnum`
  (tail number) are byte-array datarefs; over RREF they are read element by element
  (`…[0]`, `…[1]`, … each a byte as a float), up to the first NUL. Lengths: acf_ICAO 40 bytes,
  acf_tailnum 40 bytes (read the first 10 and 10 is enough for real values — builder may read
  more). Values are trimmed; empty = unknown.
- Empty tail number and no `callsign` in config: the client says once
  `X-Plane has no tail number for this aircraft; set callsign in config/client.toml` and does not
  connect until one of the two exists. Empty type: send `ZZZZ` (ICAO's "no designator") and say
  so in the identification line.
- Protocol (my call): `PROTOCOL_VERSION` 1 → 2.
  - `ClientMsg::Hello` keeps `callsign` and `aircraft_type` and gains
    `callsign_source: "config" | "sim"`.
  - New `ClientMsg::AircraftChanged { callsign, aircraft_type, callsign_source }`.
  - New `ServerMsg::Identified { callsign, aircraft_type, callsign_source }`, sent after `Welcome`
    and after each accepted `AircraftChanged`; the identification line is printed from this
    message, so it shows what the **server** registered. A refused change (callsign already used
    by another client) comes back as `ServerMsg::Rejected`-like error without closing — builder's
    choice of message, the old flight stays.
- Change detection (my call): a change is a different type or tail number that stays stable for
  2 s (loading an aircraft passes through intermediate values). With `callsign` from config only
  the type matters.
- Server on `AircraftChanged`: the old flight ends as if the client had disconnected (strip
  removed, frequencies released), the new one is registered like a fresh `Hello`, on the same
  WebSocket session.
- Weather (my call): `CloudLayer.base_ft_agl` becomes `base_ft_msl`; the server subtracts the
  station's field elevation (the airport layout's `elevation_ft`) when building METAR/ATIS,
  clamped at 0. Replay scenarios and fixtures with `base_ft_agl` are updated.
- Overlay: the identification line goes to the plugin through the existing client→overlay path
  as an info line (builder picks the exact mechanism the overlay already supports).
- Docs: README and RUNNING.md config tables, `config/client.example.toml` (callsign commented
  out with the explanation).

## Edge cases (from the builder check)

- **New flight means a full reset** (user: "это новый рейс"): everything the engine and hub keep
  for the old flight is dropped — strip, phase, clearances, frequencies, and its entry in the
  persisted snapshot (`data/cache/state-<ICAO>.json`) — also when the callsign stays the same
  (config callsign, only the type changed). The new flight must not be restored from the
  snapshot.
- **Refused change** (my call): the server keeps the old flight and answers with an error message
  on the open session. The client prints `Aircraft change refused: <reason>; still flying as
  <callsign> (<type>)` once and does not retry until the simulator's identity changes again.
- **Reconnect** (my call): a WebSocket reconnect sends `Hello` with the latest stable identity.
- **Simulator stops answering mid-session** (my call): the client keeps the last identity and
  stays connected, as today with lost telemetry.
- **Simulator not running at start** (my call): the client waits, printing every 10 s what it is
  waiting for (`Waiting for X-Plane at 127.0.0.1:49000 for the aircraft type…`); Ctrl-C quits.
- **`--text` mode** (my call): with `callsign` in the config it connects without waiting for the
  simulator and uses `ZZZZ` until X-Plane reports a type (then an ordinary aircraft change).
  Without a config callsign it waits for the tail number like the voice mode.
- **Ordering** (my call): no sequencing guarantee; a few telemetry/weather frames under the old
  identity during the change round trip are acceptable. Telemetry and weather are not held back
  until `Identified`.
- **Weather source** (my call): the client that is the weather source keeps that role across an
  aircraft change (same client id).
- **Compatibility** (my call): protocol 2 is a hard break, no protocol-1 clients. Replay
  scenarios do not carry cloud bases in `base_ft_agl`, so no scenario data needs converting;
  only code and test fixtures that build `CloudLayer` change.

## Left to the builder

- Exact RREF indices/frequency for the string datarefs and the debounce implementation.
- Message wording beyond the examples above.
- Where in the server the AGL conversion lives (convert.rs is the natural place).
