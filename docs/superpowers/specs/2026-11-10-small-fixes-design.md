# Small fixes after the inter-airport flight (plan 12)

Date: 2026-11-10. Sources: plan 11 parked items (`.superpowers/sdd/2026-11-04-plan11-inter-airport-flight/progress.md`,
`final-review.md`), the arrival-fix report (STAR side, reported level, stand names) and the English user
guides report (open questions). Each item below was checked in the code; the root cause is given with
the fix, or the item is dropped with a reason.

## Fixes

1. **Missed-approach altitude comes from the procedure.** `scenarios/epkk-ifr-vor07-missed` has
   Approach say "climb altitude three thousand feet" to an aircraft "climbing four thousand feet",
   while the CIFP VOR/DME 07 missed approach ends with a VM leg at 4000 ft.
   Root cause: `set_missed_approach` sets `plan.cleared_ft` to the published 4000
   (`missed_alt_ft`, correct). Then the first call after the missed approach runs
   `vector_items(force = true)`. Its descent rule (`cleared_ft > intercept_ft && to_point <= 15 NM`)
   fires at once, because the go-around point is always within 15 NM of the 8 NM intercept point.
   So the clearance becomes the intercept altitude (3000 ft, the FAF). The engine test
   `missed_approach_goes_back_to_approach` asserts the wrong `ClimbTo(3000)` on the ZZZZ fixture,
   whose missed altitude is also 4000.
   Fix: a missed approach sets a strip flag `missed_climb`. While it is set, the descent rule does
   not apply, so the clearance after the missed approach is the published altitude: "climb altitude
   four thousand feet". The flag is cleared when the aircraft is within 200 ft of its cleared level
   or when the intercept is given. After that the normal descent to the intercept altitude follows.
   The intercept clearance itself still brings the cleared level down to the intercept altitude.
2. **"Follow published missed approach".** There is no such item. After a go-around, Tower says only
   "contact Approach", or "climb <altitude>" when it keeps the flight (`tower_inbound`).
   New `AtcItem::FollowMissedApproach` (EN "follow published missed approach"; PL project wording
   "wykonuj opublikowaną procedurę nieudanego podejścia"). No readback element is required. Tower
   uses it for an IFR arrival whose approach has published missed-approach legs:
   - The pilot reports "going around": `[FollowMissedApproach, Contact Approach]`. When Tower keeps
     the flight it says `[FollowMissedApproach]` instead of `ClimbTo` (the plan is still cleared to
     the missed altitude).
   - Tower sends the aircraft around: `[GoAround, FollowMissedApproach]`.
   Without published missed legs, the behaviour stays as it is today.
   Approach's first call after the missed approach (heading plus climb to the missed altitude) is
   unchanged: radar vectors replace the published track.
3. **An emulated adjacent Tower blocks the en-route hand-off.** `hand_off` returns early when the
   destination has an `[[adjacent]]` Tower (`destination_tower`), so a flight is not handed to
   ACC/FIS on leaving the TMA, whatever the distance. `direct_after_turn` also clears it direct to the
   destination. Plan 11 limited only the destination Approach (`NEIGHBOUR_DEST_M`, 60 NM).
   Fix: `destination_tower` applies the same neighbour rule: an adjacent Tower whose destination is
   more than 60 NM from this station's airport is ignored, and both the hand-off and the direct
   fall through to the ACC/FIS path.
4. **A SID ceiling before a higher floor loses the floor.** Example: AtOrBelow 4000, then
   AtOrAbove 6000. `initial_climb_ft` gives 4000 (right: the clearance may not exceed the first
   ceiling). But `plan.cruise_ft = max(cruise, 4000)` stays 5000, so Approach's later climb to cruise
   stops below the 6000 floor.
   Ruling: "climb via SID" is not used. The cleared level must respect what the SID publishes. So
   the initial clearance stops at the first ceiling (as today), and the level the flight is later
   climbed to is at least the highest floor, rounded up to 1000 ft.
   Fix: new `ifr::sid_target_ft(path, cruise_ft)` = cruise raised to the rounded highest floor (the
   `target` of `initial_climb_ft`, which now uses it). Delivery and `runway_change` set
   `plan.cruise_ft` to it.
