# Сценарий: самостоятельный круг, ВПП 25 (EPKK)

Левый круг на высоте 1800 ft, в хорошую погоду. Реплики взяты из записанного полёта, по которому проходят тесты (`scenarios/epkk-circuit-vmc.toml`). Позывной — SP-ABC, сокращённо Sierra Bravo Charlie.

## Подготовка

```
cargo build --release -p atc-server -p atc-client
target/release/atc-server     # окно 1, дождаться "listening on 127.0.0.1:8740"
target/release/atc-client     # окно 2 (или --text для отладки текстом)
```

C172 стоит на стоянке EPKK. Станцию выбирает частота радиостанции, выбранной на передачу (по умолчанию COM1): ATIS 126.130, Ground 118.105, Tower 123.255. ATIS можно слушать и на COM2, если она включена на прослушивание.

## Ground 118.105

| Кто | Фраза |
|---|---|
| Ты | Krakow Ground, Sierra Papa Alpha Bravo Charlie, information Alpha, request taxi for circuits |
| Ground | Sierra Papa Alpha Bravo Charlie, taxi to holding point runway two five via Bravo five, Bravo six, Alpha, hold short runway two five, QNH one zero one three, squawk four two zero one |
| Ты | Taxi to holding point runway two five via Bravo five, Bravo six, Alpha, hold short runway two five, QNH one zero one three, squawk four two zero one, Sierra Bravo Charlie |

Выставь код 4201 и режим ALT. Маршрут руления зависит от стоянки: повторяй то, что услышал.

У предварительного старта:

| Кто | Фраза |
|---|---|
| Ground | Sierra Bravo Charlie, contact Kraków Tower one two three decimal two five five, have a nice flight |
| Ты | Contact Krakow Tower one two three decimal two five five, Sierra Bravo Charlie |

## Tower 123.255

Переключи COM1 и **молчи**: Tower вызовет тебя сам.

| Кто | Фраза |
|---|---|
| Tower | Sierra Papa Alpha Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared for take-off, report downwind |
| Ты | Runway two five, cleared for take-off, report downwind, Sierra Bravo Charlie |
| Ты (на траверзе, по ветру) | Sierra Bravo Charlie, downwind, full stop |
| Tower | Sierra Bravo Charlie, number one, report final |
| Ты | Number one, report final, Sierra Bravo Charlie |
| Ты (на прямой) | Sierra Bravo Charlie, final |
| Tower | Sierra Bravo Charlie, wind two five zero degrees, eight knots, runway two five, cleared to land |
| Ты | Runway two five, cleared to land, Sierra Bravo Charlie |
| Tower (после пробега) | Sierra Bravo Charlie, vacate, contact Kraków Ground one one eight decimal one zero five, good day |
| Ты | Vacating, contact Krakow Ground one one eight decimal one zero five, Sierra Bravo Charlie |

## Ground 118.105 — на стоянку

| Кто | Фраза |
|---|---|
| Ты | Krakow Ground, Sierra Bravo Charlie, request taxi to stand |
| Ground | Sierra Bravo Charlie, taxi to stand three five via Charlie, Bravo four |
| Ты | Taxi to stand three five via Charlie, Bravo four, Sierra Bravo Charlie |
| Ты (на стоянке) | Sierra Bravo Charlie, on stand three five |
| Ground | good day |

Если потерялся при рулении: «Sierra Bravo Charlie, I am lost, I need help». Если не найдёшь стоянку — «request follow-me».
