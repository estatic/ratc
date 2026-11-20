# Radar vectoring with a planned path (plan 14b) — design

Date: 2026-11-20. Sources: findings B8 (flight test 2026-11-11) and C13 (flight test
2026-11-19), and the user's requirements of 2026-11-11 and 2026-11-20.

## Intent

In both in-sim flights the aircraft overshot the ILS 27 localizer at Katowice (about 7 km
the first time, 0.5 NM the second) and the pilot had to correct alone. The controller gave
an intercept heading and then nothing more. A real radar controller knows in advance where
the aircraft should go, watches it the whole way, and when the aircraft drifts from that
path (late turn, missed transmission, readback trouble, wind) gives a new heading or level
with a short reason. The user's words: "the controller must know the route in advance, but
the pilot may deviate because of communication problems or delays, so the controller must
accompany the aircraft and give additional heading and altitude instructions".

The controller does not try to predict the pilot. It acts on what the radar shows: position,
track, heading, ground speed, altitude.

## Goals

1. When Approach takes an arrival on radar vectors, it builds a **planned path** to the final
   approach course: the legs (downwind / base / intercept, whichever are needed) with the
   heading and the altitude for each, ending on the final approach course outside the FAF.
2. On every radar update it compares the aircraft with the plan and **stays silent while the
   aircraft follows it** (in tolerance, or visibly turning/descending the right way).
3. When the aircraft is out of tolerance, it **re-plans from where the aircraft actually is**
   and transmits the new heading and/or level with the reason (position relative to the final
   approach course or fix, distance from touchdown).
4. The normal next instruction (next leg's heading, the descent, the intercept heading with
   the approach clearance) is given in time for the aircraft's ground speed, not at a fixed
   distance.
5. After the approach clearance it keeps watching until the aircraft is established: an
   aircraft flying through the localizer, or not turning onto it, gets a correction to
   re-intercept; when the re-plan can no longer join the final in time, the aircraft is taken
   off the approach and vectored around again.
6. Works with any simulator: the engine uses only the radar data the adapter already provides.

## Non-goals

- The filed route from the simulator's GPS / FMS plan and "the flight plan leads, vectors are
  an exception" (B12) — the next step (plan 14d), built on this planned path. Not here.
- Tracking SIDs, STARs or direct-to legs (already monitored as off-track by compliance).
- Separation between several aircraft (traffic phase 2). One aircraft at a time on vectors is
  enough here, but nothing in the design may assume there is only one.
- Predicting the pilot's turn or reaction time.

## Anti-goals (would make it a failure even if it works)

- A chatty controller: a correction while the aircraft is already turning or descending the
  right way, or repeated corrections faster than a pilot can answer.
- Contradicting itself: a correction during the readback of the previous instruction.
- A path the aircraft cannot fly: an intercept steeper than 45 degrees, an intercept inside
  the FAF, a descent steeper than a light aircraft can make, or a turn point already behind.
- Simulator-specific logic in core.

## Behaviour

### The planned path

- Built when vectoring starts (today: the arrival's first vector, after a STAR's last fix, or
  a missed approach) and rebuilt on every correction.
- Shape, per Doc 4444 / PANS-OPS practice already used in `engine/approach.rs`:
  - an aircraft that can join directly (the turn onto the intercept heading is not more than
    90 degrees) gets one heading to an intercept leg;
  - otherwise downwind abeam the final, base, intercept (today's pattern: downwind 4 NM abeam,
    base 3 NM beyond the intercept point);
  - the intercept leg meets the final approach course at 20–30 degrees (30 nominal), at least
    2 NM outside the FAF, at the FAF altitude or below the glide path (today's intercept
    altitude);
  - every leg has an altitude: the descent is planned so the intercept altitude is reached
    before the intercept leg, at no more than a light aircraft's comfortable descent
    (builder's value, about 300 ft per NM), and never below the minimum vectoring altitude
    the station data gives (if any; else the intercept altitude).
- Each leg's heading is magnetic, rounded to 5 degrees, as today.

### Giving the next instruction

- The heading for the next leg is given when the aircraft is one turn-lead from the turn
  point: the turn radius at the aircraft's present ground speed at standard rate (half rate
  above 250 kt), plus 10 s of reaction. This is the controller's own timing from radar ground
  speed, not a prediction of the pilot.
- The descent to a leg's altitude is given when the plan needs it (distance to go to where
  the altitude must be reached at the planned gradient), not at a fixed 15 NM.
- The intercept heading comes with the distance from touchdown (or position relative to the
  FAF), "cleared <approach> runway <rwy>" and "report established", as today.

### Watching and correcting

- While a readback of the last vector or level is pending, and for 10 s after it is accepted,
  the aircraft is not judged (same rule as 14c C10).
