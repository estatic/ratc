# Virtual ATC — stations at any airfield: airport wizard and multi-station server (2026-10-29)

This design replaces roadmap item 1b ("Second airport: Katowice") of `2026-10-25-roadmap.md`.
EPKT is no longer added by hand: it becomes the first station made with the wizard described
here. Everything else in the roadmap and in `2026-10-19-virtual-atc-design.md` still stands,
except the v1 "EPKK only" non-goal, which this design lifts for every airfield.

## 1. Intent

The user wants an ATC station at any airfield in the world — an international airport, a grass
strip with gliders, a heliport — without hand-writing data files and without an AI model doing
it. A user who is not a developer runs a wizard in a terminal, picks an area, ticks which
airfields get which controllers, and flies. Data comes from sources the user already has
(X-Plane) or can get for free (OpenAIP, with their own key). The project's distribution ships
the program only, no third-party aviation data.

Success: the user regenerates EPKK with the wizard and gets the same behaviour as today's
hand-made station; then creates EPKT the same way and flies EPKK → EPKT stand to stand, handed
from station to station, with no code change specific to EPKT.

## 2. Goals

1. **Wizard** — an interactive terminal command, same binary on Linux and Windows (macOS comes
   free with the CI cross-build), that creates, updates and removes stations.
2. **Any airfield type** — airports, airfields (ICAO code or not) and heliports. Off-field
   landing is not modelled in the wizard (see §5.6).
3. **Area-based creation** — the user picks one airfield; the wizard lists every airfield
   within 100 km of it; the user ticks which become stations and which positions each one has.
4. **Data from OpenAIP REST**, fetched only while the wizard runs, with the user's own API key.
5. **Update check** — on each run the wizard asks OpenAIP whether data changed for the regions
   the user already has, and offers to update. The user's own choices survive an update.
6. **Multi-station server** — the server runs every station the user created at the same time,
   and hands a flight from one to the next.
7. **En-route units** — inside downloaded countries, flights are handed between FIS / ACC units
   from OpenAIP airspaces; outside them, the last station releases the flight
   ("frequency change approved") and the radio stays silent until the next station.
8. **Destinations worldwide** — any airfield in X-Plane's apt.dat can be named as destination
   (Helsinki → Sydney is fileable), whether or not it has a station.

## 3. Non-goals and anti-goals

- **No AI in creating stations.** No Ollama, no LLM-generated data. Every value comes from a
  named source, a documented ICAO default, or the user's answer.
- **No OpenAIP data in the repository or the distribution.** Its licence (CC BY-NC 4.0) stays on
  the user's machine. No shared project key: each user registers their own.
- **No network access while flying.** The server reads only local files.
- **No GUI.** Terminal questions are enough (user decision).
- **Not a new ATC behaviour project.** Positions behave as they do at EPKK after plan 4; this
  project makes them data-driven and multi-station. New behaviours a small field might need
  (glider traffic, AFIS phraseology beyond the minimum in §5.5) are out of scope.
- **Anti-goal:** an update that silently overwrites the user's choices, or a regenerated EPKK
  that behaves worse than the hand-made one.

## 4. Sources

| Data | Source | Notes |
|---|---|---|
| Runways, taxiways, stands, field elevation, airport name | X-Plane apt.dat, as loaded by the user's X-Plane (existing `resolve_airport`, scenery_packs.ini order) | already in `atc-navdata/src/xplane.rs` |
| SID / STAR / approaches | X-Plane CIFP `Resources/default data/CIFP/<ICAO>.dat` | already in `load_procedure_set`; absent at small fields |
| Airfield list, type, frequencies (name, value, type), runways, magnetic declination | OpenAIP `/airports` | |
| CTR, TMA, ATZ, FIR, FIS sectors with limits and frequencies | OpenAIP `/airspaces` | polygons; replaces the hand-made `tma` circle |
| VFR reporting points | OpenAIP `/reporting-points` (linked to airports by id) | replaces hand-typed AIP points |
| Circuits, transition altitude when unknown, defaults | ICAO standard, documented in the wizard | user confirms or changes |
| Positions open, callsigns, preferred runway, any override | user answers in the wizard | stored as overrides |

OpenAIP Core API (checked 2026-10-29, notes in Obsidian `websites/openaip-core-api.md`):
base `https://api.core.openaip.net/api`, header `x-openaip-api-key`, list endpoints with `page`,
`limit`, `country`, `search`, `type`, `updatedAfter` (ISO 8601 UTC). No dataset or AIRAC version
endpoint exists; every item has `updatedAt`. Poland today: 431 airfields, 1018 airspaces,
439 reporting points; 42 airfields within 100 km of EPKK, 14 with an ICAO code.
OpenAIP frequencies for EPKK match the AIP values in `data/airports/EPKK/SOURCE.md`.

