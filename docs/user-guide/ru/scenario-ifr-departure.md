# Сценарий: вылет EPKK → Катовице по приборам, ВПП 25

Полёт на Cessna 172 G1000 с вылетом по SID SUDOL 2J. Реплики взяты из записанного полёта, по которому проходят тесты (`scenarios/epkk-ifr-departure-25.toml`). Позывной — SP-ABC, сокращённо Sierra Bravo Charlie. Подробное руководство — `ifr-and-vfr-points.md`.

## Подготовка

```
cargo build --release -p atc-server -p atc-client
tools/fetch-piper.sh          # один раз: голоса Delivery, Approach, Radar
target/release/atc-server     # окно 1
target/release/atc-client     # окно 2
```

До запуска двигателя:
- послушай ATIS 126.130, запомни букву информации и ВПП;
- в G1000: **FPL** EPKK → EPKT, затем **PROC → Select Departure → SUDOL 2J** (для ВПП 07 — **SUDOL 1F**);
- транспондер 2000 в режиме STBY.

## Delivery 121.980

Delivery работает 06–22 UTC. В остальное время те же реплики говори Ground.

| Кто | Фраза |
|---|---|
| Ты | Krakow Delivery, Sierra Papa Alpha Bravo Charlie, Cessna one seven two, stand three five, information Alpha, IFR to Katowice, request clearance |
| Delivery | Sierra Papa Alpha Bravo Charlie, cleared to Katowice, SUDOL two Juliett departure, climb altitude five thousand feet, squawk four two zero one |
| Ты | Cleared to Katowice, Sudol two Juliett departure, climb five thousand feet, squawk four two zero one, Sierra Bravo Charlie |
| Delivery | Sierra Bravo Charlie, readback correct, contact Kraków Ground one one eight decimal one zero five |
| Ты | Ground one one eight decimal one zero five, Sierra Bravo Charlie |

Если Delivery ответил **«say flight rules»**, в запросе не прозвучало ни IFR, ни VFR: повтори запрос и назови правила полёта («IFR to Katowice, request clearance»).

### Другая ВПП

На стоянке или при рулении можно попросить у Delivery или Ground другую ВПП для вылета:

| Кто | Фраза |
|---|---|
| Ты | Krakow Ground, Sierra Bravo Charlie, request departure runway zero seven |

ВПП дают («runway zero seven approved»), если попутный ветер на ней не больше 5 узлов, иначе ответ «unable, runway in use two five». Если разрешение уже выдано с другой ВПП, диспетчер добавит «runway changed, runway zero seven» и новый SID (для 07 — SUDOL 1F): повтори ВПП и SID. Если ты уже рулишь к другой ВПП, получишь новое руление.

## Ground 118.105

| Кто | Фраза |
|---|---|
| Ты | Krakow Ground, Sierra Papa Alpha Bravo Charlie, stand three five, information Alpha, request start up |
| Ground | Sierra Papa Alpha Bravo Charlie, start-up approved, QNH one zero one three |
| Ты | Start up approved, QNH one zero one three, Sierra Bravo Charlie |
| Ты | Krakow Ground, Sierra Bravo Charlie, information Alpha, request taxi |
| Ground | Sierra Bravo Charlie, taxi to holding point runway two five via Bravo five, Bravo six, Alpha, hold short runway two five |
| Ты | Taxi to holding point runway two five via Bravo five, Bravo six, Alpha, hold short runway two five, Sierra Bravo Charlie |
| Ground | Sierra Bravo Charlie, contact Kraków Tower one two three decimal two five five, have a nice flight |
| Ты | Tower one two three decimal two five five, Sierra Bravo Charlie |

После квитанции руления переведи транспондер в ALT.

## Tower 123.255

Переключи COM1 и **молчи**: Tower вызовет тебя сам.

| Кто | Фраза |
|---|---|
| Tower | Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared for take-off |
| Ты | Runway two five, cleared for take-off, Sierra Bravo Charlie |
| Tower (после взлёта) | Sierra Bravo Charlie, contact Kraków Approach one two one decimal zero seven five |
| Ты | Approach one two one decimal zero seven five, Sierra Bravo Charlie |

## Approach 121.075

| Кто | Фраза |
|---|---|
| Ты | Krakow Approach, Sierra Papa Alpha Bravo Charlie, Sudol two Juliett departure, passing one thousand five hundred feet, climbing five thousand feet |
| Approach | Sierra Papa Alpha Bravo Charlie, identified |
| Approach (после первого поворота SID) | Sierra Bravo Charlie, proceed direct Katowice |
| Ты | Direct Katowice, Sierra Bravo Charlie |
| Approach (~20 NM до EPKT) | Sierra Bravo Charlie, descend to altitude three thousand feet, QNH one zero one three |
| Ты | Descend three thousand feet, QNH one zero one three, Sierra Bravo Charlie |
| Approach (10 NM до EPKT) | Sierra Bravo Charlie, contact Katowice Tower one two niner decimal two five five |
| Ты | Katowice Tower one two nine decimal two five five, Sierra Bravo Charlie |

На «direct» в G1000: **Direct-to** EPKT, затем **ENT**, **ENT**.

## Katowice Tower 129.255

| Кто | Фраза |
|---|---|
| Ты | Katowice Tower, Sierra Papa Alpha Bravo Charlie, descending three thousand feet, inbound to Katowice |
| Katowice Tower | Sierra Papa Alpha Bravo Charlie, identified |

Здесь сценарий заканчивается: посадку в Катовице добавит проект 1b.

## На что обратить внимание

- Все курсы диспетчера магнитные.
- Ошибка в повторе даёт `negative, …`. Повтори правильно.
- При уходе с маршрута или высоты услышишь `check position`, `check heading` или `check level`.
- Прислушайся, понятно ли звучит «VOR D M E» и «R NAV».
