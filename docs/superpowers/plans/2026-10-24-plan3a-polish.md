# Polish Language (Ground, Tower, ATIS) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pilot can fly the EPKK circuit entirely in Polish (Ground, Tower, ATIS), or switch between Polish and English mid-session, and the controller understands, answers and speaks in the pilot's language.

**Architecture:** Polish is added at the edges only. The parser rewrites a Polish transcript into the canonical token stream the existing English scanner already understands, so intents and clearance items stay language-neutral. The engine keeps a language per aircraft and stamps it on every `AtcMessage`; the renderer dispatches on `AtcMessage::lang`. To make that possible, every free-text English string the engine emits today becomes a typed `AtcItem` (English output byte-identical, proven by the unchanged goldens).

**Tech Stack:** Rust workspace (atc-phraseology, atc-core, atc-voice, atc-server, atc-replay, atc-xplane-plugin), whisper.cpp via whisper-rs (language auto-detect), Piper TTS (Polish voices `pl_PL-*`).

**Spec:** `docs/superpowers/specs/2026-10-19-virtual-atc-design.md` (section "Язык": default English ICAO; Polish for standard Ground/Tower exchanges and ATIS; switch when the pilot speaks Polish, until session end or the pilot returns to English; a setting fixes the language; one Piper voice with different processing if not enough Polish voices).

**Phraseology source:** ICAO Doc 9432 4th ed., Polish edition by ULC/PAŻP, "Podręcznik radiotelefonicznej frazeologii lotniczej" (bilingual PL/EN), http://dlapilota.pl/files/upld/icao_doc_fl_1110.pdf, with its Appendix 1 (Polish differences, http://dlapilota.pl/files/upld/appen1_fl_1110_en_pl.pdf). Section numbers below refer to it. Wording the manual does not contain is marked *project wording* in `docs/phraseology/pl.md`.

## Global Constraints

- Supported languages: `Lang::En`, `Lang::Pl` only. Any other detected language never changes the session language.
- English output must stay byte-identical: all existing `scenarios/*.expected.txt` unchanged by Tasks 2, 3, 6, 7 (Task 9 only adds files).
- Polish digits (2.4.1): zero, jeden, dwa, trzy, cztery, pięć, sześć, siedem, osiem, dziewięć; decimal point = `kropka`.
- Polish numbers per 2.4.2/2.4.3 and Appendix 1: runway, heading, taxiway digits, squawk and callsign digits digit by digit; wind direction and speed as cardinals ("wiatr dwieście stopni dwadzieścia pięć węzłów"); QNH split into two 2-digit cardinals ("QNH dziesięć dziesięć" = 1010, "dziesięć zero osiem" = 1008), a 3-digit QNH digit by digit ("dziewięć dziewięć osiem"), whole thousands as cardinals ("QNH tysiąc", "skłok siedem tysięcy"); altitudes, visibility and cloud bases as cardinals ("tysiąc osiemset"); frequencies: the part before the point as a cardinal, the digits after it one by one, 8.33 channels with all three ("sto dwadzieścia trzy kropka dwa pięć pięć"; 118.100 → "sto osiemnaście kropka jeden").
- Polish nouns after numbers agree in number: 1 → singular, 2–4 (not 12–14) → few-form, else many-form: węzeł/węzły/węzłów, metr/metry/metrów, stopa/stopy/stóp; "stopni" for wind direction.
- ICAO alphabet in Polish: as English except `A` = "Alfa" (Doc 9432 PL 2.3).
- Station names in Polish: "Tower" → "Wieża", "Ground" stays "Ground" (manual uses "GEORGETOWN GROUND" in Polish), ATIS station text "Kraków".
- Config: `[whisper] language` = `"en"` | `"pl"` fixes the language; omitted = auto (detect, switch per Task 6).
- No Russian text outside `docs/user-guide/ru/`. New docs are English.
- Every commit message ends with `Co-Authored-By: Claude <noreply@anthropic.com>`.
- Verification for every task: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` green.

## Review Focus

1. Whisper writes Polish numbers as digits with a decimal comma or spaces ("123,255", "1013", "2 5", "4201") → parsed exactly like the words. Test in Task 5.
2. A short phrase misdetected as the other language ("roger", "Sierra Bravo Charlie downwind" tagged `pl`) → the session language does not flip unless the transmission was understood in the detected language. Test in Task 6.
3. Whisper detects a third language (e.g. `ru` for an accented pilot) → the audio is transcribed again with the session language forced. Test in Task 7.
4. Polish diacritics in the X-Plane overlay (XPLM fonts are ASCII) → shown folded to ASCII ("Wieza", "lacznosc"), never garbage or a crash. Test in Task 8.
5. A Polish transmission mixing English ICAO terms ("Kraków Ground, SP-ABC, holding point pas dwa pięć, ready for departure") → understood. Test in Task 5.

---

### Task 1: Polish number words and the phraseology reference

**Files:**
- Create: `crates/atc-phraseology/src/pl/mod.rs`, `crates/atc-phraseology/src/pl/numbers.rs`
- Modify: `crates/atc-phraseology/src/lib.rs` (add `pub mod pl;`)
- Create: `docs/phraseology/pl.md` (English document: the controller and pilot phrase tables of Tasks 4 and 5 with Doc 9432 PL section numbers or *project wording*, the number rules above, the source links)
- Test: `crates/atc-phraseology/tests/pl_numbers.rs`

**Interfaces:**
- Produces (module `atc_phraseology::pl::numbers`):
  - `pub fn digit(d: u32) -> &'static str`
  - `pub fn digits(n: u32, width: usize) -> String` — zero-padded, digit by digit
  - `pub fn cardinal(n: u32) -> String` — 0..=999_999, nominative masculine
  - `pub fn plural(n: u32, one: &'static str, few: &'static str, many: &'static str) -> &'static str`
  - `pub fn qnh(hpa: u16) -> String`
  - `pub fn squawk(code: u16) -> String`
  - `pub fn frequency(khz: u32) -> String`
  - `pub fn wind(dir_deg: u16, speed_kt: u16) -> String`
  - `pub fn altitude_ft(ft: u32) -> String`

