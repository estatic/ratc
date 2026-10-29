# План 1: чистка репозитория и текстовое ядро ATC (EPKK, Ground + Tower)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Убрать старый код и собрать на Rust детерминированное ядро диспетчера EPKK (ATIS, Ground, Tower для полёта по кругу), которое по тексту пилота и телеметрии выдаёт реплики диспетчера; проверяется реплей-стендом без звука и без X-Plane.

**Architecture:** Cargo workspace в корне. `atc-navdata` парсит X-Plane `apt.dat` и файл процедур EPKK. `atc-phraseology` разбирает текст пилота (как его выдаст Whisper) в структуру и рендерит реплики ATC по ICAO. `atc-core` — мир, полётные карточки, проверка readback, контроль выполнения, рулёжный граф, ресурс полосы, ATIS, движок диалога; без I/O. `atc-replay` — CLI, прогоняющий сценарий (телеметрия + реплики по времени) и печатающий транскрипт.

**Tech Stack:** Rust 1.98 (edition 2024), serde, toml, thiserror; anyhow только в бинарниках. Без tokio и без сети в этом плане (кроме скрипта загрузки данных).

**Spec:** `docs/superpowers/specs/2026-10-19-virtual-atc-design.md`

Это первый из трёх планов. План 2 — сервер/клиент, X-Plane UDP, Whisper, Piper, PTT. План 3 — радарный заход, срочная посадка, инструктор (RU), LLM-решатель, польский, профили пилотов, журнал решений.

## Global Constraints

- Ядро решает детерминированно; в эфир не выдаётся ни одна рулёжка, полоса, частота или высота, которой нет в навданных или в `procedures.toml`.
- Аэропорт v1 — только EPKK; парсер общий.
- Язык реплик в этом плане — только английский, ICAO-фразеология; типы проектируются с полем `Lang` (`En`, `Pl`) для плана 3.
- Других бортов в v1 нет: многобортовая логика (конфликты на рулёжках, полоса как ресурс) проверяется только синтетическими данными в тестах.
- `atc-core`, `atc-navdata`, `atc-phraseology` — без I/O и без глобального состояния (файлы читает вызывающий код или тестовые хелперы).
- Значения из AIP не выдумываются: если значение не найдено в eAIP — остановиться и спросить пользователя.
- Каждая задача заканчивается `cargo test --workspace` без ошибок и `cargo clippy --workspace -- -D warnings` без предупреждений.

## Review Focus

1. Текст Whisper в разных формах («two five», «25», «two-five», «niner», нижний регистр без пунктуации, сокращённый позывной «Sierra Bravo Charlie») должен разбираться одинаково → тест `parse_variants_equivalent` в Task 6.
2. Пилот вызывает без позывного, с чужим позывным или на частоте другой позиции → диспетчер отвечает «station calling Kraków Ground, say again your callsign», а не молчит и не падает → тест `unknown_callsign_says_again` в Task 13.
3. Пилот повторяет запрос, пока не закончен readback предыдущего разрешения → диспетчер снова требует readback, а не выдаёт второе разрешение → тест `repeat_request_while_readback_pending` в Task 13.
4. Телеметрия с разрывом, скачком позиции (перестановка в X-Plane) или паузой (время не идёт) → никаких ложных отклонений; фаза пересчитывается заново → тест `teleport_resets_phase_without_deviation` в Task 10.
5. Неизвестная стоянка, недостижимая полоса, устаревшая буква ATIS → «unable …» / «information Bravo is current, QNH …», а не паника → тесты `route_unreachable_is_error` (Task 5), `stale_atis_letter_corrected` (Task 13).

---

### Task 1: Чистка репозитория

**Files:**
- Move: `.taskmaster/docs/prd.md` → `docs/legacy/prd.md`, `.taskmaster/docs/spec.md` → `docs/legacy/spec.md` (`git mv`, сохраняя незакоммиченные правки пользователя)
- Delete (tracked): `src/`, `tests/`, `public/`, `build/`, `cmake/`, `data/atc.db`, `installer/`, `marketplace/`, `scripts/`, `docs/DEVELOPER_GUIDE.md`, `docs/USER_MANUAL.md`, `.github/`, `.taskmaster/`, `.windsurf/`, `.windsurfrules`, `Dockerfile.backend`, `Dockerfile.cpp-builder`, `Dockerfile.frontend`, `Dockerfile.plugin-builder`, `docker-compose.yml`, `docker-compose.build.yml`, `Makefile`, `bitbucket-pipelines.yml`, `build.bat`, `demo-server.js`, `validate-system.js`, `jest.config.js`, `tsconfig.json`, `.eslintrc.js`, `package.json`, `package-lock.json`, `.env.development`, `.env.production`, `.env.test`
- Delete (untracked): `node_modules/`, `dist/`, `models/` (пустая)
- Keep: `.env` (не в git, может содержать ключи — не трогать), `.vscode/`, `docs/superpowers/`
- Replace: `.gitignore`

