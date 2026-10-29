# Polish phraseology reference

Polish wording used by the virtual ATC (Ground, Tower, ATIS and pilot transmissions).

## Sources

- ICAO Doc 9432, 4th ed., Polish edition by ULC/PAŻP, "Podręcznik radiotelefonicznej frazeologii lotniczej" (bilingual PL/EN): <http://dlapilota.pl/files/upld/icao_doc_fl_1110.pdf>
- Appendix 1 of that edition (Polish differences): <http://dlapilota.pl/files/upld/appen1_fl_1110_en_pl.pdf>

Section numbers below refer to the manual. Entries marked *project wording* have no counterpart in the manual and were chosen for this project.

## Number rules

- Digits (2.4.1): zero, jeden, dwa, trzy, cztery, pięć, sześć, siedem, osiem, dziewięć. Decimal point: `kropka`.
- Runway, heading, taxiway digits, squawk and callsign digits are spoken digit by digit.
- Wind direction and speed are cardinals: "wiatr dwieście stopni dwadzieścia pięć węzłów".
- QNH is split into two 2-digit cardinals: 1010 = "dziesięć dziesięć", 1008 = "dziesięć zero osiem". A 3-digit QNH is spoken digit by digit ("dziewięć dziewięć osiem"). Whole thousands are cardinals ("QNH tysiąc", "skłok siedem tysięcy").
- Altitudes, visibility and cloud bases are cardinals ("tysiąc osiemset").
- Frequencies: the part before the point is a cardinal, the digits after it are spoken one by one; 8.33 channels keep all three ("sto dwadzieścia trzy kropka dwa pięć pięć"); 118.100 = "sto osiemnaście kropka jeden".
- Nouns after numbers agree in number: 1 takes the singular, 2-4 (except 12-14) the few-form, everything else the many-form: węzeł/węzły/węzłów, metr/metry/metrów, stopa/stopy/stóp, tysiąc/tysiące/tysięcy. Wind direction uses "stopni".
- ICAO alphabet: as English except `A` = "Alfa" (2.3).
- Stations: "Tower" becomes "Wieża", "Ground" stays "Ground", ATIS station text "Kraków".

## Controller phrases

`R` is the runway digits, e.g. "dwa pięć". The callsign comes first; items are joined with ", ".

