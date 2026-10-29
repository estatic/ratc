# Запуск Virtual ATC (X-Plane 11, EPKK)

Всё работает локально: сервер (Whisper STT + Piper TTS + диспетчерский движок) и клиент (микрофон/динамики, чтение X-Plane по UDP) общаются по WebSocket на `127.0.0.1`. Внешних сервисов нет; интернет нужен только для первой загрузки моделей.

**Все команды запускайте из корня репозитория** — пути в `config/*.toml` (модели, `data/airports`, `config/voices.toml`) заданы относительно него.

## 1. Сборка

Предварительно (Linux) нужны:

- для `whisper-rs` (Vulkan-сборка): Vulkan-тулчейн с `glslc` (shaderc), `cmake`, `libclang`;
- для `cpal` (звук): заголовки ALSA (`libasound2-dev` / `alsa-lib`);
- для `gilrs` (джойстик): `libudev` (`libudev-dev` / `systemd-libs`) и права на чтение `/dev/input/*` (обычно группа `input`).

Учтите: у `atc-server` фичи `whisper`/`piper` включены по умолчанию, поэтому даже `cargo test --workspace` собирает `whisper-rs` (долго, нужен тот же тулчейн).

```
cargo build --release -p atc-server -p atc-client
```

Бинарники: `target/release/atc-server`, `target/release/atc-client`. Release важен: Whisper в debug-сборке заметно медленнее.

## 2. Модели

```
tools/fetch-whisper.sh   # models/whisper/: ggml-large-v3-turbo-q5_0.bin (~574 МБ) и ggml-small.en-q5_1.bin (~181 МБ)
tools/fetch-piper.sh     # models/piper/: бинарник Piper и голоса: Ground, Tower, ATIS, Delivery, Approach, Radar (английские) и польские
```

Скрипты идемпотентны (повторный запуск пропускает уже скачанное). `models/` в git-ignore. Состав голосов и их соответствие станциям — `tools/MODELS.md`, скорость речи (`length_scale`) — `config/voices.toml`.

Если модели нет, сервер завершается с кодом 2 и подсказкой, какой скрипт запустить.

### Выбор модели Whisper

В `config/server.toml`, секция `[whisper]`:

- по умолчанию `models/whisper/ggml-large-v3-turbo-q5_0.bin` (многоязычная). **Выбор предварительный** — см. `docs/benchmarks/2026-10-voice.md`: STT p95 около 0.17 с, но VRAM рядом с запущенным X-Plane ещё не измерен;
- переключиться на английскую `small.en`: `model = "models/whisper/ggml-small.en-q5_1.bin"` (быстрее, вдвое меньше VRAM; на синтетическом тесте не хуже);
- `language` — язык радиообмена:
  - `"en"` (в конфиге по умолчанию) фиксирует английский;
  - `"pl"` фиксирует польский: польская подсказка Whisper, разбор польских реплик, польские ответы, ATIS и голоса (`[pl.*]` в `config/voices.toml`);
  - строка не указана — автоопределение: каждому борту отвечают на языке его последней понятой реплики (до первой — по-английски), ATIS — на языке последней понятой реплики. Если Whisper определил третий язык (например, `ru` при акценте), запись распознаётся повторно на языке борта. Короткие английские фразы с акцентом иногда определяются неверно, поэтому при полётах только на одном языке лучше его зафиксировать.

  Польская модель Whisper — та же многоязычная `large-v3-turbo` (`small.en` понимает только английский).

Прочие параметры сервера: `listen` (по умолчанию `127.0.0.1:8740`), `airport = "EPKK"`, `strictness = "standard" | "training"`.

### Схема аэропорта из X-Plane (`xplane_root`)