- [ ] **Step 1: Показать пользователю итоговый список удаления** (выше) и дождаться «да». Отдельно упомянуть: незакоммиченные изменения в `src/plugins/msfs/build/*` будут потеряны.
- [ ] **Step 2: Выполнить перенос и удаление** (`git mv`, `git rm -r`, `rm -rf` для неотслеживаемых).
- [ ] **Step 3: Записать новый `.gitignore`**: `/target`, `*.log`, `.env`, `.vscode/`, `.idea/`, `.DS_Store`, `/data/cache/`, `/models/`.
- [ ] **Step 4: Проверить**: `git status --short` показывает только удаления, переносы и `.gitignore`; `ls` — в корне только `docs/`, `.gitignore`, `.env`, `.vscode/`, `.git/`.
- [ ] **Step 5: Commit** `chore: remove legacy TypeScript/C++ code and build artifacts`

### Task 2: Workspace и геометрия

**Files:**
- Create: `Cargo.toml` (workspace, `resolver = "3"`, `members = ["crates/*"]`, общие `[workspace.package] edition = "2024"`, `[workspace.dependencies]` serde (derive), toml, thiserror, anyhow)
- Create: `crates/atc-geo/Cargo.toml`, `crates/atc-geo/src/lib.rs` (геометрия — отдельный crate, от него зависят `atc-navdata` и `atc-core`)
- Test: `crates/atc-geo/src/lib.rs` (модуль `tests`)

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)] pub struct LatLon { pub lat: f64, pub lon: f64 }`
  - `pub fn distance_m(a: LatLon, b: LatLon) -> f64` (haversine, R = 6_371_000 м)
  - `pub fn bearing_deg(a: LatLon, b: LatLon) -> f64` (истинный, 0..360)
  - `pub fn destination(p: LatLon, bearing_deg: f64, dist_m: f64) -> LatLon`
  - `pub const M_PER_NM: f64 = 1852.0; pub const FT_PER_M: f64 = 3.28084;`
  - `pub struct RunwayFrame { pub origin: LatLon, pub course_deg: f64 }` с `pub fn new(threshold: LatLon, opposite_threshold: LatLon) -> Self` и `pub fn to_local(&self, p: LatLon) -> (f64, f64)` → `(along_m, right_m)`: вдоль направления взлёта от порога и вправо от оси (положительно справа).

- [ ] **Step 1: Тесты**: `distance_known_pair` (EPKK ARP ↔ EPWA ARP, ожидание взять из любого онлайн-калькулятора haversine, допуск 0.5 %); `bearing_cardinal` (точка на 0.01° севернее → 0°±0.1, восточнее → 90°±0.1); `destination_roundtrip` (destination затем distance = исходной ±0.5 м); `runway_frame_axes` (точка 1000 м по курсу → along≈1000, right≈0; точка 100 м справа под 90° → right≈+100).
- [ ] **Step 2: `cargo test -p atc-geo`** — FAIL (нет функций).
- [ ] **Step 3: Реализовать** `lib.rs`; `to_local` через `distance_m` + разность пеленгов (sin/cos).
- [ ] **Step 4: `cargo test -p atc-geo`** — PASS.
- [ ] **Step 5: Commit** `feat(geo): workspace and geodesy helpers`

### Task 3: Парсер apt.dat

**Files:**
- Create: `crates/atc-navdata/Cargo.toml` (deps: atc-geo, thiserror), `crates/atc-navdata/src/lib.rs`, `crates/atc-navdata/src/apt.rs`, `crates/atc-navdata/src/model.rs`
- Test: `crates/atc-navdata/tests/apt_parse.rs`, фикстура `crates/atc-navdata/tests/fixtures/mini.dat` (написать вручную: 1 аэропорт, 1 полоса, 3 частоты, 6 узлов, 6 рёбер, одно ребро типа runway с `1204`, 2 стоянки; плюс второй аэропорт после, чтобы проверить границу)

**Interfaces:**
- Consumes: `atc_geo::LatLon`
- Produces (`model.rs`):
  - `pub struct Airport { pub icao: String, pub name: String, pub elevation_ft: i32, pub runways: Vec<Runway>, pub frequencies: Vec<Frequency>, pub taxi: TaxiNetwork, pub stands: Vec<Stand> }`
  - `pub struct Runway { pub width_m: f64, pub ends: [RunwayEnd; 2] }`, `pub struct RunwayEnd { pub ident: String, pub threshold: LatLon, pub displaced_m: f64 }`
  - `pub enum FreqKind { Atis, Unicom, Delivery, Ground, Tower, Approach, Departure }`, `pub struct Frequency { pub kind: FreqKind, pub khz: u32, pub name: String }`
  - `pub struct TaxiNode { pub id: u32, pub pos: LatLon, pub name: String }`
  - `pub enum EdgeKind { Taxiway, Runway }`, `pub struct TaxiEdge { pub from: u32, pub to: u32, pub one_way: bool, pub kind: EdgeKind, pub name: String, pub active_zones: Vec<ActiveZone> }`
  - `pub struct ActiveZone { pub kind: ZoneKind /* Departure, Arrival, Ils */, pub runways: Vec<String> }`
  - `pub struct TaxiNetwork { pub nodes: Vec<TaxiNode>, pub edges: Vec<TaxiEdge> }`
  - `pub struct Stand { pub name: String, pub pos: LatLon, pub heading_deg: f64 }`
  - `pub fn parse_apt_dat(text: &str, icao: &str) -> Result<Airport, NavdataError>` (`NavdataError` — thiserror: `AirportNotFound(String)`, `Malformed { line: usize, reason: String }`)

Форматы строк (apt.dat 1100/1200):
- `1 <elev_ft> <0> <0> <ICAO> <name...>` — начало аэропорта; также `16`, `17`; конец — следующий `1/16/17` или `99`.
- `100 <width_m> <surface> <shoulder> <smooth> <cl_lights> <edge_lights> <signs>` затем 2 × (`<ident> <lat> <lon> <displaced_m> <blastpad_m> <markings> <appr_lights> <tdz> <reil>`).
- Частоты: `50..56` — значение в десятках кГц (`11810` → 118100 кГц); `1050..1056` — в кГц (`118105`). Порядок типов: ATIS(50/1050), Unicom(51), Delivery(52), Ground(53), Tower(54), Approach(55), Departure(56).
- `1201 <lat> <lon> <usage> <id> <name...>`; `1202 <from> <to> <oneway|twoway> <runway|taxiway|taxiway_X> <name...>`; `1204 <departure|arrival|ils> <rwy[,rwy]>` — относится к предыдущему `1202`; `1300 <lat> <lon> <heading> <type> <ac_types> <name...>` (стоянки; `1301` игнорировать).
- Неизвестные коды строк игнорировать.

- [ ] **Step 1: Тесты** `parses_header`, `parses_runway_ends` (идентификаторы и координаты из фикстуры), `parses_freq_both_formats` (строка `54 11810` и `1054 118105` → 118100 и 118105), `parses_taxi_with_active_zone` (у runway-ребра есть зона `Departure` с полосой), `parses_stands`, `stops_at_next_airport`, `missing_airport_is_error`.
- [ ] **Step 2: `cargo test -p atc-navdata`** — FAIL.
- [ ] **Step 3: Реализовать** построчный разбор (`split_whitespace`, хвост — имя).
- [ ] **Step 4: `cargo test -p atc-navdata`** — PASS.
- [ ] **Step 5: Commit** `feat(navdata): X-Plane apt.dat parser`

### Task 4: Данные EPKK (apt.dat с Gateway + процедуры из AIP)

**Files:**
- Create: `tools/fetch-gateway-apt.sh` (bash + curl + jq + base64 + unzip): `GET https://gateway.x-plane.com/apiv1/airport/EPKK` → `airport.recommendedSceneryId`; `GET https://gateway.x-plane.com/apiv1/scenery/<id>` → `scenery.masterZipBlob` (base64 zip) → извлечь файл `EPKK.dat` в `data/airports/EPKK/apt.dat`. Если структура JSON другая — сверить с ответом и поправить скрипт; в файл положить источник и id сцены строкой-комментарием в `data/airports/EPKK/SOURCE.md`.
- Create: `data/airports/EPKK/procedures.toml`
- Create: `crates/atc-navdata/src/procedures.rs`
- Test: `crates/atc-navdata/tests/epkk_real.rs`

