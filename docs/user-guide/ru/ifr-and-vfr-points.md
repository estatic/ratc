# IFR и точки VFR на Cessna 172 G1000 (EPKK)

Три полёта для проверки в X-Plane 11 на Cessna 172 G1000: вылет по приборам EPKK→EPKT, прилёт по приборам EPKT→EPKK с заходом ILS 25 и вход в зону Кракова по точке VFR ZULU. Реплики пилота ниже взяты дословно из сценариев в `scenarios/`, ответы диспетчера — из их эталонов `*.expected.txt`, поэтому всё написанное проверяется тестами (`cargo test -p atc-replay`). Общий порядок работы с клиентом и PTT — в `docs/RUNNING.md`; пошаговый круг — в `docs/user-guide/ru/polish-circuit.md`.

Позывной в примерах — SP-ABC (`Sierra Papa Alpha Bravo Charlie`, сокращённо `Sierra Bravo Charlie`). Обмен в этом руководстве английский (`language = "en"` или автоопределение).

## 1. Подготовка

### Голоса

Для Delivery, Approach и радарных станций (Warszawa Radar, Kraków Information, Katowice Tower) добавлены три новых английских голоса. Один раз, до запуска сервера:

```
tools/fetch-piper.sh
```

Скрипт идемпотентен; состав голосов — `tools/MODELS.md`. Пока голоса не скачаны, эти станции не смогут говорить.

### Данные процедур: CIFP из X-Plane

SID, STAR и заходы сервер берёт из CIFP вашей установки X-Plane: файл `Resources/default data/CIFP/EPKK.dat` (путь задаёт `xplane_root` в `config/server.toml`, см. `docs/RUNNING.md`). Это ровно те процедуры, которые загружает G1000, поэтому диспетчер не выдаст процедуру, которой нет в кабине. Цикл данных в X-Plane 11 — 1802 (февраль 2018), он старый: названия и маршруты могут отличаться от сегодняшних схем AIP (AIRAC 10-26). Верно то, что в вашем G1000 (подробности — `data/airports/EPKK/SOURCE.md`).

Без CIFP (неверный `xplane_root` или файла нет) на любой IFR-запрос диспетчер отвечает `unable, IFR not available, VFR only`. В этом случае летайте по VFR или поправьте `xplane_root` и перезапустите сервер.

### Частоты

Станция, с которой вы говорите, определяется частотой радиостанции, выбранной на передачу на аудиопанели (COM1 MIC или COM2 MIC; по умолчанию COM1). Ответы слышны на ней и на любой другой COM, включённой на прослушивание: например, ATIS на COM2, пока вы работаете с Tower на COM1.

| Станция | Частота | Для чего |
|---|---|---|
| ATIS | 126.130 | погода, ВПП, буква информации (в ATIS названо, кому звонить первым) |
| Kraków Delivery | 121.980 | разрешение IFR; работает 06:00–22:00 UTC летом (08:00–24:00 местного), 05:00–21:00 UTC зимой |
| Kraków Ground | 118.105 | запуск, руление; вне часов Delivery выдаёт и разрешение IFR |
| Kraków Tower | 123.255 | взлёт, посадка, VFR остаётся на Tower |
| Kraków Approach | 121.075 | вылет после взлёта, прилёт, векторение, заходы (126.530 — то же подразделение) |
| Katowice Tower | 129.255 | принимает борт у EPKT |
| Kraków Information | 119.275 | выход из зоны ниже FL095, первый вызов при прилёте с запада |

### Правила, которые отличаются от привычного