`xplane_root = "/путь/к/X-Plane 11"` в `config/server.toml`: схему аэропорта (ВПП, рулёжки, стоянки — часть apt.dat) сервер берёт из той сцены, которую грузит сам X-Plane, чтобы маршруты руления называли рулёжки, видимые в симуляторе. Порядок поиска — как у X-Plane: папки `Custom Scenery`, ещё не попавшие в `scenery_packs.ini` (X-Plane допишет их наверх при следующем старте), затем включённые `SCENERY_PACK` из ini по порядку (`SCENERY_PACK_DISABLED` пропускаются), затем `Custom Scenery/Global Airports`, затем `Resources/default scenery/default apt dat`. Частоты и процедуры всегда из `data/airports/<ICAO>/`. В логе при старте: `airport layout: <файл> (X-Plane scenery): ...; taxiways: ...`. Без `xplane_root` или если аэропорт не найден — предупреждение и `data/airports/<ICAO>/apt.dat` из репозитория. Поставили или убрали сцену аэропорта — перезапустите и X-Plane, и сервер.

### IFR и точки VFR

- Три новых английских голоса (Delivery, Approach, Radar) скачиваются тем же `tools/fetch-piper.sh`; запустите его один раз до старта сервера (`tools/MODELS.md`).
- Процедуры (SID, STAR, заходы) сервер читает из CIFP установки X-Plane (`Resources/default data/CIFP/EPKK.dat`) по `xplane_root`. Без CIFP на IFR-запрос диспетчер отвечает `IFR not available, VFR only`.
- Пошаговое руководство по вылету и прилёту по приборам и входу по точке VFR на C172 G1000 — `docs/user-guide/ru/ifr-and-vfr-points.md`; формулировки с источниками — `docs/phraseology/en.md`.

### Языки: английский и польский

- Язык обмена — `[whisper] language` в `config/server.toml` (см. выше): `"en"`, `"pl"` или автоопределение (строка не указана).
- Голоса: `tools/fetch-piper.sh` скачивает английские (`[ground]`, `[tower]`, `[atis]`) и польские (`[pl.ground]`, `[pl.tower]`, `[pl.atis]`) голоса из `config/voices.toml`; без польского голоса станция говорит английским.
- Польская фразеология (что понимает разбор и что отвечает диспетчер) — `docs/phraseology/pl.md`; пошаговый польский круг — `docs/user-guide/ru/polish-circuit.md`.

## 3. X-Plane 11

Для связи плагины не нужны, настройки Settings → Network для RREF менять не нужно: X-Plane сам отвечает на UDP-порту 49000 (клиент только читает позицию, COM1 и COM2 с аудиопанелью, транспондер и погоду). Запустите X-Plane, поставьте самолёт (C172) на стоянку EPKK. Если X-Plane на другой машине или порту — поправьте `xplane = "host:port"` в `config/client.toml`.

### Текст переговоров на экране (необязательно)

Плагин `atc-overlay` показывает в X-Plane окно с последними репликами диспетчера (белым), тем, что распознано из вашей речи (зелёным), и состоянием PTT (красным во время передачи). Реплики старше минуты тускнеют.

```
tools/install-xplane-plugin.sh                  # Steam-путь по умолчанию
tools/install-xplane-plugin.sh "/путь/к/X-Plane 11"
```

Скрипт собирает плагин и кладёт его в `Resources/plugins/atc-overlay/64/lin.xpl`; после установки перезапустите X-Plane. Окно можно перетаскивать и менять его размер. Показать или скрыть окно — команда `atc/overlay/toggle` (назначается на клавишу или кнопку в Settings → Keyboard / Joystick). Клиент шлёт строки на UDP `127.0.0.1:48010`; другой адрес — `overlay = "host:port"` в `config/client.toml`.

### Follow-me: машина сопровождения

На стоянках EPKK (схема Global Airports) нет разметки на бетоне, поэтому найти свою стоянку в конце руления трудно. Ground помогает так:

- «…, I am lost, I need help» (любая просьба о помощи с рулением) — маршрут заново, а если цель — стоянка ближе 500 м, ещё и где она: `stand two zero is one hundred twenty metres ahead` (расстояние до 10 м; ahead / on your right / behind you / on your left — по направлению от носа самолёта, сектора по 90°);
- вторая просьба о помощи после того же разрешения на руление, или «request follow-me» / «follow me» / «request marshaller» — `hold position, follow-me car is on the way` (прочитать «holding position»); через 12 с симуляторного времени — `follow the follow-me car` (ответ «wilco» или «follow the follow-me», строгой квитанции нет);
- «on stand» на стоянке (самолёт стоит, до назначенной стоянки ≤ 25 м) — `good day` (на пожелание — `thank you, you too`); не на стоянке — где она, тем же оборотом.

