# English phraseology reference

English wording the virtual ATC speaks for the items added with plan 4 (procedures and positions: Delivery, Approach, adjacent units, SIDs, STARs, approaches, radar vectors, VFR points). The exact strings come from `crates/atc-phraseology/src/render.rs`; pilot-side wording that the parser accepts is in `crates/atc-phraseology/src/parse.rs`. The Polish counterparts are in `docs/phraseology/pl.md`.

## Sources

- ICAO Doc 4444 (PANS-ATM), chapter 12, Phraseologies: ATC clearances and instructions, radar and approach control, transfer of control, departure and arrival phraseology.
- ICAO Doc 9432, Manual of Radiotelephony: pilot read-back and frequency-change phraseology.
- CAP 413 (UK Radiotelephony Manual): used where Doc 4444 is silent.
- AIP Poland, AD 2 EPKK 2.20 to 2.22 (AIRAC AMDT 10-26): local rules (no initial call to Tower after a Ground transfer, VFR points).

Entries marked *project wording* have no counterpart in these documents (or only a loose one) and were chosen for this project. Where the cell says "Doc 4444 ch. 12", the phrase is a standard ICAO phraseology of that chapter; section numbers are not quoted because the project has no copy of the document to verify them against.

## Conventions

- The callsign opens a transmission, in full on first contact, abbreviated afterwards (`Sierra Bravo Charlie`). Items are joined with ", ".
- Digits one by one (`niner` for 9); altitudes as thousands and hundreds ("five thousand feet"); headings as three digits; QNH as four digits; frequencies "one two three decimal two five five" (the speech engine reads the written "nine" in a frequency as "niner").
- Altitude below the 6500 ft transition altitude is said as "altitude N feet", a flight level as "flight level N".
- Fixes and procedure names are spoken as written (`SUDOL`, `MAPIK`); the designator letter uses the ICAO alphabet ("SUDOL two Juliett departure").
- `ILS` is read as is; `VOR/DME` as "VOR D M E"; `RNAV` as "R NAV" (so the speech engine does not read a word).
- Headings are magnetic.
- Approach and the adjacent units (Warszawa Radar, Kraków Information, Katowice Tower) speak English even in a Polish session.

## Delivery, Ground, Tower

| Item | English wording | Source |
|---|---|---|
| `ClearedTo` + `Sid` + `ClimbTo` + `Squawk` | `cleared to Katowice, SUDOL two Juliett departure, climb altitude five thousand feet, squawk four two zero one` | Doc 4444 ch. 12 (ATC clearances: destination, SID, level, SSR code) |
| `ClearedTo` (VFR) + `LeaveControlZoneVia` + `Squawk` | `cleared to Nowy Targ, leave control zone via SIERRA, squawk four two zero one` | Doc 4444 ch. 12 (clearance, route) / project wording for "leave control zone via <point>" (AIP AD 2.22) |
| `StartUpApproved` | `start-up approved` (with `QNH one zero one three`) | Doc 4444 ch. 12 (start-up approved) |
| `PushAndStartApproved` | `push-back and start-up approved`, optionally `facing <direction>` | Doc 4444 ch. 12 (push-back, start-up) |
| `ReadbackCorrect` | `readback correct` | Doc 4444 ch. 12 / CAP 413 (read-back correct) |
| `Negative` | `negative` (with the correct item repeated: `negative, SUDOL two Juliett departure`) | Doc 4444 ch. 12 (negative) |
| `Contact` | `contact Kraków Ground one one eight decimal one zero five`; `contact Kraków Tower one two three decimal two five five`; `contact Kraków Approach one two one decimal zero seven five` | Doc 4444 ch. 12 (CONTACT ... frequency) |
| `Courtesy` | `have a nice flight`, `good day` | CAP 413 / project wording |
| `EnterControlZoneVia` | `enter control zone via ZULU, altitude two thousand feet` | project wording (AIP AD 2.22 VFR point entry) |
| `ReportAt` | `report at ZULU` | Doc 4444 ch. 12 (REPORT ... position) |
| `JoinDownwind` | `join downwind runway two five` (then `report downwind`) | CAP 413 (joining the circuit) |
| `FrequencyChangeApproved` | `frequency change approved` | Doc 4444 ch. 12 (frequency change approved) |
| `HoldOver` | `hold over KILO, maximum altitude two thousand feet` | project wording (AIP AD 2.22 VFR holding) |
| `Unable(SouthernEntry)` | `unable, entry from the south only via the southern points` | project wording (AIP AD 2.22 3.2) |
| `Unable(NoIfrData)` | `unable, IFR not available, VFR only` | project wording |
| `Unable(UnknownDestination)` | `unable, say again destination` | project wording |
| `Unable(Closed)` | `unable, station closed` | project wording |
| `Unable(ApproachNotAvailable)` | `unable, approach not available` | Doc 4444 ch. 12 (UNABLE) / project wording for the reason |
| `VisualApproachNotAvailable` | `visual approach not available` | project wording (night, 2100 to 0500 UTC) |
| Tower "no initial call" | Tower speaks first after a Ground transfer: `wind two five zero degrees, eight knots, runway two five, cleared for take-off` | AIP AD 2.20 |

