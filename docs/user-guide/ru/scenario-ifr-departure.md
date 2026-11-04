# Сценарий: полёт по приборам EPKK → EPKT, ВПП 25 → ILS 27

Полный полёт на Cessna 172 G1000: от стоянки в Кракове до стоянки в Катовице. Вылет по SID SUDOL 2J, заход ILS 27 по векторам. Позывной — SP-ABC, сокращённо Sierra Bravo Charlie. Подробное руководство — `ifr-and-vfr-points.md`.

Реплики диспетчеров записаны с сервера: две станции EPKK и EPKT, схема аэропортов из X-Plane 11 (default scenery), процедуры из CIFP X-Plane, погода — ветер 250°/8 kt, QNH 1013, лето, 10:00 UTC. Твои курсы, дистанции и QNH могут отличаться: повторяй то, что услышал.

## Включить станцию Катовице

В репозитории есть только станция EPKK. Станцию EPKT создай мастером (нужен `xplane_root` в `config/server.toml` и ключ OpenAIP, см. `docs/RUNNING.md`, раздел «Stations and the airport wizard»):

```
target/release/atc-server airports
```

Пункт меню 1 → найди EPKT → отметь позиции: ATIS 120.230, Delivery 124.080, Ground 121.805, Tower 129.255, Approach 121.075 (Kraków Approach) → подтверди. Станция появится в `data/airports/EPKT/`.

Затем в `config/server.toml` убери (или закомментируй) строку `airport = "EPKK"`. Без неё сервер загружает все станции из `data_dir`. В логе при запуске должно быть `stations: EPKK, EPKT`. Если в `data/airports` есть другие станции, загрузятся и они. Отдельного списка станций в конфиге нет.

Kraków Approach 121.075 указан в обеих станциях, поэтому это один диспетчер на одной частоте. После передачи в Катовице с тобой работает Approach станции EPKT.

## Подготовка

```
cargo build --release -p atc-server -p atc-client
tools/fetch-piper.sh          # один раз: голоса Delivery, Approach, Radar
target/release/atc-server     # окно 1
target/release/atc-client     # окно 2
```

До запуска двигателя:
- послушай ATIS Кракова 126.130 (буква, ВПП 25, QNH) и ATIS Катовице 120.230 (в записи: information Alpha, runway in use two seven);
- поставь самолёт на стоянку **9R** в Кракове. В сценарии X-Plane 11 стоянки 35 нет;
- транспондер 2000, режим STBY;
- G1000, план полёта: см. следующий раздел.

## План в G1000 и высоты

**FPL**: EPKK → EPKT. **PROC → Select Departure → SUDOL 2J** (ВПП 25; для ВПП 07 — SUDOL 1F). **PROC → Select Approach → ILS 27**, переход **Vectors**. NAV1: IKTO 109.90, курс 265°.

Переходная высота на обоих аэродромах 6500 ft: весь полёт идёт на QNH.

| Этап | Высота | Откуда |
|---|---|---|
| Стоянка EPKK | 791 ft (превышение аэродрома) | apt.dat X-Plane |
| Взлёт ВПП 25 | порог 779 ft | CIFP EPKK |
| KK599, первая точка SID | не ниже 3000 ft | CIFP, SUDOL 2J |
| KK602 и дальше по SID | SID требует не ниже 6000 ft (KK602) и FL100 (KK604), но разрешено 5000 ft: остановись на 5000 | CIFP; разрешение Delivery |
| Первый вызов Approach | текущая высота и «climbing five thousand feet» | разрешение |
| Direct Katowice | набор до 5000 ft продолжается | — |
| Крейсерская | 5000 ft | `ifr.cruise_ft` станции |
| 20 NM до EPKT | снижение до 3000 ft | правило Approach: превышение EPKT 995 ft + 2000 ft, округление вверх до 500 |
| Векторение на ILS | 3000 ft, ниже не снижайся | высота FAF в CIFP |
| KT537 (IF, 9.3 NM по DME IKTO) | 3000 ft | CIFP, ILS 27 |
| KT536 (FAF, 7.8 NM по DME IKTO) | 3000 ft, вход в глиссаду 3.0° | CIFP, ILS 27 |
| Порог ВПП 27 | превышение порога 1003 ft, пролёт порога 1055 ft | CIFP EPKT |
| Уход на второй круг | набор 4000 ft | CIFP, ILS 27 |
| Стоянка EPKT | 995 ft (превышение аэродрома) | apt.dat X-Plane |

## Delivery 121.980

Delivery работает летом (примерно с 28 марта по 28 октября) 06–22 UTC, зимой 05–21 UTC. В остальное время те же реплики говори Ground.

| Кто | Фраза |
|---|---|
| Ты | Krakow Delivery, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, stand nine right, information Alpha, IFR to Katowice, request clearance |
| Delivery | Sierra Papa Alpha Bravo Charlie, cleared to Katowice, SUDOL two Juliett departure, climb altitude five thousand feet, squawk four two zero one |
| Ты | Cleared to Katowice, Sudol two Juliett departure, climb five thousand feet, squawk four two zero one, Sierra Bravo Charlie |
| Delivery | Sierra Bravo Charlie, readback correct, contact Kraków Ground one one eight decimal one zero five |
| Ты | Ground one one eight decimal one zero five, Sierra Bravo Charlie |