Машина (crew car из библиотеки X-Plane 11, `lib/airport/vehicles/servicing/crew_car.obj`) появляется сразу метрах в 40 впереди самолёта на маршруте и едет по рулёжкам до стоянки, держась в 30–50 м впереди: ждёт, пока самолёт стоит или отстал больше чем на 80 м, назад не ездит, не быстрее 15 узлов, плавно поворачивает. В конце пути отъезжает на 12 м вправо и исчезает через 20 с после того, как самолёт доехал. При рулении на исполнительный машина останавливается за 20 м до него; дальше, как обычно, Ground передаёт на Tower. Новое разрешение на руление отзывает машину.

Машину рисует тот же плагин `atc-overlay` (клиент пересылает ему команды по тому же UDP 48010). После обновления **переустановите плагин** (`tools/install-xplane-plugin.sh`) и перезапустите X-Plane; нужен X-Plane 11.10+ (API инстансинга).

## 4. Сервер

```
target/release/atc-server                       # config/server.toml
target/release/atc-server --config путь.toml
target/release/atc-server --fresh               # не восстанавливать сохранённые полёты
```

Первый старт грузит модели несколько секунд, затем в логе `listening on 127.0.0.1:8740`. Уровень логов — `RUST_LOG` (по умолчанию `info`). В логе каждой передачи видны времена `stt_ms`, `engine_ms`, `tts_ms`.

Сервер сохраняет полёты в работе между запусками: выданные разрешения, фазу полёта, ожидаемые квитанции, код транспондера, букву ATIS. Снимок пишется каждые 2 с и при остановке (Ctrl-C) в `data/cache/state-<ICAO>.json` (в git не попадает). При старте он восстанавливается, если ему не больше 2 часов и он сделан для того же аэропорта; в логе — `flights in progress restored from …` или `starting afresh (…)` с причиной. Начать с чистого листа: `target/release/atc-server --fresh` (или удалить файл).

Сервер отвергает WebSocket-подключения с заголовком `Origin` — то есть из браузера подключиться нельзя, это сделано намеренно (защита от чужих веб-страниц). Нативный клиент `Origin` не шлёт.

## 5. Клиент

Настройки — `config/client.toml`: `server`, `callsign` (SP-ABC), `aircraft_type`, `xplane`, `field_elev_ft` (791 для EPKK), `overlay` (адрес плагина на экране, по умолчанию `127.0.0.1:48010`), `input_device` / `output_device` (подстрока имени устройства; без них — системные по умолчанию).

```
target/release/atc-client --help
# usage: atc-client [--config <file>] [--text] [--record <track.csv>]
```

### Голосовой режим (по умолчанию)

```
target/release/atc-client
```

Станция, с которой вы говорите, определяется частотой радиостанции, выбранной на передачу на аудиопанели X-Plane (COM1 MIC или COM2 MIC; по умолчанию COM1). Ответы слышны на ней и на любой другой COM, включённой на прослушивание. Любая слушаемая COM на 126.130 включает ATIS — он играет по кругу (например, ATIS на COM2, пока вы на связи с Ground на COM1). При смене станций клиент печатает `COM 118.105, also listening 126.130`.

Транспондер: Ground выдаёт код в конце разрешения на руление («…, QNH one zero one three, squawk four two zero one»; коды по порядку из пула 4201–4277, только восьмеричные цифры), его нужно прочитать в readback и выставить. После перехода от Ground Tower сам вызывает вас на пороге ВПП (если COM1 на частоте Tower), а на «ready for departure» отвечает так же, и сверяет транспондер X-Plane: другой код — «check squawk four two zero one», режим OFF/STBY — «squawk Charlie», разрешения на взлёт нет. Выставьте и ответьте («Squawk four two zero one, …» / «wilco, …») — Tower проверит снова и сразу даст взлёт либо повторит указание. Ставьте режим ALT.

PTT (секция `[ptt]`):