5. **Home stand by name.** The home stand is stored as a name only (`Strip::home_stand`). Taxi-in,
   follow-me, the stand hint and the "claimed" check resolve it to the first stand of that name.
   Fix: store its position too (`Strip::home_stand_at: Option<LatLon>`, serde default). Every use
   builds `TaxiTarget::Stand { name, at: home_stand_at }`, and the claim check compares positions
   when one is known.
6. **Trailing words in "unable, stand X unknown".** The parser's stand capture
   (`parse.rs`, `RequestTaxiToStand`) stops at a word or a Break, but not at the pilot's own
   callsign spelled after the stand. `resolve_stand` trims those letters when a stand matches, but an
   unknown stand is shown with them ("9 9 S B C"). The callsign tokens are already marked `used` by
   `scan_known_callsign`.
   Fix: the stand capture also stops at a `used` token, like `designator_run`.
7. **Polish "passing".** `pl::normalize` has no word for "passing", so in a Polish inbound with only
   a passing level that level is taken as the reported level, and the flight is held at it. Neither
   `docs/phraseology/pl.md` nor any source in the repo gives a Polish form. We add the two common
   forms as project wording: `mijam` (any next token) and `przechodzę` (only before a number;
   "przechodzę na drugi krąg" is a longer rule and still wins). "przelatuję" is not added: it means
   flying over a place and would collide with position reports ("nad").
8. **Stand "9R" said as "nine right".** "stand niner romeo" parses as `stand 9 R` and matches.
   "stand nine right" stops at the word "right" and gives `stand 9`, which matches stand 9 or
   nothing. Fix: in the stand capture, "left", "right", "centre" or "center" directly after a number
   becomes the letter L, R or C (as in runway designators, `parse.rs` ~l.607).
9. **The handed-in first call counts garbled transmissions.** The "<station>, roger" fallback is
   gated on `heard == 1`. A garbled first transmission (callsign heard, nothing understood) uses up
   the first call and gets "roger", which hides the misrecognition. The real call that follows then
   gets "say again".
   Fix: a strip flag `first_call_due` (set by `import_flight`, serde default). Only an understood
   transmission (`understood(&msg)`) clears it. The fallback needs the flag and an understood
   message. A garbled call gets "say again" and leaves the first call due.
10. **Polish Tower-only inbound test.** The plan 11 PL renderings of `ExpectApproach` and
    `ReportEstablished` from Tower have no engine-level test. Add one: a Polish session, a Tower-only
    station, an IFR inbound handed in; Tower answers in Polish.
11. **Docs.** The "Voices" section of `docs/user-guide/{en,ru}/ifr-and-vfr-points.md` lists Katowice
    Tower among the radar units. That is true only while EPKK emulates it. With EPKT loaded, it is a
    full station. Fix the wording in both languages. The missed-approach section (6) and
    `docs/phraseology/{en,pl}.md` follow fixes 1 and 2. Fix 7's words go into `pl.md`.

## Dropped

- **Move a replay scenario from stand 35 to stand 9R.** The replay loads the airport from the
  repository's `data/airports/EPKK/apt.dat` (`atc-replay` `load_airport`). Stand 9R is not in that
  file; it exists only in the X-Plane 11 default scenery. Moving a scenario would need 9R added to
  the hand-made apt.dat, which changes the taxi graph for every golden. The other option is to read
  the X-Plane apt.dat, which would make a plain scenario depend on a local install. Stand 35 stays.
  The guides already say that 9R comes from the X-Plane scenery.

## Non-goals

- Monitoring the published missed-approach track (Approach vectors after the missed approach, as
  today).
- Polish words for "passing" beyond the two forms above. Approach still speaks English.
- "Climb via SID" phraseology.

## Testing

Engine tests run with `--features testkit` against the ZZZZ fixtures. No test reads the git-ignored
station data. Goldens that change (expected: `epkk-ifr-vor07-missed` for fixes 1 and 2) are
regenerated only in a clean worktree with `UPDATE_GOLDEN=1`. Every changed line is explained by a
fix above.
