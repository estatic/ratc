# Scenario: IFR flight EPKK → EPKT, runway 25 → ILS 27

[Русский](../ru/scenario-ifr-departure.md) · **English**

A complete flight in a Cessna 172 G1000, from a stand in Kraków to a stand in Katowice: the SUDOL 2J departure and radar vectors to the ILS for runway 27. Callsign SP-ABC, abbreviated Sierra Bravo Charlie. The full reference is [ifr-and-vfr-points.md](ifr-and-vfr-points.md).

The controller lines below were recorded from the server running two stations, EPKK and EPKT, with the airport layouts from X-Plane 11 (default scenery), procedures from the X-Plane CIFP and this weather: wind 250°/8 kt, QNH 1013, summer, 10:00 UTC. Your headings, distances, altitudes and QNH may differ: read back what you actually hear.

## Enable the Katowice station

The repository ships only the EPKK station. Create EPKT with the wizard (it needs `xplane_root` in `config/server.toml` and an OpenAIP key, see `docs/RUNNING.md`, "Stations and the airport wizard"):

```
target/release/atc-server airports
```

Menu item 1 → find EPKT → tick the positions ATIS 120.230, Delivery 124.080, Ground 121.805, Tower 129.255, Approach 121.075 (Kraków Approach) → confirm. The station is written to `data/airports/EPKT/`.

Then remove (or comment out) the `airport = "EPKK"` line in `config/server.toml`. Without it the server loads every station in `data_dir`; the log should start with `stations: EPKK, EPKT`. Any other stations in `data/airports` are loaded too and do not affect this flight. There is no separate station list in the config.

Kraków Approach on 121.075 belongs to both stations. While you fly out of Kraków you talk to the EPKK station's Approach. 30 NM before Katowice it hands you to the EPKT station's Approach: same callsign, same frequency, but the hand-off is still announced and has to be read back.

## Preparation

```
cargo build --release -p atc-server -p atc-client
tools/fetch-piper.sh          # once: voices for Delivery, Approach, Radar
target/release/atc-server     # terminal 1
target/release/atc-client     # terminal 2
```

Before engine start:
- listen to the Kraków ATIS on 126.130 (letter, runway 25, QNH);
- put the aircraft on stand **9R** in Kraków;
- transponder 2000, STBY;
- set up the G1000 as in the next section.

## G1000 plan and altitudes

**FPL**: EPKK → EPKT. **PROC → Select Departure → SUDOL 2J** (runway 25; SUDOL 1F for runway 07). **PROC → Select Approach → ILS 27**, transition **Vectors**. NAV1: IKTO 109.90, course 265°.

The transition altitude is 6500 ft at both airports. Above it (on a flight level) set 1013 (STD), below it the QNH.

| Phase | Altitude | Source |
|---|---|---|
| Stand at EPKK | 791 ft (field elevation) | X-Plane apt.dat |
| Take-off runway 25 | threshold 779 ft | CIFP EPKK |
| Delivery clearance | FL100 | highest "at or above" constraint on SUDOL 2J |
| KK599, first SID fix | at or above 3000 ft | CIFP, SUDOL 2J |
| KK602, KK604 | at or above 6000 ft and FL100 | CIFP, SUDOL 2J |
| Hand-off to EPKT Approach (30 NM from EPKT) | still climbing; about 4200 ft in the recording | hand-off rule |
| First call to EPKT Approach | climb to 5000 ft ("climb altitude five thousand feet") | EPKT station `ifr.cruise_ft` |
| About 15 NM from the intercept point | descend to 3000 ft | EPKT Approach |
| Vectors to the ILS | 3000 ft, no lower | FAF altitude in the CIFP |
| KT537 (IF, 9.3 NM IKTO DME) | 3000 ft | CIFP, ILS 27 |
| KT536 (FAF, 7.8 NM IKTO DME) | 3000 ft, 3.0° glideslope intercept | CIFP, ILS 27 |
| Runway 27 threshold | threshold elevation 1003 ft, threshold crossing 1055 ft | CIFP EPKT |
| Missed approach | climb to 4000 ft | CIFP, ILS 27 |
| Stand at EPKT | 995 ft (field elevation) | X-Plane apt.dat |

A Cessna 172 will not reach FL100 before the hand-off to Katowice; expect to be somewhere around 4000–5000 ft. That is fine: EPKT Approach assigns the altitude itself.

## Delivery 121.980

Delivery is open 06–22 UTC in summer (roughly 28 March to 28 October) and 05–21 UTC in winter. Outside those hours say the same lines to Ground.

