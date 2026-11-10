# IFR and VFR reporting points in a Cessna 172 G1000 (EPKK)

[Русский](../ru/ifr-and-vfr-points.md) · **English**

Three flights to try in X-Plane 11 with a Cessna 172 G1000: an IFR departure EPKK → EPKT, an IFR arrival EPKT → EPKK with the ILS for runway 25, and a VFR entry into the Kraków control zone via the reporting point ZULU. The pilot lines below are copied verbatim from the scenarios in `scenarios/` and the controller lines from their reference outputs `*.expected.txt`, so everything here is checked by the tests (`cargo test -p atc-replay`). How to work the client and PTT in general: `docs/RUNNING.md`; a step-by-step circuit: [scenario-circuit.md](scenario-circuit.md) (English) and [polish-circuit.md](polish-circuit.md) (Polish).

The callsign in the examples is SP-ABC (`Sierra Papa Alpha Bravo Charlie`, abbreviated `Sierra Bravo Charlie`). The exchange in this guide is in English (`language = "en"` or auto-detection).

## 1. Preparation

### Voices

Delivery, Approach and the radar stations (Warszawa Radar, Kraków Information, Katowice Tower) use three additional English voices. Once, before starting the server:

```
tools/fetch-piper.sh
```

The script is idempotent; the list of voices is in `tools/MODELS.md`. Until the voices are downloaded these stations cannot speak.

### Procedure data: CIFP from X-Plane

The server reads SIDs, STARs and approaches from the CIFP of your X-Plane install: the file `Resources/default data/CIFP/EPKK.dat` (the path comes from `xplane_root` in `config/server.toml`, see `docs/RUNNING.md`). These are exactly the procedures the G1000 loads, so the controller never assigns a procedure your cockpit does not have. The X-Plane 11 data cycle is 1802 (February 2018), which is old: names and routes may differ from today's AIP charts (AIRAC 10-26). What your G1000 shows is what counts (details: `data/airports/EPKK/SOURCE.md`).

Without CIFP (wrong `xplane_root` or the file is missing) the controller answers any IFR request with `unable, IFR not available, VFR only`. Fly VFR then, or fix `xplane_root` and restart the server.

### Airport layout

Stand 9R (`niner romeo`) and the taxi routes in this guide come from the default X-Plane 11 EPKK scenery (the server reads it when `xplane_root` is set). The repository's `data/airports/EPKK/apt.dat`, used without `xplane_root`, has no stand 9R, and the taxi routes will differ.

### Frequencies

The station you talk to is the frequency of the radio selected for transmission on the audio panel (COM1 MIC or COM2 MIC; COM1 by default). You hear the replies on it and on any other COM selected for listening: for example the ATIS on COM2 while you work Tower on COM1.

| Station | Frequency | Used for |
|---|---|---|
| ATIS | 126.130 | weather, runway, information letter (the ATIS says whom to call first) |
| Kraków Delivery | 121.980 | IFR clearance; open 06:00–22:00 UTC in summer (08:00–24:00 local), 05:00–21:00 UTC in winter |
| Kraków Ground | 118.105 | start-up, taxi; outside Delivery hours it also gives the IFR clearance |
| Kraków Tower | 123.255 | take-off, landing; VFR traffic stays with Tower |
| Kraków Approach | 121.075 | departures after take-off, arrivals, vectors, approaches (126.530 is the same unit) |
| Katowice Tower | 129.255 | takes over at EPKT |
| Kraków Information | 119.275 | leaving the zone below FL095, first call when arriving from the west |

### Rules that may differ from what you are used to

- **Headings are magnetic.** Every controller heading ("heading two five five") is magnetic, as the G1000 compass (HDG) shows it. The magnetic variation around Kraków is about 6°E.
- **Tower calls you after Ground.** Once Ground has sent you to Tower and you have set 123.255 at the holding point, no initial call is needed (AIP AD 2.20): Tower gives you the take-off clearance on its own. Calling does no harm but is not required. If your transponder is not set, Tower says `check squawk` or `squawk Charlie`: set the code and mode ALT.
- **From Delivery to Ground:** the controller names the frequency (`contact Kraków Ground one one eight decimal one zero five`); read it back.
- **Readbacks are checked.** A mistake in the SID, level, QNH, code or runway gets `negative` and the item again. Read back exactly what is in the clearance.
- **Levels:** the transition altitude is 6500 ft. Below it altitudes on the QNH, above it flight levels. The default cruise level for a C172 is 5000 ft; name another one in your request. On a SID departure the clearance is no lower than the highest "at or above" constraint on the SID: FL100 on SUDOL 2J and SUDOL 1F.

