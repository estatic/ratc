# Procedures and All Positions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The controller follows the published EPKK procedures (SIDs, STARs, instrument approaches from the X-Plane CIFP; VFR points and local rules from AIP Poland) and staffs every real position: ATIS, Delivery, Ground, Tower, Approach, plus hand-off stubs for the adjacent units.

**Architecture:** `atc-navdata` gains a CIFP parser, a fix database read from the X-Plane install and new `procedures.toml` sections; nothing in code names EPKK. `atc-phraseology` gains the IFR/VFR vocabulary (clearances, SIDs, STARs, vectors, approaches, VFR points) in English and, for Delivery, Polish. `atc-core` gains two roles (Delivery, Approach), a flight plan on the strip, an `ifr` module (procedure geometry, SID/STAR choice, vectors to final) and new engine modules `delivery.rs` and `approach.rs`; Approach and the adjacent units always speak English. The server staffs the new stations with their own voices and the client sends the sim UTC time.

**Tech Stack:** Rust workspace (edition and toolchain as in `Cargo.toml`), serde/toml, thiserror, existing crates only.

**Spec:** `docs/superpowers/specs/2026-10-25-procedures-and-positions-design.md` (parents: `2026-10-19-virtual-atc-design.md`, `2026-10-25-roadmap.md`).

## Global Constraints

- Nothing in code may name EPKK, Kraków or a Kraków frequency; airport facts live in `data/airports/<ICAO>/procedures.toml` and the X-Plane navdata for that ICAO (spec §3.7).
- Procedures come from the X-Plane CIFP of the user's install (`Custom Data/CIFP/<ICAO>.dat` overrides `Resources/default data/CIFP/<ICAO>.dat`; fixes from `earth_fix.dat`/`earth_nav.dat` the same way). AIP text rules and frequencies come from AIP Poland AIRAC AMDT 10-26 (spec §2, §4).
- **Never commit X-Plane navdata** (CIFP, earth_fix, earth_nav: Navigraph/Jeppesen copyright). Committed tests use the fictional fixture airport `ZZZZ` (Task 1). Tests on the real EPKK data read the install from `ATC_XPLANE_ROOT` (default: `$HOME/.local/share/Steam/steamapps/common/X-Plane 11`) and skip with a printed note when it is missing, as `crates/atc-core/tests/xplane_install.rs` does.
- Frequencies (AIP, 8.33 kHz channels): ATIS 126.130, Delivery 121.980, Ground 118.105, Tower 123.255, Approach 121.075 (also 126.530 and 135.405 for the same unit), Warszawa Radar 127.660, Kraków Information 119.275, Katowice Tower 129.255.
- Transition altitude 6500 ft; default IFR cruise level for the C172: 5000 ft.
- Delivery open 0600–2200 UTC in summer time, 0500–2100 UTC in winter time; outside it Ground issues clearances (AIP AD 2.20).
- Approach and adjacent units speak English only; Delivery, Ground and Tower follow the session language as today (spec §3.1). Polish wording not in the plan-3a reference is marked `project wording` in `docs/phraseology/pl.md`.
- Altitude constraints above the cleared level do not apply; compliance checks only constraints at or below it (spec §3.2).
- Default arrival: radar vectors to final; full procedure from the IAF on request. Default approach: ILS on RWY 25, RNAV on RWY 07; VOR/DME on request (spec §3.3).
- Every engine path works for any registered aircraft, never assuming one aircraft or the user's (spec §3.6).
- Existing `scenarios/*.expected.txt` change only where an AIP rule changes an exchange (Task 8), with the reason in the commit message.
- The user's `config/client.toml` and `track.csv` are never touched or committed.
- Code style: match surrounding code (doc comments on every pub item, `thiserror` errors in libraries, `anyhow` in binaries, no `unwrap` outside tests except documented invariants).

## Review Focus

