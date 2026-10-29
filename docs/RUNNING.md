# Running Virtual ATC (X-Plane 11, EPKK)

Everything runs locally: the server (Whisper STT + Piper TTS + the controller engine) and the
client (microphone/speakers, reading X-Plane over UDP) talk over WebSocket on `127.0.0.1`. There
are no external services; the internet is needed only to download the models once.

**Run every command from the repository root**: the paths in `config/*.toml` (models,
`data/airports`, `config/voices.toml`) are relative to it.

## 1. Build

Prerequisites (Linux):

- for `whisper-rs` (Vulkan build): a Vulkan toolchain with `glslc` (shaderc), `cmake`, `libclang`;
- for `cpal` (audio): ALSA headers (`libasound2-dev` / `alsa-lib`);
- for `gilrs` (joystick): `libudev` (`libudev-dev` / `systemd-libs`) and read access to
  `/dev/input/*` (usually the `input` group).

Note: the `whisper`/`piper` features of `atc-server` are on by default, so even
`cargo test --workspace` builds `whisper-rs` (slow, and it needs the same toolchain).

```
cargo build --release -p atc-server -p atc-client
```

Binaries: `target/release/atc-server`, `target/release/atc-client`. Release matters: Whisper is
much slower in a debug build.

## 2. Models

```
tools/fetch-whisper.sh   # models/whisper/: ggml-large-v3-turbo-q5_0.bin (~574 MB) and ggml-small.en-q5_1.bin (~181 MB)
tools/fetch-piper.sh     # models/piper/: the Piper binary and the voices: Ground, Tower, ATIS, Delivery, Approach, Radar (English) and Polish
```

The scripts are idempotent (a second run skips what is already there). `models/` is
git-ignored. Which voice speaks for which station: `tools/MODELS.md`; speech rate
(`length_scale`): `config/voices.toml`.

Without a model the server exits with code 2 and names the script to run.

### Choosing the Whisper model

In `config/server.toml`, section `[whisper]`:

- the default is `models/whisper/ggml-large-v3-turbo-q5_0.bin` (multilingual). **The choice is
  provisional**, see `docs/benchmarks/2026-10-voice.md`: STT p95 is about 0.17 s, but video
  memory next to a running X-Plane has not been measured yet;
- to switch to the English-only `small.en`: `model = "models/whisper/ggml-small.en-q5_1.bin"`
  (faster, half the video memory; no worse on the synthetic test);
- `language` is the language of the radio exchange:
  - `"en"` (the config default) fixes English;
  - `"pl"` fixes Polish: Polish Whisper prompt, Polish parsing, Polish replies, ATIS and voices
    (`[pl.*]` in `config/voices.toml`);
  - line removed: auto-detection. Each aircraft is answered in the language of its last
    understood transmission (English before the first one), and the ATIS in the language of the
    last understood transmission. If Whisper detects a third language (for example `ru` for an
    accent), the recording is transcribed again in the aircraft's language. Short accented
    English phrases are sometimes detected wrongly, so when you fly in one language only, fix it.

  The Polish Whisper model is the same multilingual `large-v3-turbo` (`small.en` understands
  English only).

Other server settings: `listen` (default `127.0.0.1:8740`), `airport = "EPKK"`,
`strictness = "standard" | "training"`.

### Airport layout from X-Plane (`xplane_root`)

`xplane_root = "/path/to/X-Plane 11"` in `config/server.toml`: the server takes the airport
layout (runways, taxiways, stands, the apt.dat part) from the scenery X-Plane itself loads, so
that taxi routes name the taxiways you see in the simulator. The search order is X-Plane's:
`Custom Scenery` folders not yet in `scenery_packs.ini` (X-Plane adds them at the top on its next
start), then the enabled `SCENERY_PACK` lines of the ini in order (`SCENERY_PACK_DISABLED` is
skipped), then `Custom Scenery/Global Airports`, then
`Resources/default scenery/default apt dat`. Frequencies and procedures always come from
`data/airports/<ICAO>/`. At start the log shows
`airport layout: <file> (X-Plane scenery): ...; taxiways: ...`. Without `xplane_root`, or if the
airport is not found, there is a warning and the repository's `data/airports/<ICAO>/apt.dat` is
used. After installing or removing airport scenery, restart both X-Plane and the server.

### IFR and VFR points

- Three more English voices (Delivery, Approach, Radar) are downloaded by the same
  `tools/fetch-piper.sh`; run it once before starting the server (`tools/MODELS.md`).
- The server reads the procedures (SID, STAR, approaches) from the X-Plane install's CIFP
  (`Resources/default data/CIFP/EPKK.dat`) through `xplane_root`. Without CIFP the controller
  answers an IFR request with `IFR not available, VFR only`.
