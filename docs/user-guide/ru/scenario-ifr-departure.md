# Сценарий: полёт по приборам EPKK → EPKT, ВПП 25 → ILS 27

**Русский** · [English](../en/scenario-ifr-departure.md)

Полный полёт на Cessna 172 G1000: от стоянки в Кракове до стоянки в Катовице. Вылет по SID SUDOL 2J, заход ILS 27 по векторам. Позывной — SP-ABC, сокращённо Sierra Bravo Charlie. Подробное руководство — `ifr-and-vfr-points.md`.

Реплики диспетчеров записаны с сервера: две станции EPKK и EPKT, схема аэропортов из X-Plane 11 (default scenery), процедуры из CIFP X-Plane, погода — ветер 250°/8 kt, QNH 1013, лето, 10:00 UTC. Твои курсы, дистанции, высоты и QNH могут отличаться: повторяй то, что услышал.

## Включить станцию Катовице

В репозитории есть только станция EPKK. Станцию EPKT создай мастером (нужен `xplane_root` в `config/server.toml` и ключ OpenAIP, см. `docs/RUNNING.md`, раздел «Stations and the airport wizard»):

```
target/release/atc-server airports
```

Пункт меню 1 → найди EPKT → отметь позиции: ATIS 120.230, Delivery 124.080, Ground 121.805, Tower 129.255, Approach 121.075 (Kraków Approach) → подтверди. Станция появится в `data/airports/EPKT/`.

Затем в `config/server.toml` убери (или закомментируй) строку `airport = "EPKK"`. Без неё сервер загружает все станции из `data_dir`. В логе при запуске должно быть `stations: EPKK, EPKT`. Если в `data/airports` есть другие станции, загрузятся и они: на этот полёт они не влияют. Отдельного списка станций в конфиге нет.

Kraków Approach 121.075 есть в обеих станциях. Пока ты летишь от Кракова, с тобой работает Approach станции EPKK. За 30 NM до Катовице он передаёт тебя Approach станции EPKT: позывной и частота те же, поэтому передача беззвучная: вызова «contact» ты не услышишь и ничего не меняешь. Диспетчер EPKT просто продолжает вести тебя.

## Твой позывной

Клиент берёт позывной из симулятора: это бортовой номер загруженного самолёта (N172SP, SP-ABC). Чтобы летать под другим, впиши `callsign = "SP-ABC"` в `config/client.toml`: он главнее бортового номера. Если у самолёта нет бортового номера и `callsign` не задан, клиент сообщит об этом и не подключится.

В первом контакте диспетчер называет позывной полностью, дальше сокращает: первый символ и последние два (SP-ABC превращается в «Sierra Bravo Charlie», N172SP — в «November Sierra Papa»). Делай так же: называй полностью, пока диспетчер не сократил, потом можно и тебе. В таблицах ниже используется SP-ABC.

## Подготовка

```
cargo build --release -p atc-server -p atc-client
tools/fetch-piper.sh          # один раз: голоса Delivery, Approach, Radar
target/release/atc-server     # окно 1
target/release/atc-client     # окно 2
```

До запуска двигателя:
- послушай ATIS Кракова 126.130 (буква, ВПП 25, QNH);
- поставь самолёт на стоянку **9R** в Кракове;
- транспондер 2000, режим STBY;
- G1000, план полёта: см. следующий раздел.

## План в G1000 и высоты

**FPL**: EPKK → EPKT. **PROC → Select Departure → SUDOL 2J** (ВПП 25; для ВПП 07 — SUDOL 1F). **PROC → Select Approach → ILS 27**, переход **Vectors**. NAV1: IKTO 109.90, курс 265°.

Переходная высота на обоих аэродромах 6500 ft. Выше неё (на эшелоне) ставь в альтиметр 1013 (STD), ниже — QNH.

