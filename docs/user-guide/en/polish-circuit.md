# A circuit in Polish (EPKK, runway 25)

[Русский](../ru/polish-circuit.md) · **English**

A step-by-step circuit at Kraków (EPKK) with the radio exchange in Polish: taxi, take-off, a circuit to a full stop, landing and taxi back to the stand. The pilot lines below are copied verbatim from the scenario `scenarios/epkk-circuit-pl.toml` and the controller lines from its reference output `scenarios/epkk-circuit-pl.expected.txt`, so everything here is checked by the tests (`cargo test -p atc-replay`).

The callsign in the examples is SP-ABC (`Sierra Papa Alfa Bravo Charlie`, abbreviated `Sierra Bravo Charlie`). Note that in Polish radiotelephony the ICAO letter A is spelled "Alfa", not "Alpha".

## 1. Setup

### Language

In `config/server.toml`, section `[whisper]`:

```toml
[whisper]
model = "models/whisper/ggml-large-v3-turbo-q5_0.bin"
language = "pl"
```

- `language = "pl"`: the whole exchange is in Polish: the Whisper prompt, parsing of your transmissions, the controller's replies, the ATIS and the Polish voices. Use it if you only fly in Polish.
- Remove the `language` line for auto-detection: each aircraft is answered in the language of its last understood transmission (English until the first one). You can start in Polish with Ground and switch to English with Tower, and the controller follows you (example: `scenarios/epkk-language-switch.toml`). A short accented "roger" that Whisper takes for Polish does not switch the language.
- `language = "en"`: English only (the default in the config).

The Whisper model must be multilingual (`large-v3-turbo`, the default); `small.en` understands English only.

### Polish voices

```
tools/fetch-piper.sh
```

The script downloads both the English and the Polish Piper voices into `models/piper/voices/`: Ground is `pl_PL-darkman-medium`, Tower `pl_PL-mc_speech-medium`, ATIS `pl_PL-gosia-medium`. The mapping is in the `[pl.ground]`, `[pl.tower]` and `[pl.atis]` sections of `config/voices.toml`; a station with no Polish voice speaks with its English one. Restart `atc-server` after changing the configuration.

### EPKK frequencies

| Station | Frequency | In Polish |
|---|---|---|
| Ground | 118.105 | `Kraków Ground` |
| Tower | 123.255 | `Kraków Wieża` (the controller says "sto dwadzieścia trzy kropka dwa pięć pięć") |
| ATIS | 126.130 | broadcast in Polish in `pl` mode |

The station you talk to is the frequency of the radio selected for transmission on the audio panel (COM1 MIC or COM2 MIC; COM1 by default). You hear the replies on it and on any other COM selected for listening: for example the ATIS on COM2 while you work Tower on COM1.

## 2. How to say numbers

- Runway numbers, squawk codes, taxiways and the digits of a callsign are said digit by digit: "dwa pięć", "cztery dwa zero jeden".
- QNH is said as two two-digit numbers: 1013 is "dziesięć trzynaście".
- Frequencies: the whole part as a number, then "kropka" and the decimals digit by digit: 123.255 is "sto dwadzieścia trzy kropka dwa pięć pięć".
- The controller gives the wind in numbers: "wiatr dwieście pięćdziesiąt stopni osiem węzłów".
- You do not need perfect Polish diacritics: "koluje" and "sklok" without them are understood too.

## 3. The circuit step by step

### On the stand, Ground 118.105: request taxi

Pilot:

> Kraków Ground, Sierra Papa Alfa Bravo Charlie, dzień dobry, proszę o kołowanie do kręgów

Ground:

> Sierra Papa Alfa Bravo Charlie, dzień dobry, aktualna informacja Alfa, QNH dziesięć trzynaście, kołuj do punktu oczekiwania przed pasem dwa pięć drogą Bravo pięć, Bravo sześć, Alfa, oczekuj przed pasem dwa pięć, skłok cztery dwa zero jeden

You did not name the ATIS letter ("informacja Alfa"), so Ground gives you the current information and the QNH itself. If you have listened to the ATIS, add "informacja Alfa" to the request.

### Reading back the taxi clearance

Pilot (read back the route, the hold short, the QNH and the code; then set the transponder to 4201, mode ALT):

> Kołuję do punktu oczekiwania przed pasem dwa pięć drogą Bravo pięć, Bravo sześć, Alfa, oczekuję przed pasem dwa pięć, QNH dziesięć trzynaście, skłok cztery dwa zero jeden, Sierra Bravo Charlie