- [ ] **Step 1: Write the failing tests**

```rust
use atc_phraseology::pl::numbers::*;

#[test]
fn cardinals() {
    assert_eq!(cardinal(0), "zero");
    assert_eq!(cardinal(13), "trzynaście");
    assert_eq!(cardinal(25), "dwadzieścia pięć");
    assert_eq!(cardinal(118), "sto osiemnaście");
    assert_eq!(cardinal(250), "dwieście pięćdziesiąt");
    assert_eq!(cardinal(1800), "tysiąc osiemset");
    assert_eq!(cardinal(3400), "trzy tysiące czterysta");
    assert_eq!(cardinal(12_000), "dwanaście tysięcy");
}

#[test]
fn plural_forms() {
    assert_eq!(plural(1, "węzeł", "węzły", "węzłów"), "węzeł");
    assert_eq!(plural(3, "węzeł", "węzły", "węzłów"), "węzły");
    assert_eq!(plural(12, "węzeł", "węzły", "węzłów"), "węzłów");
    assert_eq!(plural(22, "węzeł", "węzły", "węzłów"), "węzły");
    assert_eq!(plural(25, "węzeł", "węzły", "węzłów"), "węzłów");
}

#[test]
fn doc_9432_pl_examples() {
    assert_eq!(qnh(1010), "dziesięć dziesięć");
    assert_eq!(qnh(1008), "dziesięć zero osiem");
    assert_eq!(qnh(1013), "dziesięć trzynaście");
    assert_eq!(qnh(998), "dziewięć dziewięć osiem");
    assert_eq!(qnh(1000), "tysiąc");
    assert_eq!(squawk(2400), "dwa cztery zero zero");
    assert_eq!(squawk(4203), "cztery dwa zero trzy");
    assert_eq!(squawk(7000), "siedem tysięcy");
    assert_eq!(frequency(118_100), "sto osiemnaście kropka jeden");
    assert_eq!(frequency(118_025), "sto osiemnaście kropka zero dwa pięć");
    assert_eq!(frequency(118_105), "sto osiemnaście kropka jeden zero pięć");
    assert_eq!(frequency(123_255), "sto dwadzieścia trzy kropka dwa pięć pięć");
    assert_eq!(wind(200, 25), "dwieście stopni dwadzieścia pięć węzłów");
    assert_eq!(wind(160, 18), "sto sześćdziesiąt stopni osiemnaście węzłów");
    assert_eq!(wind(250, 1), "dwieście pięćdziesiąt stopni jeden węzeł");
    assert_eq!(altitude_ft(1800), "tysiąc osiemset stóp");
    assert_eq!(digits(25, 2), "dwa pięć");
    assert_eq!(digits(7, 2), "zero siedem");
}
```

- [ ] **Step 2: Run to verify they fail** — `cargo test -p atc-phraseology --test pl_numbers` → FAIL (module `pl` not found).
- [ ] **Step 3: Implement `numbers.rs`** with the signatures above. `frequency`: integer MHz part via `cardinal`; kHz part as 3 digits with trailing zeros dropped, keeping at least one digit (118.100 → "jeden", 118.025 → "zero dwa pięć", 123.255 → "dwa pięć pięć"); this matches the English renderer's existing trailing-zero rule. `qnh`: `< 1000` → `digits(hpa, 3)`; a multiple of 1000 → `cardinal`; otherwise `cardinal(hpa / 100)` + " " + (`hpa % 100 < 10` ? "zero " + digit : `cardinal(hpa % 100)`). `squawk`: a multiple of 1000 → `cardinal(code / 1000)` + " " + `plural(code/1000, "tysiąc", "tysiące", "tysięcy")` (7 → "siedem tysięcy"), otherwise `digits(code, 4)`.
- [ ] **Step 4: Run to verify they pass** — same command → PASS.
- [ ] **Step 5: Write `docs/phraseology/pl.md`** (tables from Tasks 4 and 5, number rules, sources).
- [ ] **Step 6: Commit** — `git commit -m "feat(phraseology): Polish number words per Doc 9432 PL; Polish phraseology reference"`

---

### Task 2: Typed ATC items instead of free text