| Этап | Высота | Откуда |
|---|---|---|
| Стоянка EPKK | 791 ft (превышение аэродрома) | apt.dat X-Plane |
| Взлёт ВПП 25 | порог 779 ft | CIFP EPKK |
| Разрешение Delivery | FL100 | самое высокое ограничение «не ниже» на SUDOL 2J |
| KK599, первая точка SID | не ниже 3000 ft | CIFP, SUDOL 2J |
| KK602, KK604 | не ниже 6000 ft и FL100 | CIFP, SUDOL 2J |
| Передача Approach EPKT (30 NM до EPKT) | ещё набираешь; в записи ~4200 ft | правило передачи |
| Первая команда Approach EPKT | набор до 5000 ft («climb altitude five thousand feet») | `ifr.cruise_ft` станции EPKT |
| ~15 NM до точки входа в курсовой | снижение до 3000 ft | Approach EPKT |
| Векторение на ILS | 3000 ft, ниже не снижайся | высота FAF в CIFP |
| KT537 (IF, 9.3 NM по DME IKTO) | 3000 ft | CIFP, ILS 27 |
| KT536 (FAF, 7.8 NM по DME IKTO) | 3000 ft, вход в глиссаду 3.0° | CIFP, ILS 27 |
| Порог ВПП 27 | превышение порога 1003 ft, пролёт порога 1055 ft | CIFP EPKT |
| Уход на второй круг | набор 4000 ft | CIFP, ILS 27 |
| Стоянка EPKT | 995 ft (превышение аэродрома) | apt.dat X-Plane |

На Cessna 172 до FL100 к передаче в Катовице не набрать: на ней ты будешь где-то на 4000–5000 ft. Это нормально, Approach EPKT задаст высоту сам.

## Полёт с G1000

- **FPL.** Нажми **FPL**. Если поля аэродромов пусты, нажми ручку **FMS**, чтобы включить курсор, набери EPKK как аэродром вылета и EPKT как аэродром назначения (для списка вылетов нужен аэродром вылета) малой и большой ручками FMS и нажми **ENT**.
- **Вылет.** **PROC → Select Departure → SUDO2J** (SUDOL 2J, ВПП 25) → **Load**. ВПП и SID должны совпадать с тем, что выдал Delivery; для ВПП 07 это SUDO1F (SUDOL 1F).
- **После взлёта.** Нажми **NAV**, чтобы автопилот вёл по SID. Когда диспетчер даёт курс, перейди на **HDG** и лети им; на NAV вернись, когда дадут «direct» или ты снова на маршруте.
- **Высоты.** Выставь заданную высоту в окне выбранной высоты и набирай или снижайся по **ALT** / **VS** (что ждать, смотри в таблице высот выше).
- **Переходная высота.** В EPKK она 6500 ft. Когда тебе разрешили эшелон выше неё, поставь **STD BARO** (1013); ниже — QNH от диспетчера.
- **ILS.** Настрой курсовой маяк на **NAV1** (IKTO 109.90), проверь, что **CDI** показывает **LOC1** (он переключается сам, когда настроен ILS; если нет, нажми **CDI**) и включи **APR**, когда получишь разрешение на заход.

## Пустой список вылетов

Если **PROC → Select Departure** ничего не показывает для EPKK (или Select Approach — для EPKT), причина обычно в симуляторе, а не в этой программе. Навигационная база в папке `Custom Data` X-Plane (файл `earth_424.dat`) заменяет данные по умолчанию. Если в ней нет процедур этого аэродрома, G1000 их не покажет.

Сервер проверяет это при запуске и выводит предупреждение для каждой затронутой станции:

```
X-Plane Custom Data replaces the default navigation data and has no procedures for EPKK: the aircraft GPS will list no departures or arrivals there. Move "Custom Data" aside or install a navigation database that covers EPKK.
```

Убери `Custom Data` в сторону (переименуй) и перезапусти X-Plane либо поставь базу, которая покрывает аэродром.

## Delivery 121.980

Delivery работает летом (примерно с 28 марта по 28 октября) 06–22 UTC, зимой 05–21 UTC. В остальное время те же реплики говори Ground.

| Кто | Фраза |
|---|---|
| Ты | Krakow Delivery, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, stand nine right, information Alpha, IFR to Katowice, request clearance |
| Delivery | Sierra Papa Alpha Bravo Charlie, cleared to Katowice, SUDOL two Juliett departure, climb flight level one zero zero, squawk four two zero one |
| Ты | Cleared to Katowice, Sudol two Juliett departure, climb flight level one zero zero, squawk four two zero one, Sierra Bravo Charlie |
| Delivery | Sierra Bravo Charlie, readback correct, contact Kraków Ground one one eight decimal one zero five |
| Ты | Ground one one eight decimal one zero five, Sierra Bravo Charlie |

Поставь код 4201. В G1000 выставь выбранную высоту FL100 (10000 ft).

Если Delivery ответил **«say flight rules»**, в запросе не прозвучало ни IFR, ни VFR. Повтори запрос с правилами полёта («IFR to Katowice, request clearance»).

### Другая ВПП

На стоянке или при рулении можно попросить у Delivery или Ground другую ВПП для вылета:

| Кто | Фраза |
|---|---|
| Ты | Krakow Ground, Sierra Bravo Charlie, request departure runway zero seven |

