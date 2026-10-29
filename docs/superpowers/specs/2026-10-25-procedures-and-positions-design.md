# Procedures and all positions — design (project 1 of the 2026-10-25 roadmap)

Parent documents: `2026-10-19-virtual-atc-design.md` (v1 spec, still binding except where the
roadmap `2026-10-25-roadmap.md` lifts the SID/STAR/Approach non-goal).

## 1. Intent

Today the controller knows only Ground and Tower and a hand-made VFR circuit. Real Kraków is worked
by ATIS, Delivery, Ground, Tower and Approach, with published SIDs, STARs, instrument approaches and
VFR entry points. The user wants the controller to follow those published procedures instead of
inventing paths, both for their own flights and, in project 2, for the server-driven traffic.

The user is not an experienced IFR pilot. The controller must therefore be correct by the book
(AIP Poland + ICAO phraseology) so the user can learn from it, and the user will test on the
aircraft they know: the **Cessna 172 G1000** in X-Plane 11.

Success, in the user's words from the brainstorm: the controller follows the departure, arrival,
approach and circuit procedures, and every position that exists in reality is there.

## 2. Sources (authoritative, in this order)

1. **AIP Poland, eAIP IFR, AIRAC AMDT 10-26 (effective 2026-10-01), AD 2 EPKK**, PANSA:
   `https://docs.pansa.pl/ais/eaipifr/AIRAC%20AMDT%2010-26_2026_10_01/eAIP/AD%202%20EPKK%201-en-GB.html`
   and chart 13-1 (Visual Operation Chart, 16 APR 2026). A summary of the parts used here is in the
   user's Obsidian vault (`websites/EPKK AIP — ATC procedures (AIRAC 10-26).md`) and is restated in §4.
2. **X-Plane 11 navdata in the user's install** (`xplane_root` in `config/server.toml`):
   `Resources/default data/CIFP/EPKK.dat` (ARINC 424 procedures), `earth_fix.dat`, `earth_nav.dat`;
   `Custom Data/` overrides `Resources/default data/` when present (it is empty today). Cycle 1802
   (Navigraph/Jeppesen, February 2018). EPKK.dat contains SIDs to BABKO, BAREX, EMBIK, LENOV,
   LUXAR, NAVUR, NUMBA, POBOK, SUDOL, TUSIN (1F for RWY 07, 2J for RWY 25; CIFP identifiers
   BABK1F … TUSI2J), STARs from BAREX, GOVRI, KUKAM, LENOV, MAPIK, MEBAN, NETIR, PADKA (1G for
   RWY 25, 1H for RWY 07), approaches ILS 25 (I25), VOR/DME 07 and 25 (D07, D25), RNAV 07 and 25
   (R07, R25). Spoken designators are built from the CIFP fix names ("SUDOL two Juliett").
3. **ICAO Doc 4444 ch. 12 / Doc 9432** for phraseology; CAP 413 where those are silent.

**Decision (mine): procedures come from the X-Plane CIFP, not from today's AIP charts.** The
cycle is old, but it is exactly what the user's G1000 loads, so a clearance "via LENOV 2J" always
exists in the cockpit. AIP text rules (§4) are current and used as-is. Where the two disagree
(a procedure name in AIP that is not in the CIFP), the CIFP wins and the difference is listed in
`data/airports/EPKK/SOURCE.md`.

## 3. Goals

### 3.1 Positions

| Position | Frequency (AIP, 8.33) | Role |
|---|---|---|
| Kraków ATIS | 126.130 | as today, plus: names the initial-contact unit, "DELIVERY is operating on frequency 121.980" during Delivery hours |
| Kraków Delivery | 121.980 | IFR clearances. Open 0600–2200 UTC summer (0500–2100 winter) per AIP; outside those hours Ground issues clearances |
| Kraków Ground | 118.105 | as today, plus push/start for IFR departures |
| Kraków Tower | 123.255 | as today, plus IFR departures/arrivals; AIP rule: after GROUND→TOWER transfer the pilot omits the initial call |
| Kraków Approach | 121.075 (126.530 accepted as the same unit) | radar approach control in the Kraków TMA: departures after take-off, STAR arrivals, vectors, approach clearances, VFR entering/leaving controlled airspace above the CTR |
| Adjacent units | see below | "boundary" units: they only accept a handed-off flight and hand an inbound one to Kraków Approach. No en-route service |

Adjacent units, from AIP AIRAC 10-26 (copies in the Obsidian vault, `raw/virtual-atc/`):
- **Warszawa Radar 127.660**: ACC sector EPWW J (LOW), FL095–FL335, the sector over Kraków
  (ENR 2.1.2). Flights leaving the TMA at or above FL095.