1. A pilot request naming a destination that is not in apt.dat (or misheard) → Delivery answers "unable, say again destination" (not a panic, not a SID to nowhere). Test in Task 7.
2. The X-Plane install lacks the CIFP for the airport (old install, other sim) → server starts, logs one warning, IFR requests are answered "unable IFR, VFR only" and the VFR/circuit behaviour is unchanged. Tests in Task 2 and Task 12.
3. The pilot reads back a SID or STAR designator spoken in another form ("Sudol two J", "SUDOL 2 Juliett", Polish "Sudol dwa Juliett") → accepted; a wrong designator → corrected. Test in Task 5.
4. Telemetry without UTC time (old client, replay) → Delivery treated as open (no crash, no permanent Ground fallback). Test in Task 7.
5. An aircraft on vectors that ignores a heading for 30 s, or descends below the cleared altitude → one deviation call in English, not repeated every sample. Test in Task 10.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/atc-navdata/src/cifp.rs` (new) | Parse one CIFP airport file into procedures and runway thresholds |
| `crates/atc-navdata/src/fixes.rs` (new) | Fix/navaid coordinates from earth_fix.dat / earth_nav.dat; resolve procedure legs |
| `crates/atc-navdata/src/xplane.rs` | + `load_procedure_set` (find CIFP + fixes in the install) |
| `crates/atc-navdata/src/procedures.rs` | + VFR points, VFR holds, adjacent units, TMA, Delivery hours, IFR defaults |
| `crates/atc-navdata/tests/fixtures/xplane/` (new) | Fictional `ZZZZ` CIFP, earth_fix, earth_nav excerpts (committed) |
| `data/airports/EPKK/procedures.toml`, `SOURCE.md` | EPKK values from AIP |
| `crates/atc-phraseology/src/types.rs` | New intents, heard items, ATC items, readback items |
| `crates/atc-phraseology/src/render.rs`, `pl/render.rs` | English rendering of new items; Polish for Delivery/Ground/Tower items |
| `crates/atc-phraseology/src/parse.rs`, `pl/normalize.rs` | New requests and items; vocabulary of points/fixes/designators |
| `crates/atc-core/src/ifr.rs` (new) | Procedure paths, SID/STAR choice, vectors-to-final geometry, track deviation |
| `crates/atc-core/src/strip.rs` | `Role::{Delivery, Approach}`, `FlightPlan`, hand-off target |
| `crates/atc-core/src/phase.rs` | `FlightPhase::Airborne` for flights not in the circuit |
| `crates/atc-core/src/state.rs` | `AircraftState::utc_s`, `day_of_year` |
| `crates/atc-core/src/compliance.rs` | Track, heading and procedure-altitude deviations |
| `crates/atc-core/src/engine/delivery.rs` (new) | Clearance requests (Delivery or Ground) |
| `crates/atc-core/src/engine/approach.rs` (new) | Departures after take-off, arrivals, vectors, approach clearances, hand-offs |
| `crates/atc-core/src/engine/vfr.rs` (new) | VFR entry/exit via points, holds, night rule |
| `crates/atc-core/src/engine/{mod,ground,tower}.rs` | Wiring, push/start, Tower calls the aircraft at the holding point |
| `crates/atc-proto`, `atc-client`, `atc-xplane-plugin` | UTC time and day of year in telemetry |
| `crates/atc-server/src/{app,layout,config,main}.rs`, `atc-voice` | Stations, voices, loading the procedure set |
| `config/voices.toml`, `tools/fetch-piper.sh`, `tools/MODELS.md` | Voices for Delivery, Approach, adjacent units |
| `crates/atc-replay/src/lib.rs`, `crates/atc-core/src/testkit.rs` | Path tracks, new frequencies, xplane-navdata scenarios |
| `scenarios/epkk-ifr-*.toml`, `epkk-vfr-*.toml` | New golden scenarios |
| `docs/user-guide/ru/ifr-and-vfr-points.md`, `docs/phraseology/*.md` | User guide and phraseology reference |

---

### Task 1: CIFP parser

**Files:**
- Create: `crates/atc-navdata/src/cifp.rs`
- Create: `crates/atc-navdata/tests/fixtures/xplane/Resources/default data/CIFP/ZZZZ.dat`
- Create: `crates/atc-navdata/tests/cifp_parse.rs`
- Modify: `crates/atc-navdata/src/lib.rs` (add `pub mod cifp;`)

**Interfaces:**
- Consumes: `NavdataError` (`apt.rs`), `atc_geo::LatLon`.
- Produces:
  ```rust
  pub enum ProcKind { Sid, Star, Approach }
  pub enum ApproachKind { Ils, VorDme, Rnav }          // route type I / D / R
  pub enum PathTerm { If, Tf, Cf, Df, Fa, Vm }          // exactly the leg types EPKK.dat/EPKT.dat use
  pub enum AltConstraint { None, At(i32), AtOrAbove(i32), AtOrBelow(i32), Between { above: i32, below: i32 } }
  pub struct FixRef { pub ident: String, pub region: String, pub section: char }  // 'P','E','D','G'
  pub struct Leg { pub fix: Option<FixRef>, pub path: PathTerm, pub alt: AltConstraint,
                   pub course_deg: Option<f64>, pub iaf: bool, pub faf: bool, pub map: bool, pub missed: bool }
  pub struct Procedure { pub kind: ProcKind, pub ident: String, pub transition: String,
                         pub approach: Option<ApproachKind>, pub legs: Vec<Leg> }
  pub struct CifpRunway { pub ident: String, pub threshold: LatLon, pub elevation_ft: i32 }
  pub struct Cifp { pub icao: String, pub runways: Vec<CifpRunway>, pub procedures: Vec<Procedure> }
  pub fn parse_cifp(text: &str, icao: &str) -> Result<Cifp, NavdataError>;
  impl Cifp {
      pub fn sids(&self, runway: &str) -> Vec<&Procedure>;        // transition "RW25" or "RW25B"-style matching runway
      pub fn stars(&self, runway: &str) -> Vec<&Procedure>;
      pub fn approaches(&self, runway: &str) -> Vec<&Procedure>;  // final segment (transition "") first, then transitions
  }
  ```

Format notes (X-Plane CIFP, one record per line, `KIND:` then comma-separated fields, `;` ends the line): field 0 sequence, 1 route type (`5` SID/STAR, `A` approach transition, `I`/`D`/`R` final approach), 2 procedure ident, 3 transition (`RW25`, IAF name, or blank), 4 fix ident, 5 region, 6 section, 8 waypoint description (4 chars; char 4 `A` IAF, `F` FAF, `M` missed approach point, char 2 `Y` fly-over), 11 path terminator, 20 magnetic course in tenths of a degree, 22 altitude descriptor (`+` at or above, `-` at or below, `B` between, blank at, `J`/`H` at — glide-slope intercept), 23 altitude 1, 24 altitude 2 (`FL090` = 9000 ft). Legs after the `M` leg are `missed`. `RWY:RW07 ,…;N50043143,E019460514,0000;` gives the threshold. `PRDAT` lines are ignored. An unknown path terminator is `NavdataError::Malformed` naming the line.

- [ ] **Step 1: Write the fixture.** `ZZZZ.dat` is a fictional airport at N10°/E010° (no real procedure names): runways `RW09`/`RW27`; SIDs `ALFA1A` (RW27: CF to `ZZ001` with `+` 3000, TF `ZZ002` with `+FL080`, TF `ALFAX`) and `BRAV1A` (RW27, ends at `BRAVX` to the south); STAR `ALFA1B` (RW27: IF `ALFAX`, TF `ZZ010`, TF `ZZ011` IAF); approach `I27` with transition `ZZ011` (`A` route type) and final (`I`: IF `ZZ020` `J 03000 03000`, CF `ZZ021` FAF `H 03000 03000`, CF `RW27` MAP, CF `ZZ022` missed, VM course 270 `04000`); approach `R09` (RNAV, IF/TF/TF/MAP); one `PRDAT` line. Write each record in exactly the column layout of the samples above.

- [ ] **Step 2: Write the failing tests** in `crates/atc-navdata/tests/cifp_parse.rs`:
  ```rust
  #[test] fn parses_fixture_procedures() {
      let c = parse_cifp(&fixture("ZZZZ"), "ZZZZ").unwrap();
      assert_eq!(c.runways.len(), 2);
      let sids: Vec<_> = c.sids("27").iter().map(|p| p.ident.as_str()).collect();
      assert_eq!(sids, ["ALFA1A", "BRAV1A"]);
      let alfa = c.sids("27")[0];
      assert_eq!(alfa.legs[0].path, PathTerm::Cf);
      assert_eq!(alfa.legs[0].alt, AltConstraint::AtOrAbove(3000));
      assert_eq!(alfa.legs[1].alt, AltConstraint::AtOrAbove(8000));
      assert_eq!(c.stars("27")[0].legs.last().unwrap().iaf, true);
  }
  #[test] fn approach_marks_faf_map_and_missed() {
      let c = parse_cifp(&fixture("ZZZZ"), "ZZZZ").unwrap();
      let i27 = c.approaches("27").into_iter().find(|p| p.transition.is_empty()).unwrap();
      assert_eq!(i27.approach, Some(ApproachKind::Ils));
      assert!(i27.legs[1].faf && i27.legs[2].map);
      assert!(i27.legs[3].missed && i27.legs[4].missed);
      assert_eq!(i27.legs[4].path, PathTerm::Vm);
      assert_eq!(i27.legs[0].alt, AltConstraint::At(3000));
  }
  #[test] fn unknown_leg_type_is_an_error() { /* a line with path "RF" -> Err(Malformed { line: n, .. }) */ }
  ```
  and an install test `epkk_cifp_loads_all_procedures` (skips without `ATC_XPLANE_ROOT`): 20 SIDs (10 `…1F` on 07, 10 `…2J` on 25), 16 STARs, approaches `I25`, `D07`, `D25`, `R07`, `R25`; `SUDO2J` first leg `KK599` `AtOrAbove(3000)`.

- [ ] **Step 3: Run** `cargo test -p atc-navdata --test cifp_parse` — Expected: FAIL (`parse_cifp` not found).
- [ ] **Step 4: Implement** `cifp.rs` per the interfaces and format notes.
- [ ] **Step 5: Run** `cargo test -p atc-navdata` — Expected: PASS (install test prints "skipped" when no install).
- [ ] **Step 6: Commit** `feat(navdata): parse X-Plane CIFP procedures`.

### Task 2: Fix database and procedure set from the X-Plane install

**Files:**
- Create: `crates/atc-navdata/src/fixes.rs`
- Modify: `crates/atc-navdata/src/xplane.rs`, `lib.rs`
- Create: fixture `crates/atc-navdata/tests/fixtures/xplane/Resources/default data/earth_fix.dat`, `earth_nav.dat` (ZZZZ fixes only, real file header lines)
- Test: `crates/atc-navdata/tests/procedure_set.rs`

**Interfaces:**
- Consumes: Task 1 `Cifp`, `Leg`, `FixRef`.
- Produces:
  ```rust
  pub struct ResolvedLeg { pub leg: Leg, pub pos: Option<LatLon> }        // None for VM/FA legs
  pub struct ResolvedProcedure { pub procedure: Procedure, pub legs: Vec<ResolvedLeg> }
  pub struct ProcedureSet { pub icao: String, pub cycle: String, pub runways: Vec<CifpRunway>,
                            pub procedures: Vec<ResolvedProcedure>, pub fixes: BTreeMap<String, LatLon> }
  impl ProcedureSet {
      pub fn sids(&self, runway: &str) -> Vec<&ResolvedProcedure>;
      pub fn stars(&self, runway: &str) -> Vec<&ResolvedProcedure>;
      pub fn approach(&self, runway: &str, kind: ApproachKind) -> Option<&ResolvedProcedure>; // final segment
      pub fn fix(&self, ident: &str) -> Option<LatLon>;   // any fix used by a procedure of this airport
  }
  // xplane.rs
  pub fn load_procedure_set(xp_root: &Path, icao: &str) -> Result<ProcedureSet, ResolveError>;
  // model.rs: pub struct Destination { pub icao: String, pub name: String, pub pos: LatLon }
  pub fn load_destinations(xp_root: &Path, around: LatLon, radius_nm: f64, names: &[(String, String)] /* ICAO -> preferred name */) -> Result<Vec<Destination>, ResolveError>;
      // airports in the apt.dat files X-Plane loads (same search order as resolve_airport), header-row name title-cased unless `names` has one
  ```
  Runway fixes (`RW25`, section `G`) resolve to the CIFP runway threshold. `ResolveError::NotFound` when no CIFP file exists for the ICAO; a leg fix not found in either file is `ResolveError::UnresolvedFix { icao, ident }` (new variant). `cycle` is the `data cycle NNNN` from the earth_fix.dat header. The files are streamed; only the fixes of the airport's procedures are kept.

- [ ] **Step 1: Write the failing tests:** `destinations_within_radius` (fixture apt.dat with three airports, one beyond the radius; preferred name wins); `zzzz_set_resolves_every_leg` (fixture root: every non-VM leg has `pos`, `fix("ALFAX")` is the earth_fix value, `RW27` resolves to the threshold); `custom_data_overrides_default` (a temp dir copy with `Custom Data/CIFP/ZZZZ.dat` lacking `BRAV1A` → `sids("27").len() == 1`); `missing_cifp_is_not_found`; install test `epkk_set_resolves` (`fix("SUDOL")` ≈ 50.6533 N 18.4117 E, `fix("KK599")` ≈ 50.0571 N 19.6378 E, cycle `"1802"`).
- [ ] **Step 2: Run** `cargo test -p atc-navdata --test procedure_set` — Expected: FAIL.
- [ ] **Step 3: Implement** `fixes.rs` and `load_procedure_set`.
- [ ] **Step 4: Run** `cargo test -p atc-navdata` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(navdata): resolve procedure fixes from the X-Plane install`.

