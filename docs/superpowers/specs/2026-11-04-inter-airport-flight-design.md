# Inter-airport IFR flight: hand-off to the destination, SID altitude, taxi-in, stand names

Date: 2026-11-04. Source: the EPKK -> EPKT full-flight run (stations EPKK and EPKT loaded
together, scenario `docs/user-guide/ru/scenario-ifr-departure.md`).

## Why

A complete IFR flight between two loaded stations cannot be flown cleanly:

1. **Bounce at the destination Tower.** About 10 NM before EPKT, Kraków Approach (EPKK engine)
   hands the flight to Katowice Tower, which EPKK lists as an `[[adjacent]]` Tower. Because EPKT is
   a real loaded station, the network moves the flight there (`import_flight`: owner Approach,
   `handed_in`, IFR plan, no approach set). Katowice Tower has no path for an airborne IFR inbound
   reporting its level, so it answers "say again". On the next tick `handover()` sends the flight
   from Tower to Approach ("contact Kraków Approach 121.075"), and only then does the EPKT engine's
   Approach vector it to ILS 27. Root cause: `approach.rs` `hand_off()` / `destination_tower()` were
   written for an emulated adjacent tower, and a real destination Tower cannot accept a handed-in
   IFR flight.
2. **Clearance below the SID.** Delivery clears "climb 5000" (`ifr.cruise_ft`), but SUDOL 2J
   requires at or above 6000 ft at KK602 and at or above FL100 at KK604. `ifr::initial_climb_ft`
   only applies ceilings; `AtOrAbove` floors are ignored.
3. **Taxi-in gets a departure taxi.** After landing, "request taxi" without a stand goes to
   `taxi_to_holding_point` (it explicitly accepts `TaxiIn`) and the pilot gets a taxi to the runway
   with ATIS, QNH and squawk.
4. **Stand names with spaces cannot be requested.** EPKT GA stands are named like "APRON4 GA".
   The parser drops the keyword and joins the rest ("apron four g a" -> "4GA"); matching is an exact
   case-insensitive compare, so it never matches.

## Goals

1. **Arrival hand-off goes to the destination's arrival unit.** When the destination airport is a
   loaded station:
   - If it has an Approach position, the departure station's Approach hands the flight to it when the
     flight enters the destination's area (where it now hands to the adjacent Tower). If both
     transfer is still a real hand-off between two controllers, even on the same frequency
     (EPKK and EPKT both list "Kraków Approach" 121.075): the current controller tells the pilot
     to contact the destination position, the pilot reads it back and calls the new controller,
     whose engine answers the first call (no "say again").
   - The destination Approach then vectors to the approach and hands to its Tower once established,
     as it already does today.
   - If it has no Approach position, the flight goes to its Tower as today, and the Tower accepts a
     handed-in IFR inbound: it identifies the flight and gives the expected approach and runway
     (e.g. "<callsign>, Katowice Tower, expect ILS approach runway 27, report established"), and
     never bounces it back to an Approach position of another station.
   - A handed-in flight is never answered with "say again" just because it is new on this position.
2. **The initial climb respects the SID's floors.** The cleared altitude is at least the highest
   `AtOrAbove` floor on the SID path (rounded up to the next 1000 ft, or the flight level above the
   transition altitude), unless a ceiling on the SID caps it lower; ceilings keep winning as today.
   For SUDOL 2J that is FL100. The flight-planned cruise level, when higher, is still used.
3. **Taxi after landing goes to a stand.** "request taxi" from `TaxiIn` (or a strip that has
   landed) is a taxi-in: to the stand the pilot named, else the flight's home stand, else "say
   stand" (the existing `taxi_to_stand` path). No QNH, squawk or holding point. A real departure
   request from that state ("request start up", a departure clearance) keeps working.
4. **Stand names are matched loosely.** Heard and stored names are compared with case, spaces and
   punctuation ignored, and the keyword ("stand", "parking", "apron") kept when it is part of the
   name. "apron four golf alpha", "apron 4 GA", "parking APRON4 GA" all match "APRON4 GA". When
   several stands share a name, the nearest free one to the aircraft is chosen.

## Non-goals