Today the engine builds English strings with `AtcItem::Free` ("say again", "follow-me car is on the way", the stand position, incursion and "without clearance" warnings, greetings and farewells, "standby", "say intentions", "say stand", "pass your message"), puts plain-English targets in `Taxi::to` ("holding point runway 25", "stand 5"), and English reasons in `Unable(String)`. Polish rendering needs these typed.

**Files:**
- Modify: `crates/atc-phraseology/src/types.rs`, `crates/atc-phraseology/src/render.rs`, `crates/atc-phraseology/src/lib.rs`
- Modify: every `AtcItem::Free` / `free(...)` / `Taxi { to: ... }` / `Unable(..)` construction in `crates/atc-core/src/` (`grep -rn "Free(\|free(\|Unable(\|to: format" crates/atc-core/src` lists them; today 23 `Free` sites in `engine/{mod,ground,tower,stand,follow_me}.rs`, `atis.rs`)
- Test: `crates/atc-phraseology/tests/render.rs`, existing goldens

**Interfaces:**
- Produces (in `atc_phraseology::types`, re-exported from the crate root):
  - `pub enum TaxiTo { HoldingPoint { runway: String }, Stand { name: String } }`; `AtcItem::Taxi { to: TaxiTo, via: Vec<String>, crossings: Vec<String> }`
  - `pub enum Side { Ahead, Right, Behind, Left }`
  - `pub enum Courtesy { Hello, GoodMorning, GoodAfternoon, GoodEvening, GoodDay, HaveANiceFlight, ThankYouYouToo }` (every greeting/farewell/wish the engine emits today; add a variant per distinct English string found)
  - `pub enum UnableReason { NoRoute, … }` (one variant per distinct reason string the engine builds today)
  - New `AtcItem` variants: `SayAgain`, `SayIntentions`, `SayStand`, `PassYourMessage`, `Standby`, `FollowMeOnTheWay`, `FollowTheFollowMe`, `StandPosition { stand: String, metres: u32, side: Side }`, `TakenOffWithoutClearance`, `LandedWithoutClearance`, `Courtesy(Courtesy)`, plus one variant per remaining `Free(format!(..))` site (e.g. the runway incursion and altitude-check texts) carrying its values as fields, and `Unable(UnableReason)`
  - `AtcItem::Free` is removed.

- [ ] **Step 1: Write the failing test** `typed_items_render_as_before` in `crates/atc-phraseology/tests/render.rs`: one `AtcMessage` per new variant, asserting the exact English string each `Free` site produced before (copy each from the engine code, e.g. `StandPosition { stand: "20".into(), metres: 120, side: Side::Left }` → `"stand two zero is one hundred twenty metres on your left"`, `FollowMeOnTheWay` → `"follow-me car is on the way"`, `Taxi { to: TaxiTo::HoldingPoint { runway: "25".into() }, via: vec!["B5".into(), "B6".into(), "A".into()], crossings: vec![] }` → `"taxi to holding point runway two five via Bravo five, Bravo six, Alpha"`).
- [ ] **Step 2: Run to verify it fails** — `cargo test -p atc-phraseology --test render` → FAIL (variants missing).
- [ ] **Step 3: Add the types and English rendering**; remove `AtcItem::Free`; fix every engine site to build the typed item.
- [ ] **Step 4: Run the whole workspace** — `cargo test --workspace` → PASS with **no** change to any `scenarios/*.expected.txt` (`git status scenarios/` clean).
- [ ] **Step 5: Commit** — `git commit -m "refactor: typed ATC items instead of free English text"`

---

### Task 3: Courtesy and acknowledgement words come from the parser

The engine scans raw English text for greetings, farewells, wishes and acknowledgements (`greeting`, `farewell` and the acknowledgement word list in `crates/atc-core/src/engine/mod.rs:862-1016`). Move the detection into the parser so a Polish parser can fill the same fields.

**Files:**
- Modify: `crates/atc-phraseology/src/types.rs` (`PilotMessage`), `crates/atc-phraseology/src/parse.rs`
- Modify: `crates/atc-core/src/engine/mod.rs`, `crates/atc-core/src/engine/stand.rs` (use the parsed fields; delete the text scanners)
- Test: `crates/atc-phraseology/tests/parse.rs`