ВПП дают («runway zero seven approved»), если попутный ветер на ней не больше 5 узлов, иначе ответ «unable, runway in use two five». Если разрешение уже выдано с другой ВПП, диспетчер добавит «runway changed, runway zero seven» и новый SID (для 07 — SUDOL 1F): повтори ВПП и SID. Если ты уже рулишь к другой ВПП, получишь новое руление.

## Ground 118.105

| Кто | Фраза |
|---|---|
| Ты | Krakow Ground, Sierra Papa Alpha Bravo Charlie, stand nine right, information Alpha, request start up |
| Ground | Sierra Papa Alpha Bravo Charlie, start-up approved, QNH one zero one three |
| Ты | Start up approved, QNH one zero one three, Sierra Bravo Charlie |
| Ты | Krakow Ground, Sierra Bravo Charlie, information Alpha, request taxi |
| Ground | Sierra Bravo Charlie, taxi to holding point runway two five via Zulu four, Zulu five, Alpha, hold short runway two five |
| Ты | Taxi to holding point runway two five via Zulu four, Zulu five, Alpha, hold short runway two five, Sierra Bravo Charlie |
| Ground (перед ВПП, ~2 мин руления) | Sierra Bravo Charlie, contact Kraków Tower one two three decimal two five five, have a nice flight |
| Ты | Tower one two three decimal two five five, Sierra Bravo Charlie |

QNH 1013 поставь в альтиметр: на стоянке он покажет 791 ft. После квитанции руления переведи транспондер в ALT.

Маршрут руления зависит от сценария аэропорта, который загружает X-Plane. Повторяй тот, что сказал Ground.

## Tower 123.255

Переключи COM1 и **молчи**: Tower вызовет тебя сам.

| Кто | Фраза |
|---|---|
| Tower | Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared for take-off |
| Ты | Runway two five, cleared for take-off, Sierra Bravo Charlie |
| Tower (сразу после отрыва) | Sierra Bravo Charlie, contact Kraków Approach one two one decimal zero seven five |
| Ты | Approach one two one decimal zero seven five, Sierra Bravo Charlie |

После взлёта лети по SID в режиме NAV и набирай высоту. KK599 пройди не ниже 3000 ft.

## Kraków Approach 121.075: вылет

| Кто | Фраза |
|---|---|
| Ты (на 1500 ft) | Krakow Approach, Sierra Papa Alpha Bravo Charlie, Sudol two Juliett departure, passing one thousand five hundred feet, climbing flight level one zero zero |
| Approach | Sierra Papa Alpha Bravo Charlie, identified |

В первом вызове назови свою текущую высоту.

Примерно за 30 NM до EPKT (~7 мин после взлёта, ещё до KK602) тебя принимает Approach станции EPKT. Частота та же, передача беззвучная: COM1 не трогай и ничего не говори. Продолжай лететь по SID и набирать высоту, пока он не задаст маршрут. «Direct Katowice» и 20-мильного снижения от Approach Кракова в этом полёте нет: передача происходит раньше.

## Kraków Approach 121.075: прилёт и заход ILS 27

Перед вызовом послушай ATIS Катовице на COM2, 120.230 (в записи: information Alpha, runway in use two seven). Не вызывай: передача от Approach Кракова была беззвучной, ты уже опознан. Жди. Approach EPKT сам один раз вызовет тебя с курсом, высотой и ожидаемым заходом, а ты повторишь:

| Кто | Фраза |
|---|---|
| Approach | Sierra Papa Alpha Bravo Charlie, turn right heading three four five, climb altitude five thousand feet, QNH one zero one three, expect ILS approach runway two seven |
| Ты | Right heading three four five, climb five thousand feet, QNH one zero one three, expect ILS approach runway two seven, Sierra Bravo Charlie |
| Approach (~15 NM до точки входа в курсовой) | Sierra Bravo Charlie, descend to altitude three thousand feet |
| Ты | Descend three thousand feet, Sierra Bravo Charlie |
| Approach | Sierra Bravo Charlie, turn right heading zero zero five |
| Ты | Right heading zero zero five, Sierra Bravo Charlie |
| Approach (~10 NM от порога) | Sierra Bravo Charlie, turn left heading two niner five, one zero miles from touchdown, cleared ILS approach runway two seven, report established |
| Ты | Left heading two niner five, cleared ILS approach runway two seven, wilco, Sierra Bravo Charlie |
| Approach (на курсовом, внутри FAF) | Sierra Bravo Charlie, contact Katowice Tower one two niner decimal two five five |
| Ты | Tower one two niner decimal two five five, Sierra Bravo Charlie |