**Interfaces:**
- Produces:
  - `pub struct Procedures { pub icao: String, pub source: String, pub airac: String, pub transition_altitude_ft: i32, pub circuits: Vec<CircuitDef>, pub preferred_runway: String, pub positions: Vec<PositionDef> }`
  - `pub struct CircuitDef { pub runway: String, pub side: CircuitSide /* Left, Right */, pub altitude_ft_msl: i32 }`
  - `pub struct PositionDef { pub role: FreqKind, pub callsign: String /* "Kraków Ground" */, pub khz: u32 }`
  - `pub fn parse_procedures(text: &str) -> Result<Procedures, NavdataError>`

- [ ] **Step 1: Запустить скрипт**, убедиться, что `data/airports/EPKK/apt.dat` содержит строку `1 ... EPKK` и строки `1201/1202`.
- [ ] **Step 2: Заполнить `procedures.toml` из eAIP Polska** (AIS Polska, AD 2 EPKK: AD 2.17/2.18/2.22 и VAC): высота круга и сторона для 07 и 25, transition altitude, позывные и частоты TWR/GND/ATIS. Поле `source` — раздел AIP, `airac` — дата цикла. Если частоты в AIP расходятся с `apt.dat` — приоритет AIP, расхождение записать в `SOURCE.md`. Значения, которых нет в AIP, не заполнять — спросить пользователя.
- [ ] **Step 3: Тесты** (`epkk_real.rs`, читают файлы из `data/airports/EPKK/` через `env!("CARGO_MANIFEST_DIR")`): `epkk_has_runway_07_25`, `epkk_has_ground_and_tower_freqs`, `procedures_cover_both_runways`, `procedures_freqs_match_positions` (каждый `PositionDef.khz` есть среди частот `apt.dat`; если AIP расходится с `apt.dat` — тест ослабить до проверки `SOURCE.md` и сообщить пользователю).
- [ ] **Step 4: `cargo test -p atc-navdata`** — PASS.
- [ ] **Step 5: Commit** `feat(navdata): EPKK airport data and AIP procedures`

### Task 5: Рулёжный граф и маршрут

**Files:**
- Create: `crates/atc-core/src/taxi.rs`
- Test: `crates/atc-core/src/taxi.rs` (на фикстуре из Task 3) + `crates/atc-navdata/tests/epkk_real.rs` (дополнить)