## Approach and adjacent units

| Item | English wording | Source |
|---|---|---|
| `Identified` | `identified` | Doc 4444 ch. 12 (IDENTIFIED) |
| `Star` | `MAPIK one Golf arrival` | Doc 4444 ch. 12 (clearance to arrival route) / project wording for the spoken form |
| `MaintainLevel` | `maintain altitude five thousand feet` | Doc 4444 ch. 12 (MAINTAIN) |
| `ClimbTo` | `climb altitude five thousand feet` / `climb flight level eight zero` | Doc 4444 ch. 12 (CLIMB TO) |
| `DescendTo` | `descend to altitude three thousand feet` | Doc 4444 ch. 12 (DESCEND TO) |
| `Heading` | `turn left heading one zero five` / `turn right heading ...`; without a turn direction `fly heading ...` | Doc 4444 ch. 12 (TURN LEFT/RIGHT HEADING, FLY HEADING) |
| `DirectTo` | `proceed direct Katowice` | Doc 4444 ch. 12 (PROCEED DIRECT TO) |
| `ExpectApproach` | `expect ILS approach runway two five`; `expect VOR D M E approach runway zero seven`; `expect R NAV approach runway zero seven` | Doc 4444 ch. 12 (EXPECT ... APPROACH) |
| `DistanceToGo` | `one one miles from touchdown` | Doc 4444 ch. 12 (distance from touchdown) |
| `ClearedApproach` | `cleared ILS approach runway two five` | Doc 4444 ch. 12 (CLEARED ... APPROACH) |
| `ReportEstablished` | `report established` | Doc 4444 ch. 12 (REPORT ESTABLISHED) |
| Approach clearance, full | `turn right heading two two zero, one one miles from touchdown, cleared ILS approach runway two five, report established` | Doc 4444 ch. 12 (composed) |
| `RadarServiceTerminated` | `radar service terminated` | Doc 4444 ch. 12 (RADAR SERVICE TERMINATED) |
| Hand-off to Katowice Tower | `contact Katowice Tower one two niner decimal two five five` | Doc 4444 ch. 12 (CONTACT ...) |
| Hand-off to Kraków Information | `contact Kraków Information one one niner decimal two seven five` | Doc 4444 ch. 12 (CONTACT ...) |
| Missed approach (pilot) | `going around`, then `missed approach runway zero seven, climbing four thousand feet` | Doc 4444 ch. 12 (pilot report) / CAP 413 |
| Missed approach (controller) | `turn left heading two seven zero, climb altitude three thousand feet` | Doc 4444 ch. 12 (missed approach instruction) |
| Pilot readback of an approach clearance | `cleared ILS approach runway two five, wilco` after the heading | Doc 4444 ch. 12 / CAP 413 (read-back; `wilco` for `report established`) |
| Pilot reports | `established ILS runway two five`; `established VOR DME approach runway zero seven` | Doc 4444 ch. 12 (REPORT ESTABLISHED) |
| Pilot first call (IFR departure) | `Sudol two Juliett departure, passing one thousand five hundred feet, climbing five thousand feet` | CAP 413 (initial call to Approach) |
| Pilot first call (arrival) | `IFR, five thousand feet, inbound Krakow, information Alpha` | CAP 413 (initial call) |

## Project wording (monitoring and compliance)

These interventions have no verbatim counterpart in Doc 4444 or CAP 413 and are based on the standard radar phraseology of the same chapter. They are chosen to be short, unambiguous and to name the correct action.

| Item | English wording | Note |
|---|---|---|
| `OffTrack` | `you are two miles off track, check position` (`one mile` for 1) | project wording; the lateral-deviation threshold is in the compliance code |
| `CheckHeading` | `check heading, fly heading two five five` | project wording |
| `CheckLevel` | `check level, maintain altitude five thousand feet` | project wording |
| `CheckEstablished` | `check established, report established localizer runway two five` (`final approach track` for VOR/DME and RNAV) | project wording |
| `CheckAltitude` | `check altitude, circuit altitude one thousand eight hundred feet` | existing (plan 3) |
| `CheckSquawk` / `SquawkCharlie` | `check squawk four two zero one` / `squawk Charlie` | existing (plan 3), CAP 413 |

## Pilot read-back labels

When a read-back is wrong or incomplete, the controller says `read back <items>` using these labels: departure procedure (SID), arrival procedure (STAR), level, direct routing, approach clearance, destination, plus the earlier runway, hold short instruction, taxi route, clearance, altitude, heading, frequency, QNH and squawk. Source: Doc 4444 ch. 12 (READ BACK) with the labels as project wording.