- A step-by-step guide to instrument departures and arrivals and to VFR entry via the reporting
  points on a C172 G1000 (in Russian): `docs/user-guide/ru/ifr-and-vfr-points.md`; the wording
  with its sources: `docs/phraseology/en.md`.

### Languages: English and Polish

- The exchange language is `[whisper] language` in `config/server.toml` (see above): `"en"`,
  `"pl"` or auto-detection (line removed).
- Voices: `tools/fetch-piper.sh` downloads the English (`[ground]`, `[tower]`, `[atis]`) and
  Polish (`[pl.ground]`, `[pl.tower]`, `[pl.atis]`) voices of `config/voices.toml`; without a
  Polish voice a station speaks with its English one.
- Polish phraseology (what the parser understands and what the controller answers):
  `docs/phraseology/pl.md`; a step-by-step Polish circuit (in Russian):
  `docs/user-guide/ru/polish-circuit.md`.

## Stations and the airport wizard

A station set (positions, VFR points, airspaces, transition altitude, circuits) is described by
a `Procedures` table. For EPKK it is the hand-made, AIP-sourced
`data/airports/EPKK/procedures.toml`. For any other airport the airport wizard generates it
from OpenAIP and the X-Plane layout:

```
target/release/atc-server airports        # Windows: atc-server.exe airports
```

Requirements: `xplane_root` in `config/server.toml` (the wizard reads the airport layout from
your scenery and refuses to create a station without it) and an OpenAIP API key (openaip.net ->
register -> Profile -> API clients; free). Menu 5 asks for the key and checks it.

### Menu

1. **Create stations in an area.** Search by ICAO code or name, optionally load neighbouring
   countries (ISO codes, e.g. `SK CZ`), then pick airfields from the list of those within
   100 km (closed ones cannot be picked). For each: pick the positions, review the value
   screen, confirm. Neighbouring airfields' towers within 100 km become adjacent units.
2. **Update data.** Downloads the OpenAIP changes and regenerates every station.
3. **Edit a station.** The value screen: every setting with its source; "Change a value" takes
   a TOML value and writes it to `overrides.toml`.
4. **Remove a station.** Deletes `station.toml` and `wizard.toml`; `overrides.toml` only if you
   agree. EPKK falls back to its `procedures.toml`.
5. **Set the OpenAIP API key.**

At start the wizard also checks every downloaded country for changes and asks whether to update.

### Files in `data/airports/<ID>/`

| File | What | In git |
|---|---|---|
| `station.toml` | generated; each key has a `# source:` comment | no |
| `overrides.toml` | your values over the generated ones, with `[sources]` | yes |
| `wizard.toml` | the OpenAIP airfield and country it was made from | no |
| `apt.dat` | the airport's X-Plane layout (written when absent) | yes |
| `procedures.toml` | hand-made station, used only when there is no `station.toml` | yes |

Precedence: `station.toml` + `overrides.toml` win over `procedures.toml`. The server loads one
or the other, not a merge of both. To fly at an airport set `airport = "<ID>"` in
`config/server.toml`.

### Overrides

`overrides.toml` holds top-level `Procedures` keys. Tables merge field by field; arrays (such as
`vfr_points` or `approach_alt_khz`) and scalars replace the generated value whole, so list every
element you want. The `[sources]` table, last in the file, says where each value comes from; it
is not merged into the station.

```toml
preferred_runway = "25"
vfr_holds = ["KILO", "ZULU"]
tma = { radius_nm = 40.0, ceiling_ft = 9500 }
delivery_hours = { summer_utc = [6, 22], winter_utc = [5, 21] }

[sources]
preferred_runway = "user ruling 2026-10-19"
vfr_holds = "AIP AD 2.22 3.3"
tma = "user ruling 2026-10-19"
delivery_hours = "AIP AD 2.20"
```

OpenAIP does not carry everything: VFR holds, delivery hours and some VFR points have to come
from the AIP. EPKK's `overrides.toml` is a worked example, and the test
`cargo test -p atc-stations --test epkk` checks that a station generated from an EPKK-shaped
fixture plus these overrides equals the hand-made `procedures.toml`.

When an update finds that an override names something OpenAIP no longer has (a VFR point, a hold,
a position, an adjacent unit), the wizard prints `<ID>: <key>: <name> is not in OpenAIP any
more`. The override is kept; check it and remove or fix it in `overrides.toml`. A frequency you
had ticked that has disappeared is reported the same way.

### Where the key and the data live

The key (`openaip.toml`) and the downloaded OpenAIP data (`openaip/<COUNTRY>/`) stay on your
machine, in the per-user directories (the `directories` crate, application `vatc`):

| OS | Key | OpenAIP data |
|---|---|---|
| Linux | `~/.config/vatc` | `~/.local/share/vatc/openaip` |
| macOS | `~/Library/Application Support/vatc` | `~/Library/Application Support/vatc/openaip` |
| Windows | `%APPDATA%\vatc\config` | `%APPDATA%\vatc\data\openaip` |