Создать `crates/atc-core/Cargo.toml` и `src/lib.rs` (deps: atc-geo, atc-navdata, atc-phraseology, thiserror; feature `testkit`).

**Interfaces:**
- Consumes: `Airport`, `TaxiNetwork`, `EdgeKind`, `ActiveZone`
- Produces:
  - `pub struct TaxiGraph` с `pub fn build(net: &TaxiNetwork) -> Self`
  - `pub enum TaxiTarget { HoldingPoint { runway: String }, Stand { name: String } }`
  - `pub struct TaxiRoute { pub nodes: Vec<u32>, pub via: Vec<String>, pub crossings: Vec<String>, pub hold_short: Option<String>, pub length_m: f64 }`
  - `pub fn route(&self, airport: &Airport, from: LatLon, to: &TaxiTarget) -> Result<TaxiRoute, RouteError>` (`RouteError::{UnknownStand, UnknownRunway, Unreachable}`)
  - `pub fn holding_points(&self, runway: &str) -> Vec<u32>` — узлы taxiway-рёбер, смежные с runway-ребром, у которого active zone содержит `runway`.

Правила: Дейкстра по длине; старт — ближайший узел к `from`; runway-рёбра проходимы только как пересечение (вход и выход с разных сторон за ≤ 2 ребра) и только не для целевой полосы; каждое пересечение — в `crossings` (имя полосы из active zone, напр. `"07/25"` → идентификатор той полосы); `via` — имена taxiway-рёбер по порядку, повторы подряд схлопнуты, пустые имена пропущены; `HoldingPoint` → цель — ближайший по пути узел из `holding_points`, `hold_short = Some(runway)`; `one_way` соблюдается.

- [ ] **Step 1: Тесты** `route_to_holding_point_names_taxiways`, `route_collapses_repeated_names`, `route_respects_one_way`, `route_records_runway_crossing`, `route_unreachable_is_error` (изолированный узел), `unknown_stand_is_error`; в `epkk_real.rs`: `epkk_every_stand_reaches_both_holding_points`.
- [ ] **Step 2: `cargo test --workspace`** — FAIL.
- [ ] **Step 3: Реализовать** (`std::collections::BinaryHeap`).
- [ ] **Step 4: `cargo test --workspace`** — PASS.
- [ ] **Step 5: Commit** `feat(core): taxi graph routing`

### Task 6: Разбор речи пилота

**Files:**
- Create: `crates/atc-phraseology/Cargo.toml`, `src/lib.rs`, `src/normalize.rs`, `src/parse.rs`, `src/types.rs`
- Test: `crates/atc-phraseology/tests/parse.rs`

**Interfaces:**
- Produces (`types.rs`, используются ядром):
  - `#[derive(Clone, Debug, PartialEq, Eq, Hash)] pub struct Callsign(pub String)` (нормализовано: `"SPABC"`), `pub fn abbreviated(&self) -> String` (ICAO: первый символ + последние два, `"SBC"`), `pub fn matches(&self, heard: &str) -> bool` (полный или сокращённый)
  - `pub enum Lang { En, Pl }`
  - `pub enum CircuitLeg { Upwind, Crosswind, Downwind, Base, Final }`
  - `pub enum PilotIntent { RequestTaxi { atis: Option<char>, vfr_circuits: bool }, ReadyForDeparture { runway: Option<String> }, ReportLeg(CircuitLeg), RequestFullStop, RequestTaxiToStand { stand: Option<String> }, Readback, Unknown }`
  - `#[derive(Default)] pub struct HeardItems { pub runways: Vec<String>, pub hold_short: Vec<String>, pub taxiways: Vec<String>, pub altitudes_ft: Vec<i32>, pub headings: Vec<u16>, pub freqs_khz: Vec<u32>, pub qnh: Vec<u16>, pub squawk: Vec<u16>, pub kinds: Vec<ClearanceKind> }`
  - `pub enum ClearanceKind { Taxi, LineUp, Takeoff, Land, TouchAndGo, Cross, HoldPosition, GoAround, Contact }`
  - `pub struct PilotMessage { pub addressee: Option<String>, pub callsign: Option<String>, pub intent: PilotIntent, pub items: HeardItems, pub raw: String }`
  - `pub fn parse_pilot(text: &str, lang: Lang) -> PilotMessage` (в этом плане `Pl` → `intent: Unknown`)
- `normalize.rs`: `pub fn normalize(text: &str) -> Vec<Token>`; `enum Token { Word(String), Number(String) /* строка цифр */, Letter(char) }` — нижний регистр, пунктуация и дефисы → пробелы; цифры-слова (`zero…nine`, `niner`, `tree`, `fife`, `thousand`, `hundred`) и цифры сливаются в одно число; `decimal`/`point` внутри числа → частота; фонетический алфавит (`alfa|alpha … zulu`) → `Letter`.

Распознавание: адресат — `ground|tower|information|kraków|krakow`; позывной — первая непрерывная серия `Letter`/цифр после адресата или в начале длиной ≥ 3; `runway <num>` → runway (`"25"`, `"07"`, с L/R/C); `hold(ing)? short (of)? (runway)? <num>` → hold_short; `via|taxiway <letters>` → taxiways; `qnh <4 digits>`; `squawk <4 digits>`; `<num> feet` / `altitude <num>` → altitude; `heading <3 digits>`; `<3-digit> decimal <1-3 digits>` или `<6 digits>` → freq кГц; ключевые фразы `cleared for take ?off`, `cleared to land`, `touch and go`, `line up (and wait)?`, `cross(ing)? runway`, `hold(ing)? position`, `going around`, `contact`.