### Task 3: `procedures.toml`: VFR points, adjacent units, TMA, Delivery hours, IFR defaults

**Files:**
- Modify: `crates/atc-navdata/src/procedures.rs`, `data/airports/EPKK/procedures.toml`, `data/airports/EPKK/SOURCE.md`
- Test: `crates/atc-navdata/tests/procedures_toml.rs` (new)

**Interfaces:**
- Produces (all new fields `#[serde(default)]` so old files still parse):
  ```rust
  pub struct VfrPoint { pub name: String, pub lat: f64, pub lon: f64, #[serde(default)] pub south: bool }
  pub struct AdjacentUnit { pub callsign: String, pub khz: u32, pub kind: AdjacentKind,
                            #[serde(default)] pub airport: Option<String>, #[serde(default)] pub min_level_ft: Option<i32> }
  pub enum AdjacentKind { Acc, Fis, Tower }
  pub struct Tma { pub radius_nm: f64, pub ceiling_ft: i32 }
  pub struct DeliveryHours { pub summer_utc: [u8; 2], pub winter_utc: [u8; 2] }
  pub struct IfrDefaults { pub cruise_ft: i32 }
  // Procedures gains: vfr_points: Vec<VfrPoint>, vfr_holds: Vec<String>, vfr_hold_max_ft: Option<i32>,
  //   adjacent: Vec<AdjacentUnit>, tma: Option<Tma>, delivery_hours: Option<DeliveryHours>,
  //   ifr: Option<IfrDefaults>, approach_alt_khz: Vec<u32>
  // PositionDef unchanged; a position with role Approach is the approach unit.
  impl Procedures { pub fn vfr_point(&self, name: &str) -> Option<&VfrPoint>; }   // case-insensitive
  ```