**Interfaces:**
- Produces: `PilotMessage` gains `pub greeting: Option<Courtesy>` (opening greeting before the callsign, as today's rule), `pub farewell: Option<Courtesy>` (the reply the engine owes: `GoodDay` or `ThankYouYouToo`, as `farewell()` decides today) and `pub ack: bool` (roger, wilco, copy, understand, understood).
- Consumes: `Courtesy` from Task 2.

- [ ] **Step 1: Write the failing tests** `courtesy_fields`: `"Kraków Ground, good morning, Sierra Bravo Charlie, request taxi"` → `greeting == Some(Courtesy::GoodMorning)`; `"…Sierra Bravo Charlie, have a nice day"` → `farewell == Some(Courtesy::ThankYouYouToo)`; `"…Sierra Bravo Charlie, good day"` → `farewell == Some(Courtesy::GoodDay)`; `"Sierra Papa Alpha Bravo Charlie, understand"` → `ack`.
- [ ] **Step 2: Run** `cargo test -p atc-phraseology --test parse courtesy_fields` → FAIL.
- [ ] **Step 3: Implement** the fields in `parse.rs` (port the engine's rules unchanged); the engine reads them.
- [ ] **Step 4: Run** `cargo test --workspace` → PASS, goldens unchanged.
- [ ] **Step 5: Commit** — `git commit -m "refactor: greetings, farewells and acknowledgements are parsed"`

---

### Task 4: Polish rendering

**Files:**
- Create: `crates/atc-phraseology/src/pl/render.rs`
- Modify: `crates/atc-phraseology/src/render.rs` (`render` dispatches on `msg.lang`; the current body becomes the English path)
- Test: `crates/atc-phraseology/tests/render_pl.rs`

**Interfaces:**
- Consumes: Task 1 numbers, Task 2 items.
- Produces: `pub fn render(msg: &AtcMessage, first_contact: bool) -> String` (unchanged signature, now language-aware); `pub fn station_name(station: &str, lang: Lang) -> String` ("Kraków Tower" + `Pl` → "Kraków Wieża").

Exact Polish wording (callsign first, items joined with ", " as in English; `R` = runway digits, e.g. "dwa pięć"):

| Item | Polish | Source |
|---|---|---|
| `Taxi` to holding point | `kołuj do punktu oczekiwania przed pasem R drogą Bravo pięć, Bravo sześć, Alfa` | 4.4 |
| `Taxi` to stand | `kołuj do stanowiska trzy pięć drogą Charlie, Bravo cztery` | 4.9 |
| `Taxi` crossings | `, przetnij pas R` | 4.4 |
| `HoldShort` | `oczekuj przed pasem R` | 4.4 |
| `CrossRunway` | `przetnij pas R` | 4.4 |
| `LineUpAndWait` | `pas R, zajmij pas i czekaj` | 4.5 |
| `ClearedTakeoff` | `wiatr <wind>, pas R, zezwalam startować`; calm: `wiatr cisza` | 4.5 / project wording (calm) |
| `ClearedLand` | `wiatr <wind>, pas R, zezwalam lądować` | 4.7 |
| `ClearedTouchAndGo` | `wiatr <wind>, pas R, zezwalam konwojer` | 4.7 |
| `Report(leg)` | `zgłoś ` + Upwind `pozycję pod wiatr`, Crosswind `pozycję boczną`, Downwind `z wiatrem`, Base `bejs`, Final `prostą` | 4.6 (z wiatrem, prosta, "BASE wymawiane jako bejs"); project wording (upwind, crosswind) |
| `Sequence(n)` | `numer <cardinal>` | 4.6 |
| `Qnh` | `QNH <qnh>` | 2.4 |
| `Squawk` / `CheckSquawk` / `SquawkCharlie` | `skłok <code>` / `sprawdź skłok <code>` / `skłok Charlie` | 2.4 / project wording |
| `Contact` | `łączność z Kraków Wieża sto dwadzieścia trzy kropka dwa pięć pięć` | 4.5, 4.9 |
| `HoldPosition` | `stój` | 4.5 |
| `GoAround` | `przejdź na drugi krąg` | 4.8 |
| `Negative` | `nieprawidłowo` | Appendix 1 |
| `ReadBack(items)` | `powtórz ` + items joined by " i ": Runway `pas`, HoldShort `instrukcję oczekiwania przed pasem`, Route `drogę kołowania`, Kind `zezwolenie`, Altitude `wysokość`, Heading `kurs`, Frequency `częstotliwość`, Qnh `QNH`, Squawk `skłok` | 2.6 (POWTÓRZ) |
| `SayAgainCallsign` | `wołający Kraków Wieża, powtórz znak wywoławczy` (no callsign prefix) | 2.8.1.5 |
| `SayAgain` / `Roger` / `Standby` | `powtórz` / `przyjąłem` / `czekaj` | 2.6 |
| `SayIntentions` / `SayStand` / `PassYourMessage` | `podaj zamiary` / `podaj stanowisko` / `nadawaj` | project wording |
| `AtisCurrent(c)` | `aktualna informacja Bravo` | project wording |
| `Vacate { via }` | `zwolnij pas` (+ ` drogą X`) | 4.9 |
| `Unable(r)` | `nie mogę, ` + reason (one Polish phrase per `UnableReason`, e.g. NoRoute `brak drogi kołowania`) | 2.6 (NIE MOGĘ) |
| `FollowMeOnTheWay` / `FollowTheFollowMe` | `samochód follow me jest w drodze` / `kołuj za samochodem follow me` | project wording |
| `StandPosition` | `stanowisko dwa zero jest sto dwadzieścia metrów` + Ahead `przed tobą`, Right `po twojej prawej`, Behind `za tobą`, Left `po twojej lewej` | project wording |
| `TakenOffWithoutClearance` / `LandedWithoutClearance` | `wystartowałeś bez zezwolenia` / `wylądowałeś bez zezwolenia` | project wording |
| `Courtesy` | Hello/GoodMorning/GoodAfternoon `dzień dobry`, GoodEvening `dobry wieczór`, GoodDay `do widzenia`, HaveANiceFlight `miłego lotu`, ThankYouYouToo `dziękuję, wzajemnie` | project wording |
| other typed variants from Task 2 (incursion, altitude check) | translated the same way; altitude check: `sprawdź wysokość, wysokość kręgu tysiąc osiemset stóp` | project wording |

- [ ] **Step 1: Write the failing tests** in `render_pl.rs`, one per row, e.g.:

```rust
fn pl(items: Vec<AtcItem>, first: bool) -> String {
    render(&AtcMessage { to: Some(Callsign::new("SP-ABC")), from: "Kraków Tower".into(), items, lang: Lang::Pl }, first)
}

#[test]
fn takeoff_clearance_pl() {
    let wind = Wind { dir_deg: 250, speed_kt: 8, calm: false };
    assert_eq!(
        pl(vec![AtcItem::ClearedTakeoff { runway: "25".into(), wind }, AtcItem::Report(CircuitLeg::Downwind)], true),
        "Sierra Papa Alfa Bravo Charlie, wiatr dwieście pięćdziesiąt stopni osiem węzłów, pas dwa pięć, zezwalam startować, zgłoś z wiatrem"
    );
}

#[test]
fn taxi_with_qnh_and_squawk_pl() {
    assert_eq!(
        pl(vec![
            AtcItem::Taxi { to: TaxiTo::HoldingPoint { runway: "25".into() }, via: vec!["T".into(), "Z5".into(), "A".into()], crossings: vec![] },
            AtcItem::HoldShort("25".into()), AtcItem::Qnh(1013), AtcItem::Squawk(4201)], false),
        "Sierra Bravo Charlie, kołuj do punktu oczekiwania przed pasem dwa pięć drogą Tango, Zulu pięć, Alfa, oczekuj przed pasem dwa pięć, QNH dziesięć trzynaście, skłok cztery dwa zero jeden"
    );
}

#[test]
fn station_names_pl() {
    assert_eq!(station_name("Kraków Tower", Lang::Pl), "Kraków Wieża");
    assert_eq!(station_name("Kraków Ground", Lang::Pl), "Kraków Ground");
    assert_eq!(station_name("Kraków Tower", Lang::En), "Kraków Tower");
}
```

- [ ] **Step 2: Run** `cargo test -p atc-phraseology --test render_pl` → FAIL.
- [ ] **Step 3: Implement `pl/render.rs`** and the dispatch.
- [ ] **Step 4: Run** → PASS; `cargo test --workspace` → PASS, goldens unchanged.
- [ ] **Step 5: Commit** — `git commit -m "feat(phraseology): Polish rendering of ATC messages"`

---

### Task 5: Polish parsing

**Files:**
- Create: `crates/atc-phraseology/src/pl/normalize.rs`
- Modify: `crates/atc-phraseology/src/parse.rs` (remove the `lang != Lang::En` early return; for `Lang::Pl` tokenize with `pl::normalize::normalize` and run the same `Scan`)
- Test: `crates/atc-phraseology/tests/parse_pl.rs`

**Interfaces:**
- Produces: `pub fn normalize(text: &str) -> Vec<Token>` in `atc_phraseology::pl::normalize`: Polish words become the canonical English tokens the scanner expects; English words pass through unchanged (mixed transmissions, Review Focus 5); digits and Polish number words become `Token::Number`; ICAO letters become `Token::Letter`.
- Consumes: `Token`, `Scan` (unchanged).

Canonical rewrites (Polish, lowercase, inflected forms included → canonical English):

| Polish (pilot) | Canonical |
|---|---|
| `wieża`, `wieży` | `tower` |
| `ground`, `ziemia` | `ground` |
| `proszę o kołowanie`, `prośba o kołowanie` | `request taxi` |
| `do kręgów`, `na kręgi`, `na krąg` | `for circuits` |
| `informacja <letter>`, `mam informację <letter>`, `z informacją <letter>` | `information <letter>` |
| `do stanowiska <n>`, `na stanowisko <n>` | `to stand <n>` |
| `gotowy do startu`, `gotowy do odlotu` | `ready for departure` |
| `pas <digits>`, `pasem`, `pasa` | `runway <digits>` |
| `punkt oczekiwania`, `punktu oczekiwania` | `holding point` |
| `oczekuj(ę)/będę oczekiwać przed pasem <r>`, `przed pasem <r>` | `hold short runway <r>` |
| `drogą`, `przez` (before a taxiway letter) | `via` |
| `kołuj(ę)`, `kołowanie` | `taxi` |
| `skłok`, `squawk`, `kod` | `squawk` |
| `zezwalam/zezwalasz startować`, `zezwolenie na start`, `start dozwolony` | `cleared for take-off` |
| `zezwalam/zezwalasz lądować`, `zezwolenie na lądowanie` | `cleared to land` |
| `konwojer`, `dotyk i start` | `touch and go` |
| `z wiatrem` | `downwind`; `bejs`, `baza`, `base` → `base`; `prosta`, `prostej`, `prostą` → `final`; `pod wiatr` → `upwind`; `boczna`, `boczną` → `crosswind` |
| `z pełnym zatrzymaniem`, `do pełnego zatrzymania` | `full stop` |
| `łączność z`, `kontakt z` | `contact` |
| `kropka` | `decimal` |
| `numer <n>` | `number <n>` |
| `powtórz`, `proszę powtórzyć` | `say again` |
| `przyjąłem`, `przyjęłam`, `wykonuję`, `zrozumiałem`, `zrozumiałam` | `roger` |
| `dzień dobry`, `cześć` / `do widzenia`, `miłego dnia`, `miłego lotu`, `dziękuję` | greeting / farewell words (fill `greeting`/`farewell` like their English counterparts) |
| `na stanowisku` | `on stand` |
| `zgubiłem się`, `zgubiłam się`, `proszę o pomoc`, `nie mogę znaleźć` | `request progressive taxi` |
| `proszę o follow me`, `follow me` | `request follow me` |
| `stoję`, `czekam` (after `stój`) | `holding position` |

Number words: Polish digits, cardinals 0–999 999 incl. `tysiąc/tysiące/tysięcy`, `sto…dziewięćset`, teens and tens; a QNH said as two cardinals ("dziesięć trzynaście") becomes `1013`; written forms "1013", "123,255" / "123.255" (decimal comma between digits → `decimal`), "2 5" (spaced digits merge as in English).

- [ ] **Step 1: Write the failing tests** in `parse_pl.rs`: each Polish pilot transmission must parse to the same `intent` and `items` as its English twin (assert with `parse_pilot(.., Lang::Pl)` vs `parse_pilot(.., Lang::En)`, plus explicit values):

```rust
#[test]
fn polish_request_taxi() {
    let m = pl("Kraków Ground, Sierra Papa Alfa Bravo Charlie, dzień dobry, informacja Alfa, proszę o kołowanie do kręgów");
    assert_eq!(m.addressee.as_deref(), Some("ground"));
    assert_eq!(m.callsign.as_deref(), Some("SPABC"));
    assert_eq!(m.intent, PilotIntent::RequestTaxi { atis: Some('A'), vfr_circuits: true });
    assert_eq!(m.greeting, Some(Courtesy::GoodMorning) /* "dzień dobry" maps to the same reply as good morning */);
}

#[test]
fn polish_taxi_readback() {
    let m = pl("Kołuję do punktu oczekiwania przed pasem dwa pięć drogą Tango, Zulu pięć, Alfa, oczekuję przed pasem dwa pięć, QNH dziesięć trzynaście, skłok cztery dwa zero jeden, Sierra Bravo Charlie");
    assert_eq!(m.items.taxiways, vec!["T", "Z5", "A"]);
    assert_eq!(m.items.hold_short, vec!["25"]);
    assert_eq!(m.items.qnh, vec![1013]);
    assert_eq!(m.items.squawk, vec![4201]);
    assert_eq!(m.callsign.as_deref(), Some("SBC"));
}
```

Also: `polish_ready_for_departure` ("Kraków Wieża, Sierra Papa Alfa Bravo Charlie, punkt oczekiwania pas dwa pięć, gotowy do startu" → `ReadyForDeparture { runway: Some("25") }`), `polish_takeoff_readback` ("Pas dwa pięć, zezwalasz startować, zgłoszę z wiatrem, Sierra Bravo Charlie" → kinds `[Takeoff]`, runway `25`), `polish_downwind_full_stop` ("Sierra Bravo Charlie, z wiatrem, z pełnym zatrzymaniem" → `ReportLeg(Downwind)` and full stop as English), `polish_final` ("Sierra Bravo Charlie, prosta" → `ReportLeg(Final)`), `polish_contact_readback` ("Łączność z Kraków Ground sto osiemnaście kropka jeden zero pięć, Sierra Bravo Charlie" → `freqs_khz == [118105]`), `polish_taxi_to_stand`, `polish_say_again`, `polish_on_stand`, `polish_lost` (→ `RequestTaxiHelp`), `polish_follow_me`, and the Review Focus cases `polish_digits_and_decimal_comma` ("łączność z Kraków Wieża 123,255" → `[123255]`; "QNH 1013" → `[1013]`; "pas 2 5" → runway `25`) and `mixed_polish_english` ("Kraków Ground, SP-ABC, holding point pas dwa pięć, ready for departure" → `ReadyForDeparture { runway: Some("25") }`).

- [ ] **Step 2: Run** `cargo test -p atc-phraseology --test parse_pl` → FAIL.
- [ ] **Step 3: Implement `pl/normalize.rs`** and the parse dispatch; replace `polish_and_garbage_are_unknown` in `tests/parse.rs` with `garbage_is_unknown` (keep its English garbage assertions).
- [ ] **Step 4: Run** → PASS; `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(phraseology): Polish pilot transmissions are parsed"`

---

### Task 6: Session language in the engine; Polish ATIS

**Files:**
- Modify: `crates/atc-core/src/engine/mod.rs` (`on_transmission`, `heard`, `transmit`, the `lang: Lang::En` sites at 467/633/659, ATIS text at 188), `crates/atc-core/src/strip.rs` (`lang` field), `crates/atc-core/src/atis.rs` (`text(&self, airport_name: &str, lang: Lang)`), `EngineConfig`
- Modify callers: `crates/atc-server/src/pipeline.rs`, `crates/atc-replay/src/lib.rs`, tests that call `on_transmission`
- Test: `crates/atc-core/tests/engine.rs`, `crates/atc-core/src/atis.rs` unit tests

**Interfaces:**
- Produces:
  - `pub enum LanguageMode { Auto, Fixed(Lang) }`; `EngineConfig::language: LanguageMode` (default `Fixed(Lang::En)` so existing callers keep today's behaviour)
  - `pub fn on_transmission(&mut self, freq_khz: u32, text: &str, lang: Lang, t: SimTime) -> Vec<AtcOutput>`
  - `pub fn atis_lang(&self) -> Lang` — `Fixed(l)` → `l`; `Auto` → the language of the most recent understood pilot transmission, `En` before any
  - `Strip::lang: Lang` (starts `En` in `Auto`, `l` in `Fixed(l)`)
- Rules:
  - `Fixed(l)`: parse with `l`, reply in `l`, ignore the `lang` argument.
  - `Auto`: parse with the given `lang`; if the result is understood (intent not `Unknown`, or it is a readback/ack/courtesy the engine handles), set `strip.lang = lang`; otherwise reparse with `strip.lang` and keep it (Review Focus 2). Replies are rendered in `strip.lang`. Readbacks are judged on items, so a Polish clearance read back in English is accepted.
  - Polish ATIS (`Atis::text(.., Lang::Pl)`), project wording: `Kraków, informacja Alfa, pas w użyciu dwa pięć, wiatr dwieście pięćdziesiąt stopni osiem węzłów, widzialność dziesięć kilometrów lub więcej, brak chmur znaczących, temperatura szesnaście, punkt rosy minus dwadzieścia cztery, QNH dziesięć trzynaście, przy pierwszym kontakcie potwierdź odbiór informacji Alfa`; calm → `wiatr cisza`; visibility < 10 km → `widzialność pięć kilometrów` / `widzialność osiemset metrów`; clouds `nieliczne` (FEW), `rozproszone` (SCT), `poprzerywane` (BKN), `całkowite` (OVC) + ` <cardinal> stóp`.

- [ ] **Step 1: Write the failing tests** in `engine.rs`: `polish_taxi_request_gets_polish_clearance` (Auto; `on_transmission(GROUND, "Kraków Ground, Sierra Papa Alfa Bravo Charlie, informacja Alfa, proszę o kołowanie do kręgów", Lang::Pl, t)` → output text starts `"Sierra Papa Alfa Bravo Charlie, kołuj do punktu oczekiwania przed pasem dwa pięć"` and the station is `"Kraków Ground"`); `language_switches_back_to_english` (then an English understood call → English reply); `misdetected_short_phrase_keeps_language` (Polish session, `"roger"` tagged `Lang::En` with nothing understood → no switch; next reply still Polish); `polish_clearance_english_readback_accepted`; `fixed_english_ignores_polish_tag` (default config, `Lang::Pl` tag on an English text → English reply, as today); `atis_follows_session_language`. In `atis.rs`: `polish_atis_text` with the exact string above.
- [ ] **Step 2: Run** `cargo test -p atc-core` → FAIL.
- [ ] **Step 3: Implement**; update all callers (server pipeline passes the transcript language mapped `"pl"` → `Lang::Pl`, else `Lang::En`; replay passes the scenario line's language, Task 9).
- [ ] **Step 4: Run** `cargo test --workspace` → PASS, goldens unchanged.
- [ ] **Step 5: Commit** — `git commit -m "feat(core): the controller speaks the pilot's language; Polish ATIS"`

---

### Task 7: Voice: language detection, Polish prompt, Polish voices

**Files:**
- Modify: `crates/atc-voice/src/lib.rs` (traits), `crates/atc-voice/src/whisper.rs`, `crates/atc-voice/src/fakes.rs`, `crates/atc-voice/src/prompt.rs`, `crates/atc-voice/src/voices_config.rs`, `crates/atc-voice/src/piper.rs`
- Modify: `crates/atc-server/src/{config.rs,pipeline.rs,atis.rs,app.rs}`, `config/server.toml`, `config/voices.toml`, `tools/fetch-piper.sh`, `models/MODELS.md`
- Test: `crates/atc-voice` unit tests, `crates/atc-server/tests/*` with fakes

**Interfaces:**
- Produces:
  - `SpeechToText::transcribe(&self, pcm16k: &Pcm, prompt: &str, language: Option<&str>) -> Result<Transcript, VoiceError>` — `None` = use the configured language or auto-detect
  - `TextToSpeech::synthesize(&self, voice: VoiceId, lang: &str, text: &str) -> Result<Pcm, VoiceError>`; voices keyed by `(VoiceId, lang)`; a missing Polish voice falls back to the English voice of that `VoiceId` with one `warn!` per voice
  - `voices.toml`: existing `[ground]`, `[tower]`, `[atis]` are English; new `[pl.ground]`, `[pl.tower]`, `[pl.atis]`
  - `initial_prompt(airport, stations, callsigns, lang)` — Polish variant: "Kraków, lotnisko, łączność VFR." + stations + callsigns + "pas dwa pięć, punkt oczekiwania, oczekuj przed pasem, QNH, zezwalam startować, zezwalam lądować, z wiatrem, prosta, z pełnym zatrzymaniem, kołuj drogą Alfa, Bravo, Charlie, skłok, informacja Alfa, Sierra Papa Alfa Bravo Charlie"; the server uses the aircraft's session language for the prompt.
- Rules:
  - `[whisper] language` omitted → engine `LanguageMode::Auto`; `"en"`/`"pl"` → `Fixed`. Default `config/server.toml` stays `"en"`; document `"pl"` and auto in RUNNING.md.
  - Auto: transcribe with `None`; if `Transcript::lang` ∉ {`en`, `pl`}, transcribe again with `Some(<session language>)` and use that result (Review Focus 3).
  - Polish voices (Piper, rhasspy/piper-voices on Hugging Face, medium quality): Ground `pl_PL-darkman-medium`, Tower `pl_PL-mc_speech-medium`, ATIS `pl_PL-gosia-medium`; `tools/fetch-piper.sh` downloads them like the English ones; `models/MODELS.md` lists them with licences from their model cards.

- [ ] **Step 1: Write the failing tests**: `voices_config_reads_polish_tables`; `missing_polish_voice_falls_back_to_english`; `polish_prompt_is_short_and_polish` (contains "zezwalam startować", under the existing length limit); server test with the fake STT `third_language_is_transcribed_again` (fake returns `lang = "ru"` first, then `"pl"` when forced; the engine receives the forced transcript and `Lang::Pl`); `fixed_language_is_passed_to_whisper`.
- [ ] **Step 2: Run** `cargo test -p atc-voice -p atc-server` → FAIL.
- [ ] **Step 3: Implement**; run `tools/fetch-piper.sh` once to fetch the Polish voices (`/models/` is git-ignored).
- [ ] **Step 4: Run** `cargo test --workspace` → PASS. Run the ignored live test `cargo test -p atc-voice --test piper_live -- --ignored` with a Polish sentence added to it → a non-empty PCM.
- [ ] **Step 5: Commit** — `git commit -m "feat(voice): language detection, Polish prompt and Polish Piper voices"`

---

### Task 8: ASCII-safe overlay text

**Files:**
- Modify: `crates/atc-xplane-plugin/src/lib.rs` (pure helper), `crates/atc-xplane-plugin/src/plugin.rs` (apply before `XPLMDrawString`)
- Test: unit tests in `crates/atc-xplane-plugin/src/lib.rs`

**Interfaces:**
- Produces: `pub fn ascii_fold(s: &str) -> String` — ą→a, ć→c, ę→e, ł→l, ń→n, ó→o, ś→s, ź→z, ż→z (and uppercase), other non-ASCII → `?`.

- [ ] **Step 1: Write the failing test** `folds_polish_diacritics`: `ascii_fold("Kraków Wieża: łączność z Ground")` == `"Krakow Wieza: lacznosc z Ground"`; `ascii_fold("Ä€")` == `"??"`.
- [ ] **Step 2: Run** `cargo test -p atc-xplane-plugin` → FAIL.
- [ ] **Step 3: Implement** and use it for every overlay line drawn.
- [ ] **Step 4: Run** → PASS; build the plugin; install with `tools/install-xplane-plugin.sh` only if `pgrep -ail x-plane` is empty.
- [ ] **Step 5: Commit** — `git commit -m "fix(plugin): overlay text folded to ASCII for the X-Plane font"`

---

### Task 9: Polish scenarios and the Polish circuit guide

**Files:**
- Modify: `crates/atc-replay/src/lib.rs` (`[[say]]` gains `lang = "pl"`, default `"en"`; top-level `language = "auto" | "en" | "pl"` → `LanguageMode`, default `"en"`), `crates/atc-replay/tests/golden.rs` (scenario count)
- Create: `scenarios/epkk-circuit-pl.toml` + `.expected.txt` (whole circuit of `epkk-circuit-vmc` in Polish: ATIS-less start, taxi, readback, contact Tower, ready, take-off, downwind full stop, final, landing, vacate, taxi to stand, on stand + "do widzenia"), `scenarios/epkk-language-switch.toml` + `.expected.txt` (`language = "auto"`: Polish taxi, English with Tower, a misdetected short "roger" tagged `pl`)
- Create: `docs/user-guide/ru/polish-circuit.md` (Russian, step-by-step Polish circuit for the user: frequencies, every pilot phrase in Polish with what the controller answers, how to set `[whisper] language = "pl"` or auto, how to fetch Polish voices)
- Modify: `docs/RUNNING.md` (short English section: languages, config, voices)

**Interfaces:**
- Consumes: Tasks 4–7.

- [ ] **Step 1: Write the scenarios**; generate the expected files with `UPDATE_GOLDEN=1 cargo test -p atc-replay` and **read them line by line** against the Task 4 table and the pilot phrases of Task 5.
- [ ] **Step 2: Run** `cargo test -p atc-replay` → PASS, golden count updated, old goldens unchanged.
- [ ] **Step 3: Write `docs/user-guide/ru/polish-circuit.md`** from `epkk-circuit-pl.expected.txt` (pilot lines verbatim from the scenario, so the guide is tested), and the RUNNING.md section.
- [ ] **Step 4: Run** `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` → all green; rebuild release `atc-server` and `atc-client`.
- [ ] **Step 5: Commit** — `git commit -m "test(replay): Polish circuit and language switch scenarios; Polish circuit guide"`