## 2. Loading procedures into the G1000 (standard X-Plane 11 way)

On an IFR flight the procedures are loaded into the flight plan before departure (or before the approach). This is the general sequence for the X-Plane 11 G1000; page names and button positions may differ slightly in your version.

1. Press **FPL** and enter the departure and destination airports into the active plan (for example EPKK and EPKT).
2. Press **PROC**. In the procedures menu choose:
   - **Select Departure** for the SID;
   - **Select Arrival** for the STAR;
   - **Select Approach** for the approach.
3. In the list choose the airport, the procedure (SID/STAR), the runway and the transition, and confirm with **ENT**. The procedure is inserted into the flight plan.
4. Check the plan on the FPL page: the SID fixes and their order. The SID loaded for departure must match the controller's clearance.
5. For the approach choose **Select Approach**: the type (ILS RWY 25, RNAV RWY 07, VOR/DME), the runway and the transition (for radar vectors, "Vectors" or "Vectors to Final" where available). Confirm with **ENT**.
6. Activation: with the cursor knob choose **Activate Approach** (or **Activate Vector-to-Final** if you are on vectors) as you get close, once the controller has cleared you for the approach.

The names the controller says match the names in the G1000 list for cycle 1802: SIDs `SUDOL 2J` (runway 25) and `SUDOL 1F` (runway 07), STARs `MAPIK 1G` (runway 25) and `1H` (runway 07), approaches `ILS 25`, `RNAV 07`, `VOR/DME 07`.

## 3. Flight 1: IFR departure EPKK → EPKT

Scenarios: `scenarios/epkk-ifr-departure-25.toml` (runway 25), `scenarios/epkk-ifr-departure-07.toml` (runway 07).

### Before start-up

- Listen to the ATIS (COM1 = 126.130) and note the information letter (Alpha in the examples) and the runway.
- Load the SID into the G1000: **SUDOL 2J** for runway 25, **SUDOL 1F** for runway 07, destination EPKT. The runway and the SID depend on the wind; the controller tells you the SID in the clearance, so compare it with the one loaded and change it if needed.
- Set the transponder to 2000 (STBY): you will be given a code.

### Delivery 121.980: clearance

Pilot (10 minutes before start-up):

> Krakow Delivery, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, stand niner romeo, information Alpha, IFR to Katowice, request clearance

Delivery (runway 25):

> Sierra Papa Alpha Bravo Charlie, cleared to Katowice, SUDOL two Juliett departure, climb flight level one zero zero, squawk four two zero one

The pilot reads back and sets code 4201:

> Cleared to Katowice, Sudol two Juliett departure, climb flight level one zero zero, squawk four two zero one, Sierra Bravo Charlie

If you read back the wrong SID (for example `Sudol one Foxtrot` instead of `two Juliett`), Delivery answers:

> Sierra Bravo Charlie, negative, SUDOL two Juliett departure

Read it back correctly. After a correct readback:

> Sierra Bravo Charlie, readback correct, contact Kraków Ground one one eight decimal one zero five

Pilot:

> Ground one one eight decimal one zero five, Sierra Bravo Charlie

For runway 07 it is the same with `SUDOL one Foxtrot departure`.

Outside Delivery hours (for example at 23:00 UTC) Delivery is silent. Call Ground: it gives the clearance itself (`scenarios/epkk-ifr-clearance-night.toml`):

> Krakow Ground, Sierra Papa Alpha Bravo Charlie, stand niner romeo, information Alpha, IFR to Katowice, request clearance

Ground:

> Sierra Papa Alpha Bravo Charlie, cleared to Katowice, SUDOL two Juliett departure, climb flight level one zero zero, squawk four two zero one

After your readback Ground answers `readback correct`, with no change of frequency.

### Ground 118.105: start-up and taxi

Pilot:

> Krakow Ground, Sierra Papa Alpha Bravo Charlie, stand niner romeo, information Alpha, request start up

Ground:

> Sierra Papa Alpha Bravo Charlie, start-up approved, QNH one zero one three

Pilot:

> Start up approved, QNH one zero one three, Sierra Bravo Charlie

Taxi:

> Krakow Ground, Sierra Bravo Charlie, information Alpha, request taxi

Ground (runway 25):

> Sierra Bravo Charlie, taxi to holding point runway two five via Zulu four, Zulu five, Alpha, hold short runway two five

The pilot reads back and switches the transponder to ALT:

> Taxi to holding point runway two five via Zulu four, Zulu five, Alpha, hold short runway two five, Sierra Bravo Charlie

For runway 07 the route is different: `via Zulu three, Zulu two, Zulu one, Golf, Bravo four, Bravo three, Bravo two, Bravo one, Foxtrot, hold short runway zero seven`.

At the runway Ground sends you to Tower:

> Sierra Bravo Charlie, contact Kraków Tower one two three decimal two five five, have a nice flight

Pilot:

> Tower one two three decimal two five five, Sierra Bravo Charlie

Set COM1 to 123.255.

### Tower 123.255: take-off

You do not call: Tower calls you.

> Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared for take-off

Pilot:

> Runway two five, cleared for take-off, Sierra Bravo Charlie

Take off and fly the SID (autopilot in NAV mode on the G1000 plan, or by hand). Shortly after take-off Tower sends you to Approach:

> Sierra Bravo Charlie, contact Kraków Approach one two one decimal zero seven five

Pilot:

> Approach one two one decimal zero seven five, Sierra Bravo Charlie

Set COM1 to 121.075.

### Approach 121.075: after take-off

First call:

> Krakow Approach, Sierra Papa Alpha Bravo Charlie, Sudol two Juliett departure, passing one thousand five hundred feet, climbing flight level one zero zero

Approach:

> Sierra Papa Alpha Bravo Charlie, identified

Climb to FL100 along the SID: the clearance takes the highest "at or above" constraint on the SID (FL100 on SUDOL 2J and SUDOL 1F), capped by the SID's ceilings.

What happens next depends on which stations are loaded.

**Only the EPKK station is loaded.** Once you have passed the first turn of the SID and are climbing, Approach gives you a shortcut:

> Sierra Bravo Charlie, proceed direct Katowice

Pilot (in the G1000 **Direct-to** EPKT, **ENT**, **ENT**):

> Direct Katowice, Sierra Bravo Charlie

About 20 NM before EPKT:

> Sierra Bravo Charlie, descend to altitude three thousand feet, QNH one zero one three

Pilot:

> Descend three thousand feet, QNH one zero one three, Sierra Bravo Charlie

10 NM before EPKT Approach sends you to Katowice Tower:

> Sierra Bravo Charlie, contact Katowice Tower one two niner decimal two five five

Pilot:

> Katowice Tower one two nine decimal two five five, Sierra Bravo Charlie

Set COM1 to 129.255 and call:

> Katowice Tower, Sierra Papa Alpha Bravo Charlie, descending three thousand feet, inbound to Katowice

Katowice Tower:

> Sierra Papa Alpha Bravo Charlie, identified

That is the end of flight 1: here Katowice Tower is a boundary unit of the EPKK station and provides no further service. The landing at EPKT is not simulated in this case.

**Both stations (EPKK and EPKT) are loaded.** 30 NM before EPKT Approach hands you to the EPKT station's Approach. The hand-off is always announced and read back, even on the same frequency (both stations use 121.075). In this case the departure station gives no `direct Katowice` and no 20 NM descent: the hand-off comes first, and in a C172 you are still climbing (about 4200 ft in the recording).

> Sierra Bravo Charlie, contact Kraków Approach one two one decimal zero seven five

Pilot:

> Approach one two one decimal zero seven five, Sierra Bravo Charlie

This is the EPKT station's Approach. Call it (same frequency, leave COM1 alone) with the altitude you are passing and your cleared level:

> Kraków Approach, Sierra Papa Alpha Bravo Charlie, passing four thousand seven hundred feet, climbing flight level one zero zero, inbound to Katowice, information Alpha