- Out of tolerance (builder's values, starting points): more than 0.5 NM off the planned leg
  (1 NM on downwind), or more than 15 degrees off the assigned heading without turning
  towards it, or more than 300 ft off the planned altitude without climbing/descending
  towards it.
- A correction re-plans from the present position and says the new heading and/or level with
  the reason, e.g. "you are one mile north of the final approach course, turn left heading
  two eight zero", "descend to altitude two thousand five hundred feet", "turn left heading
  two seven zero to re-intercept the localizer". At most one unsolicited correction per
  20 s per aircraft.
- After the approach clearance, until established: crossing the final approach course by more
  than about 0.3 NM, or flying towards it at more than 45 degrees within 1 NM of it, gets a
  re-intercept correction (heading up to 45 degrees onto the course).
- If the re-plan cannot join the final at least 2 NM outside the FAF, the aircraft is taken
  off the approach: "turn <left/right> heading <h>, <climb/maintain> <alt>, I will vector you
  for a new approach" (approach clearance cancelled), and a new pattern is planned.
- An aircraft that does not fly the assigned heading at all is handled by the existing
  "check heading, fly heading <h>" (compliance); the plan is rebuilt after it.

### Decisions made after the builder check (designer's calls, to be checked by the user)

1. **Established is judged by radar.** A pilot's "established" report (or the aircraft on the
   final inside the FAF) transfers the flight to Tower only when the radar agrees: within
   about 0.3 NM of the final approach course and tracking within 15 degrees of it. Otherwise
   Approach answers with the correction ("negative, you are half a mile north of the
   localizer, turn left heading two six zero to re-intercept") and keeps the flight. Tower
   never vectors.
2. **One owner while a plan is active.** On vectors the planner owns heading and altitude
   judgement: compliance's heading and level deviation calls are suppressed for an aircraft
   with an active plan. An aircraft that has not turned at all after the reaction time gets
   the planner's correction in the "check heading" form ("check heading, turn left heading
   two eight zero"); a wrong heading or a drift gets the plain correction with the reason.
3. **Scope of the final-course watch:** every arrival Approach vectored onto the final, any
   approach type. The wording names the course by type: "localizer" for ILS, "final approach
   course" otherwise. Approaches cleared at the IAF (full procedure) keep today's off-track
   monitoring only.
4. **Legs are ground tracks.** The plan stores tracks; the heading given is the track
   corrected for the drift the radar shows (radar track from successive positions minus
   heading, smoothed over the last ~30 s). A correction is transmitted only when the new
   heading differs from the last one given by at least 10 degrees, or the aircraft is out of
   the cross-track tolerance and not converging. The test pilot gets a steady wind.
5. **Break-off:** turn away from the final towards the side the new pattern is flown on
   (the circuit side when the aircraft is on the centreline), "maintain" the higher of the
   present altitude (rounded to 100 ft) and the intercept altitude, with "I will vector you
   for a new approach"; this cancels the approach clearance on the strip (no separate
   "cancel approach clearance" phrase), needs the usual heading/altitude readback, and is not
   a missed approach (no missed-approach flags, no change for Tower, which does not own the
   flight yet).

### Relation to existing behaviour

- `vector_to_final`, `steer`, the pattern legs and the `PatternLeg` strip state are replaced by
  the planned path; their geometry rules (intercept point, 30-degree intercept, pattern
  distances, the 90-degree pattern threshold, the far-side pattern) carry over.
- The intercept heading at a fixed 2 NM from the centreline, the descent at a fixed 15 NM and
  the 20-degree re-vector threshold are replaced by the timing and tolerance rules above.
- "check established" at the FAF (spec 3.5) stays.
- Approach and adjacent units always speak English (unchanged).

## Constraints

- Simulator-neutral: core and server use only `AircraftState` (position, heading, ground
  speed, altitude, vertical speed); nothing simulator-specific is added.
- Every behaviour gets a test with a pilot that reacts with delay: the engine-test pilot in
  `crates/atc-core/tests/engine.rs` (`approach_arrival::Pilot`, which already obeys headings
  and reads back) gains a configurable reaction delay and overshoot. The two flight-test
  geometries (B8: base across the localizer; C13: 90-degree base, intercept heading followed
  ~40 s late) are reproduced at EPKK ILS 25 (the only tracked station) and must end
  established without the pilot correcting on its own.
- Replay scenarios that change output are regenerated and each changed line checked.
- English only in code and docs (except `docs/user-guide/ru/`).

## Approaches considered

- **A. Planned path and accompaniment (chosen by the user).** One plan, re-planned from the
  present position on every correction; silence while followed. One rule covers late turns,
  overshoots and breaking off.
- B. Keep today's position rules and add corrections after the intercept heading. Less change,
  but the controller still has no route known in advance, and every new case becomes a patch.
- C. Predict the trajectory with a turn model. Rejected: the pilot cannot be predicted.

## Left to the builder

The data structure of the plan and where it lives (strip vs track state), exact tolerances and
the descent gradient within the ranges above, exact wording of corrections in the existing
phraseology style (Doc 4444 vectoring phrases), and how the plan is shown in logs.