- **клавиатура** (`key = "space"`) — **переключатель**: нажмите пробел, чтобы начать передачу, нажмите снова, чтобы закончить. Терминал не отдаёт события отпускания клавиши, поэтому hold-to-talk с клавиатуры невозможен. Зажатая клавиша не завершает передачу: пока автоповтор идёт (каждое нажатие повторяется не позже чем через 1 с после предыдущего), нажатия игнорируются; новый «тап» засчитывается после паузы не менее 1 с с последнего нажатия. Итого: тап — начало, тап — конец;
- **кнопка джойстика** (`joystick_button = 0`) — обычный hold-to-talk: держите — передача идёт, отпустили — конец. Уберите строку, чтобы оставить только клавишу. На Linux номера считаются от `BTN_TRIGGER`: кнопка 0 — это курок (trigger), 1, 2, ... — следующие кнопки. Если не уверены, какой номер у нужной кнопки, запустите с `RUST_LOG=debug` — клиент пишет `joystick button N pressed`;
- выход: `q`, `Esc` или `Ctrl-C`.

### Текстовый режим (отладка)

```
target/release/atc-client --text
```

Микрофон, динамики и PTT не открываются. Печатаете реплику пилота и Enter — она уходит на частоту радиостанции, выбранной на передачу; ответ диспетчера печатается текстом. Выход — Ctrl-D. Удобно, чтобы проверить сервер и движок без голоса. Если X-Plane не запущен, клиент ответит `no telemetry from X-Plane yet (is it running? check xplane in the config) - line dropped`.

## 6. Запись трека и воспроизведение

```
target/release/atc-client --record track.csv        # можно вместе с --text
```

Пишется CSV (заголовок `t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz,xpdr_code,xpdr_mode`, строка на кадр телеметрии, кадры на паузе пропускаются; файл сбрасывается построчно, переживёт убийство клиента). `xpdr_mode` — как в X-Plane: 0 off, 1 standby, 2 on (в X-Plane это же и ALT), 3 test; пустое поле — неизвестно. Старые записи без двух последних колонок тоже читаются.

Воспроизведение через `atc-replay`: он принимает **сценарий** TOML, а не CSV. Скопируйте `scenarios/epkk-circuit-vmc.toml`, замените строку `track = "synthetic-circuit:25"` на `track_file = "track.csv"` (путь относительно папки сценария; ровно одно из `track` / `track_file`), при необходимости поправьте реплики `[[say]]` и погоду (транспондер задаётся `transponder = { code = 7000, mode = 2 }` — в начале сценария или в `[[say]]`, тогда пилот выставляет его перед репликой; пример — `scenarios/epkk-wrong-squawk.toml`), затем:

```
cargo run --release -p atc-replay -- scenarios/мой-сценарий.toml
```

Язык в сценарии: `language = "auto" | "en" | "pl"` в начале файла — режим языка, как `[whisper] language` (по умолчанию `"en"`), и `lang = "pl"` в `[[say]]` — язык, который определил бы Whisper (по умолчанию `"en"`); примеры — `scenarios/epkk-circuit-pl.toml` и `scenarios/epkk-language-switch.toml`.

Транскрипт печатается в stdout (формат `[время] КТО: текст`) и детерминирован; эталоны — `scenarios/*.expected.txt`. Проверка штатного сценария: `cargo run -q -p atc-replay -- scenarios/epkk-circuit-vmc.toml | diff - scenarios/epkk-circuit-vmc.expected.txt`.

## Частые проблемы

| Симптом | Причина |
|---|---|
| сервер выходит с кодом 2 | нет модели: `tools/fetch-whisper.sh` / `tools/fetch-piper.sh` |
| `no telemetry from X-Plane yet` | X-Plane не запущен, не тот `xplane` в `config/client.toml` или UDP 49000 блокирует файрвол |
| ошибка чтения `config/...` или `data/airports/...` | запуск не из корня репозитория |
| клиент не подключается из браузера/через `Origin` | так задумано, см. п. 4 |
| Ground называет рулёжки, которых нет в симуляторе | не задан `xplane_root` или X-Plane не перезапущен после установки сцены; см. `airport layout:` в логе сервера |