- **Курсы магнитные.** Все курсы диспетчера («heading two five five») — магнитные, как показывает компас G1000 (HDG). Магнитное склонение в районе Кракова около 6°E.
- **Tower сам вызывает вас после Ground.** Когда Ground передал на Tower и вы выставили 123.255 перед ВПП, начальный вызов не нужен (AIP AD 2.20): Tower сам даёт разрешение на взлёт. Ваш вызов не вреден, но не требуется. Если транспондер не выставлен, Tower скажет `check squawk` или `squawk Charlie`: поставьте код и режим ALT.
- **Переход с Delivery на Ground:** диспетчер сам называет частоту (`contact Kraków Ground one one eight decimal one zero five`), читайте её обратно.
- **Квитанции проверяются.** Ошибка в SID, высоте, QNH, коде, ВПП — `negative` и повтор. Читайте обратно именно то, что в разрешении.
- **Уровни:** переходная высота 6500 ft. Ниже — высоты по QNH, выше — эшелоны. Эшелон по умолчанию для C172 — 5000 ft; другой назовите в запросе.

## 2. Загрузка процедур в G1000 (стандартный порядок X-Plane 11)

В полёте по приборам процедуры загружаются в план полёта до вылета (или до подхода). Ниже общий порядок для G1000 в X-Plane 11; названия страниц и положение кнопок могут слегка отличаться в вашей сборке.

1. Нажмите **FPL** и введите в активный план аэропорт вылета и аэропорт назначения (например, EPKK и EPKT).
2. Нажмите **PROC**. В меню процедур выберите:
   - **Select Departure** — выбор вылета (SID);
   - **Select Arrival** — выбор прибытия (STAR);
   - **Select Approach** — выбор захода.
3. В списке выберите нужный аэропорт, процедуру (SID/STAR), ВПП, переход (transition) и подтвердите **ENT**. Процедура вставляется в план полёта.
4. Проверьте план на странице FPL: фиксы SID и порядок. Для вылета SID должен быть загружен в соответствии с разрешением диспетчера.
5. Для захода выберите **Select Approach**: тип (ILS RWY 25, RNAV RWY 07, VOR/DME), ВПП и переход (с вектором — вариант «Vectors» или «Vectors to Final», где он есть). Подтвердите **ENT**.
6. Активация: курсорной ручкой выберите **Activate Approach** (или **Activate Vector-to-Final**, если заходите на векторах) при приближении, когда диспетчер даст разрешение на заход.

Названия, которые произносит диспетчер, совпадают с названиями в списке G1000 для цикла 1802: SID — `SUDOL 2J` (ВПП 25) и `SUDOL 1F` (ВПП 07), STAR — `MAPIK 1G` (ВПП 25) и `1H` (ВПП 07), заходы — `ILS 25`, `RNAV 07`, `VOR/DME 07`.

## 3. Полёт 1: вылет EPKK→EPKT по приборам

Сценарии: `scenarios/epkk-ifr-departure-25.toml` (ВПП 25), `scenarios/epkk-ifr-departure-07.toml` (ВПП 07).

### Перед запуском

- Прослушайте ATIS (COM1 = 126.130), запомните букву информации (в примерах — Alpha) и ВПП.
- Загрузите в G1000 SID: на ВПП 25 — **SUDOL 2J**, на ВПП 07 — **SUDOL 1F**, пункт назначения — EPKT. ВПП и SID зависят от ветра; диспетчер скажет вам SID в разрешении, сверьте с загруженным и при необходимости смените.
- Выставьте транспондер 2000 (режим STBY) — код вам выдадут.

### Delivery 121.980 — разрешение

Пилот (за 10 минут до запуска):

> Krakow Delivery, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, stand three five, information Alpha, IFR to Katowice, request clearance

Delivery (ВПП 25):

> Sierra Papa Alpha Bravo Charlie, cleared to Katowice, SUDOL two Juliett departure, climb altitude five thousand feet, squawk four two zero one

Пилот читает обратно и ставит код 4201:

> Cleared to Katowice, Sudol two Juliett departure, climb five thousand feet, squawk four two zero one, Sierra Bravo Charlie

Если вы прочитаете SID не тот (например, `Sudol one Foxtrot` вместо `two Juliett`), Delivery ответит:

> Sierra Bravo Charlie, negative, SUDOL two Juliett departure

Прочитайте верно. После правильной квитанции:

> Sierra Bravo Charlie, readback correct, contact Kraków Ground one one eight decimal one zero five

Пилот:

> Ground one one eight decimal one zero five, Sierra Bravo Charlie

На ВПП 07 то же с `SUDOL one Foxtrot departure`.

Вне часов Delivery (например, 23:00 UTC) Delivery молчит. Вызовите Ground — он сам выдаёт разрешение (`scenarios/epkk-ifr-clearance-night.toml`):

> Krakow Ground, Sierra Papa Alpha Bravo Charlie, stand three five, information Alpha, IFR to Katowice, request clearance

Ground:

> Sierra Papa Alpha Bravo Charlie, cleared to Katowice, SUDOL two Juliett departure, climb altitude five thousand feet, squawk four two zero one

После квитанции Ground ответит `readback correct` и без передачи на другую частоту.

### Ground 118.105 — запуск и руление

Пилот:

> Krakow Ground, Sierra Papa Alpha Bravo Charlie, stand three five, information Alpha, request start up

Ground:

> Sierra Papa Alpha Bravo Charlie, start-up approved, QNH one zero one three

Пилот:

> Start up approved, QNH one zero one three, Sierra Bravo Charlie

Руление:

> Krakow Ground, Sierra Bravo Charlie, information Alpha, request taxi

Ground (ВПП 25):

> Sierra Bravo Charlie, taxi to holding point runway two five via Bravo five, Bravo six, Alpha, hold short runway two five

Пилот читает обратно и переводит транспондер в ALT:

> Taxi to holding point runway two five via Bravo five, Bravo six, Alpha, hold short runway two five, Sierra Bravo Charlie

На ВПП 07 путь другой: `via Bravo four, Bravo three, Bravo two, Bravo one, Foxtrot, hold short runway zero seven`.

Перед ВПП Ground передаёт на Tower:

> Sierra Bravo Charlie, contact Kraków Tower one two three decimal two five five, have a nice flight

Пилот:

> Tower one two three decimal two five five, Sierra Bravo Charlie

Переставьте COM1 на 123.255.

### Tower 123.255 — взлёт

Вы ничего не вызываете: Tower сам вызывает вас.

> Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared for take-off

Пилот:

> Runway two five, cleared for take-off, Sierra Bravo Charlie

Взлетайте и летите по SID (автопилот в режиме NAV по плану G1000 или вручную). Вскоре после взлёта Tower передаёт вас на Approach:

> Sierra Bravo Charlie, contact Kraków Approach one two one decimal zero seven five

Пилот:

> Approach one two one decimal zero seven five, Sierra Bravo Charlie

Переставьте COM1 на 121.075.

### Approach 121.075 — после взлёта

Первый вызов:

> Krakow Approach, Sierra Papa Alpha Bravo Charlie, Sudol two Juliett departure, passing one thousand five hundred feet, climbing five thousand feet

Approach:

> Sierra Papa Alpha Bravo Charlie, identified

Набирайте 5000 ft по SID (ограничения SID выше 5000 ft не действуют, вы остановитесь на 5000). Когда вы прошли первый поворот SID и набираете эшелон, Approach выдаёт срезку:

> Sierra Bravo Charlie, proceed direct Katowice

Пилот (в G1000 **Direct-to** на EPKT, **ENT**, **ENT**):

> Direct Katowice, Sierra Bravo Charlie

Примерно за 20 NM до EPKT:

> Sierra Bravo Charlie, descend to altitude three thousand feet, QNH one zero one three

Пилот:

> Descend three thousand feet, QNH one zero one three, Sierra Bravo Charlie

За 10 NM до EPKT Approach передаёт вас на Katowice Tower:

> Sierra Bravo Charlie, contact Katowice Tower one two niner decimal two five five

Пилот:

> Katowice Tower one two nine decimal two five five, Sierra Bravo Charlie

Переставьте COM1 на 129.255 и вызовите:

> Katowice Tower, Sierra Papa Alpha Bravo Charlie, descending three thousand feet, inbound to Katowice

Katowice Tower:

> Sierra Papa Alpha Bravo Charlie, identified

На этом полёт 1 закончен: Katowice Tower — граничная станция, дальнейшего обслуживания у неё нет. Посадка в EPKT в этой версии не моделируется.

### Если уходите вне плана

- Уход от трассы или курса: диспетчер скажет `you are N miles off track, check position`, `check heading, fly heading …` или `check level, maintain altitude …`. Вернитесь на трассу или уровень.
- `unable, IFR not available, VFR only` — нет CIFP (см. раздел 1).

## 4. Полёт 2: прилёт EPKT→EPKK по приборам, ILS 25

Сценарии: `scenarios/epkk-ifr-arrival-vectors.toml` (вылет из EPKT, векторение), `scenarios/epkk-ifr-arrival-star-ils25.toml` (прилёт по STAR с северо-запада).

### Как начать

EPKT как аэропорт в этой версии не моделируется (Katowice Tower — граничная станция). Есть два способа:

- взлететь из EPKT самостоятельно и после набора около 4000 ft сразу вызвать Kraków Approach;
- запустить X-Plane уже в воздухе над районом EPKT на 4000–5000 ft и вызвать Kraków Approach.

В обоих случаях первый вызов — Kraków Approach 121.075 (ближе 40 NM до EPKK; принимается вызов без ожидания передачи с другой станции). Ground и Delivery EPKT не отвечают.

Заранее прослушайте ATIS EPKK (126.130) и загрузите в G1000 ILS 25 (PROC → Select Approach, ВПП 25, ILS; переход «Vectors», потому что Approach ведёт вас на векторах).

### Approach 121.075 — первый вызов

Пилот:

> Krakow Approach, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, IFR, airborne Katowice, passing four thousand feet climbing five thousand feet, inbound Krakow, information Alpha

Approach (борт уже внутри точек начала STAR, поэтому STAR не даётся, а даются векторы):

> Sierra Papa Alpha Bravo Charlie, identified, turn left heading one zero five, climb altitude five thousand feet, QNH one zero one three, expect ILS approach runway two five

Пилот читает обратно и летит курсом 105 (HDG, магнитный):

> Left heading one zero five, climb five thousand feet, QNH one zero one three, expect ILS approach runway two five, Sierra Bravo Charlie

Затем по одной команде:

> Sierra Bravo Charlie, descend to altitude three thousand feet

> Sierra Bravo Charlie, turn right heading one six zero

Читайте обратно каждую (`Descend three thousand feet, Sierra Bravo Charlie`, `Right heading one six zero, Sierra Bravo Charlie`). Затем перехват:

> Sierra Bravo Charlie, turn right heading two two zero, one one miles from touchdown, cleared ILS approach runway two five, report established

Пилот:

> Right heading two two zero, cleared ILS approach runway two five, wilco, Sierra Bravo Charlie

Держите курс 220, перехватите LOC/GS (в G1000 активируйте заход — **Activate Approach** / **Activate Vector-to-Final**, затем NAV/APR на автопилоте). Когда вы на курсе и глиссаде:

> Sierra Bravo Charlie, established ILS runway two five

Approach:

> Sierra Bravo Charlie, contact Kraków Tower one two three decimal two five five

Пилот:

> Tower one two three decimal two five five, Sierra Bravo Charlie

### Tower 123.255 — посадка

Пилот (COM1 = 123.255):

> Krakow Tower, Sierra Papa Alpha Bravo Charlie, established ILS runway two five

Tower:

> Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared to land

Пилот:

> Runway two five, cleared to land, Sierra Bravo Charlie

После посадки Tower:

> Sierra Bravo Charlie, vacate, contact Kraków Ground one one eight decimal one zero five, good day

Пилот:

> Vacating, Ground one one eight decimal one zero five, Sierra Bravo Charlie

### Ground 118.105 — на стоянку

Пилот:

> Krakow Ground, Sierra Papa Alpha Bravo Charlie, runway vacated, request taxi to stand three five

Ground:

> Sierra Papa Alpha Bravo Charlie, taxi to stand three five via Charlie, Bravo four

Пилот:

> Taxi to stand three five via Charlie, Bravo four, Sierra Bravo Charlie

На стоянке:

> Sierra Bravo Charlie, on stand three five

Ground ответит `good day`.

### Вариант: прилёт по STAR с северо-запада

Если вы подходите издалека (например, из Ополе) и ещё не дошли до точки начала STAR, первый вызов делается Kraków Information (119.275):

> Krakow Information, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, IFR from Opole to Krakow, five thousand feet, inbound Krakow

Information:

> Sierra Papa Alpha Bravo Charlie, contact Kraków Approach one two one decimal zero seven five

Пилот:

> Approach one two one decimal zero seven five, Sierra Bravo Charlie

Дальше Approach:

> Krakow Approach, Sierra Papa Alpha Bravo Charlie, IFR, five thousand feet, inbound Krakow, information Alpha

> Sierra Papa Alpha Bravo Charlie, identified, MAPIK one Golf arrival, maintain altitude five thousand feet, QNH one zero one three, expect ILS approach runway two five

Пилот читает обратно и загружает в G1000 **MAPIK 1G** (PROC → Select Arrival, ВПП 25) и ILS 25:

> MAPIK one Golf arrival, maintain five thousand feet, QNH one zero one three, expect ILS approach runway two five, Sierra Bravo Charlie

Дальше летите по STAR до её конца, затем векторы (`turn right heading two five five, descend to altitude three thousand feet`; `turn left heading two two zero, … cleared ILS approach runway two five, report established`), передача на Tower и посадка — как выше.

## 5. Полёт 3: вход в зону по точке VFR ZULU

Сценарий: `scenarios/epkk-vfr-zulu-in-sierra-out.toml` (в нём сначала вылет через SIERRA и затем возврат через ZULU).

Точки VFR — часть AIP AD 2.22: BRAVO, DELTA, ECHO, HOTEL, INDIA, KILO, LIMA, MIKE, OSCAR, ROMEO, SIERRA, UNIFORM, ZULU. Правила:

- **С юга — только через SIERRA и KILO.** Просьба войти или выйти с юга через другую точку получит `unable, entry from the south only via the southern points`. ZULU — точка входа с севера и запада, а не южная.
- Ожидание над KILO (к западу от точки) или ZULU, не выше 2000 ft AMSL.
- VFR после взлёта остаётся на Tower (не переходите на Approach).
- Выход через точку: доложите над ней и перейдите на Kraków Information 119.275.

### Вылет через SIERRA (для понимания)

Delivery:

> Krakow Delivery, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, stand three five, information Alpha, VFR to Nowy Targ via Sierra, request clearance

> Sierra Papa Alpha Bravo Charlie, cleared to Nowy Targ, leave control zone via SIERRA, squawk four two zero one

Пилот читает обратно: `Cleared to Nowy Targ, leave control zone via Sierra, squawk four two zero one, Sierra Bravo Charlie`. Далее Ground, Tower — как в полёте 1. После разрешения на взлёт Tower:

> Sierra Bravo Charlie, report at SIERRA

Пилот: `Wilco, Sierra Bravo Charlie`. Над SIERRA:

> Sierra Bravo Charlie, at Sierra, two thousand feet

> Sierra Bravo Charlie, leave control zone via SIERRA, frequency change approved, contact Kraków Information one one niner decimal two seven five

### Возврат через ZULU

Если вы вернулись и хотите войти, вызовите Tower, ещё не дойдя до зоны (за 3–5 миль до ZULU, на 2000 ft):