Поставь код 4201. В G1000 выставь выбранную высоту 5000 ft.

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
| Ground (перед ВПП) | Sierra Bravo Charlie, contact Kraków Tower one two three decimal two five five, have a nice flight |
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
| Ты | Krakow Approach, Sierra Papa Alpha Bravo Charlie, Sudol two Juliett departure, passing one thousand five hundred feet, climbing five thousand feet |
| Approach | Sierra Papa Alpha Bravo Charlie, identified |
| Approach (у KK602) | Sierra Bravo Charlie, proceed direct Katowice |
| Ты | Direct Katowice, Sierra Bravo Charlie |
| Approach (20 NM до EPKT, ты на 5000 ft) | Sierra Bravo Charlie, descend to altitude three thousand feet, QNH one zero one three |
| Ты | Descend three thousand feet, QNH one zero one three, Sierra Bravo Charlie |
| Approach (10 NM до EPKT) | Sierra Bravo Charlie, contact Katowice Tower one two niner decimal two five five |
| Ты | Katowice Tower one two nine decimal two five five, Sierra Bravo Charlie |

В первом вызове назови свою текущую высоту («passing one thousand five hundred feet» — пример).

На «direct» в G1000: **Direct-to** EPKT, затем **ENT**, **ENT**. Набор продолжай до 5000 ft.

На «descend» выставь 3000 ft и снижайся. Ниже 3000 ft не уходи: Approach скажет «check level, maintain altitude three thousand feet».

## Katowice Tower 129.255: возврат на Approach

Kraków Approach в Кракове передаёт тебя на Katowice Tower, но заход в Катовице ведёт Approach станции EPKT. Tower вызов не разбирает и сразу возвращает тебя на 121.075:

| Кто | Фраза |
|---|---|
| Ты | Katowice Tower, Sierra Papa Alpha Bravo Charlie, three thousand feet |
| Katowice Tower | Sierra Papa Alpha Bravo Charlie, say again |
| Katowice Tower | Sierra Bravo Charlie, contact Kraków Approach one two one decimal zero seven five |
| Ты | Approach one two one decimal zero seven five, Sierra Bravo Charlie |

Не повторяй вызов после «say again»: через секунду придёт «contact Kraków Approach».

## Kraków Approach 121.075: заход ILS 27

| Кто | Фраза |
|---|---|
| Ты | Krakow Approach, Sierra Papa Alpha Bravo Charlie, IFR, three thousand feet, inbound Katowice, information Alpha |
| Approach | Sierra Papa Alpha Bravo Charlie, identified, turn right heading zero four zero, maintain altitude three thousand feet, QNH one zero one three, expect ILS approach runway two seven |
| Ты | Right heading zero four zero, maintain three thousand feet, QNH one zero one three, expect ILS approach runway two seven, Sierra Bravo Charlie |
| Approach (~11 NM от порога) | Sierra Bravo Charlie, turn left heading two niner five, one one miles from touchdown, cleared ILS approach runway two seven, report established |
| Ты | Left heading two niner five, cleared ILS approach runway two seven, wilco, Sierra Bravo Charlie |
| Ты (курсовой пойман) | Sierra Bravo Charlie, established ILS runway two seven |
| Approach | Sierra Bravo Charlie, contact Katowice Tower one two niner decimal two five five |
| Ты | Tower one two niner decimal two five five, Sierra Bravo Charlie |

«Information Alpha» — буква ATIS Катовице (120.230).

Если на вызове ты ещё выше 3000 ft, Approach скажет «descend to altitude three thousand feet» вместо «maintain».

Курс 040 уводит тебя на восток, за точку входа в курсовой. Это нормально: диспетчер ведёт тебя в обход. Курсы задавай в режиме HDG. Повтори курс сразу: без квитанции через ~20 с Approach скажет «read back heading…», а если не повернёшь, то «check heading, fly heading zero four zero».

На «cleared ILS approach» в G1000: **PROC → Activate Vector-to-Final**, NAV1 109.90, режим APR. Держи 3000 ft до глиссады у KT536, дальше снижайся по ней.

## Katowice Tower 129.255: посадка

| Кто | Фраза |
|---|---|
| Ты | Katowice Tower, Sierra Papa Alpha Bravo Charlie, established ILS runway two seven |
| Katowice Tower | Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two seven, cleared to land |
| Ты | Runway two seven, cleared to land, Sierra Bravo Charlie |
| Katowice Tower (после посадки, на пробеге) | Sierra Bravo Charlie, vacate, contact Katowice Ground one two one decimal eight zero five, good day |
| Ты | Vacating, Ground one two one decimal eight zero five, Sierra Bravo Charlie |

Если не повторишь частоту, Tower через ~15 с напомнит: «read back frequency».

## Katowice Ground 121.805

Освободи ВПП и остановись за линией ожидания. В запросе обязательно назови стоянку.

| Кто | Фраза |
|---|---|
| Ты | Katowice Ground, Sierra Papa Alpha Bravo Charlie, runway vacated, request taxi to stand one |
| Ground | Sierra Papa Alpha Bravo Charlie, taxi to stand one via Sierra, Hotel, Delta |
| Ты | Taxi to stand one via Sierra, Hotel, Delta, Sierra Bravo Charlie |
| Ты (на стоянке) | Sierra Bravo Charlie, on stand one |
| Ground | Sierra Bravo Charlie, good day |

Без стоянки («request taxi») Ground ответит как улетающему: руление к ВПП 27 с QNH и кодом.

Стоянку называют буквами и цифрами («stand one»). Имена из нескольких слов, например «APRON4 GA» или «Hangar Aircom», назвать нельзя.

Полёт окончен: выключи транспондер (STBY) и двигатель.

## На что обратить внимание

- Все курсы диспетчера магнитные.
- Ошибка в повторе даёт `negative, …`. Повтори правильно.
- При уходе с маршрута или высоты услышишь `check position`, `check heading` или `check level`.
- В каждой реплике называй позывной: без него диспетчер напомнит.
- Прислушайся, понятно ли звучит «VOR D M E» и «R NAV».