- **Kraków Information 119.275**: FIS, for IFR flights leaving the TMA below FL095 into class G
  (VAC 13-1 shows FIS KRAKÓW 119.275; a second FIS frequency, 119.950, south of the TMA, is not
  used in v1).
- **Katowice Tower 129.255**: AIP AD 2 EPKT: Kraków Approach (121.075, 135.405) also serves
  Katowice; IFR departures from EPKT call Kraków Approach. There is no Katowice Approach. A
  flight to EPKT is handed by Kraków Approach to Katowice Tower near EPKT; project 1b replaces this
  stub with full EPKT positions.

**Decision (mine):** one Approach unit, not separate Approach/Director sectors. apt.dat calls
126.525 "Director"; AIP calls both frequencies "Kraków Approach".

**Decision (mine):** Approach and the adjacent units speak English only (v1 spec: radar and urgent
instructions always English). Delivery follows the session language like Ground and Tower; if a
Polish clearance wording is not in the phraseology reference used for plan 3a, it is marked
"project wording" as before.

### 3.2 IFR departure (user on the C172 G1000)

1. ATIS letter and initial-contact unit.
2. Clearance request about 10 min before start-up: callsign, stand, information letter, IFR,
   destination, requested level if not the default. The destination is said by the pilot (ICAO code
   or airport name known to apt.dat). **Decision (mine): no flight-plan file or form; the request
   is the flight plan.** Default cruise level for the C172: 5000 ft (below the 6500 ft transition
   altitude), pilot may request another.
3. Clearance: destination, SID, initial climb, squawk; readback checked like today
   ("cleared to Katowice via SUDOL 2J departure, climb 5000 ft, squawk 4201"). Every IFR departure
   gets a SID (AIP 2.21: departures follow the published SIDs), even a short hop. The SID is chosen
   from the runway in use and the destination: the SID whose last fix is closest in direction to the
   destination (EPKT, west-north-west → SUDOL, NUMBA or EMBIK; ties broken by the shorter SID).
   **Altitude constraints above the cleared level do not apply** (ICAO reading: the aircraft stops
   at the cleared level); compliance checks only constraints at or below it. SIDs end at fixes
   50–70 NM out, so Approach shortens the route as AIP allows ("expect direct routing/shortcuts"):
   once the aircraft has passed the SID's first turn and is climbing to its level, Approach clears
   it direct to the destination and then hands it off: to Katowice Tower near EPKT for a flight to
   Katowice (Kraków Approach serves EPKT), otherwise at the TMA boundary to Warszawa Radar or
   Kraków Information (adjacent units above).
4. Ground: push and start (C172 parks nose-out on GA stands; "start-up approved" without push is
   valid), taxi as today (follow-me rule from AIP 2.20 is already implemented).
5. Tower: line up, take-off clearance; after take-off "contact Kraków Approach 121.075".
6. Approach: radar identification, climb, possible direct-to a SID fix, monitoring the SID track
   and altitude; hand-off to the adjacent unit at the TMA boundary or on reaching the cleared level
   near it. After the hand-off the session ends for that flight (the adjacent unit says
   "identified" once).

### 3.3 IFR arrival

1. The pilot starts outside the Kraków TMA and calls the adjacent unit or Kraków Approach directly.
   Other airports are not modelled: for the acceptance flight EPKT→EPKK the pilot departs EPKT on
   their own (or starts airborne in X-Plane) and makes the first call airborne. **Decision (mine):**
   the adjacent units have no ground or clearance logic; calling Kraków Approach directly within
   about 40 NM is accepted.
2. Approach: identification, then the route to the approach:
   - a STAR is assigned only when the aircraft is outside the STAR's start fix on its way in
     ("MAPIK 1G arrival, descend 5000 ft, QNH 1013, expect ILS approach runway 25");
   - when the aircraft is already inside or beside the start fixes (EPKT is), Approach clears it
     direct to the IAF or gives radar vectors; no STAR is joined half-way;
   - altitude constraints above the cleared level do not apply, as for SIDs.
3. **Default (mine, from AIP 2.21 "radar vectoring … with distance-to-go"): radar vectors to the
   final approach** (headings, altitudes, distance-to-go, "cleared ILS approach runway 25" when
   intercepting). On pilot request ("request full procedure" / "request RNAV approach") Approach
   clears the full procedure from the IAF instead.
   Approach types: ILS 25, VOR/DME 07/25, RNAV 07/25 as in the CIFP. Default on RWY 25: ILS; on
   RWY 07 (no ILS): RNAV 07, which the G1000 flies; VOR/DME on request.
4. Transfer to Tower at the final approach fix (or "established" report); Tower clears to land;
   after landing as today.
5. Missed approach: the published missed-approach procedure from the CIFP; Tower hands the flight
   back to Approach.

### 3.4 VFR