EPKT Approach identifies you and leads you to the approach using the EPKT station's data: a STAR or a heading, an altitude, the QNH and `expect ILS approach runway two seven`. In the full-flight recording that is `turn right heading three four five, climb altitude five thousand feet` (see [scenario-ifr-departure.md](scenario-ifr-departure.md)). If there is nothing to say to the first call of a handed-in flight, the controller answers `Kraków Approach, roger` rather than `say again`.

**The EPKT station is loaded without an Approach position.** The real EPKT does have Approach (Kraków Approach 121.075), so this only applies to a station built without one. Then 10 NM before EPKT you are sent to that station's Tower. It identifies you, names the expected approach ("expect ILS approach runway two seven, report established"; with no instrument approach, "expect visual approach runway two seven, report final"), keeps you, clears you to land and, on a go-around, gives you the climb and the expected approach again.

The full EPKK → EPKT flight to the stand, with every line and altitude, is in [scenario-ifr-departure.md](scenario-ifr-departure.md).

### If you leave the plan

- Off the route or heading: the controller says `you are N miles off track, check position`, `check heading, fly heading …` or `check level, maintain altitude …`. Return to the route or level.
- `unable, IFR not available, VFR only`: there is no CIFP (see section 1).

## 4. Flight 2: IFR arrival EPKT → EPKK, ILS 25

Scenarios: `scenarios/epkk-ifr-arrival-vectors.toml` (departure from EPKT, radar vectors), `scenarios/epkk-ifr-arrival-star-ils25.toml` (STAR arrival from the north-west).

### How to start

**Only the EPKK station is loaded.** EPKT is not simulated as an airport (Katowice Tower is a boundary unit of EPKK). There are two ways:

- take off from EPKT on your own and call Kraków Approach as soon as you have climbed to about 4000 ft;
- start X-Plane already in the air over the EPKT area at 4000–5000 ft and call Kraków Approach.

In both cases the first call is to Kraków Approach 121.075 (within 40 NM of EPKK; a call is accepted without a hand-off from another station). EPKT Ground and Delivery do not answer. The exchange below is written for this case.

