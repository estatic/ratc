# Radio discipline: one speaker per frequency, readbacks without a callsign

Date: 2026-11-03. Source: the user's EPKK circuit test (C172 N172SP).

## Why

A real frequency is half-duplex: one station talks, everyone else (controller and other
aircraft) listens and waits. In the test the controller talked over the pilot several
times:

- The readback reminder fired 10 s after the taxi clearance, while the pilot was still
  reading it back (a 13.9 s transmission). The server only learns what the pilot said after
  PTT release + speech-to-text, so it thought the pilot was silent.
- The pilot broke off a transmission and keyed again at once. The answer to the first part
  ("say again your callsign", then "read back frequency") was spoken during the second.
- One pilot call got two controller transmissions back to back (a say-again-callsign and a
  readback reminder).

Root cause: the server's hub keeps a per-frequency PTT lock, but only between pilots.
Controller speech (`pipeline::emit_all`) never looks at it. Outputs from telemetry
(`session.rs`, spawned per sample) and from transmissions are emitted concurrently, so the
controller can also talk over itself.

The same test showed that readbacks without a callsign were refused with "Station calling
Kraków Ground, say again your callsign", although only one aircraft was in the exchange.

## Goals

1. **First to talk holds the frequency.** One speaker per frequency at a time: a pilot (PTT
   or typed text) or a controller position on it.
2. **The pilot cannot step on others.** Pressing PTT while the controller or another pilot is
   talking does not transmit. The server does not hear, transcribe or answer it (as if
   nobody received it). The client shows "frequency busy". The pilot must release and press
   again once the frequency is free; holding PTT does not start a transmission when the
   frequency clears.
3. **The controller never steps on the pilot.** From the moment PTT is pressed the frequency
   is the pilot's. Controller transmissions on it wait in a queue until the pilot has
   released PTT, the transmission has been transcribed and the engine has answered it (that
   answer goes first).
4. **One controller transmission at a time per frequency, in order.** Each item is checked
   again just before it goes on air. An item that no longer applies is dropped, e.g. a
   readback reminder after the readback arrived, or a second reaction to the same pilot call.
   Replies to pilot transmissions are never dropped.
5. **The readback reminder counts from a free frequency.** The 10 s wait does not run while
   the frequency is busy (pilot talking or being processed, controller talking). It restarts
   when the frequency becomes free.
6. **Readback or call without a callsign.** When the aircraft is unambiguous (exactly one
   aircraft worked by that position on the frequency, and it is the one the position last
   spoke to; the rule `repeat()` already uses), the transmission is handled as that
   aircraft's:
   - 1st and 2nd time in a flight: accepted. A correct readback is answered with
     "<callsign>, roger" so the pilot hears who they were taken for. Other calls are answered
     normally.
   - 3rd and every later time: accepted the same way, and the answer ends with a reminder to
     use the callsign. English: "<callsign>, use your callsign in every transmission". Polish:
     an equivalent phrase.
   - Ambiguous (no aircraft, or more than one candidate): unchanged, "say again your
     callsign".

## Non-goals

- Simulated blocked transmissions (squeal, "blocked, say again"). The first speaker simply
  wins.
- Cutting off controller speech that is already playing.
- A stuck-mic timeout. A held PTT keeps the frequency.
- ATIS. It has its own frequency and keeps looping as now.
- Pilot-to-pilot relay changes, beyond the pilot-versus-controller floor.

## Behaviour details

- **Controller floor duration.** The server sends a whole utterance's audio in one burst and
  the client plays it in real time. The server therefore holds the frequency from the
  utterance start until start + audio duration + a small margin for playback latency
  (builder's choice, around 300 ms). If TTS fails (text only), the duration is estimated
  from the words (`WORDS_PER_S` = 2.5 already exists in the engine).
- **Gap.** After any transmission ends, the next controller transmission on that frequency
  starts no sooner than 1 s later (my call; it sounds natural and gives the pilot a chance
  to key first).
- **Pilot processing.** After PTT release the frequency stays the pilot's until the
  transcript is handled: answer queued first, or no answer / empty transcript /
  say-again. The pilot's own second PTT in that window is allowed (it is the same speaker
  continuing). Another pilot's PTT in that window is blocked.