- VFR points from AIP AD 2.22 §3.1 (BRAVO, DELTA, ECHO, HOTEL, INDIA, KILO, LIMA, MIKE, OSCAR,
  ROMEO, SIERRA, UNIFORM, ZULU, coordinates in the AIP) go into `procedures.toml` and the parser.
- Entry into the CTR on request to Tower with a point ("SP-ABC, at ZULU, 2000 ft, request
  joining"); exit via a point; arrivals and departures from the south only via SIERRA or KILO;
  VFR holding over KILO (west of the point) or ZULU at or below 2000 ft when busy.
- VFR flights stay on Tower after take-off (AIP 2.22 §1).
- Circuits: unchanged (ICAO standard, left, 1800 ft); AIP publishes none.
- Night: visual approaches are prohibited 2100–0500 UTC; Tower does not clear them.

### 3.5 Compliance

The existing compliance monitoring (altitude, position, hold-short) is extended to SID/STAR and
approach tracks and to radar vectors: lateral deviation from the procedure or assigned heading,
altitude busts against the cleared level, and approach-not-established. Interventions use ICAO
phraseology. Thresholds are left to the builder.

### 3.6 Built for project 2

Every position and procedure works on any aircraft in the engine, not only the user's: project 2
will put server-driven traffic through exactly these positions. No logic may assume a single
aircraft or that the aircraft is the user's.

### 3.7 Built for project 1b (a second airport)

Project 1b adds Katowice (EPKT) as a full airport with its own positions. Nothing here may assume
EPKK: the airport, its positions, procedures, VFR points and AIP rules are data
(`data/airports/<ICAO>/` plus the X-Plane navdata for that ICAO), and adding EPKT in project 1b
should need data, not code. The Katowice boundary unit of this project is a stub to be replaced.

## 4. AIP rules that must hold (AD 2.20 / 2.21 / 2.22, AIRAC 10-26)

- Clearances not earlier than 30 min before the estimated off-block time; request 10 min before
  ready.
- Ground and Delivery open "subject to traffic"; ATIS states the initial-contact unit.
- After GROUND→TOWER transfer: no initial call; Tower calls the aircraft.
- Aircraft at the holding point are assumed ready for immediate departure.
- IFR after take-off: contact Approach as soon as possible on the frequency Tower gives. VFR stays
  on Tower.
- Departures follow the published SIDs, especially the first turn.
- STARs end at the IAFs; VOR approaches are not linked to STARs; holdings as directed by ATC;
  shortcuts off-peak; turn to final may be by radar vectors.
- Radio failure inbound not on a STAR: squawk 7600, after 3 min to KAK DVOR/DME, hold, VOR
  approach (the controller recognises 7600 and keeps the airspace clear; full RCF handling is not
  in scope).
- Transition altitude 6500 ft; levels above it as flight levels, below as altitudes with QNH.

## 5. Non-goals

- En-route control; other airports' approach control (the adjacent units are boundary stubs).
- Datalink departure clearance (DCL), low-visibility procedures, de-icing.
- Holding patterns flown by the user on instruction are in scope only as the instruction
  "hold at <fix> as published"; holding sequencing logic belongs to project 4.
- Helicopter procedures (project 5).
- Updating the CIFP to a current AIRAC cycle.

## 6. Anti-goals (failure even if it works)

- The controller clears a procedure, fix or approach that the G1000 cannot load.
- The controller invents a route or altitude where the CIFP or AIP gives one.
- Any existing scenario (`scenarios/*.toml`) changes its expected output, except where an AIP rule
  in §4 genuinely changes an exchange (e.g. no initial call to Tower), and then the expected file
  is updated with that reason in the commit.
- Approach logic that only works for one aircraft.
- Added voice latency on the existing path.

## 7. How it is checked

- Unit tests on the CIFP parser against `EPKK.dat` (all procedures above load, fixes resolve).
- Replay scenarios (text, synthetic tracks): IFR departure EPKK→EPKT on RWY 25 and 07, IFR arrival
  with STAR + ILS 25, arrival with vectors to final, VOR/DME 07 approach with missed approach, VFR
  entry via ZULU and exit via SIERRA, a clearance request outside Delivery hours (Ground issues it).
- In-sim acceptance by the user on the C172 G1000: one IFR departure EPKK→EPKT, one IFR arrival
  EPKT→EPKK with ILS 25, one VFR entry via a VFR point. A Russian user guide for these flights in
  `docs/user-guide/ru/`.

## 8. Left to the builder

The CIFP leg types to support beyond what EPKK uses (support exactly those EPKK.dat contains),
the internal model of procedures, compliance thresholds, the exact phrase templates within ICAO
phraseology, how adjacent-unit frequencies are stored, and the order of work inside the plan.
Frequencies of the adjacent units and the SID initial-climb values must be taken from the AIP or
CIFP, not guessed; if not found, the plan lists them as open items for the user.