OpenAIP data is licensed CC BY-NC 4.0 (non-commercial, attribution); it is not committed to this
repository, and the generated `station.toml` is git-ignored for the same reason.

## 3. X-Plane 11

No plugin is needed for the link and no Settings → Network change for RREF: X-Plane answers on
UDP port 49000 by itself (the client only reads the position, COM1 and COM2 with the audio
panel, the transponder and the weather). Start X-Plane and put the aircraft (C172) on a stand at
EPKK. If X-Plane runs on another machine or port, set `xplane = "host:port"` in
`config/client.toml`.

### Exchange text on screen (optional)

The `atc-overlay` plugin shows a window in X-Plane with the controller's latest lines (white),
what was understood from your speech (green) and the PTT state (red while transmitting). Lines
older than a minute fade.

```
tools/install-xplane-plugin.sh                  # Steam's default path
tools/install-xplane-plugin.sh "/path/to/X-Plane 11"
```

The script builds the plugin and puts it in `Resources/plugins/atc-overlay/64/lin.xpl`; restart
X-Plane after installing. The window can be moved and resized. The command
`atc/overlay/toggle` shows or hides it (bind it to a key or button in Settings → Keyboard /
Joystick). The client sends the lines to UDP `127.0.0.1:48010`; another address:
`overlay = "host:port"` in `config/client.toml`.

### Follow-me car

The EPKK stands (Global Airports layout) have no markings on the apron, so finding your stand at
the end of the taxi is hard. Ground helps like this:

- "…, I am lost, I need help" (any request for taxi help): the route again, and when the target
  is a stand closer than 500 m, also where it is: `stand two zero is one hundred twenty metres
  ahead` (distance to 10 m; ahead / on your right / behind you / on your left, relative to the
  aircraft's nose, 90° sectors);
- a second request for help after the same taxi clearance, or "request follow-me" / "follow me"
  / "request marshaller": `hold position, follow-me car is on the way` (read back "holding
  position"); 12 s of simulator time later: `follow the follow-me car` (answer "wilco" or
  "follow the follow-me"; no strict readback);
- "on stand" at the stand (aircraft stopped, at most 25 m from the assigned stand): `good day`
  (to a wish: `thank you, you too`); not at the stand: where it is, in the same words.

The car (the crew car of the X-Plane 11 library, `lib/airport/vehicles/servicing/crew_car.obj`)
appears right away about 40 m ahead of the aircraft on the route and drives along the taxiways
to the stand, staying 30–50 m ahead: it waits while the aircraft is stopped or more than 80 m
behind, never reverses, drives at most 15 knots and turns smoothly. At the end it pulls 12 m to
the right and disappears 20 s after the aircraft arrives. On a taxi to the holding point the car
stops 20 m before it; from there Ground hands over to Tower as usual. A new taxi clearance
recalls the car.

The car is drawn by the same `atc-overlay` plugin (the client forwards the commands on the same
UDP 48010). After an update **reinstall the plugin** (`tools/install-xplane-plugin.sh`) and
restart X-Plane; X-Plane 11.10+ is needed (instancing API).

## 4. Server

```
target/release/atc-server                       # config/server.toml
target/release/atc-server --config path.toml
target/release/atc-server --fresh               # don't restore the saved flights
```

The first start loads the models for a few seconds, then the log shows
`listening on 127.0.0.1:8740`. Log level: `RUST_LOG` (default `info`). The log of each
transmission shows the times `stt_ms`, `engine_ms`, `tts_ms`.

The server keeps the flights in progress between runs: issued clearances, flight phase,
expected readbacks, transponder code, ATIS letter. The snapshot is written every 2 s and on stop
(Ctrl-C) to `data/cache/state-<ICAO>.json` (not in git). At start it is restored when it is at
most 2 hours old and made for the same airport; the log says
`flights in progress restored from …` or `starting afresh (…)` with the reason. To start from a
clean slate: `target/release/atc-server --fresh` (or delete the file).

The server rejects WebSocket connections that carry an `Origin` header, so a browser cannot
connect; this is on purpose (protection from foreign web pages). The native client sends no
`Origin`.

## 5. Client

Settings: `config/client.toml`: `server`, `callsign` (SP-ABC), `aircraft_type`, `xplane`,
`field_elev_ft` (791 for EPKK), `overlay` (the on-screen plugin's address, default
`127.0.0.1:48010`), `input_device` / `output_device` (part of the device name; system defaults
without them).

```
target/release/atc-client --help
# usage: atc-client [--config <file>] [--text] [--record <track.csv>]
```

### Voice mode (default)

```
target/release/atc-client
```

The station you talk to is the frequency of the radio selected for transmission on X-Plane's
audio panel (COM1 MIC or COM2 MIC; COM1 by default). Replies are heard on it and on any other
COM selected for listening. Any listened COM on 126.130 starts the ATIS, which plays in a loop
(for example the ATIS on COM2 while you talk to Ground on COM1). When stations change the client
prints `COM 118.105, also listening 126.130`.

Transponder: Ground gives a code at the end of the taxi clearance ("…, QNH one zero one three,
squawk four two zero one"; codes in turn from the pool 4201–4277, octal digits only); read it
back and set it. After the hand-off from Ground, Tower calls you at the runway holding point
(when COM1 is on the Tower frequency), answers "ready for departure" the same way, and checks
X-Plane's transponder: a different code gives "check squawk four two zero one", mode OFF/STBY
gives "squawk Charlie", and no take-off clearance. Set it and answer ("Squawk four two zero one,
…" / "wilco, …"): Tower checks again and either clears you for take-off right away or repeats the
instruction. Use mode ALT.

PTT (section `[ptt]`):

- **keyboard** (`key = "space"`) is a **toggle**: press space to start transmitting, press it
  again to stop. A terminal gives no key-release events, so hold-to-talk on the keyboard is not
  possible. A held key does not end the transmission: while auto-repeat goes on (each press
  repeated at most 1 s after the previous one) the presses are ignored; a new tap counts after a
  pause of at least 1 s since the last press. In short: tap to start, tap to stop;
- **joystick button** (`joystick_button = 0`) is ordinary hold-to-talk: hold to transmit,
  release to stop. Remove the line to keep only the key. On Linux the numbers count from
  `BTN_TRIGGER`: button 0 is the trigger, 1, 2, ... the next buttons. If you are not sure which
  number a button has, run with `RUST_LOG=debug`: the client logs `joystick button N pressed`;
- quit: `q`, `Esc` or `Ctrl-C`.

### Text mode (debugging)

```
target/release/atc-client --text
```

The microphone, speakers and PTT are not opened. Type a pilot transmission and press Enter: it
goes out on the frequency of the radio selected for transmission; the controller's reply is
printed as text. Quit with Ctrl-D. Handy for checking the server and the engine without voice.
If X-Plane is not running the client answers
`no telemetry from X-Plane yet (is it running? check xplane in the config) - line dropped`.

## 6. Recording a track and replaying it

```
target/release/atc-client --record track.csv        # also works with --text
```

This writes a CSV (header
`t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz,xpdr_code,xpdr_mode`, one line per telemetry
frame, frames while paused are skipped; the file is flushed line by line and survives the client
being killed). `xpdr_mode` is X-Plane's: 0 off, 1 standby, 2 on (the same as ALT in X-Plane),
3 test; an empty field means unknown. Older recordings without the last two columns are read
too.

Replay with `atc-replay`: it takes a TOML **scenario**, not the CSV. Copy
`scenarios/epkk-circuit-vmc.toml`, replace the line `track = "synthetic-circuit:25"` with
`track_file = "track.csv"` (a path relative to the scenario's folder; exactly one of `track` /
`track_file`), adjust the `[[say]]` lines and the weather if needed (the transponder is set with
`transponder = { code = 7000, mode = 2 }`, at the start of the scenario or in a `[[say]]`, where
the pilot sets it before the line; example: `scenarios/epkk-wrong-squawk.toml`), then:

```
cargo run --release -p atc-replay -- scenarios/my-scenario.toml
```

Language in a scenario: `language = "auto" | "en" | "pl"` at the top of the file is the language
mode, like `[whisper] language` (default `"en"`), and `lang = "pl"` in a `[[say]]` is the
language Whisper would have detected (default `"en"`); examples:
`scenarios/epkk-circuit-pl.toml` and `scenarios/epkk-language-switch.toml`.

The transcript is printed to stdout (format `[time] WHO: text`) and is deterministic; the
reference transcripts are `scenarios/*.expected.txt`. Checking the standard scenario:
`cargo run -q -p atc-replay -- scenarios/epkk-circuit-vmc.toml | diff - scenarios/epkk-circuit-vmc.expected.txt`.

## Common problems

| Symptom | Cause |
|---|---|
| the server exits with code 2 | a model is missing: `tools/fetch-whisper.sh` / `tools/fetch-piper.sh` |
| `no telemetry from X-Plane yet` | X-Plane is not running, wrong `xplane` in `config/client.toml`, or a firewall blocks UDP 49000 |
| error reading `config/...` or `data/airports/...` | not started from the repository root |
| the client cannot connect from a browser / with `Origin` | on purpose, see section 4 |
| Ground names taxiways that are not in the simulator | `xplane_root` not set, or X-Plane not restarted after installing scenery; see `airport layout:` in the server log |