Если два Approach на разных частотах (другие данные станций), действует старый порядок: голосовая передача («contact ...», повтори её), затем твой собственный первый вызов с проходимой высотой, разрешённым эшелоном и «inbound to Katowice, information Alpha»; ответ тогда начинается с «identified».

Это векторение: STAR нет. Курс 345 ведёт тебя на север, восточнее Катовице; затем 005 и разворот влево на курсовой ВПП 27 с востока. Набор останови на 5000 ft: это высота прилёта станции EPKT. Если на вызове ты уже выше 5000 ft, услышишь «descend to altitude five thousand feet». Если ты уже близко к точке входа в курсовой, сразу дадут 3000 ft.

Курсы задавай в режиме HDG. Повтори курс сразу: без квитанции через ~20 с Approach скажет «read back heading…», а если не повернёшь, то «check heading, fly heading …».

На «descend» выставь 3000 ft и снижайся. Ниже 3000 ft не уходи: Approach скажет «check level, maintain altitude three thousand feet».

На «cleared ILS approach» в G1000: **PROC → Activate Vector-to-Final**, NAV1 109.90, режим APR. Держи 3000 ft до глиссады у KT536, дальше снижайся по ней.

Если ты уже на курсовом и вошёл в глиссаду, Approach может передать тебя Tower, не дожидаясь доклада «established». Тогда доложи «established» уже Tower.

## Katowice Tower 129.255: посадка

Tower сам разрешает посадку заходящему по приборам на прямой, вызов «final» не нужен. Если вызвать Tower до прямой, на осевой он скажет «continue approach, N miles from touchdown», вне её — «you are N miles <north, south, ...> of the final approach course». Посадку он разрешит, когда будешь на прямой.

| Кто | Фраза |
|---|---|
| Ты | Katowice Tower, Sierra Papa Alpha Bravo Charlie, established ILS runway two seven |
| Katowice Tower | Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two seven, cleared to land |
| Ты | Runway two seven, cleared to land, Sierra Bravo Charlie |
| Katowice Tower (на пробеге) | Sierra Bravo Charlie, vacate, contact Katowice Ground one two one decimal eight zero five, good day |
| Ты | Vacating, Ground one two one decimal eight zero five, Sierra Bravo Charlie |

Если не повторишь частоту, Tower через ~15 с напомнит: «read back frequency».

## Katowice Ground 121.805

Освободи ВПП и остановись за линией ожидания.

| Кто | Фраза |
|---|---|
| Ты | Katowice Ground, Sierra Papa Alpha Bravo Charlie, runway vacated, request taxi |
| Ground | Sierra Papa Alpha Bravo Charlie, say stand |
| Ты | Katowice Ground, Sierra Bravo Charlie, request taxi to stand apron four golf alpha |
| Ground | Sierra Bravo Charlie, taxi to stand apron four Golf Alpha via Sierra, Hotel |
| Ты | Taxi to stand apron four Golf Alpha via Sierra, Hotel, Sierra Bravo Charlie |
| Ты (на стоянке, ~5 мин руления) | Sierra Bravo Charlie, on stand apron four golf alpha |
| Ground | Sierra Bravo Charlie, good day |

Стоянку можно назвать сразу: «runway vacated, request taxi to stand apron four GA» — ответ тот же, без «say stand». Без названной стоянки Ground ведёт тебя на твою (если она известна) или спрашивает «say stand». Руление после посадки — всегда к стоянке, без QNH и кода.

Имена стоянок из сценария сравниваются по буквам и цифрам: «APRON4 GA» можно назвать как «apron four GA» или «apron four golf alpha». Стоянок с этим именем несколько, Ground выбирает ближайшую свободную. Ground произносит имя так: «apron four Golf Alpha». Если попросить «any stand» («any available spot»), Ground назначит свободную стоянку авиации общего назначения или скажет «unable, no stand available», если свободных нет.

Полёт окончен: выключи транспондер (STBY) и двигатель. В записи от запроса разрешения до стоянки прошло ~40 минут: взлёт на 3-й минуте, посадка на 34-й.

## На что обратить внимание

- Все курсы диспетчера магнитные.
- Ошибка в повторе даёт `negative, …`. Повтори правильно.
- При уходе с маршрута или высоты услышишь `check position`, `check heading` или `check level`.
- В каждой реплике называй позывной: без него диспетчер напомнит.
- Прислушайся, понятно ли звучит «VOR D M E» и «R NAV».