- A `stations = [...]` setting in server.toml (loading all stations stays the way to run two).
- FIS/ACC (`enroute.toml`) between the airports.
- Changing EPKK `cruise_ft` or other station data by hand; goal 2 fixes the clearance in code.
- New scenery stands (stand 35 / Bravo 5/6 are missing in XP11 default scenery: docs only).

## Constraints

- Single-station behaviour (emulated adjacent tower, `adjacent_unit_inbound_is_silent`,
  `no_adjacent_unit_releases_at_boundary`, etc.) stays as it is.
- Tests do not depend on git-ignored station data (EPKT is wizard-made): multi-station tests use the
  server fixtures (`crates/atc-server/tests/fixtures/stations/`), extended as needed.
- English docs and code. The RU scenario (`scenario-ifr-departure.md`, on PR branch
  `docs/epkk-epkt-full-flight`) and `ifr-and-vfr-points.md` are updated to the new behaviour after
  that PR is merged; `ifr-and-vfr-points.md` stops pointing at stand 35.
- Existing tests stay green, apart from tests that assert the old behaviour, which are updated.

## My calls (for review)

- Hand-off to another station is always announced with "contact", even on the same frequency (user's decision).
- SID floor beats the 5000 ft default (gives FL100 for SUDOL 2J) instead of "climb via SID".
- Duplicate stand names: the nearest free stand wins.

## Decisions after the builder check (my calls)

- **Trigger and data.** When stations are loaded together, the network gives each station's engine
  the destination stations' Approach positions as extra adjacent units (kind Approach, airport,
  callsign, khz), next to the hand-written `[[adjacent]]` list (a real loaded unit wins, as with
  `real_unit_wins_over_emulated_adjacent`). The departure Approach hands an IFR flight to the
  destination Approach when it is within 30 NM of the destination airport (new constant), or at
  once if it is already closer when it reaches Approach. The 10 NM destination-Tower hand-off
  stays for destinations without such an Approach.
- **Transfer is always announced.** Same or different frequency, a hand-off to another station
  is the normal "<callsign>, contact <position callsign> <frequency>" with readback, then the
  pilot's first call to the new controller, handled by the destination engine's existing handed-in
  first-call logic (identification, then descent / vectors). The user's rule: a different airport
  is a different controller, and the pilot is told so. The callsign comes from the destination's
  station data (EPKT's OpenAIP data names it "Kraków Approach"; the user can rename it in the
  wizard).
- **Destination Tower without an Approach.** A handed-in IFR flight with no approach set is
  identified and given the expected approach for the active runway from the station's approach
  procedures (ILS if there is one, else the first published approach, else "expect visual approach
  runway <rwy>"), and the Tower keeps it (no hand-over to Approach). A handed-in VFR flight follows
  the Tower's existing VFR-inbound path. The general rule: a handed-in flight's first call is never
  answered with "say again".
- **SID floor rule.** Cleared altitude = the higher of the planned/default cruise level and the
  highest `AtOrAbove` floor (or `Between` lower bound) on the SID, then capped by the first ceiling
  on the path as today. Rounded up to a whole 1000 ft; above the station's transition altitude it
  is said as a flight level. Applies whether the cruise level comes from the plan or
  `ifr.cruise_ft`.
- **Stand matching.** The parser passes what was heard after the keyword, keyword included, as a
  raw string (letters from the phonetic alphabet, digits from numbers). The engine normalises both
  the heard string and each stored name (uppercase, alphanumerics only) and matches either with or
  without a leading "STAND"/"PARKING"/"APRON" on both sides. "Free" = no other connected aircraft
  within the stand's radius and not the assigned stand of another active strip; among duplicates
  the nearest free one wins, else the nearest.
- **Taxi-in detection.** `FlightPhase::TaxiIn` is the flag. In `TaxiIn`, "request taxi" is always a
  taxi-in, even if the strip carries a departure clearance; a new departure starts from the stand
  ("request start up" / taxi from `Parked`).
- **Fixtures.** New test stations may be added under `crates/atc-server/tests/fixtures/stations/`.
  The hand-off cases are tested through `Network` (`route()` / `move_flight`), not only engine
  unit tests: same-frequency Approach, different-frequency Approach, destination with Tower only.

## Left to the builder

- Exact Tower phrasing for a handed-in IFR inbound (English and Polish, phraseology crate pattern).
