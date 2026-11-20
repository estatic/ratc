# Flight test fixes (plan 14c) — design

Date: 2026-11-19. Source: the second in-sim EPKK→EPKT IFR flight (X-Plane 11, C172 G1000,
callsign N172SP), flown after plan 14a. The user's verdict: "much better"; what is left is
in the radio exchange, plus radar vectoring (B8, plan 14b, a separate design, out of scope).

## Intent

A transmission the pilot made is heard whole, once. A readback or an acknowledgement in
normal, slightly imperfect phraseology is accepted. The controller never answers a
transmission the pilot has already corrected, and does not call a deviation while the pilot
is still reading the instruction back.

## Findings and required behaviour

**C1 — PTT bounce.** A joystick button that lifts for a moment cut transmissions in two
("Katowice Tower," 2.5 s, then the rest), and a 0.2 s press reached Whisper, which made up
"Thank you." from it.
- A release followed by a press of the same source within 300 ms is the same transmission:
  no `Release` event, the audio continues.
- The release is reported only after those 300 ms, and the microphone keeps streaming until
  then, so the last syllable is not lost either.
- A transmission shorter than 0.5 s of audio is not sent; the client prints
  `[ptt] off - too short, nothing sent`.
- The key toggle is unaffected.

**C2 — replies to superseded transmissions.** "read back frequency" (the answer to the
made-up "Thank you.") was spoken after the pilot's full, correct readback, which the
controller had already accepted. When an aircraft's new transmission is answered, every
answer to an earlier transmission of the same aircraft on that frequency that is still
queued and not yet speaking is dropped. Controller transmissions of their own (telemetry,
e.g. a hand-off) are kept.

**C3 — "heading, two nine five".** A comma (Break) between "heading" and its number lost the
heading. A Break between heading/altitude/flight level/QNH/squawk keywords and the number
right after them does not separate them.

**C4 — teen numbers and numbers before the callsign.**
- "ten" … "nineteen" normalise to 10 … 19 (stand eleven, stand 11L).
- A number right after "stand" is the stand, never part of the callsign ("stand 11, November
  Sierra Papa" is callsign NSP, not 11NSP).
- "stand <name>" alone (with or without callsign / station, no other intent) to Ground from
  an aircraft that asked for taxi or was told "say stand" is a taxi request to that stand.

**C5 — arrival without a stand.** "runway vacated, request taxi" was answered "say stand".
An arriving aircraft that requests taxi without a stand is assigned one by Ground, by the
"any stand" rules of 14a (B6): "taxi to stand <X> via ...".

**C6 — "Katowice Ground, Roger".** A transmission whose only words besides station names and
the callsign are acknowledgement words (roger, wilco, copied, thank you, words of the last
instruction) is an acknowledgement, not a call: no "pass your message".

**C7 — follow-me readback.** "follow the follow me card to stand for" was answered "say
again". The comparison with the words last told tolerates the common homophones: for/four,
to/two, card/car, and "follow me" / "follow-me".

**C8 — "check heading, November Sierra Papa" answered "say again".** After a garbled
"check heading, now we've missed your papa" the clean acknowledgement got "say again". Root
cause by reproduction (suspected: joined with the garbled fragment). An acknowledgement of a
"check heading/level" (its words, or the heading/level number) is accepted.

**C9 — callsign missing its first letter.** "Katowice Tower, Sierra Papa" for N-SP was
answered "say again your callsign". A callsign that is the abbreviated or full callsign of
the only aircraft working this frequency minus its first letter or digit is that aircraft
(same rule as 14a's one-letter difference).

**C10 — heading check during the readback.** The heading monitor called "check heading" 35 s
after "turn right heading 340", while the pilot was still reading back (one "read back QNH"
in between). The heading and level deviation timers start when the instruction's readback
is accepted (or, with no readback asked, when the instruction ends), plus 10 s reaction time;
while a readback of that instruction is pending no deviation is called.

**C11 — altimeter setting.** On "descend to 3000" the aircraft levelled at 2455 ft, ~545 ft
low: the difference between standard pressure and QNH 992. When an altitude deviation
(altitude, not flight level, below the transition level) matches the QNH/standard difference
within 100 ft, the controller says "check altimeter setting, QNH <qnh>" instead of
"check level"; in Polish the existing Polish phraseology style.

**C12 — Delivery and an old ATIS letter.** The pilot called Delivery with "information
Alpha" while Bravo was current; Delivery did not correct it (Ground did). Delivery corrects
an old letter like Ground: "information <X> is current, QNH ...", in the clearance reply.

## Constraints

- Every exchange from the flight that is fixed gets a test reproducing it: a replay scenario
  at EPKK (the only station tracked in git), else an engine/parser/client unit test.
- English only in code and docs, except `docs/user-guide/ru/`.
- Simulator names stay out of server, core and the client's main part.
- No user station data under `data/airports` other than EPKK's tracked files.

## Left to the builder

Internal structure, exact type names, where the debounce lives (merger or source thread),
wording of new phrases in the style of the existing phraseology.