- EPKK values: the 13 VFR points of AIP AD 2.22 §3.1 with their coordinates (BRAVO 50°15'53"N 19°26'05"E … ZULU 50°06'33"N 19°46'46"E; `south = true` for SIERRA and KILO); `vfr_holds = ["KILO", "ZULU"]`, `vfr_hold_max_ft = 2000`; Approach position `Kraków Approach` 121075, `approach_alt_khz = [126530, 135405]`; adjacent: `Warszawa Radar` 127660 Acc `min_level_ft = 9500`, `Kraków Information` 119275 Fis, `Katowice Tower` 129255 Tower `airport = "EPKT"`; `tma = { radius_nm = 40, ceiling_ft = 9500 }`; `delivery_hours = { summer_utc = [6, 22], winter_utc = [5, 21] }`; `ifr = { cruise_ft = 5000 }`. Each value gets its AIP section in `SOURCE.md` (TMA radius: "approximation of the KRAKÓW TMA polygon, ruling").

- [ ] **Step 1: Write the failing tests:** `epkk_procedures_have_vfr_points_and_units` (13 points, `vfr_point("zulu")` coordinates, south set = {KILO, SIERRA}, three adjacent units, approach position 121075); `old_procedures_file_still_parses` (a TOML with only the v1 fields).
- [ ] **Step 2: Run** `cargo test -p atc-navdata --test procedures_toml` — FAIL.
- [ ] **Step 3: Implement** the fields and data.
- [ ] **Step 4: Run** `cargo test --workspace` — PASS (no behaviour change yet).
- [ ] **Step 5: Commit** `feat(navdata): VFR points, adjacent units and IFR data in procedures.toml`.

### Task 4: Phraseology types and rendering

**Files:**
- Modify: `crates/atc-phraseology/src/types.rs`, `render.rs`, `pl/render.rs`, `lib.rs`
- Test: `crates/atc-phraseology/tests/render.rs`, `render_pl.rs`

**Interfaces:**
- Produces (new `AtcItem` variants; existing ones unchanged):
  ```rust
  ClearedTo { destination: String },              // spoken name: "Katowice"
  Sid { fix: String, number: u8, letter: char },  // ("SUDOL", 2, 'J') -> "SUDOL two Juliett departure"
  Star { fix: String, number: u8, letter: char }, // ("MAPIK", 1, 'G') -> "MAPIK one Golf arrival"
  ClimbTo(Level), DescendTo(Level), MaintainLevel(Level),
  Heading { deg: u16, turn: Option<Turn> },        // "turn left heading two two zero"
  DirectTo { fix: String },
  ExpectApproach { kind: ApproachKind, runway: String },
  ClearedApproach { kind: ApproachKind, runway: String },
  ReportEstablished, Identified, RadarServiceTerminated,
  DistanceToGo(u16),                              // "eight miles from touchdown"
  StartUpApproved, PushAndStartApproved { facing: Option<String> },
  ReadbackCorrect,
  EnterControlZoneVia { point: String, alt_ft: u32 }, LeaveControlZoneVia { point: String },
  HoldOver { point: String, max_ft: u32 }, ReportAt { point: String },
  VisualApproachNotAvailable,
  Unable(UnableReason)  // new reasons: UnknownDestination, NoIfrData, Closed
  ```
  `Level` = `Altitude(i32)` / `FlightLevel(u16)`; `Turn` = `Left`/`Right`; `atc_phraseology::ApproachKind { Ils, VorDme, Rnav }` is phraseology's own type (`atc-phraseology` must not depend on `atc-navdata`); core converts between the two with one `fn` in `ifr.rs`. New `ReadbackItem`s: `Sid(String, u8, char)`, `Star(String, u8, char)`, `Level(Level)`, `Direct(String)`, `Approach(ApproachKind, String)`, `Destination(String)`. Hand-offs to adjacent units reuse the existing `Contact { station, khz }`. Items carry the spoken designator (fix name of the SID's last / STAR's first fix, number, letter — Task 6 `spoken_designator`), never the raw CIFP ident.
- English wordings follow ICAO Doc 4444 ch. 12 (spec §2): "cleared to Katowice, SUDOL two Juliett departure, climb altitude five thousand feet, squawk four two zero one"; "identified"; "descend to altitude three thousand feet, QNH one zero one three"; "cleared ILS approach runway two five"; "report established"; "radar service terminated"; "start-up approved"; "readback correct".
- Polish (Delivery/Ground/Tower items only): "zezwolenie do Katowic, odlot SUDOL dwa Juliett, wznoszenie na wysokość pięć tysięcy stóp, squawk cztery dwa zero jeden", "uruchomienie zezwalam", "wypychanie i uruchomienie zezwalam", "powtórzenie poprawne", "wejście w strefę przez ZULU, wysokość dwa tysiące stóp" — mark them `project wording` in `docs/phraseology/pl.md` where plan 3a's reference lacks them. Approach-only items rendered with `lang = Pl` fall back to English (the engine never asks, but render must not panic).

- [ ] **Step 1: Write failing render tests**, one per new item in English (exact strings above and in the same style for the rest), the Delivery clearance as one message, and Polish tests for the Delivery/Ground/Tower items.
- [ ] **Step 2: Run** `cargo test -p atc-phraseology` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p atc-phraseology` — PASS.
- [ ] **Step 5: Commit** `feat(phraseology): IFR and VFR clearance items`.

### Task 5: Pilot parsing: requests, procedure words, VFR points

**Files:**
- Modify: `crates/atc-phraseology/src/parse.rs`, `types.rs`, `pl/normalize.rs`, `lib.rs`
- Test: `crates/atc-phraseology/tests/parse.rs`, `parse_pl.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Vocabulary { pub callsigns: Vec<Callsign>, pub points: Vec<String>, pub fixes: Vec<String>,
                          pub places: Vec<(String, String)> /* (spoken name lowercase ascii, ICAO) */ }
  pub fn parse_pilot_with(text: &str, lang: Lang, vocab: &Vocabulary) -> PilotMessage;
  // parse_pilot_known(text, lang, known) == parse_pilot_with(text, lang, &Vocabulary { callsigns: known, ..Default })
  // new PilotIntent variants:
  RequestClearance { destination: Option<String> /* ICAO */, ifr: bool, level_ft: Option<i32> },
  RequestStartUp { push: bool },
  ReportDeparture,                       // first call to Approach after take-off ("passing 2500 climbing 5000")
  ReportInbound { point: Option<String> },  // first call inbound (IFR or VFR) — "at ZULU", "inbound from Katowice"
  RequestApproach { kind: Option<ApproachKind>, full_procedure: bool },
  ReportEstablished,
  RequestJoin { point: Option<String> }, RequestLeave { point: Option<String> },
  // HeardItems gains: sids: Vec<(String, u8, char)>, stars: Vec<(String, u8, char)>, directs: Vec<String>,
  //   approaches: Vec<(ApproachKind, String)>, destination: Option<String>, points: Vec<String>
  ```
- Rules: a VFR point word ("Zulu", "Kilo", …) is a point only when it is in `vocab.points` and follows `at`/`via`/`over`/`from`/`to`/`overhead` or a request verb; otherwise it stays a letter (taxiway, callsign). A designator is a known fix name followed by a number and an ICAO letter word ("SUDOL two Juliett", "Sudol 2 J", "sudol dwa juliett"), optionally "departure"/"arrival". Destinations: ICAO code spelled or as four letters, or a place name from `vocab.places` ("Katowice", Polish "do Katowic" via the existing inflection tolerance). "IFR" / "I F R" / "instrument" set `ifr`. "request full procedure" / "request ILS|VOR|RNAV approach" set `RequestApproach`.

- [ ] **Step 1: Write failing tests** (English and Polish), including Review Focus 3: `"Krakow Delivery, Sierra Papa Alpha Bravo Charlie, stand five, information Alpha, IFR to Katowice, request clearance"` → `RequestClearance { destination: Some("EPKT"), ifr: true, level_ft: None }`; readbacks `"cleared to Katowice, Sudol two J departure, climb five thousand, squawk four two zero one, SBC"`, `"SUDOL 2 Juliett"`, Polish `"Sudol dwa Juliett"` → `sids == [("SUDOL", 2, 'J')]`; `"Krakow Tower, SBC, at Zulu, two thousand feet, request joining"` → `RequestJoin { point: Some("ZULU") }`; `"taxi via Zulu"` without ZULU in vocabulary stays a taxiway; `"request vectors"` / `"request full procedure"`; `"established ILS runway two five"` → `ReportEstablished`.
- [ ] **Step 2: Run** `cargo test -p atc-phraseology` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p atc-phraseology` and `cargo test --workspace` — PASS (existing parse tests unchanged).
- [ ] **Step 5: Commit** `feat(phraseology): parse IFR requests, procedures and VFR points`.

### Task 6: `ifr` module: procedure geometry, SID/STAR choice, vectors

**Files:**
- Create: `crates/atc-core/src/ifr.rs`
- Modify: `crates/atc-core/src/lib.rs`
- Test: `crates/atc-core/tests/ifr.rs` (fixture `ZZZZ` via `atc_navdata::xplane::load_procedure_set` on the committed fixture root)

**Interfaces:**
- Consumes: Task 2 `ProcedureSet`, `ResolvedProcedure`; Task 3 `Procedures`; `atc_geo` (`distance_m`, bearing helpers in `atc_geo`; add `bearing_deg(a, b)` and `destination(p, bearing, m)` there if missing).
- Produces:
  ```rust
  pub struct Path { pub points: Vec<(String, LatLon)>, pub constraints: Vec<(usize, AltConstraint)> }
  pub fn procedure_path(p: &ResolvedProcedure) -> Path;                  // fixes in order, VM/FA legs dropped
  pub fn cross_track_m(path: &Path, pos: LatLon) -> f64;                 // distance to the nearest segment
  pub fn applicable_constraints(path: &Path, cleared_ft: i32) -> Vec<(usize, AltConstraint)>; // drops those above
  pub fn choose_sid<'a>(set: &'a ProcedureSet, runway: &str, from: LatLon, dest: LatLon) -> Option<&'a ResolvedProcedure>;
      // last fix bearing closest to bearing(from, dest); ties: fewer legs
  pub fn choose_star<'a>(set: &'a ProcedureSet, runway: &str, ac: LatLon, airport: LatLon) -> Option<&'a ResolvedProcedure>;
      // the STAR whose start fix is closest in bearing to the aircraft AND farther from the airport than the aircraft; None otherwise
  pub fn iaf(set: &ProcedureSet, runway: &str, kind: ApproachKind) -> Option<(String, LatLon)>;
  pub struct Vector { pub heading_deg: u16, pub turn: Turn, pub alt_ft: i32 }
  pub fn vector_to_final(ac: LatLon, ac_hdg: f64, thr: LatLon, course_deg: f64, intercept_nm: f64, alt_ft: i32) -> Vector;
      // heading to the 8 NM (intercept_nm) point when > 3 NM from it; then a 30° intercept heading onto the course
  pub fn distance_to_go_nm(ac: LatLon, thr: LatLon) -> u16;
  pub fn spoken_designator(p: &ResolvedProcedure) -> (String, u8, char);  // (fix name, number, letter) e.g. ("SUDOL", 2, 'J')
  ```
- [ ] **Step 1: Write failing tests** on the ZZZZ fixture: `choose_sid` for a destination due north → `ALFA1A`, due south → `BRAV1A`; `choose_star` from beyond `ALFAX` → `ALFA1B`, from inside it → `None`; `applicable_constraints(cleared 5000)` drops the FL080 constraint; `cross_track_m` 0 on a fix and ≈ 1852 one mile abeam; `vector_to_final` from 15 NM abeam the centreline gives a heading towards the 8 NM point, from 5 NM before the point a 30° intercept; `spoken_designator(ALFA1A) == ("ALFAX", 1, 'A')`. Install test: `choose_sid(EPKK set, "25", EPKK, EPKT)` ∈ {SUDO2J, NUMB2J, EMBI2J}.
- [ ] **Step 2: Run** `cargo test -p atc-core --test ifr` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(core): procedure geometry, SID/STAR choice and vectors`.

### Task 7: Delivery, clearances, UTC time, flight plan on the strip

**Files:**
- Create: `crates/atc-core/src/engine/delivery.rs`
- Modify: `crates/atc-core/src/state.rs`, `strip.rs`, `engine/mod.rs`, `engine/ground.rs`, `atis.rs`, `crates/atc-proto/src/*.rs` (TelemetryFrame), `crates/atc-client/src/datarefs.rs`, `crates/atc-server/src/convert.rs`, `crates/atc-replay/src/lib.rs` (pass `None`), `crates/atc-core/src/testkit.rs`
- Test: `crates/atc-core/tests/engine.rs` (new `mod delivery`), `crates/atc-proto` serde test, `crates/atc-core/tests/xplane_install.rs`

**Interfaces:**
- Consumes: Tasks 2–6.
- Produces:
  ```rust
  // state.rs
  pub struct AircraftState { …, pub utc_s: Option<f64>, pub day_of_year: Option<u16> }
  // proto TelemetryFrame: #[serde(default)] pub utc_s: Option<f64>, #[serde(default)] pub day_of_year: Option<u16>
  //   (client reads sim/time/zulu_time_sec and sim/time/local_date_days; day 0 = 1 Jan)
  // strip.rs
  pub enum Role { Ground, Tower, Delivery, Approach }
  pub struct FlightPlan { pub ifr: bool, pub destination: String /* ICAO */, pub dest_name: String,
                          pub cruise_ft: i32, pub sid: Option<String>, pub star: Option<String>,
                          pub approach: Option<(ApproachKind, String)>, pub vectors: bool, pub cleared_ft: i32 }
  // ApproachKind here is atc_phraseology::ApproachKind. No EOBT exists in this model, so the AIP
  // "not earlier than 30 min before EOBT" rule is not checked (ruling).
  // Strip gains: plan: Option<FlightPlan>, handed_off: Option<String> /* adjacent unit callsign */
  // engine
  impl AtcEngine {
      pub fn new(airport: Airport, procs: Procedures, cfg: EngineConfig) -> Self;          // unchanged: no IFR data
      pub fn with_procedure_set(self, set: ProcedureSet) -> Self;                          // enables IFR
      pub fn with_destinations(self, d: Vec<Destination>) -> Self;  // atc_navdata::model::Destination (Task 2)
      pub fn delivery_open(&self, utc_s: Option<f64>, day_of_year: Option<u16>) -> bool;  // None -> true (Review Focus 4)
  }
  ```
- Behaviour:
  - Destinations: the engine knows only the `Destination`s it was given; the server builds them once at start (Task 12) from the airports within 100 NM in the X-Plane apt.dat (header row name, title-cased; a city name from `procedures.toml` adjacent units wins, e.g. "Katowice" for EPKT), and the tests and replay pass them explicitly. `Vocabulary::places` is built from them. An ICAO or name not among them → Review Focus 1.
  - `RequestClearance` on Delivery (or on Ground when Delivery is closed; on Ground while Delivery is open → `Contact` Delivery, as today's redirect): unknown/missing destination → `Unable(UnknownDestination)` + `SayAgain`-style "say again destination" (Review Focus 1); no procedure set → `Unable(NoIfrData)` for `ifr: true`; otherwise `[ClearedTo, Sid, ClimbTo(min(first SID constraint at/below cruise, cruise)), Squawk]` with readback required for destination, SID, level and squawk; "readback correct" + `Contact` Ground after a correct readback. A VFR flight with a destination gets `[ClearedTo, LeaveControlZoneVia point-in-that-direction, Squawk]` (VFR exit points from Task 3).
  - Summer time: day of year 86..=300 (last-Sunday rule approximated; ruling, documented on the function).
  - ATIS: when the procedure set exists, the ATIS text adds "for clearance contact Delivery one two one decimal niner eight" while Delivery is open, otherwise "for start-up contact Ground …" (AIP 2.20 wording).
  - Ground: `RequestStartUp { push }` → `StartUpApproved` / `PushAndStartApproved { facing: None }` + QNH, before taxi; an IFR aircraft without a clearance asking for taxi is told to contact Delivery (or gets the clearance first when Delivery is closed).
  - Delivery speaks in the session language (Polish wordings from Task 4).
- [ ] **Step 1: Write failing engine tests** (fixture ZZZZ procedure set + a `procedures.toml` for ZZZZ in `crates/atc-core/tests/fixtures/`): clearance exchange with readback; wrong SID in readback → correction; unknown destination; no procedure set → unable IFR; Delivery closed at 23:00 UTC → Ground answers the clearance request; `utc_s: None` → Delivery open; start-up approved; Polish clearance.
- [ ] **Step 2: Run** `cargo test -p atc-core` — FAIL.
- [ ] **Step 3: Implement** (client datarefs and proto included; `cargo test -p atc-client -p atc-proto`).
- [ ] **Step 4: Run** `cargo test --workspace` — PASS; existing goldens unchanged.
- [ ] **Step 5: Commit** `feat(core): Delivery clearances, start-up and sim UTC time`.

### Task 8: Tower calls the aircraft after the Ground→Tower transfer (AIP 2.20)

**Files:**
- Modify: `crates/atc-core/src/engine/{mod,tower}.rs`, all affected `scenarios/*.toml` and `.expected.txt`, `docs/user-guide/ru/*.md` where they show the old exchange
- Test: `crates/atc-core/tests/engine.rs`

**Behaviour:** AIP AD 2.20 §2.2: after the transfer from Ground the pilot omits the initial call. When an aircraft whose handover to Tower completed is `HoldingShort` with COM1 on the Tower frequency, Tower calls it with the departure (line-up or take-off clearance, as `departure()` decides). A pilot who calls anyway ("ready for departure") is answered as today, never twice: if Tower has already transmitted the departure, the call gets `ReadBack(pending)` only when a readback is open, otherwise nothing.
- [ ] **Step 1: Failing test** `tower_calls_aircraft_at_holding_point_after_transfer` (no pilot call; output on the Tower frequency: "SP-ABC, Kraków Tower, runway two five, cleared for take-off, wind …"); `pilot_call_after_tower_called_is_not_answered_twice`.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.**
- [ ] **Step 4: Update the scenarios** whose pilot line "holding point runway two five, ready for departure" now meets an already-issued clearance: drop that `[[say]]` and regenerate `.expected.txt` (`cargo run -p atc-replay -- scenarios/<name>.toml > scenarios/<name>.expected.txt`); diff each against the old file and check that only the departure exchange changed.
- [ ] **Step 5: Run** `cargo test --workspace` — PASS.
- [ ] **Step 6: Commit** `feat(core): Tower calls the aircraft after the Ground transfer (AIP AD 2.20 §2.2)` with the list of changed scenarios in the body.

### Task 9: Approach — departures

**Files:**
- Create: `crates/atc-core/src/engine/approach.rs`
- Modify: `crates/atc-core/src/phase.rs`, `compliance.rs`, `engine/{mod,tower}.rs`
- Test: `crates/atc-core/tests/engine.rs` (`mod approach_departure`), `crates/atc-core/tests/phase.rs` if present else in `engine.rs`

**Interfaces:**
- Produces: `FlightPhase::Airborne` (airborne and not flying the circuit: an aircraft with an IFR plan, or VFR leaving/joining via points; `PhaseTracker::set_circuit(&mut self, on: bool)` — circuit legs are only produced when on; `Circuit(Final)` is still produced on the extended centreline within 10 NM, `|right| < 0.5 NM`, `|d| < 30°`, so Tower's landing logic works for arrivals).
  `Deviation::OffTrack { procedure: String, metres: u32 }` (> 1 NM from the cleared SID/STAR path for 20 s), `Deviation::HeadingDeviation { assigned: u16, actual: u16 }` (> 20° for 30 s), `AltitudeDeviation` reused with the cleared level; each reported once per instruction (Review Focus 5).
- Behaviour:
  - Tower, on the IFR aircraft becoming `Airborne` after take-off: `Contact { station: Approach callsign, khz }` (AIP 2.22 §1). VFR stays on Tower.
  - Approach, first call (`ReportDeparture`): `Identified` (+ `ClimbTo(cleared)` if not yet cleared that high). After the SID's first turn (the aircraft passed the SID's second fix) and while climbing: `DirectTo { fix: destination name or the SID's last fix if the destination is beyond the TMA }`.
  - Hand-off: destination with an adjacent `Tower` unit for that airport → `Contact` that tower at 10 NM from the destination; otherwise at `tma.radius_nm` from the airport: `Contact` Warszawa Radar when cleared at or above its `min_level_ft`, else `RadarServiceTerminated` + `Contact` the FIS unit. The strip's `handed_off` is set; the adjacent unit answers the first call on its frequency once with `Identified` (its own station name) and nothing after that.
  - Approach and adjacent units always render in English (`transmit_in(..., Lang::En)`).
- [ ] **Step 1: Failing tests** with a synthetic path track (Task 13's `path_track` is not there yet: build the samples inline with a small helper in the test): IFR departure on ZZZZ RWY 27 via ALFA1A to a destination 30 NM north with an adjacent tower → Tower hands to Approach after lift-off; Approach identifies; direct after the turn; hand-off at 10 NM; a destination 80 NM north at 5000 ft → `RadarServiceTerminated` + FIS at 40 NM; 1.5 NM off the SID for 25 s → one `OffTrack` call; a VFR departure stays with Tower.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `feat(core): Approach handles IFR departures and hand-offs`.

### Task 10: Approach — arrivals, vectors and approach clearances

**Files:**
- Modify: `crates/atc-core/src/engine/{approach,mod,tower}.rs`, `strip.rs`
- Test: `crates/atc-core/tests/engine.rs` (`mod approach_arrival`)

**Behaviour:**
- Registration: an aircraft first heard on the Approach frequency while airborne (`ReportInbound`) gets an IFR plan with this airport as destination (`ifr` as said; a VFR inbound call on Approach is told to contact Tower with the VFR point).
- First reply: `Identified`, then: STAR from `choose_star` → `[Star, DescendTo(min(cleared, 5000-ft-class default from IfrDefaults)), Qnh, ExpectApproach]`; no STAR (inside the start fixes) → `[DirectTo(IAF) or Vector, DescendTo, Qnh, ExpectApproach]` (`DirectTo` when the pilot asked for the full procedure, else vectors).
- Vectors (default): a new `Heading`/`DescendTo` instruction whenever `vector_to_final` changes the heading by ≥ 20° or the aircraft is within 2 NM of the intercept point; `DistanceToGo` with the intercept heading; then `ClearedApproach` + `ReportEstablished`.
- Full procedure: `ClearedApproach` when the aircraft reaches the IAF.
- `ReportEstablished` (or `Circuit(Final)` inside the FAF): `Contact` Tower; Tower answers the first call with the landing clearance flow it uses today.
- Go-around / missed approach on Tower: Tower says `GoAround` as today, then hands an IFR aircraft to Approach (`Contact` Approach); Approach vectors it again.
- Approach type: `RequestApproach { kind }` on Approach changes it if the CIFP has it for the runway; otherwise `Unable`.
- Night (UTC 21:00–05:00 summer / 20:00–04:00 winter) visual approaches are not offered (`VisualApproachNotAvailable` on request).
- Squawk 7600 (AIP 2.22 §2.1): Approach stops issuing instructions to the aircraft and logs it; no deviation calls while 7600 is set (full radio-failure handling is out of scope, spec §4).
- [ ] **Step 1: Failing tests:** ZZZZ inbound from beyond `ALFAX` → `ALFA1B` arrival, expect ILS 27; inbound from inside → vectors with the first heading towards the 8 NM point, intercept heading, `ClearedApproach`, transfer to Tower on "established"; "request full procedure" → direct IAF, approach clearance at the IAF; missed approach → back to Approach; heading ignored 35 s → one `HeadingDeviation` (Review Focus 5); squawk 7600 on vectors → no further Approach transmissions to it.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `feat(core): Approach handles arrivals, vectors and approach clearances`.

### Task 11: VFR entry and exit via points

**Files:**
- Create: `crates/atc-core/src/engine/vfr.rs`
- Modify: `crates/atc-core/src/engine/{mod,tower}.rs`
- Test: `crates/atc-core/tests/engine.rs` (`mod vfr_points`)

**Behaviour (AIP 2.22 §3):** `RequestJoin { point }` on Tower from an airborne aircraft outside the circuit → `[EnterControlZoneVia { point, alt_ft: vfr_hold_max_ft }, ReportAt { point }]` then, on "at <point>", joining instructions into the circuit ("join downwind runway 25", existing circuit logic takes over, `set_circuit(true)`); a southern entry via a point that is not `south` → `Unable` + "enter via SIERRA or KILO" (nearest south point); busy runway (another aircraft cleared to land or on final) → `HoldOver { point: nearest of vfr_holds, max_ft }`. Departure with a VFR destination (cleared by Delivery/Tower in Task 7) → after take-off `ReportAt { point }`, at the point `LeaveControlZoneVia` acknowledgement and "frequency change approved" (`Contact` FIS).
- [ ] **Step 1: Failing tests:** entry via ZULU-like fixture point; southern entry via a northern point refused; hold over a hold point while another aircraft is on final; VFR exit via a point.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `feat(core): VFR entry and exit via the published points`.

### Task 12: Server, client and voices

**Files:**
- Modify: `crates/atc-server/src/{app,layout,main,config}.rs`, `crates/atc-voice/src/lib.rs` (`VoiceId::{Delivery, Approach, Radar}`), `crates/atc-voice/src/voices_config.rs`, `config/voices.toml`, `tools/fetch-piper.sh`, `tools/MODELS.md`, `crates/atc-xplane-plugin/src/*` (only if the overlay needs the new station names)
- Test: `crates/atc-server/tests/{layout,ws}.rs`

**Behaviour:**
- `layout.rs`: destinations from `atc_navdata::xplane::load_destinations` (empty without `xplane_root`); `load_procedure_set(xplane_root, icao) -> Option<ProcedureSet>`: `None` with one `warn!` when `xplane_root` is unset or the CIFP is missing (Review Focus 2); the engine is built `with_procedure_set` only when `Some`. Log the cycle and the procedure counts.
- `StationMap`: Delivery → `VoiceId::Delivery`, Approach (and `approach_alt_khz` as the same station) → `VoiceId::Approach`, each adjacent unit → `VoiceId::Radar`. Missing voice entries fall back to the Ground voice with one warning (as Polish voices do).
- Voices (Piper, CC0/CC-BY as in MODELS.md, fetched by `tools/fetch-piper.sh`): `[delivery]` `en_US-lessac-medium`, `[approach]` `en_GB-northern_english_male-medium`, `[radar]` `en_US-joe-medium`, `[pl.delivery]` = `pl_PL-darkman-medium` (same as Polish Ground — only three medium Polish voices exist). Record licences in MODELS.md.
- [ ] **Step 1: Failing tests:** `StationMap` staffs 121980/121075/126530/135405/127660/119275/129255 with the voices above; layout without CIFP returns `None` and the server answers an IFR clearance request with "unable IFR" (ws test with the fake STT).
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement**, run `tools/fetch-piper.sh` locally to fetch the three voices (not committed).
- [ ] **Step 4: Run** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` — PASS.
- [ ] **Step 5: Commit** `feat(server): staff Delivery, Approach and adjacent units with their own voices`.

### Task 13: Replay path tracks and new golden scenarios

**Files:**
- Modify: `crates/atc-core/src/testkit.rs`, `crates/atc-replay/src/lib.rs`, `crates/atc-replay/tests/golden.rs`
- Create: `scenarios/epkk-ifr-departure-25.toml`, `epkk-ifr-departure-07.toml`, `epkk-ifr-arrival-star-ils25.toml`, `epkk-ifr-arrival-vectors.toml`, `epkk-ifr-vor07-missed.toml`, `epkk-vfr-zulu-in-sierra-out.toml`, `epkk-ifr-clearance-night.toml` (+ `.expected.txt` each)

**Interfaces:**
- `testkit::path_track(airport: &Airport, procs: &Procedures, set: Option<&ProcedureSet>, spec: &PathSpec, dt_s: f64) -> Vec<AircraftState>` with
  `pub struct PathSpec { pub start: PathStart /* Stand(name) | Air { pos: LatLon, alt_ft, hdg } */, pub runway: Option<String> /* departure runway: taxi, line up, take off */, pub waypoints: Vec<Waypoint>, pub land: Option<String> /* runway to land on, then taxi to a stand */ }`, `pub struct Waypoint { pub at: WaypointPos /* Fix(String) | Point(String) | LatLon */, pub alt_ft: f64, pub kt: f64 }`. Straight segments with 3°/s turns, constant climb/descent between waypoints.
- Scenario TOML: `track = "path"` with `[path]` (`start`, `runway`, `waypoints = [{ fix = "KK599", alt = 3000, kt = 90 }, …]`, `land`); `navdata = "xplane"` marks scenarios that need the install (`ATC_XPLANE_ROOT`); `freq` accepts `delivery`, `approach`, `radar`, `information`; `at` accepts `utc:HH:MM` (sets `utc_s` on the track from the start time `utc = "10:00"`, `day = 200`).
- Scenarios with `navdata = "xplane"` get the procedure set and the destinations from the same `atc-navdata` functions the server uses. `golden.rs`: scenarios with `navdata = "xplane"` run only when the install is found (count printed), the others as today; the total count assertion becomes "8 without navdata + 7 with".
- [ ] **Step 1: Write the scenarios** from spec §7 (each `[[say]]` line in ICAO phraseology; IFR departure EPKK→EPKT via SUDO2J/SUDO1F; arrival from the north-west on MAPI1G with ILS 25; arrival from EPKT on vectors; VOR/DME 07 with missed approach and re-vectoring; VFR entry via ZULU and exit via SIERRA; clearance request at 23:00 UTC answered by Ground).
- [ ] **Step 2: Run** `cargo run -p atc-replay -- scenarios/<name>.toml` for each and read the transcript against spec §3–§4 by hand (every hand-off, frequency and wording); fix the engine where it is wrong (separate commits with tests), then write the `.expected.txt`.
- [ ] **Step 3: Run** `cargo test --workspace` — PASS (with and without `ATC_XPLANE_ROOT`).
- [ ] **Step 4: Commit** `test(replay): IFR and VFR-point golden scenarios`.

### Task 14: Documentation

**Files:**
- Create: `docs/user-guide/ru/ifr-and-vfr-points.md`
- Modify: `docs/phraseology/pl.md`, `docs/phraseology/en.md` (create if missing), `docs/RUNNING.md` (Russian section until the publication translation), `data/airports/EPKK/SOURCE.md` (CIFP cycle and CIFP-vs-AIP name differences)

- [ ] **Step 1:** Russian user guide for the three acceptance flights on the C172 G1000 (spec §7): EPKK→EPKT IFR (G1000: load SUDOL 2J in PROC, what to say to Delivery, Ground, Tower, Approach; when Katowice Tower takes over), EPKT→EPKK IFR (start airborne or depart EPKT, first call to Kraków Approach, vectors to ILS 25), VFR entry via ZULU. Each pilot line quoted from the golden scenarios.
- [ ] **Step 2:** English phraseology reference of every new item with its ICAO source; Polish additions marked `project wording`.
- [ ] **Step 3:** `SOURCE.md`: CIFP cycle 1802, procedure list, the AIP vs CIFP name differences (AIRAC 10-26 charts vs cycle 1802), the TMA radius ruling.
- [ ] **Step 4: Commit** `docs: IFR and VFR-point user guide and phraseology`.