Ground says nothing: the readback is accepted. A wrong readback gets "nieprawidłowo" and the clearance again; an incomplete one gets "powtórz …" with what is missing.

Radio rules:

- One person talks on a frequency at a time: whoever started first.
- While the controller or another pilot is talking on the frequency, a PTT press or typed text is rejected: the client prints `[ptt] frequency <frequency> busy - wait, then press again`, and that transmission is not heard. Release PTT and press again when the frequency is free.
- From the moment you press PTT the frequency is yours; the controller answers after you have finished (a pause of about 1 s).
- A readback reminder comes 10 s after the frequency becomes free, and never while you are transmitting.
- A readback without a callsign is accepted when only one aircraft is in the exchange (a correct one is answered "<callsign>, roger"); from the third such time on, the controller adds "używaj znaku wywoławczego w każdej korespondencji" (use your callsign in every transmission).

### Holding point: change to Tower

When you have stopped at the holding point, Ground:

> Sierra Bravo Charlie, łączność z Kraków Wieża sto dwadzieścia trzy kropka dwa pięć pięć, miłego lotu

Pilot:

> Łączność z Kraków Wieża sto dwadzieścia trzy kropka dwa pięć pięć, Sierra Bravo Charlie

Set COM1 to 123.255.

### Tower 123.255: take-off

After the change from Ground it is Tower, not the pilot, that makes the first call (AIP AD 2.20 §2.2). As soon as you are at the holding point with COM1 on 123.255, Tower calls you with the take-off clearance; you do not need to say anything. If you call Tower anyway ("Kraków Wieża, Sierra Papa Alfa Bravo Charlie, punkt oczekiwania pas dwa pięć, gotowy do startu"), the answer is the same, and a repeated call after the clearance gets no answer.

Tower calls:

> Sierra Papa Alfa Bravo Charlie, wiatr dwieście pięćdziesiąt stopni osiem węzłów, pas dwa pięć, zezwalam startować, zgłoś z wiatrem

Pilot:

> Pas dwa pięć, zezwalasz startować, zgłoszę z wiatrem, Sierra Bravo Charlie

Take off.

### Downwind (z wiatrem): full-stop landing

Pilot:

> Sierra Bravo Charlie, z wiatrem, z pełnym zatrzymaniem

Tower:

> Sierra Bravo Charlie, numer jeden, zgłoś prostą

Pilot:

> Numer jeden, zgłoszę prostą, Sierra Bravo Charlie

Without "z pełnym zatrzymaniem" (full stop), Tower clears you for a touch and go on final ("zezwalam konwojer").

### Final (prosta): landing clearance

Pilot:

> Sierra Bravo Charlie, prosta

Tower:

> Sierra Bravo Charlie, wiatr dwieście pięćdziesiąt stopni osiem węzłów, pas dwa pięć, zezwalam lądować

Pilot:

> Pas dwa pięć, zezwalasz lądować, Sierra Bravo Charlie

### After landing: vacate the runway

When you have slowed down, Tower:

> Sierra Bravo Charlie, zwolnij pas, łączność z Kraków Ground sto osiemnaście kropka jeden zero pięć, do widzenia

Pilot:

> Zwalniam pas, łączność z Kraków Ground sto osiemnaście kropka jeden zero pięć, Sierra Bravo Charlie

Vacate the runway and set COM1 to 118.105.

### Ground 118.105: taxi to the stand

Pilot:

> Kraków Ground, Sierra Bravo Charlie, proszę o kołowanie do stanowiska

Ground:

> Sierra Bravo Charlie, kołuj do stanowiska trzy pięć drogą Charlie, Bravo cztery

Pilot:

> Kołuję do stanowiska trzy pięć drogą Charlie, Bravo cztery, Sierra Bravo Charlie

### On the stand

Pilot:

> Kraków Ground, Sierra Bravo Charlie, na stanowisku, do widzenia

Ground:

> Sierra Bravo Charlie, do widzenia

If the aircraft is not on the stand yet, Ground tells you where it is: "stanowisko trzy pięć jest dwadzieścia metrów przed tobą" (przed tobą: ahead; po twojej prawej: on your right; za tobą: behind you; po twojej lewej: on your left).

## 4. Useful phrases

| Pilot | Meaning |
|---|---|
| `proszę powtórzyć` / `powtórz` | say again |
| `przyjąłem` | roger |
| `zgubiłem się` / `proszę o pomoc` | I am lost, I need help with the taxi route |
| `proszę o follow me` | request a follow-me car |
| `stoję` | holding position |
| `przechodzę na drugi krąg` | going around |

The full list of Polish pilot and controller phrases is in `docs/phraseology/pl.md`.