- [ ] **Step 1: Тесты**: `request_taxi_with_atis` («Kraków Ground, Sierra Papa Alpha Bravo Charlie, Cessna one seven two at stand five, information Alpha, request taxi for VFR circuits» → RequestTaxi{atis: Some('A'), vfr_circuits: true}, callsign "SPABC"); `readback_taxi` («taxi holding point runway two five via alpha bravo hold short runway two five QNH one zero one three Sierra Bravo Charlie» → runways ["25"], hold_short ["25"], taxiways ["A","B"], qnh [1013]); `readback_takeoff` (kinds содержит Takeoff, runways ["25"]); `report_downwind`; `frequency_decimal` («one one eight decimal one» → 118100); **`parse_variants_equivalent`**: «Runway 25», «runway two five», «runway two-five», «RUNWAY TWO FIVE.» дают одинаковые `items`; «niner» = 9; сокращённый позывной «Sierra Bravo Charlie» → `Callsign("SPABC".into()).matches(...)` true.
- [ ] **Step 2: `cargo test -p atc-phraseology`** — FAIL.
- [ ] **Step 3: Реализовать** `normalize.rs`, `parse.rs` (ручной сканер по токенам, без regex-crate не обязательно — на выбор исполнителя).
- [ ] **Step 4: `cargo test -p atc-phraseology`** — PASS.
- [ ] **Step 5: Commit** `feat(phraseology): pilot transmission parser (EN)`

### Task 7: Рендер реплик ATC

**Files:**
- Create: `crates/atc-phraseology/src/render.rs`, дополнить `types.rs`
- Test: `crates/atc-phraseology/tests/render.rs`

**Interfaces:**
- Produces:
  - `pub struct Wind { pub dir_deg: u16, pub speed_kt: u16, pub calm: bool }`
  - `pub enum AtcItem { Taxi { to: String /* "holding point runway 25" | "stand 5" */, via: Vec<String>, crossings: Vec<String> }, HoldShort(String), CrossRunway(String), LineUpAndWait(String), ClearedTakeoff { runway: String, wind: Wind }, ClearedLand { runway: String, wind: Wind }, ClearedTouchAndGo { runway: String, wind: Wind }, Report(CircuitLeg), Sequence(u8), Qnh(u16), Contact { station: String, khz: u32 }, HoldPosition, GiveWay { traffic: String }, Follow { traffic: String }, GoAround, Negative, ReadBack(Vec<ReadbackItem>), SayAgainCallsign { station: String }, AtisCurrent(char), Vacate { via: Option<String> }, Unable(String), Roger, Free(String) }`
  - `pub struct AtcMessage { pub to: Option<Callsign>, pub from: String /* "Kraków Ground" */, pub items: Vec<AtcItem>, pub lang: Lang }`
  - `pub fn render(msg: &AtcMessage, first_contact: bool) -> String` — позывной в начале: полный при `first_contact`, иначе сокращённый, по фонетике ICAO.
  - Чтение чисел по ICAO: цифры по одной («two five»), `9` → «niner», высоты «one thousand five hundred», частота «one one eight decimal one», QNH «one zero one three», ветер «two four zero degrees, one zero knots», штиль «wind calm», буквы — фонетикой.
- `ReadbackItem` определяется в `atc-core` (Task 8)? Нет — чтобы не было цикла, **определить `ReadbackItem` здесь** в `types.rs`: `pub enum ReadbackItem { Runway(String), HoldShort(String), Route(Vec<String>), Kind(ClearanceKind), Altitude(i32), Heading(u16), Frequency(u32), Qnh(u16), Squawk(u16) }`. `ReadBack(items)` рендерится как «read back hold short instruction», «read back runway», «read back QNH».