Urgent instructions (runway incursion, take-off or landing without clearance, altitude check, and the "hold position" said with them) are spoken in English even in a Polish session, per the design spec ("Язык"); the Polish rows below are kept for the renderer.

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
| ATIS initial contact (IFR airports) | `w sprawie zezwolenia łączność z Delivery <freq>` / `w sprawie uruchomienia łączność z Ground <freq>`, before `przy pierwszym kontakcie ...` | project wording |
| `Vacate { via }` | `zwolnij pas` (+ ` drogą X`) | 4.9 |
| `Unable(r)` | `nie mogę, ` + reason (one Polish phrase per `UnableReason`, e.g. NoRoute `brak drogi kołowania`) | 2.6 (NIE MOGĘ) |
| `FollowMeOnTheWay` / `FollowTheFollowMe` | `samochód follow me jest w drodze` / `kołuj za samochodem follow me` | project wording |
| `StandPosition` | `stanowisko dwa zero jest sto dwadzieścia metrów` + Ahead `przed tobą`, Right `po twojej prawej`, Behind `za tobą`, Left `po twojej lewej` | project wording |
| `TakenOffWithoutClearance` / `LandedWithoutClearance` | `wystartowałeś bez zezwolenia` / `wylądowałeś bez zezwolenia` | project wording |
| `Courtesy` | Hello/GoodMorning/GoodAfternoon `dzień dobry`, GoodEvening `dobry wieczór`, GoodDay `do widzenia`, HaveANiceFlight `miłego lotu`, ThankYouYouToo `dziękuję, wzajemnie` | project wording |
| other typed variants from Task 2 (incursion, altitude check) | translated the same way; altitude check: `sprawdź wysokość, wysokość kręgu tysiąc osiemset stóp` | project wording |
| `TaxiAndReportReady` | `kołuj do punktu oczekiwania przed pasem R, zgłoś gotowość` | project wording |
| `NotClearedForTakeoff` / `NotClearedToLand` | `nie masz zezwolenia na start` / `nie masz zezwolenia na lądowanie` | project wording |
| `RunwayIncursion` | `wjechałeś na pas R bez zezwolenia` | project wording |
| `UnableReason::UnknownStand` | `nieznane stanowisko <stand>` | project wording |
| `GiveWay` / `Follow` | `ustąp pierwszeństwa <traffic>` / `kołuj za <traffic>` | project wording |
| `ClearedTo` | `zezwolenie do <genitive>` (`Katowice` -> `Katowic`; simple rules for `-ice`, `-a`, a few final consonants, other names unchanged) | project wording |
| `Sid` | `odlot SUDOL dwa Juliett` (fix as written, Polish digit, Polish ICAO letter) | project wording |
| `ClimbTo` | `wznoszenie na wysokość pięć tysięcy stóp` / `wznoszenie na poziom lotu osiem zero` | project wording |
| `StartUpApproved` / `PushAndStartApproved` | `uruchomienie zezwalam` / `wypychanie i uruchomienie zezwalam` (+ `, dziobem w kierunku <facing>`) | project wording |
| `ReadbackCorrect` | `powtórzenie poprawne` | project wording |
| `EnterControlZoneVia` / `LeaveControlZoneVia` | `wejście w strefę przez ZULU, wysokość dwa tysiące stóp` / `wyjście ze strefy przez ZULU` | project wording |
| `HoldOver` / `ReportAt` | `oczekuj nad ZULU, wysokość maksymalna dwa tysiące stóp` / `zgłoś nad ZULU` | project wording |
| `JoinDownwind` / `FrequencyChangeApproved` | `wejdź w krąg na pozycję z wiatrem, pas dwa pięć` / `zmiana częstotliwości zatwierdzona` | project wording |
| `UnableReason::SouthernEntry` | `wlot od południa tylko przez punkty południowe` (after `nie mogę`) | project wording |
| `UnableReason::UnknownDestination` / `NoIfrData` / `Closed` | `powtórz miejsce przeznaczenia` / `brak procedur IFR, tylko VFR` / `stanowisko zamknięte` (after `nie mogę`) | project wording |
| `UnableReason::ApproachNotAvailable` | `podejście niedostępne` (after `nie mogę`; Approach speaks English, so only a fallback) | project wording |
| New `ReadBack` labels | Sid `odlot`, Star `przylot`, Level `poziom`, Direct `trasę bezpośrednią`, Approach `zezwolenie na podejście`, Destination `miejsce przeznaczenia` | project wording |
| Approach-only items (`Star`, `DescendTo`, `MaintainLevel`, `Heading`, `DirectTo`, `ExpectApproach`, `ClearedApproach`, `ReportEstablished`, `Identified`, `RadarServiceTerminated`, `OffTrack`, `CheckHeading`, `CheckLevel`, `CheckEstablished`, `DistanceToGo`, `VisualApproachNotAvailable`) | rendered in English even with `lang = Pl` (Approach speaks English only) | spec §3.1 |
| Runway suffix `L` / `R` / `C` | `lewy` / `prawy` / `środkowy` after the digits | project wording |

## Pilot phrases

The parser rewrites Polish pilot words to canonical English tokens; English words pass through unchanged, so mixed transmissions work. The manual does not list pilot-side normalization, so this table is *project wording* throughout; the phrases themselves follow the controller wording above.

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
| `stoję`, `stój`, `czekam` (only when no word follows) | `holding position` |
| `przetnij`, `przecinam`, `przecinamy`, `przetnę` | `cross` |
| `zajmij`, `zajmuję`, `zajmujemy` | `line up`; `i czekaj`, `i czekam` → `and wait` |
| `przejdź`/`przechodzę`/`odchodzę na drugi krąg`, `na drugi krąg`, `drugi krąg` | `going around` |
| `zgłoszę`, `zgłoś` | `report` |
| `nie zrozumiałem`, `nie zrozumiałam` | `say again` |

Number words: Polish digits, cardinals 0–999 999 incl. `tysiąc/tysiące/tysięcy`, `sto…dziewięćset`, teens and tens; a QNH said as two cardinals ("dziesięć trzynaście") becomes `1013`; written forms "1013", "123,255" / "123.255" (decimal comma between digits → `decimal`), "2 5" (spaced digits merge as in English).