**EPKK and EPKT are both loaded.** EPKT Delivery, Ground and Tower answer: the IFR clearance to Kraków (from the EPKT station's data), start-up, taxi and take-off happen at the EPKT station just as at EPKK in flight 1, and after take-off EPKT Tower sends you to its Approach. 30 NM before EPKK that Approach hands you to the EPKK station's Approach the same way as in flight 1: `contact Kraków Approach one two one decimal zero seven five`, readback, call on the same frequency. EPKK Approach takes you as a handed-in flight and then leads you to the ILS 25 as below; its first reply (heading, altitude) may differ from the one shown.

Listen to the EPKK ATIS (126.130) in advance and load the ILS 25 into the G1000 (PROC → Select Approach, runway 25, ILS; transition "Vectors", because Approach vectors you).

### Approach 121.075: first call

Pilot:

> Krakow Approach, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, IFR, airborne Katowice, passing four thousand feet climbing five thousand feet, inbound Krakow, information Alpha

Approach (you are already inside the STAR start fixes, so you get vectors instead of a STAR):

> Sierra Papa Alpha Bravo Charlie, identified, turn left heading one zero five, climb altitude five thousand feet, QNH one zero one three, expect ILS approach runway two five

The pilot reads back and flies heading 105 (HDG, magnetic):

> Left heading one zero five, climb five thousand feet, QNH one zero one three, expect ILS approach runway two five, Sierra Bravo Charlie

Then one instruction at a time:

> Sierra Bravo Charlie, descend to altitude three thousand feet

> Sierra Bravo Charlie, turn right heading one six zero

Read back each one (`Descend three thousand feet, Sierra Bravo Charlie`, `Right heading one six zero, Sierra Bravo Charlie`). Then the intercept:

> Sierra Bravo Charlie, turn right heading two two zero, one one miles from touchdown, cleared ILS approach runway two five, report established

Pilot:

> Right heading two two zero, cleared ILS approach runway two five, wilco, Sierra Bravo Charlie

Hold heading 220 and intercept the LOC/GS (in the G1000 activate the approach, **Activate Approach** / **Activate Vector-to-Final**, then NAV/APR on the autopilot). When you are on the localizer and the glideslope:

> Sierra Bravo Charlie, established ILS runway two five

Approach:

> Sierra Bravo Charlie, contact Kraków Tower one two three decimal two five five

Pilot:

> Tower one two three decimal two five five, Sierra Bravo Charlie

### Tower 123.255: landing

Pilot (COM1 = 123.255):

> Krakow Tower, Sierra Papa Alpha Bravo Charlie, established ILS runway two five

Tower:

> Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared to land

Pilot:

> Runway two five, cleared to land, Sierra Bravo Charlie

After landing, Tower:

> Sierra Bravo Charlie, vacate, contact Kraków Ground one one eight decimal one zero five, good day

Pilot:

> Vacating, Ground one one eight decimal one zero five, Sierra Bravo Charlie

### Ground 118.105: to the stand

Pilot:

> Krakow Ground, Sierra Papa Alpha Bravo Charlie, runway vacated, request taxi to stand niner romeo

Ground:

> Sierra Papa Alpha Bravo Charlie, taxi to stand niner Romeo via Charlie, Bravo four, Golf, Zulu one, Zulu two, Zulu three

Pilot:

> Taxi to stand niner romeo via Charlie, Bravo four, Golf, Zulu one, Zulu two, Zulu three, Sierra Bravo Charlie

The route depends on where you vacated the runway. The one above is from exit Charlie (the usual one after landing on 25). From exit Delta: `via Delta, Bravo three, Bravo four, Golf, Zulu one, Zulu two, Zulu three`; from exit Alpha at the end of the runway: `via Alpha, Zulu five, Zulu four`.

On the stand:

> Sierra Bravo Charlie, on stand niner romeo

Ground answers `good day`.

If you say "request taxi" without a stand, Ground sends you to your own stand (with no QNH or code); if you have none, it asks `say stand`. Stand names are matched loosely: ignoring case, spaces and the keyword ("apron four golf alpha" = "APRON4 GA"); among stands with the same name the nearest free one is taken; words after the name (such as your callsign) are ignored.

### Variant: STAR arrival from the north-west

If you come from further away (for example from Opole) and have not yet reached a STAR start fix, the first call goes to Kraków Information (119.275):

> Krakow Information, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, IFR from Opole to Krakow, five thousand feet, inbound Krakow

Information:

> Sierra Papa Alpha Bravo Charlie, contact Kraków Approach one two one decimal zero seven five

Pilot:

> Approach one two one decimal zero seven five, Sierra Bravo Charlie

Then Approach:

> Krakow Approach, Sierra Papa Alpha Bravo Charlie, IFR, five thousand feet, inbound Krakow, information Alpha

> Sierra Papa Alpha Bravo Charlie, identified, MAPIK one Golf arrival, maintain altitude five thousand feet, QNH one zero one three, expect ILS approach runway two five

The pilot reads back and loads **MAPIK 1G** (PROC → Select Arrival, runway 25) and the ILS 25 into the G1000:

> MAPIK one Golf arrival, maintain five thousand feet, QNH one zero one three, expect ILS approach runway two five, Sierra Bravo Charlie

Fly the STAR to its end, then vectors (`turn right heading two five five, descend to altitude three thousand feet`; `turn left heading two two zero, … cleared ILS approach runway two five, report established`), the hand-off to Tower and the landing as above.

## 5. Flight 3: entering the zone via VFR point ZULU

Scenario: `scenarios/epkk-vfr-zulu-in-sierra-out.toml` (it first departs via SIERRA and then comes back via ZULU).

The VFR reporting points are part of AIP AD 2.22: BRAVO, DELTA, ECHO, HOTEL, INDIA, KILO, LIMA, MIKE, OSCAR, ROMEO, SIERRA, UNIFORM, ZULU. Rules:

- **From the south, only via SIERRA and KILO.** A request to enter or leave to the south via any other point gets `unable, entry from the south only via the southern points`. ZULU is an entry point from the north and west, not a southern one.
- Hold over KILO (west of the point) or ZULU, no higher than 2000 ft AMSL.
- VFR traffic stays with Tower after take-off (do not change to Approach).
- Leaving via a point: report over it and change to Kraków Information 119.275.

### Departure via SIERRA (for context)

Delivery:

> Krakow Delivery, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, stand niner romeo, information Alpha, VFR to Nowy Targ via Sierra, request clearance

> Sierra Papa Alpha Bravo Charlie, cleared to Nowy Targ, leave control zone via SIERRA, squawk four two zero one

The pilot reads back: `Cleared to Nowy Targ, leave control zone via Sierra, squawk four two zero one, Sierra Bravo Charlie`. Then Ground and Tower as in flight 1. After the take-off clearance, Tower:

> Sierra Bravo Charlie, report at SIERRA

Pilot: `Wilco, Sierra Bravo Charlie`. Over SIERRA:

> Sierra Bravo Charlie, at Sierra, two thousand feet

> Sierra Bravo Charlie, leave control zone via SIERRA, frequency change approved, contact Kraków Information one one niner decimal two seven five

### Coming back via ZULU

When you come back and want to enter, call Tower before you reach the zone (3–5 miles before ZULU, at 2000 ft):

> Krakow Tower, Sierra Papa Alpha Bravo Charlie, returning to Krakow due weather, three miles west of Zulu, two thousand feet, information Alpha, request joining via Zulu

Tower:

> Sierra Bravo Charlie, enter control zone via ZULU, altitude two thousand feet, report at ZULU

Pilot:

> Enter control zone via Zulu, two thousand feet, report at Zulu, Sierra Bravo Charlie

Over ZULU:

> Sierra Bravo Charlie, at Zulu

Tower:

> Sierra Bravo Charlie, join downwind runway two five, report downwind

Pilot:

> Join downwind runway two five, wilco, Sierra Bravo Charlie

Then as in a circuit: abeam the field

> Sierra Bravo Charlie, downwind runway two five, full stop

> Sierra Bravo Charlie, number one, report final

> Sierra Bravo Charlie, final runway two five

> Sierra Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared to land

Read back every reply (`Number one, wilco, …`, `Runway two five, cleared to land, …`); after landing, Ground and the stand as in flight 2.

## 6. Approach to runway 07: RNAV or VOR/DME, missed approach

Scenario: `scenarios/epkk-ifr-vor07-missed.toml`. Runway 07 has no ILS. By default Approach gives you the RNAV 07 (the G1000 flies it); on request, the VOR/DME 07. The request goes in the first call:

> Krakow Approach, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, IFR, five thousand feet, inbound Krakow, information Alpha, request VOR DME approach runway zero seven

Approach:

> Sierra Papa Alpha Bravo Charlie, identified, turn right heading one seven zero, maintain altitude five thousand feet, QNH one zero zero niner, expect VOR D M E approach runway zero seven

Pilot:

> Right heading one seven zero, maintain five thousand feet, QNH one zero zero niner, expect VOR DME approach runway zero seven, Sierra Bravo Charlie

(For the RNAV 07 add no request: the `R NAV` approach is the default.) Then, as for the ILS, a descent, an intercept heading and:

> Sierra Bravo Charlie, turn left heading one zero zero, one zero miles from touchdown, cleared VOR D M E approach runway zero seven, report established

Pilot: `Left heading one zero zero, cleared VOR DME approach runway zero seven, wilco, Sierra Bravo Charlie`.

Going around: tell Tower and answer the hand-off.

> Sierra Bravo Charlie, going around

Tower: `Sierra Bravo Charlie, contact Kraków Approach one two one decimal zero seven five`. The pilot reads back and sets COM1 to 121.075:

> Krakow Approach, Sierra Papa Alpha Bravo Charlie, missed approach runway zero seven, climbing four thousand feet

Approach:

> Sierra Bravo Charlie, turn left heading two seven zero, climb altitude three thousand feet

Approach then vectors you back and gives you a second approach.

## 7. If something goes wrong

| What you hear | What to do |
|---|---|
| `negative, …` after a readback | Read back exactly what is in the clearance |
| `say again` | Say it again, shorter and clearer |
| `check squawk …` / `squawk Charlie` | Check the code and mode ALT |
| `you are N miles off track, check position` | Return to the SID, STAR or route |
| `check heading, fly heading …` | Hold the heading on the magnetic compass (HDG) |
| `check level, maintain …` | Return to the assigned level |
| `unable, approach not available` | Ask for another approach or runway |
| `unable, IFR not available, VFR only` | No CIFP: check `xplane_root`, restart the server |
| `unable, station closed` | The station is outside its hours; call Ground for the IFR clearance |

The full list of controller wording with its (ICAO) sources is in `docs/phraseology/en.md`.