- [ ] **Step 1: Тесты** точных строк: `taxi_clearance_text` → `"Sierra Papa Alpha Bravo Charlie, taxi to holding point runway two five via Alpha, Bravo, hold short runway two five, QNH one zero one three"`; `takeoff_text` (сокращённый позывной) → `"Sierra Bravo Charlie, wind two four zero degrees, one zero knots, runway two five, cleared for take-off, report downwind"` (элементы: ClearedTakeoff, Report(Downwind)); `negative_correction` → `"Sierra Bravo Charlie, negative, hold short runway two five"`; `contact_text` → `"..., contact Kraków Tower one one eight decimal one"`; `say_again_callsign` → `"Station calling Kraków Ground, say again your callsign"`; `niner_spoken`.
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(phraseology): ICAO rendering of ATC messages`

(Конкретную частоту в тестах брать фиктивной `118100`; реальные — только из `procedures.toml` во время работы.)

### Task 8: Readback — что требовать и проверка

**Files:**
- Create: `crates/atc-core/src/readback.rs`
- Test: там же

**Interfaces:**
- Consumes: `AtcItem`, `ReadbackItem`, `HeardItems`, `ClearanceKind`
- Produces:
  - `pub enum Strictness { Training, Standard }`
  - `pub fn required_readback(items: &[AtcItem], strictness: Strictness) -> Vec<ReadbackItem>` — Standard: полоса и тип разрешения для Taxi-to-runway/LineUp/Takeoff/Land/TouchAndGo/Cross, HoldShort, Route (via), QNH, Frequency в Contact, высоты/курсы/squawk; Training: только Runway, HoldShort, Kind.
  - `pub enum ReadbackResult { Correct, Wrong(Vec<(ReadbackItem, String /* что услышали */)>), Missing(Vec<ReadbackItem>) }`
  - `pub fn check_readback(required: &[ReadbackItem], heard: &HeardItems) -> ReadbackResult` — Wrong приоритетнее Missing: если в heard есть значение того же типа, но другое → Wrong; если значения этого типа нет → Missing. Route: порядок важен, сравнение по списку букв.
  - `pub fn correction(result: &ReadbackResult, issued: &[AtcItem]) -> Vec<AtcItem>` — Wrong → `[Negative, <исходный элемент>]`; Missing → `[ReadBack(missing)]`.

- [ ] **Step 1: Тесты** `correct_taxi_readback`, `wrong_runway_is_wrong_not_missing` (выдано 25, услышано 07), `missing_hold_short`, `training_ignores_qnh`, `route_order_matters`, `correction_for_wrong_repeats_item`.
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(core): readback requirements and checking`

### Task 9: Телеметрия, фаза полёта, участок круга

**Files:**
- Create: `crates/atc-core/src/state.rs`, `crates/atc-core/src/phase.rs`, `crates/atc-core/src/testkit.rs` (`#[cfg(any(test, feature = "testkit"))]`)
- Test: `crates/atc-core/src/phase.rs`

**Interfaces:**
- Produces:
  - `pub struct SimTime(pub f64)` (секунды)
  - `pub struct AircraftState { pub callsign: Callsign, pub t: SimTime, pub pos: LatLon, pub alt_ft_msl: f64, pub heading_deg: f64, pub ias_kt: f64, pub gs_kt: f64, pub vs_fpm: f64, pub on_ground: bool, pub com1_khz: u32 }`
  - `pub enum FlightPhase { Parked, TaxiOut, HoldingShort, OnRunway, TakeoffRoll, Circuit(CircuitLeg), Landed, TaxiIn }`
  - `pub struct PhaseTracker` с `pub fn new(rwy: RunwayFrame, rwy_half_width_m: f64, side: CircuitSide, field_elev_ft: f64) -> Self` и `pub fn update(&mut self, s: &AircraftState) -> FlightPhase`
  - testkit: `pub fn circuit_track(airport: &Airport, procs: &Procedures, runway: &str, dt_s: f64) -> Vec<AircraftState>` — синтетический полёт: стоянка → руление по `route()` (5 м/с) → взлёт → прямоугольный круг (стороны 1 NM upwind до поворота, downwind на 0.8 NM сбоку, высота круга из procedures, 80 kt) → посадка → руление на стоянку.
- Правила участка (в `RunwayFrame` активной полосы, `right_m` знак по `side`; длина полосы L): в воздухе и `|right| < 300 м`, `along > L` → Upwind (если heading ≈ курсу ±30°) или Final (along < 0 и heading ≈ курсу); со стороны круга `|right| > 300 м`: heading ≈ курс+90°/−90° к стороне → Crosswind, ≈ курс+180° → Downwind, обратно к оси перед порогом → Base. Гистерезис: участок меняется после 3 подряд одинаковых выборок.

- [ ] **Step 1: Тесты** `synthetic_circuit_visits_legs_in_order` (на `circuit_track` для EPKK 25: последовательность уникальных фаз = Parked, TaxiOut, HoldingShort, OnRunway, TakeoffRoll, Upwind, Crosswind, Downwind, Base, Final, Landed, TaxiIn), `left_vs_right_circuit`.
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(core): flight phase and circuit leg tracking`

### Task 10: Контроль выполнения

**Files:**
- Create: `crates/atc-core/src/compliance.rs`
- Test: там же

**Interfaces:**
- Consumes: `AircraftState`, `FlightPhase`, `RunwayFrame`, `TaxiGraph::holding_points`
- Produces:
  - `pub struct Permissions { pub may_enter_runway: bool, pub may_take_off: bool, pub may_land: bool, pub assigned_alt_ft: Option<i32> }`
  - `pub enum Deviation { RunwayIncursion { runway: String }, TakeoffWithoutClearance, LandingWithoutClearance, AltitudeDeviation { assigned: i32, actual: i32 } }`
  - `pub struct ComplianceMonitor` с `pub fn new(...)` и `pub fn update(&mut self, s: &AircraftState, phase: FlightPhase, perm: &Permissions) -> Vec<Deviation>` (каждое отклонение выдаётся один раз до устранения).
- Правила: incursion — `|right_m|` < (поперечное расстояние ближайшей линии ожидания этой полосы − 5 м) и `!may_enter_runway`; takeoff — на полосе, gs > 40 kt, `!may_take_off`; landing — переход air→ground в пределах полосы и `!may_land`; altitude — `|alt − assigned| > 200 ft` дольше 10 с (на Downwind assigned = высота круга). **Разрыв телеметрии**: если `Δt > 5 с`, `Δt ≤ 0` или скачок позиции > 2 × gs·Δt + 500 м — сбросить таймеры и не выдавать отклонений по этой выборке.

- [ ] **Step 1: Тесты** `incursion_detected_past_hold_line`, `no_incursion_when_cleared`, `takeoff_without_clearance`, `altitude_deviation_needs_10s`, **`teleport_resets_phase_without_deviation`** (скачок на 20 км и пауза с одинаковым `t` — пустой список).
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(core): compliance monitoring`

