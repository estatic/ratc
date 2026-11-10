# Traffic: scheduled bot aircraft flown by the server, drawn by the simulator

Date: 2026-11-10. Roadmap item 2 (docs/superpowers/specs/2026-10-25-roadmap.md). Decisions from a
conversation with the user on 2026-11-10; earlier decisions on traffic from 2026-10-25.

## Why

Today the pilot is alone in the sky: the controller never has to make anyone wait, and the
frequency is silent between the pilot's own calls. Real practice needs other aircraft that land,
take off and talk on the same frequency, so the pilot hears and obeys "hold short, landing
traffic", waits for the frequency and fits into a flow. The user does not want invented traffic
("не хочу выдумывать лишнего"): it comes from a real schedule.

## The scene that must work (acceptance)

The pilot is at the holding point of the active runway at EPKK and reports ready.

1. Tower: "<callsign>, hold short runway two five, landing traffic" (or the engine's existing
   equivalent); the pilot reads back.
2. A bot from the schedule (e.g. "LOT three seven Kilo") is on final. It talks on the Tower
   frequency in its own pilot voice ("established ILS two five" / "on final"), gets "cleared to
   land" and reads it back. While it talks, the frequency is busy for everyone (plan 10 rules).
3. The pilot sees it in X-Plane: a model of the right type (or the nearest installed one), gear
   down, landing lights, flare, touchdown, roll-out, vacating via a taxiway.
4. Once the runway is free (or, my call to be checked against the engine's rules, once the bot has
   landed and the departure can follow), Tower gives the pilot line up / take-off.
5. The bot reports vacated, is handed to Ground, taxis to a stand and disappears after shutdown.

## Goals

1. **Server flies the bots.** Each bot is flown by the server with a simple per-type performance
   model (climb/descent rates, speeds, turn rate, approach speed) along the taxi graph, SIDs, STARs,
   vectors and approaches the engine already knows. A bot obeys every instruction the controller
   gives it the same way in every simulator. The simulator's own AI is never used to fly a bot
   (no simulator offers commands such as heading/level/hold short to its AI; see Prior research).
2. **The simulator only draws.** A per-simulator adapter receives bot states (position, altitude,
   attitude, speed, gear/flaps/lights, on-ground) and draws them with models installed in that
   simulator, chosen by ICAO type and, where possible, airline livery; no exact match -> the
   nearest installed model (same class: narrow-body jet, regional turboprop, single piston...);
   a bot is never left undrawn for lack of a model. The simulator's built-in AI traffic must be
   off (documented; the adapter may turn it off where the sim allows).
3. **Same rules for bots and people (hard requirement): a bot is a client.** Speech recognition runs
   on the server; `atc-client` only captures audio, PTT and telemetry. A bot is a virtual client of
   the server speaking the same protocol (`atc-proto`): `Hello`, `Telemetry` frames produced by its
   flight model (position, altitude, radios, transponder), and `TextTransmission` for its calls
   and readbacks. From the session onwards the server handles it exactly like a human pilot: the
   same floor/queue (plan 10), parser, engine and replies. Only speech-to-text is skipped (as for
   typed text today). Bots run inside the server process as virtual sessions (no process or socket
   per bot; user agreed to "bot as a client", in-process is my call), and the server/engine has
   no "is this a bot" branch in ATC logic. A test proves that the same transmission from a bot and
   from a human gets the same reply. Bots read back correctly (no invented errors), but a wrong
   readback would be corrected like a human's.
   - **How a bot understands ATC.** `AtcText` (sent to every client) gains the structured meaning
     of the transmission (the engine's items: cleared to land runway 25, heading 270, contact X on
     F, ...) next to the text. Every client receives the same message; the bot acts on the
     structure, the human client may ignore it (later: on-screen hints). No parser of controller
     phrases is written.