- **Broken-off transmissions.** Joining a fragment with the next transmission on the
  frequency (`FRAGMENT_S`) keeps working. The callsign-less rule (goal 6) must not prevent a
  fragment from being joined with a following transmission that carries the callsign.
- **Typed text** (`TextTransmission`) follows the same floor. While busy it is rejected with
  `FrequencyBlocked` and dropped. While it is processed, the frequency is the pilot's.
- **Low-confidence "say again"** and every other controller output go through the same
  queue.
- **Vehicle orders** (follow-me car) are not delayed by the queue.
- **Several clients.** Each pilot is held off and queued the same way. A reply to pilot A
  waits while pilot B talks.
- **Callsign-miss counter.** Per flight (strip). It does not reset on hand-off between
  positions (my call: it is the pilot's habit, not the position's).

## Decisions after the builder check (my calls)

- **The floor is per frequency, not per position.** Several positions can share one frequency
  (combined positions), and they share one queue. A queued item survives a hand-off.
- **Pilot floor lifecycle.** The pilot's floor is taken at `PttStart` when it is granted. It
  lasts past `PttEnd` and is released as soon as the transcript has been handled. On the
  normal path that means once the engine's answer is queued, and the answer then goes first.
  On an STT error, an empty transcript, a low-confidence say-again (queued first) or a
  dropped job, it is released at once. A refused press is refused at `ptt_start`
  (`FrequencyBlocked`). The client already discards its audio while blocked, and the server
  ignores audio without a granted PTT. "Holding PTT does nothing when the frequency clears"
  is enforced server-side: only a new `PttStart` after a `PttEnd` can be granted.
- **No "controller talking" indicator.** The client shows only the rejection on press. The
  pilot hears the controller anyway.
- **Readback reminder.** The engine gets a busy/free input per frequency from the server. While
  the frequency is busy, the reminder cannot fall due. The 10 s wait is a reset: it counts
  10 s from the moment the frequency became free. The "aircraft started moving" trigger
  also waits for a free frequency. `strip.reminded` is set only when the reminder is actually
  spoken. A reminder dropped at revalidation leaves it unset, so the reminder can fall due
  again.
- **What revalidation drops.** Only readback reminders (`ReadBack(pending)` from
  `readback_reminder`). One is dropped when, at air time, its readback is no longer pending,
  or when the aircraft has transmitted on that frequency since the reminder was generated.
  The engine has already answered that transmission, e.g. with the correction. Every other
  output (replies, deviation calls, crossing, hand-offs, follow-me) is always spoken, in
  order.
- **Callsign-miss counter.** It counts every transmission the engine attributed to an aircraft
  by inference (goal 6), whatever its content, including a bare "roger". An inferred
  transmission is handled, so it is not kept as a fragment. Joining a callsign-less fragment
  with a following transmission still applies when the fragment could not be attributed.
  The counter lives on the strip for the strip's lifetime. It is carried in `FlightHandover`
  and the persisted snapshot, with a serde default of 0 so old files still load.

## Constraints

- Existing behaviour and tests stay green, apart from tests that assert the old overlapping
  or callsign-refusing behaviour, which are updated.
- Deterministic engine (sim time). The floor is server-side wall-clock state. The engine
  learns "frequency busy/free" from the server through whatever interface the builder picks.
- No protocol break: `FrequencyBlocked` already exists. A new client message text is fine,
  e.g. "[ptt] frequency busy - wait for Kraków Ground to finish".
- English docs and code. RUNNING.md and the RU user guide mention the rule.

## Left to the builder

- Where the floor lives (extend `hub` or a new module) and how the per-frequency queue is
  run (one task per frequency, or a mutex-guarded queue).
- How items are revalidated (e.g. outputs carry a kind and generation that the engine checks
  with a `still_due`-style call).
- The exact Polish wording of the callsign reminder (the phraseology crate has the pattern).