### Task 11: Полоса как ресурс и конфликты на рулёжках (синтетика)

**Files:**
- Create: `crates/atc-core/src/runway.rs`, `crates/atc-core/src/conflict.rs`
- Test: там же

**Interfaces:**
- Produces:
  - `pub struct RunwayResource { pub occupied_by: Option<Callsign>, pub landing: Option<Callsign>, pub crossing: Option<Callsign> }` с `pub fn can_clear(&self, who: &Callsign, use_: RunwayUse /* Takeoff, Land, Cross, LineUp */) -> Result<(), Callsign /* кто мешает */>`, `pub fn occupy(&mut self, ...)`, `pub fn release(&mut self, who: &Callsign)`
  - `pub struct TaxiMover { pub callsign: Callsign, pub route: Vec<u32>, pub progress_m: f64, pub speed_mps: f64 }`
  - `pub enum ConflictKind { HeadOn { edge: (u32, u32) }, Crossing { node: u32 } }`
  - `pub struct TaxiConflict { pub a: Callsign, pub b: Callsign, pub kind: ConflictKind, pub t_s: f64 }`
  - `pub fn predict_conflicts(g: &TaxiGraph, movers: &[TaxiMover], horizon_s: f64) -> Vec<TaxiConflict>` — по каждому мувер строит окна времени занятия рёбер/узлов; конфликт: одно ребро в противоположных направлениях с пересекающимися окнами, или один узел с окнами, расходящимися < 15 с; горизонт 120 с.
  - `pub enum Resolution { GiveWay { who: Callsign, to: Callsign }, HoldPosition { who: Callsign }, Follow { who: Callsign, leader: Callsign } }`
  - `pub fn resolution_options(c: &TaxiConflict, movers: &[TaxiMover]) -> Vec<Resolution>` и `pub fn default_resolution(options: &[Resolution], c: &TaxiConflict, movers: &[TaxiMover]) -> Resolution` — уступает тот, кто позже приходит в точку конфликта (HeadOn → HoldPosition для позднего; Crossing → GiveWay позднего раннему; один путь, одно направление → Follow).

- [ ] **Step 1: Тесты** `runway_blocks_takeoff_when_occupied`, `crossing_blocked_when_landing_reserved`, `head_on_detected`, `crossing_node_detected`, `no_conflict_when_separated_in_time`, `default_resolution_later_gives_way`.
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(core): runway resource and taxi conflict prediction`

### Task 12: ATIS

**Files:**
- Create: `crates/atc-core/src/atis.rs`
- Test: там же

**Interfaces:**
- Produces:
  - `pub struct Weather { pub wind: Wind, pub visibility_m: u32, pub clouds: Vec<(CloudCover /* Few, Sct, Bkn, Ovc */, u32 /* base ft AGL */)>, pub temp_c: i16, pub dewpoint_c: i16, pub qnh_hpa: u16 }`
  - `pub struct Atis { pub letter: char, pub runway: String, pub weather: Weather }` и `pub struct AtisService` с `pub fn update(&mut self, w: Weather, airport: &Airport, procs: &Procedures) -> Option<&Atis>` (новая буква A→Z→A, когда меняется полоса, QNH, ветер ≥ 30° или ≥ 5 kt, видимость/облачность меняет категорию) и `pub fn current(&self) -> &Atis`, `pub fn text(&self, airport_name: &str) -> String`.
  - Полоса: максимальная встречная составляющая; при |встречная| < 3 kt — `procs.preferred_runway`.
- Текст: `"Kraków information Alpha, runway in use two five, wind two four zero degrees one zero knots, visibility one zero kilometres or more, few clouds two thousand five hundred feet, temperature one five, dew point eight, QNH one zero one three, on initial contact advise you have information Alpha"`.

- [ ] **Step 1: Тесты** `runway_by_headwind`, `calm_uses_preferred`, `letter_advances_on_qnh_change`, `letter_stable_on_small_change`, `atis_text_exact`.
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(core): ATIS generation`

### Task 13: Движок диалога Ground + Tower

**Files:**
- Create: `crates/atc-core/src/engine.rs`, `crates/atc-core/src/strip.rs`
- Test: `crates/atc-core/tests/engine.rs` (feature `testkit`)

