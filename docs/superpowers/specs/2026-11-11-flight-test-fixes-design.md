# Flight test fixes (plan 14a) — design

Date: 2026-11-11. Source: the first in-sim EPKK→EPKT IFR flight (X-Plane 11, C172 G1000,
callsign N172SP from the sim). The user's verdict: "half successful — controllers behave
strangely, too many speech-recognition errors; the radio must feel alive."

This document covers the point fixes (plan 14a). Radar vectoring and the filed route from the
sim's GPS plan (findings B8, B12) are a separate design (plan 14b) and are out of scope here.
Traffic phase 2 waits for both.

## Intent

A pilot who speaks normal, slightly imperfect phraseology through Whisper must get the answer
a real controller would give. A controller that cannot use a transmission says what is wrong
in a way the pilot can act on. Nothing is said on the radio after the simulator is gone.

## Findings and required behaviour

**B4 — simulator lost.** The client keeps playing the ATIS loop and controller speech after
X-Plane crashed or quit; the X-Plane reader only resubscribes every 5 s.
- The sim adapter reports `Lost` once, after it had data and then got none for its silence
  timeout. The next telemetry means the sim is back ("<sim> connected, ..." is printed again).
- On `Lost` the client stops all playback (including a looping ATIS), prints one line
  `<sim> lost - radios off until it is back`, and tells the server.
- The server then treats the client as listening to nothing (untuned) until its next telemetry,
  so no ATIS or controller audio is sent to it.

**B1 — wrong addressee, right position.** "Kraków Ground, ..., request clearance" said on the
Delivery frequency was redirected to Ground. A controller who serves the request answers it
and states who they are ("..., Kraków Delivery, cleared to ..."). An addressee-based redirect
remains only for a call with no request (a bare initial call to another position).

**B2 — pending redirect blocks requests.** While a redirect's "contact X" readback is pending,
a request this position serves is answered as a request (the redirect is dropped) instead of
"read back frequency".

**B7 — hand-off within one frequency.** EPKK and EPKT Approach are the same unit (same
callsign "Kraków Approach", same 121.075). The flight was told "contact Kraków Approach
121.075" while on it and got "identified" twice. A hand-off to a unit on the frequency the
flight is already on, with the same callsign, is silent: no "contact", no readback, no second
"identified". The receiving station continues with its next instruction to the flight without
waiting for a call.

**B9 — clean readback answered "say again".** After two badly recognised readbacks of
"contact Katowice Tower 129.255" and a garbled fragment ("Conta Katowice-Krácow Tower"), the
clean readback "Contact Katowice Tower, one two nine decimal two five five, November Sierra
Papa" got "say again" twice. Root cause to be found by reproduction; a correct readback is
always accepted.

**B3 — destination after "IFR".** Whisper writes "IFR to Katowice" as "IFR 2 Katowice" or
"IFR Tokatowice". A known place after IFR/VFR (with "2", "two", "to" or glued "to" between) is
the destination; a clearance request naming exactly one known place, nowhere else used, takes
it as the destination.

**B13 — recognition tolerance.**
- Normalisation of observed Whisper output: "one way" → runway (before a runway number),
  "ninea", "nine-year", "niner" → nine, "129, decimal 255" / "129 decimal 255" → 129.255,
  "establish" → established, "horshirt" → hold short, "9-Ride" / "9 ride" → 9 right.
- A callsign that differs from the only aircraft working this frequency by one letter or digit
  ("November Sierra Bravo" for N172SP abbreviated N-SP) is that aircraft.
- The Whisper prompt of each transmission also carries what the controller expects: the
  speaker's own callsign (full and abbreviated spoken form), and on a Ground position the
  airport's stand names in spoken form. The prompt stays within Whisper's 224-token window.

**B5 — stand names.** "APRON4 GA" was never matched from "a prone for Golf Alpha",
"Upron for GALPHA", "Aparon 4, Gulf Alpha", "Apron for Golf Alpha". Stand matching accepts
phonetic letters (and their common mis-hearings golf/gulf), "for" as 4 inside a stand name,
apron mis-hearings, and one-character typos when exactly one stand is that close.

**B6 — "any stand".** "request taxi to any available spot" / "any stand" makes Ground assign
a free stand suitable for the aircraft (light propeller aircraft: GA / tie-down stands first),
nearest to the aircraft by taxi distance or straight line, never a de-icing stand, never one
occupied or assigned to another aircraft.

**B10 — "final" answered "report final".** An IFR arrival's call to Tower before it is on the
final (by geometry) is answered with what is true: "continue approach, <N> miles from
touchdown" on the extended centreline beyond the final zone, or "you are <N> miles
<north/south/...> of the final approach course" when off it. Tower clears an IFR arrival to
land unprompted once it is on the final, so a "report final" request is never needed for an
IFR arrival.

**B11 — PTT message.** A press refused because the frequency was busy prints
`[ptt] off - nothing sent (frequency was busy)`, not the microphone hint.

**Navigation database warning.** When the simulator's GPS navigation data overrides the default
(X-Plane `Custom Data/earth_424.dat` present) and has no procedures for a loaded station
(`Custom Data/CIFP/<ICAO>.dat` missing), the server warns at start-up that the aircraft's GPS
departure/arrival lists will be empty for that airport, and how to fix it.

**Wizard UX.**
1. The "Airfields within 100 km" list marks airfields that already have a station with
   `(station)` and does not preselect them.
2. Airfields the wizard cannot build (no X-Plane layout: heliports, ultralight sites) are shown
   like closed ones and cannot be selected.
3. A menu entry removes a stored country so it is no longer downloaded on update.

**Guide.** The IFR departure scenario guide (EN + RU, in sync) explains how the callsign is set
(from the sim, or `callsign` in client.toml), how to fly the G1000 in this scenario (flight
plan entry, PROC → departure, NAV for the SID, ALT/VS, APR for the ILS, STD BARO above the
transition altitude), and the navigation-database pitfall above.

## Constraints

- Every exchange from the flight that the change fixes gets a test reproducing it: a replay
  scenario where it happens at EPKK (the only station tracked in git), else an engine test.
- English only in code and docs, except `docs/user-guide/ru/`.
- Simulator names stay out of server, core and the client's main part (the sim-neutral guard).
- No secrets, no user station data under `data/airports` other than EPKK's tracked files.

## Left to the builder

Internal structure of each fix, exact new type names beyond those in the plan, spoken wording
of new items (in the style of existing phraseology, Polish included where the item exists in
Polish).
