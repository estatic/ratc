# Scenario: a VFR circuit, runway 25 (EPKK)

[Русский](../ru/scenario-circuit.md) · **English**

A left-hand circuit at 1800 ft in good weather. The lines come from the recorded flight the tests replay (`scenarios/epkk-circuit-vmc.toml`). Callsign SP-ABC, abbreviated Sierra Bravo Charlie.

## Preparation

```
cargo build --release -p atc-server -p atc-client
target/release/atc-server     # terminal 1, wait for "listening on 127.0.0.1:8740"
target/release/atc-client     # terminal 2 (or --text to type instead of talking)
```

The C172 is parked on a stand at EPKK. The station you talk to is chosen by the frequency of the radio selected for transmission (COM1 by default): ATIS 126.130, Ground 118.105, Tower 123.255. You can also listen to the ATIS on COM2 if it is selected for listening.

## Ground 118.105

| Who | Phrase |
|---|---|
| You | Krakow Ground, Sierra Papa Alpha Bravo Charlie, information Alpha, request taxi for circuits |
| Ground | Sierra Papa Alpha Bravo Charlie, taxi to holding point runway two five via Bravo five, Bravo six, Alpha, hold short runway two five, QNH one zero one three, squawk four two zero one |
| You | Taxi to holding point runway two five via Bravo five, Bravo six, Alpha, hold short runway two five, QNH one zero one three, squawk four two zero one, Sierra Bravo Charlie |

Set squawk 4201 and mode ALT. The taxi route depends on the stand: read back what you hear.

At the holding point:

| Who | Phrase |
|---|---|
| Ground | Sierra Bravo Charlie, contact Kraków Tower one two three decimal two five five, have a nice flight |
| You | Contact Krakow Tower one two three decimal two five five, Sierra Bravo Charlie |

## Tower 123.255

Switch COM1 and **stay quiet**: Tower calls you.

| Who | Phrase |
|---|---|
| Tower | Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared for take-off, report downwind |
| You | Runway two five, cleared for take-off, report downwind, Sierra Bravo Charlie |
| You (on downwind, abeam the field) | Sierra Bravo Charlie, downwind, full stop |
| Tower | Sierra Bravo Charlie, number one, report final |
| You | Number one, report final, Sierra Bravo Charlie |
| You (on final) | Sierra Bravo Charlie, final |
| Tower | Sierra Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared to land |
| You | Runway two five, cleared to land, Sierra Bravo Charlie |
| Tower (after the landing roll) | Sierra Bravo Charlie, vacate, contact Kraków Ground one one eight decimal one zero five, good day |
| You | Vacating, contact Krakow Ground one one eight decimal one zero five, Sierra Bravo Charlie |

## Ground 118.105: back to the stand

| Who | Phrase |
|---|---|
| You | Krakow Ground, Sierra Bravo Charlie, request taxi to stand |
| Ground | Sierra Bravo Charlie, taxi to stand three five via Charlie, Bravo four |
| You | Taxi to stand three five via Charlie, Bravo four, Sierra Bravo Charlie |
| You (on the stand) | Sierra Bravo Charlie, on stand three five |
| Ground | Sierra Bravo Charlie, good day |

If you get lost while taxiing, say "Sierra Bravo Charlie, I am lost, I need help". If you cannot find the stand, say "request follow-me".