**Interfaces:**
- Consumes: всё выше.
- Produces:
  - `pub enum Role { Ground, Tower }`
  - `pub struct PilotProfile { pub callsign: Callsign, pub aircraft_type: String }`
  - `pub struct Strip { pub callsign: Callsign, pub owner: Role /* Ground, Tower */, pub phase: FlightPhase, pub issued: Vec<AtcItem>, pub pending_readback: Option<Vec<ReadbackItem>>, pub permissions: Permissions, pub first_contact_done: HashSet<Role>, pub full_stop_requested: bool }`
  - `pub struct EngineConfig { pub strictness: Strictness }`
  - `pub struct AtcEngine` с:
    - `pub fn new(airport: Airport, procs: Procedures, cfg: EngineConfig) -> Self`
    - `pub fn register_pilot(&mut self, p: PilotProfile)`
    - `pub fn on_weather(&mut self, w: Weather)`
    - `pub fn on_telemetry(&mut self, s: AircraftState) -> Vec<AtcOutput>`
    - `pub fn on_transmission(&mut self, freq_khz: u32, text: &str, t: SimTime) -> Vec<AtcOutput>`
  - `pub struct AtcOutput { pub freq_khz: u32, pub station: String, pub message: AtcMessage, pub text: String }`
- Поведение (позиция определяется частотой из `procedures.positions`; передача на частоте без позиции — тишина):
  - Позывной не распознан или не зарегистрирован → `SayAgainCallsign`.
  - Ground, `RequestTaxi`: если `atis` ≠ текущей букве или отсутствует → добавить `AtisCurrent(letter)` и `Qnh`; маршрут к holding point полосы в работе → `Taxi`, `HoldShort`, `Qnh`; `pending_readback = required_readback(...)`. Пересечения из маршрута добавляются в `Taxi.crossings`, разрешение на пересечение выдаётся отдельно при подходе (`CrossRunway`, только если `RunwayResource::can_clear(Cross)`).
  - Любая передача при `pending_readback.is_some()`: проверить `check_readback`; Correct → снять pending, ответ не нужен (тишина) для Ground/Tower, кроме случая, когда пилот добавил запрос; иначе → `correction(...)`. **Повторный запрос при незакрытом readback** → повторить `ReadBack(pending)`, новое разрешение не выдавать.
  - Фаза `HoldingShort` у Ground → Ground выдаёт `Contact { Tower }` (один раз), owner = Tower после корректного readback.
  - Tower, `ReadyForDeparture` → при `can_clear(Takeoff)` `ClearedTakeoff{wind}` + `Report(Downwind)`, иначе `HoldPosition`/`LineUpAndWait`.
  - Tower, `ReportLeg(Downwind)` → `Sequence(1)` + `Report(Final)`; `ReportLeg(Final)` → `ClearedTouchAndGo` или при `full_stop_requested` `ClearedLand`.
  - После посадки с полной остановкой, фаза `Landed` → `Vacate` + `Contact{Ground}`; Ground по `RequestTaxiToStand` → маршрут к стоянке.
  - Отклонения из `ComplianceMonitor` → немедленные реплики: incursion → `[HoldPosition]` + `Free("you have entered runway two five without clearance")`, takeoff/landing без разрешения → `Free(...)`, altitude → `Free("check altitude, circuit altitude <alt> feet")`.

- [ ] **Step 1: Тесты** `full_circuit_happy_path` (testkit-трек + реплики пилота в нужные моменты → последовательность `AtcItem` по ролям), **`unknown_callsign_says_again`**, **`repeat_request_while_readback_pending`**, **`stale_atis_letter_corrected`**, `wrong_readback_gets_negative`, `incursion_triggers_hold_position`, `transmission_on_unused_freq_is_silent`.
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(core): Ground and Tower dialogue engine`

### Task 14: Реплей-стенд

**Files:**
- Create: `crates/atc-replay/Cargo.toml` (bin, deps: atc-core с feature testkit, atc-navdata, anyhow, serde, toml), `crates/atc-replay/src/main.rs`
- Create: `scenarios/epkk-circuit-vmc.toml`, `scenarios/epkk-bad-readback.toml`, `scenarios/epkk-incursion.toml`, ожидаемые транскрипты `scenarios/*.expected.txt`
- Test: `crates/atc-replay/tests/golden.rs`

**Interfaces:**
- Формат сценария: `airport = "EPKK"`, `pilot = { callsign = "SPABC", type = "C172" }`, `weather = {...}`, `track = "synthetic-circuit:25"` (через `circuit_track`) или `track_file = "..."` (CSV `t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz` — для плана 2), `[[say]] at = "phase:Parked" | t = 12.0, freq = "ground"|"tower", text = "..."`, `[[event]] at = ..., kind = "skip-hold-short"` (модификатор трека для incursion).
- CLI: `atc-replay <scenario.toml>` печатает транскрипт `[t] PILOT/STATION: text`.
- Golden-тест сравнивает вывод с `.expected.txt` (обновление: `UPDATE_GOLDEN=1 cargo test -p atc-replay`).

- [ ] **Step 1: Написать три сценария и тест** `golden_transcripts_match`.
- [ ] **Step 2: FAIL** (нет бинарника).
- [ ] **Step 3: Реализовать** main.rs; сгенерировать `.expected.txt` через `UPDATE_GOLDEN=1`, **прочитать каждый транскрипт глазами** и сверить с фразеологией ICAO; ошибки править в коде, не в ожиданиях.
- [ ] **Step 4: `cargo test --workspace && cargo clippy --workspace -- -D warnings`** — PASS.
- [ ] **Step 5: Ручная проверка**: `cargo run -p atc-replay -- scenarios/epkk-circuit-vmc.toml` — показать транскрипт пользователю.
- [ ] **Step 6: Commit** `feat(replay): scenario replay harness with golden transcripts`