| Who | Phrase |
|---|---|
| You | Krakow Delivery, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, stand nine right, information Alpha, IFR to Katowice, request clearance |
| Delivery | Sierra Papa Alpha Bravo Charlie, cleared to Katowice, SUDOL two Juliett departure, climb flight level one zero zero, squawk four two zero one |
| You | Cleared to Katowice, Sudol two Juliett departure, climb flight level one zero zero, squawk four two zero one, Sierra Bravo Charlie |
| Delivery | Sierra Bravo Charlie, readback correct, contact Kraków Ground one one eight decimal one zero five |
| You | Ground one one eight decimal one zero five, Sierra Bravo Charlie |

Set squawk 4201 and the G1000 selected altitude to FL100 (10000 ft).

If Delivery answers **"say flight rules"**, your request named neither IFR nor VFR. Ask again with the flight rules ("IFR to Katowice, request clearance").

### A different runway

On the stand or while taxiing you can ask Delivery or Ground for another departure runway:

| Who | Phrase |
|---|---|
| You | Krakow Ground, Sierra Bravo Charlie, request departure runway zero seven |

The runway is approved ("runway zero seven approved") when its tailwind is 5 kt or less; otherwise you hear "unable, runway in use two five". If you already have a clearance for the other runway, the controller adds "runway changed, runway zero seven" and the new SID (SUDOL 1F for 07): read back the runway and the SID. If you are already taxiing to the other runway, you get a new taxi clearance.

## Ground 118.105

| Who | Phrase |
|---|---|
| You | Krakow Ground, Sierra Papa Alpha Bravo Charlie, stand nine right, information Alpha, request start up |
| Ground | Sierra Papa Alpha Bravo Charlie, start-up approved, QNH one zero one three |
| You | Start up approved, QNH one zero one three, Sierra Bravo Charlie |
| You | Krakow Ground, Sierra Bravo Charlie, information Alpha, request taxi |
| Ground | Sierra Bravo Charlie, taxi to holding point runway two five via Zulu four, Zulu five, Alpha, hold short runway two five |
| You | Taxi to holding point runway two five via Zulu four, Zulu five, Alpha, hold short runway two five, Sierra Bravo Charlie |
| Ground (at the runway, about 2 min of taxiing) | Sierra Bravo Charlie, contact Kraków Tower one two three decimal two five five, have a nice flight |
| You | Tower one two three decimal two five five, Sierra Bravo Charlie |

Set QNH 1013: the altimeter reads 791 ft on the stand. After reading back the taxi clearance, switch the transponder to ALT.

The taxi route depends on the airport scenery X-Plane loads. Read back the one Ground gives you.

## Tower 123.255

Switch COM1 and **stay quiet**: Tower calls you.

| Who | Phrase |
|---|---|
| Tower | Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared for take-off |
| You | Runway two five, cleared for take-off, Sierra Bravo Charlie |
| Tower (right after lift-off) | Sierra Bravo Charlie, contact Kraków Approach one two one decimal zero seven five |
| You | Approach one two one decimal zero seven five, Sierra Bravo Charlie |

After take-off fly the SID in NAV mode and keep climbing. Cross KK599 at or above 3000 ft.

## Kraków Approach 121.075: departure

| Who | Phrase |
|---|---|
| You (at 1500 ft) | Krakow Approach, Sierra Papa Alpha Bravo Charlie, Sudol two Juliett departure, passing one thousand five hundred feet, climbing flight level one zero zero |
| Approach | Sierra Papa Alpha Bravo Charlie, identified |
| Approach (30 NM from EPKT, about 7 min after take-off, before KK602) | Sierra Bravo Charlie, contact Kraków Approach one two one decimal zero seven five |
| You | Approach one two one decimal zero seven five, Sierra Bravo Charlie |

On the first call say the altitude you are actually passing.

The frequency does not change, so leave COM1 alone: this is the hand-off to the EPKT station's Approach. Keep flying the SID and climbing until that controller gives you a route. On this flight you do not get "direct Katowice" or a 20 NM descent from the Kraków Approach: the hand-off comes first.

## Kraków Approach 121.075: arrival and ILS 27

Before calling, listen to the Katowice ATIS on COM2, 120.230 (in the recording: information Alpha, runway in use two seven). Then call Approach with the altitude you are passing and your cleared level:

