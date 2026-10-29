# EPKK data sources

## apt.dat
- Source: X-Plane Scenery Gateway, https://gateway.x-plane.com/apiv1/airport/EPKK
- Licence: GPLv2 (Scenery Gateway terms); not covered by the project licence
- recommendedSceneryId: 112298 (fetched 2026-10-19 with tools/fetch-gateway-apt.sh; file EPKK.dat from masterZipBlob)
- Contains 3 runway taxi edges, 142 1201/1202 lines in total; edge types seen: runway, taxiway_C/D/E.

## AIP (AIP Polska eAIP IFR, AIRAC AMDT 10-26, effective 01 OCT 2026)
Page: https://docs.pansa.pl/ais/eaipifr/AIRAC%20AMDT%2010-26_2026_10_01/eAIP/AD%202%20EPKK%201-en-GB.html

| Item | AIP value | Section |
|---|---|---|
| Transition altitude | 6500 ft AMSL | AD 2.17 |
| TWR KRAKOW TOWER | 123.255 MHz | AD 2.18 |
| GND KRAKOW GROUND | 118.105 MHz | AD 2.18 |
| DELIVERY KRAKOW DELIVERY | 121.980 MHz | AD 2.18 |
| ATIS | 126.130 MHz (D-ATIS) | AD 2.18 |
| APP KRAKOW APPROACH | 121.075, 126.530 (also 126.975 listed) | AD 2.18 |

## Frequency discrepancy (AIP wins)
apt.dat has 25 kHz-style channel names: ATIS 126.125, DEL 121.975, GND 118.100, TWR 123.250, APP 121.075 / 126.525.
AIP publishes the 8.33 kHz channels: 126.130, 121.980, 118.105, 123.255, 121.075 / 126.530.
Differences are 5 kHz (the 8.33 channel name vs its 25 kHz label); APP 121.075 is identical.

## Values not published in AIP (user/controller rulings)
- Circuits for RWY 07 and RWY 25: Left, 1800 ft AMSL (1000 ft AGL above apt.dat field elevation 791 ft = 1791, rounded to 100 ft). ICAO standard, user-approved -- not published in AIP (neither IFR nor VFR eAIP publishes an EPKK circuit).
- preferred_runway = "25": controller ruling, prevailing westerlies. AIP names no preferred runway.

## 8.33 / 25 kHz label difference
AIP lists 8.33 kHz channel frequencies (e.g. 118.105); apt.dat carries the 25 kHz label of the same channel (118.100). Tests match PositionDef.khz to apt.dat within 5 kHz; procedures.toml uses the AIP values.

## Plan 4 additions (AIP Poland AIRAC AMDT 10-26)
| Item | Value | Section |
|---|---|---|
| 13 VFR points (BRAVO ... ZULU) | coordinates as published, converted from DMS to decimal degrees | AD 2.22 3.1 |
| Arrivals/departures from the south | possible only via SIERRA and KILO (`south = true`) | AD 2.22 3.2 |
| VFR holding | over KILO or ZULU, not higher than 2000 ft AMSL (KILO hold west of the point) | AD 2.22 3.3 |
| Kraków Approach | 121.075, 126.530 (+126.975 listed, unused); 135.405 from EPKT | AD 2.18; AD 2 EPKT 2.18 |
| Katowice Tower | 129.255; "IFR departures from EPKT contact KRAKÓW APPROACH" | AD 2 EPKT 2.18 |
| Warszawa Radar | 127.660, sector EPWW J (LOW), FL095-FL335 | ENR 2.1.2 |
| Kraków Information (FIS) | 119.275 | VAC chart AD 2 EPKK 13-1 (16 APR 2026) |
| Magnetic variation | 6°E (2024) | AIP VAC AD 2 EPKK 13-1: VAR 6°E 2024 |
| Delivery hours | 0600-2200 UTC summer, 0500-2100 UTC winter | AD 2.20 |

## Rulings and design decisions (not published as such)
- TMA radius 40 NM / ceiling FL095 (9500 ft): approximation of the KRAKÓW TMA polygon, ruling.
- Default IFR cruise 5000 ft: design decision (spec 3.2).

## X-Plane CIFP (procedures)
- Source: the user's X-Plane 11 install, `Resources/default data/CIFP/EPKK.dat` (ARINC 424), read at run time through `xplane_root` in `config/server.toml`. The file is **not** copied into the repository (navdata licence); real-install tests read it from the install and skip with a note when it is missing. Fix coordinates come from the install's `earth_fix.dat` / `earth_nav.dat`; `Custom Data/` overrides `Resources/default data/` when present.
- AIRAC cycle **1802** (February 2018, Navigraph/Jeppesen), which is what the user's G1000 loads.
- Procedures present in the file (CIFP identifier = first four letters of the fix + number + letter):

| Kind | Procedures | Runway |
|---|---|---|
| SID | BABKO, BAREX, EMBIK, LENOV, LUXAR, NAVUR, NUMBA, POBOK, SUDOL, TUSIN: `1F` | 07 |
| SID | the same ten fixes: `2J` | 25 |
| STAR | BAREX, GOVRI, KUKAM, LENOV, MAPIK, MEBAN, NETIR, PADKA: `1G` | 25 |
| STAR | the same eight fixes: `1H` | 07 |
| Approach | `I25` ILS 25; `D07`, `D25` VOR/DME 07 and 25; `R07`, `R25` RNAV 07 and 25 | 07, 25 |

- Spoken names are built from the CIFP fix and designator ("SUDOL two Juliett departure", "MAPIK one Golf arrival").
- **CIFP wins over AIP for procedure names.** The cycle is eight years older than AIRAC 10-26, so SID/STAR/approach names, fixes (`KK599`-style computer fixes) and altitudes may differ from today's charts. A clearance is only issued for a procedure the G1000 can load; where the current AIP chart differs, the CIFP is followed and the difference is not corrected here. AIP *text* rules (AD 2.20 to 2.22) are current and used as published.
- Without this file (no `xplane_root`, or no EPKK.dat) the controller answers IFR requests "unable, IFR not available, VFR only" and runs VFR only.
- TMA radius 40 NM / ceiling FL095: the ruling listed under "Rulings and design decisions" above; it decides whether an arriving flight is "inside the TMA" and gets vectors instead of a STAR.