> Krakow Tower, Sierra Papa Alpha Bravo Charlie, returning to Krakow due weather, three miles west of Zulu, two thousand feet, information Alpha, request joining via Zulu

Tower:

> Sierra Bravo Charlie, enter control zone via ZULU, altitude two thousand feet, report at ZULU

Пилот:

> Enter control zone via Zulu, two thousand feet, report at Zulu, Sierra Bravo Charlie

Над ZULU:

> Sierra Bravo Charlie, at Zulu

Tower:

> Sierra Bravo Charlie, join downwind runway two five, report downwind

Пилот:

> Join downwind runway two five, wilco, Sierra Bravo Charlie

Дальше как в круге: на траверзе

> Sierra Bravo Charlie, downwind runway two five, full stop

> Sierra Bravo Charlie, number one, report final

> Sierra Bravo Charlie, final runway two five

> Sierra Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared to land

Каждый ответ читайте обратно (`Number one, wilco, …`, `Runway two five, cleared to land, …`), после посадки — Ground и стоянка, как в полёте 2.

## 6. Заход на ВПП 07: RNAV или VOR/DME, уход на второй круг

Сценарий: `scenarios/epkk-ifr-vor07-missed.toml`. На ВПП 07 нет ILS. По умолчанию Approach ведёт на RNAV 07 (его выполняет G1000), по просьбе — VOR/DME 07. Просьба в первом вызове:

> Krakow Approach, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, IFR, five thousand feet, inbound Krakow, information Alpha, request VOR DME approach runway zero seven

Approach:

> Sierra Papa Alpha Bravo Charlie, identified, turn right heading one seven zero, maintain altitude five thousand feet, QNH one zero zero niner, expect VOR D M E approach runway zero seven

Пилот:

> Right heading one seven zero, maintain five thousand feet, QNH one zero zero niner, expect VOR DME approach runway zero seven, Sierra Bravo Charlie

(Для RNAV 07 просьбу не добавляйте: заход `R NAV` назначается по умолчанию.) Далее, как для ILS, снижение, курс на перехват и:

> Sierra Bravo Charlie, turn left heading one zero zero, one zero miles from touchdown, cleared VOR D M E approach runway zero seven, report established

Пилот: `Left heading one zero zero, cleared VOR DME approach runway zero seven, wilco, Sierra Bravo Charlie`.

Уход на второй круг: скажите Tower и ответьте на передачу.

> Sierra Bravo Charlie, going around

Tower: `Sierra Bravo Charlie, contact Kraków Approach one two one decimal zero seven five`. Пилот читает обратно, переставляет COM1 на 121.075:

> Krakow Approach, Sierra Papa Alpha Bravo Charlie, missed approach runway zero seven, climbing four thousand feet

Approach:

> Sierra Bravo Charlie, turn left heading two seven zero, climb altitude three thousand feet

После этого Approach снова ведёт вас к заходу векторами и даёт второй заход.

## 7. Если что-то пошло не так

| Что вы слышите | Что делать |
|---|---|
| `negative, …` после квитанции | Прочитайте обратно именно то, что в разрешении |
| `say again` | Повторите короче и чётче |
| `check squawk …` / `squawk Charlie` | Проверьте код и режим ALT |
| `you are N miles off track, check position` | Вернитесь на SID/STAR/трассу |
| `check heading, fly heading …` | Выдерживайте курс по магнитному компасу (HDG) |
| `check level, maintain …` | Вернитесь на назначенный уровень |
| `unable, approach not available` | Запросите другой заход или полосу |
| `unable, IFR not available, VFR only` | Нет CIFP: проверьте `xplane_root`, перезапустите сервер |
| `unable, station closed` | Станция вне часов работы; для разрешения IFR вызовите Ground |

Полный список формулировок диспетчера с источниками (ICAO) — `docs/phraseology/en.md`.