## 5. Design

### 5.1 The wizard

A subcommand of the existing server binary (`atc-server airports …`), so CI already builds it
for every platform (my call; the user asked only for "Linux and Windows"). Plain prompts on
stdin/stdout, numbered menus, works in Windows `cmd`/PowerShell and any Linux terminal.

Main menu: create stations in an area · update data · edit a station · remove a station ·
set the OpenAIP key.

**Key.** Asked once, stored in the user's config directory (`~/.config/vatc/` on Linux,
`%APPDATA%\vatc\` on Windows), never in the repo, never logged. An invalid key gives a clear
message and the OpenAIP sign-up URL.

**Create in an area.**
1. User types an ICAO code or a name; the wizard searches OpenAIP and the user picks one.
2. The wizard downloads the country (or countries, when the 100 km circle crosses a border) —
   airports, airspaces, reporting points — into the local store (§5.3).
3. It lists every airfield within 100 km: distance, ICAO (or "—"), name, type, frequencies.
4. The user ticks airfields. For each one it offers the positions that have an OpenAIP
   frequency (ATIS, Delivery, Ground, Tower, Approach, AFIS/Information, Radio/A-A) with the
   published callsign and frequency, all ticked by default; the user unticks or edits them.
   An airfield with no position ticked is a **silent field** (§5.6).
5. For each station it shows the values it will use and where each comes from (X-Plane,
   OpenAIP, ICAO default) — runways, circuits, preferred runway, transition altitude, VFR
   points, adjacent units — and lets the user change any of them. Changes become overrides.
6. It warns, and does not create the station, when X-Plane has no layout for the airfield
   (no apt.dat entry with runways), naming the missing piece.

**Update.** At start, and from the menu: for each country in the store, one request
`?country=XX&updatedAfter=<last sync>&limit=1` per data type; if any `totalCount > 0` the
wizard says what changed (counts per country and type) and offers to update. The update
re-downloads that country and regenerates the affected stations; overrides are reapplied. An
override whose target disappeared (a removed frequency, a renamed reporting point) is shown to
the user, not dropped silently.

### 5.2 What a station is on disk

Today `data/airports/EPKK/procedures.toml` is hand-written and mixes facts with rulings. After
this project a station is:

- `stations/<ICAO>/station.toml` — generated by the wizard from sources + defaults: the same
  content the engine reads today (`Procedures`: positions, circuits, VFR points, adjacent
  units, transition altitude, TMA/CTR, hours…), with each value tagged with its source.
- `stations/<ICAO>/overrides.toml` — the user's answers only. Never rewritten by an update.

The engine reads the merge (overrides win). The exact file names, the location (user data
directory vs repo `data/`), and whether the merge is done at generation time or load time are
left to the builder, with one constraint: the user's overrides live in their own file and
survive regeneration.

`Procedures` gains what the data now provides: airspace polygons (CTR/TMA/ATZ) and the station
type (controlled / AFIS / silent). Phase 1 only carries the polygons; the engine keeps using the
`tma` circle until phase 2 switches it, so phase 1 stays a data change (my call).

**Overrides may set any `Procedures` field** (my call), not only the user's rulings: AIP facts
OpenAIP does not carry (EPKK's `vfr_holds` and their maximum altitude, the `south` flag on VFR
points, `delivery_hours`, `approach_alt_khz`, the TMA ceiling, `min_level_ft` of Warszawa Radar)
become EPKK overrides, so the regenerated EPKK loses nothing. The wizard's edit step offers every
field; each override records its source (AIP section, user ruling).

**Airfields without an ICAO code** (my call) are keyed by their X-Plane apt.dat ident (a station
needs an apt.dat match anyway, §5.1 step 6) and are named by the pilot and the controller by
their name ("Sąspów"). Strips, files and snapshots use that ident where they use the ICAO today.

### 5.3 Local store

Downloaded OpenAIP data is cached per country with the time of the last sync, under the user's
data directory, outside the repo. The server never reads the store directly: it reads
stations, plus one generated **en-route file** with the FIS/ACC/FIR sectors (polygons, limits,
callsign, frequency) of all downloaded countries, for §5.4.

### 5.4 The server with many stations

The server loads every station in the stations directory and the en-route file. `airport =` in
`config/server.toml` goes away (or becomes optional, for running one station while testing).

- **Units, not airfields, own frequencies** (my call). A position is a unit identified by
  callsign + frequency and may serve several airfields (Kraków Approach serves EPKK and EPKT).
  The wizard merges duplicates OpenAIP lists under several airfields into one unit. A flight
  between two airfields of the same unit is not handed off on that leg.
- Each station keeps today's engine (`AtcEngine`) with its own strips. The builder decides
  whether that is one engine per station plus a dispatcher, or one engine generalised to many
  airports — the plan must state which and why.
- **En-route units answer** (user decision 2026-10-29), at a minimum: they reply to the first
  call ("identified", weather/QNH), watch for the aircraft leaving their sector (polygon +
  level) and hand it to the next unit, and keep the level last assigned. No IFR route
  management. This is the one new controller role of this project (an exception to §3).
- A transmission is routed by frequency to the station or en-route unit owning it.
- Hand-offs: the existing "contact <unit> <freq>" logic now targets real stations and the
  en-route sectors the aircraft is entering (polygon + level). Inside downloaded countries the
  flight goes FIS to FIS / Radar to Radar. Leaving the last known unit's coverage, the flight
  is released: "frequency change approved" (no frequency). The radio is silent until the
  aircraft calls a station.
- Destinations: every airfield in X-Plane's apt.dat, worldwide, replacing
  `load_destinations_near`'s radius. Loading must stay fast (the scan already exists; cache it
  if needed).
- Persistence across restarts (`persist.rs`, one snapshot per airport today) covers every
  station.
- The Whisper prompt, today built from one airport's names, is built from the stations and
  units near the aircraft.

### 5.5 Station types

- **Controlled** — the positions ticked in the wizard, behaving as at EPKK. A field with only a
  Tower gives at least take-off and landing clearances and runs the circuit.
- **AFIS / Information** — when the user ticks an AFIS position: traffic and runway information,
  no clearances. Minimum phraseology only; anything richer is out of scope (§3).
- **Silent** — no position. A station handing a flight to it says "change to <name>
  <frequency>" when a frequency exists, else "frequency change approved", and nothing answers
  there.

### 5.6 Off-field landing

Not an airfield, so not in the wizard. Any unit releases a flight that asks for it with
"frequency change approved", with or without a frequency. No data is needed.

## 6. EPKK and the hand-made data

The current `data/airports/EPKK/` (hand-made, commit 644df24) is backed up in the Obsidian vault
at `raw/virtual-atc/epkk-handmade-2026-10-29/`. EPKK is regenerated with the wizard; the user's
rulings (preferred runway 25, left circuits at 1800 ft AMSL, positions and callsigns) become
EPKK's `overrides.toml`. Every difference between the regenerated station and the backup is
either a wizard bug or a recorded data difference — the plan includes this comparison as an
acceptance step. The existing EPKK tests keep passing; tests that need fixed data use a
committed test fixture made of the project's own data (not OpenAIP data).

The Gateway apt.dat in `data/airports/EPKK/` (GPLv2) stays only if the tests need it.

## 7. Phases

One design, three plans, in this order (user-approved):

1. **Wizard and data** — OpenAIP client, local store, key handling, create / update / edit /
   remove, generated station + overrides files, the engine reading them, EPKK regenerated and
   compared with the backup.
2. **Multi-station server** — many stations at once, routing by frequency, hand-offs between
   stations and en-route units, release outside coverage, worldwide destinations, persistence
   for all stations.
3. **EPKT and the EPKK ↔ EPKT scenario** — EPKT made with the wizard only; any code change it
   needs is a shortcoming of phases 1–2 and is fixed in shared code. A RU user-guide page for
   the wizard and the scenario.

Before phase 1: finish roadmap 1a (runway stability, departure-runway request, amended
clearance, "say flight rules", airborne IFR clearance) on branch `insim-fixes` and merge it.

## 8. Constraints

- Rust, in the existing workspace; English docs and code; the RU guide under
  `docs/user-guide/ru/`.
- The wizard must work in a plain Windows console (no ANSI-only UI; UTF-8 names such as
  "KRAKÓW" must print correctly).
- OpenAIP requests: paginate, respect rate limits, retry politely, show progress for a country
  download; a failed download leaves the previous store intact.
- The key is a secret: not in the repo, not in logs, not in error messages.
- OpenAIP data stays on the user's machine; test fixtures use synthetic or project-owned data.
- X-Plane remains the source of layout and procedures; OpenAIP never overrides runway geometry.

## 9. Left to the builder

- Crate layout (a new `atc-openaip` / `atc-stations` crate or not), HTTP client choice.
- File formats and locations of the store, stations and overrides (within §5.2's constraint).
- One engine per station vs one multi-airport engine (§5.4), stated and justified in plan 2.
- How OpenAIP types and frequency types map to our position roles; how airspace types map to
  CTR/TMA/ATZ/FIS/FIR (document the table).
- ICAO defaults for fields without published data (circuit side and height, transition
  altitude by country), documented in one place.
- Matching OpenAIP airfields to X-Plane apt.dat entries (ICAO, else position within a few
  hundred metres).