4. **Bot voices and language.** Every bot speaks with a pilot voice distinct from the controllers'
   voices (Piper voices; several, assigned per bot). Airline callsigns are spoken with the
   airline's telephony designator ("LOT three seven Kilo", "Ryanair four Alpha Bravo"). Bots speak
   English in this project. The design keeps a per-bot language (user's decision): local airline
   and especially general-aviation crews may talk to local controllers in the local language
   (Polish at EPKK) later, so the bot phrase generator and voice choice take a language parameter
   from the start, with English the only one implemented now.
5. **Traffic from a real schedule (OpenSky), downloaded by the user.** The repo ships no schedule
   data. A user tool (CLI command and/or a step in the airport wizard; builder's choice) takes the
   user's own OpenSky API client credentials and downloads, once, the arrivals and departures of
   chosen airports for chosen days (e.g. one weekday + one weekend day), resolves aircraft types
   (OpenSky aircraft database, airline fallback) and writes a compact schedule file (flight,
   callsign, ICAO type, registration if known, origin, destination, time). No raw tracks are
   stored. The file lives under git-ignored local data, like wizard stations. Credentials are
   secrets: never committed, never logged (same handling as the OpenAIP key). Without a schedule
   file the program runs as today, without traffic.
6. **One world clock: UTC on the server (user's decision).** The shared world runs on the server's
   UTC clock. Client and server exchange time only in UTC; the client converts any local or sim
   time zone itself. The schedule is replayed by the server's UTC date: a downloaded weekday is
   used on weekdays, the weekend day on weekends. Pause and time acceleration in the simulator are
   not allowed where the adapter can prevent them (X-Plane: the plugin undoes pause/acceleration;
   other sims as far as their API allows); where it cannot, the world keeps running and the
   pilot's own aircraft simply froze. My call (to check per sim): the adapter sets the simulator's
   clock to the server's UTC where the sim allows, so daylight matches the traffic. The engine
   stays deterministic: it takes time as an input, tests inject it.
7. **Traffic lives around the pilots.** The server simulates the flights from the schedule that
   will, in the near future, come near any connected pilot (their airport or their route through a
   loaded station's airspace). Several pilots share one world: every pilot near EPKK sees the same
   bot.
   - **Drawn** only within the visibility range the pilot's simulator reports (per pilot).
   - **Flown and controlled** also beyond that range: it keeps flying, talking and receiving
     clearances, so it is in its place in the flow when it comes into view.
   - **Dropped** when it leaves for good (climbed out, all clearances received, no further calls
     planned), or when its path does not cross any pilot's path (it would never be seen or heard
     in a way that matters).
   - Bots exist only within loaded stations' airspace: an inbound from an airport without a loaded
     station appears where the emulated adjacent unit (e.g. Warszawa Radar) would hand it to our
     Approach, already on its STAR; an outbound disappears after its hand-off to such a unit.
8. **Load.** Bot movement is cheap. Speech synthesis is not: a bot's transmission is voiced only if
   at least one pilot listens to that frequency; otherwise it goes through the same path silently
   (text only). Behaviour is otherwise identical.
9. **Controller handles traffic (user's decision: full sequencing, not spawn-time shifting).**
   - **Tower:** landing traffic vs a departure on one runway: hold short ("landing traffic"),
     line up and wait, take-off once the landing traffic has vacated (my call; or landed and
     clear, if the engine's runway rules allow it), landing clearance only with a free runway.
     Conditional clearances ("behind the landing A320, line up and wait behind") are not in this
     project (my call).
   - **Approach/Tower sequencing of several arrivals:** landing order, speed control, vectors for
     spacing, "number two, follow the A320 on final", standard IFR spacing on final. The real
     schedule is replayed as is; the controller, not the spawner, resolves bunching.
   - **Ground:** a bot taxiing in and the pilot (and other bots) do not collide; the existing taxi
     conflict prediction (`crates/atc-core/src/conflict.rs`) is the starting point.
   - Separation minima and spacing values come from ICAO Doc 4444 (and the Polish AIP where it
     differs); cite the sources in the code/docs.
10. **Scenario bots for tests and debugging (not user-facing).** A scenario file / debug command
    spawns a bot in a given state ("arrival, 8 NM final runway 25, now"), so the acceptance scene
    and phases 2–4 can be tested before the schedule exists. It is not exposed to users, so no
    invented traffic reaches normal use.
11. **Multi-simulator by design.** Everything simulator-specific sits behind one adapter interface:
    read own-aircraft state (position, radios, transponder, type), report visibility and sim
    clock, draw bots, show on-screen text and the follow-me car, and (where a sim stores it)
    provide airport layout and procedures. The server and engine never know which simulator is
    connected. The first implementation is X-Plane 11; the design must show how X-Plane 12 and
    MSFS 2020/2024 (SimConnect: AI objects created then released from AI control, positioned by
    the client; layout/procedures from the sim's data) fit without server changes. Prepar3D is
    in the same family as MSFS.
12. **README and docs.** README gets a section on getting OpenSky API access and downloading a
    schedule, with OpenSky attribution and a note that the data is for personal use under
    OpenSky's terms (check their terms and cite them). RUNNING.md and the user guides (EN + RU)
    describe traffic.

## Phases (my call; each phase ends with something the user can try)

1. **Adapter boundary.** Move today's X-Plane-specific parts (client UDP datarefs, overlay plugin
   messages, the server reading apt.dat/CIFP from `xplane_root`) behind the adapter interface,
   without behaviour change. Existing tests stay green.
2. **Bots fly and talk.** Bot as an in-process virtual client; structured meaning in `AtcText`;
   performance model; flight along taxi graph/STAR/vectors/ILS; bot pilot "brain" that turns ATC
   items into actions and produces its own calls and readbacks; bot voices and telephony
   callsigns; UTC world clock. Verified with scenario bots (goal 10) in tests and replay.
3. **Tower: landing traffic vs departure** (the acceptance scene), ground de-confliction for a
   taxiing-in bot.
4. **Arrival sequencing** for several arrivals (goal 9, Approach/Tower). Possibly split into its own
   plan; it is the largest engine change.
5. **Drawing in X-Plane 11.** Plugin side: AI slots taken from X-Plane (built-in AI disabled for
   them), models picked from installed aircraft by type, smooth motion (as done for the follow-me
   car), gear/flaps/lights; visibility range from the sim; pause/acceleration prevention and clock
   sync.
6. **Schedule.** OpenSky download tool, schedule file format, spawning and dropping by the rules of
   goal 7, sim-clock replay. README section.
   Arrivals first (the scene); departing bots (push, taxi, take-off, SID, hand-off) follow in the
   same phase or the next, builder's split.

## Non-goals

- Using a simulator's own AI to fly bots.
- Shipping schedule data in the repo.
- X-Plane 12 / MSFS / Prepar3D adapters (only the interface and a written fit check).
- Helicopters (separate project), the fictional GA airline (roadmap item 3), immediate-landing
  scenario (item 4).
- Bots making mistakes, bots talking to each other, controller AI tuning.
- Pilots seeing each other is not part of this project unless it falls out of the bot drawing path
  for free (my call).

## Anti-goals (would be a failure even if it works)

- Any ATC rule, parse or phrase path that differs for bots.
- Traffic data or OpenSky credentials in the repo or logs.
- Server code that names a simulator.

## Constraints

- Rust workspace; existing crates (atc-core engine, atc-server, atc-client, atc-proto,
  atc-phraseology, atc-xplane-plugin, atc-stations, atc-navdata). Deterministic engine (time is an
  input: the server's UTC world clock in use, injected in tests), tests not depending on
  git-ignored station data or a local X-Plane install.
- Radio discipline (plan 10) applies to bots unchanged.
- English code and docs; RU only under docs/user-guide/ru/ (EN guide alongside).
- Never commit config/server.toml, config/client.toml, track.csv, docs/.obsidian/, wizard station
  data, schedule files or credentials.
- Hardware: AMD RX 7900 (20 GB VRAM shared with X-Plane) on Linux; Piper on the CPU.

## Prior research (2026-11-10)

- X-Plane SDK: a plugin can acquire AI aircraft slots, disable X-Plane's AI per plane
  (`XPLMDisableAIForPlane`) and position them each frame; no API commands X-Plane's AI. XP 12.4 can
  initialise an AI flight, not command it.
- MSFS SimConnect: AI aircraft can be created with a flight plan (`AICreateEnrouteATCAircraft`) and
  re-planned (`AISetAircraftFlightPlan`), but they work with MSFS's own ATC; no heading/level
  commands; waypoint changes reported unreliable. `AIReleaseControl` hands the object to the
  client, which then positions it (same model as X-Plane).

## Future (roadmap entry, not this project)

Periodic analysis of recorded exchanges between the controller and pilots/bots to find
phraseology errors, odd replies and needless "say again"s: first as a report fixed by hand, later
AI-proposed and applied corrections (phraseology, style, humour) that pass the same tests and
review. Vulgar or obscene content is always excluded by an explicit filter, not only by model
behaviour.

## Left to the builder

- Adapter interface shape and where it lives (client-side trait vs protocol messages).
- Performance model data per type (sources, fallback by class).
- Telephony designator table (source and licence; fallback: spell the ICAO code).
- How X-Plane models are found and matched (installed aircraft vs CSL packages), within goal 2.
- Schedule file format and the OpenSky endpoints used.