| Who | Phrase |
|---|---|
| You | Kraków Approach, Sierra Papa Alpha Bravo Charlie, passing four thousand seven hundred feet, climbing flight level one zero zero, inbound to Katowice, information Alpha |
| Approach | Sierra Papa Alpha Bravo Charlie, identified, turn right heading three four five, climb altitude five thousand feet, QNH one zero one three, expect ILS approach runway two seven |
| You | Right heading three four five, climb five thousand feet, QNH one zero one three, expect ILS approach runway two seven, Sierra Bravo Charlie |
| Approach (about 15 NM from the localizer intercept point) | Sierra Bravo Charlie, descend to altitude three thousand feet |
| You | Descend three thousand feet, Sierra Bravo Charlie |
| Approach | Sierra Bravo Charlie, turn right heading zero zero five |
| You | Right heading zero zero five, Sierra Bravo Charlie |
| Approach (about 10 NM from the threshold) | Sierra Bravo Charlie, turn left heading two niner five, one zero miles from touchdown, cleared ILS approach runway two seven, report established |
| You | Left heading two niner five, cleared ILS approach runway two seven, wilco, Sierra Bravo Charlie |
| Approach (on the localizer, inside the FAF) | Sierra Bravo Charlie, contact Katowice Tower one two niner decimal two five five |
| You | Tower one two niner decimal two five five, Sierra Bravo Charlie |

This is radar vectoring, with no arrival procedure. Heading 345 takes you north, east of Katowice; then 005, and a left turn onto the runway 27 localizer from the east. Level off at 5000 ft, the EPKT station's arrival altitude. If you are already above 5000 ft when you call, you hear "descend to altitude five thousand feet" instead; if you are already close to the intercept point, you get 3000 ft straight away.

Fly the headings in HDG mode. Read a heading back straight away: without a readback Approach says "read back heading…" after about 20 s, and if you do not turn, "check heading, fly heading …".

On "descend" set 3000 ft and descend. Do not go below 3000 ft, or Approach says "check level, maintain altitude three thousand feet".

On "cleared ILS approach", in the G1000: **PROC → Activate Vector-to-Final**, NAV1 109.90, APR mode. Hold 3000 ft until you meet the glideslope at KT536, then follow it down.

If you are already on the localizer and on the glideslope, Approach may hand you to Tower without waiting for "established". Report "established" to Tower instead.

## Katowice Tower 129.255: landing

| Who | Phrase |
|---|---|
| You | Katowice Tower, Sierra Papa Alpha Bravo Charlie, established ILS runway two seven |
| Katowice Tower | Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two seven, cleared to land |
| You | Runway two seven, cleared to land, Sierra Bravo Charlie |
| Katowice Tower (on the landing roll) | Sierra Bravo Charlie, vacate, contact Katowice Ground one two one decimal eight zero five, good day |
| You | Vacating, Ground one two one decimal eight zero five, Sierra Bravo Charlie |

If you do not read back the frequency, Tower reminds you after about 15 s: "read back frequency".

## Katowice Ground 121.805

Vacate the runway and stop past the holding line.

| Who | Phrase |
|---|---|
| You | Katowice Ground, Sierra Papa Alpha Bravo Charlie, runway vacated, request taxi |
| Ground | Sierra Papa Alpha Bravo Charlie, say stand |
| You | Katowice Ground, Sierra Bravo Charlie, request taxi to stand apron four golf alpha |
| Ground | Sierra Bravo Charlie, taxi to stand apron four Golf Alpha via Sierra, Hotel |
| You | Taxi to stand apron four Golf Alpha via Sierra, Hotel, Sierra Bravo Charlie |
| You (on the stand, about 5 min of taxiing) | Sierra Bravo Charlie, on stand apron four golf alpha |
| Ground | Sierra Bravo Charlie, good day |

You can name the stand straight away: "runway vacated, request taxi to stand apron four GA" gets the same clearance without "say stand". Without a stand, Ground sends you to your own stand if it knows one, otherwise it asks "say stand". After landing the taxi clearance always goes to a stand, with no QNH or squawk.

Stand names from the scenery are matched on their letters and digits, so "APRON4 GA" can be said as "apron four GA" or "apron four golf alpha". Several stands share that name; Ground picks the nearest free one. Ground says it as "apron four Golf Alpha".

That is the end of the flight: transponder to STBY, shut down. In the recording it took about 40 minutes from the clearance request to the stand: take-off in minute 3, landing in minute 34.

## Things to watch

- All controller headings are magnetic.
- A wrong readback gets `negative, …`. Read back again correctly.
- Leaving the route or the level gets `check position`, `check heading` or `check level`.
- Say your callsign in every transmission, or the controller reminds you.
- Check that "VOR D M E" and "R NAV" sound clear.
