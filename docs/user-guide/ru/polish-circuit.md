# Полёт по кругу на польском (EPKK, ВПП 25)

Пошаговый круг над Краковом (EPKK) с радиообменом на польском: руление, взлёт, круг с полной остановкой, посадка, руление на стоянку. Реплики пилота ниже взяты дословно из сценария `scenarios/epkk-circuit-pl.toml`, ответы диспетчера — из его эталона `scenarios/epkk-circuit-pl.expected.txt`, поэтому всё, что здесь написано, проверяется тестами (`cargo test -p atc-replay`).

Позывной в примерах — SP-ABC (`Sierra Papa Alfa Bravo Charlie`, сокращённо `Sierra Bravo Charlie`). Обратите внимание: в польском варианте алфавита ИКАО буква A — «Alfa», а не «Alpha».

## 1. Настройка

### Язык

В `config/server.toml`, секция `[whisper]`:

```toml
[whisper]
model = "models/whisper/ggml-large-v3-turbo-q5_0.bin"
language = "pl"
```

- `language = "pl"` — весь обмен на польском: подсказка Whisper, разбор реплик, ответы диспетчера, ATIS и польские голоса. Рекомендуется, если летаете только по-польски.
- Строку `language` убрать — автоопределение: каждому борту отвечают на языке его последней понятой реплики (до первой — по-английски). Можно начать по-польски с Ground и перейти на английский с Tower — диспетчер переключится вслед за вами (пример — `scenarios/epkk-language-switch.toml`). Короткое «roger» с акцентом, которое Whisper принял за польское, язык не переключает.
- `language = "en"` — только английский (значение по умолчанию в конфиге).

Модель Whisper нужна многоязычная (`large-v3-turbo`, по умолчанию); `small.en` понимает только английский.

### Польские голоса

```
tools/fetch-piper.sh
```

Скрипт скачивает и английские, и польские голоса Piper в `models/piper/voices/`: Ground — `pl_PL-darkman-medium`, Tower — `pl_PL-mc_speech-medium`, ATIS — `pl_PL-gosia-medium`. Соответствие задано в секциях `[pl.ground]`, `[pl.tower]`, `[pl.atis]` файла `config/voices.toml`; если польского голоса нет, станция говорит английским голосом. После изменения конфигурации перезапустите `atc-server`.

### Частоты EPKK

| Станция | Частота | По-польски |
|---|---|---|
| Ground | 118.105 | `Kraków Ground` |
| Tower | 123.255 | `Kraków Wieża` (диспетчер произносит «sto dwadzieścia trzy kropka dwa pięć pięć») |
| ATIS | 126.130 | в режиме `pl` передаётся по-польски |

Станция, с которой вы говорите, определяется частотой радиостанции, выбранной на передачу на аудиопанели (COM1 MIC или COM2 MIC; по умолчанию COM1). Ответы слышны на ней и на любой другой COM, включённой на прослушивание: например, ATIS на COM2, пока вы работаете с Tower на COM1.

## 2. Как читать числа

- Номер ВПП, транспондер, рулёжки и цифры позывного — по цифрам: «dwa pięć», «cztery dwa zero jeden».
- QNH — двумя двузначными числами: 1013 — «dziesięć trzynaście».
- Частоты — целая часть числом, после «kropka» — по цифрам: 123.255 — «sto dwadzieścia trzy kropka dwa pięć pięć».
- Ветер диспетчер даёт числами: «wiatr dwieście pięćdziesiąt stopni osiem węzłów».
- Польские буквы можно не выговаривать идеально: «koluje», «sklok» без диакритики тоже понимаются.

## 3. Круг шаг за шагом

### Стоянка, Ground 118.105 — запрос руления

Пилот:

> Kraków Ground, Sierra Papa Alfa Bravo Charlie, dzień dobry, proszę o kołowanie do kręgów

Ground:

> Sierra Papa Alfa Bravo Charlie, dzień dobry, aktualna informacja Alfa, QNH dziesięć trzynaście, kołuj do punktu oczekiwania przed pasem dwa pięć drogą Bravo pięć, Bravo sześć, Alfa, oczekuj przed pasem dwa pięć, skłok cztery dwa zero jeden

Вы не назвали букву ATIS («informacja Alfa»), поэтому Ground сообщает актуальную информацию и QNH сам. Если ATIS прослушан, добавьте в запрос «informacja Alfa».

### Квитанция разрешения на руление

Пилот (повторить маршрут, ожидание перед ВПП, QNH и код; затем выставить транспондер 4201, режим ALT):

> Kołuję do punktu oczekiwania przed pasem dwa pięć drogą Bravo pięć, Bravo sześć, Alfa, oczekuję przed pasem dwa pięć, QNH dziesięć trzynaście, skłok cztery dwa zero jeden, Sierra Bravo Charlie

Ground молчит — квитанция принята. Ошибка в квитанции — диспетчер ответит «nieprawidłowo» и повторит разрешение, неполная квитанция — «powtórz …» с тем, чего не хватает.

### Предварительный старт — переход на Tower

Когда вы остановились перед ВПП, Ground:

> Sierra Bravo Charlie, łączność z Kraków Wieża sto dwadzieścia trzy kropka dwa pięć pięć, miłego lotu

Пилот:

> Łączność z Kraków Wieża sto dwadzieścia trzy kropka dwa pięć pięć, Sierra Bravo Charlie

Переставьте COM1 на 123.255.

### Tower 123.255 — взлёт

После перехода от Ground первый вызов делает не пилот, а Tower (AIP AD 2.20 §2.2). Как только вы стоите перед ВПП и COM1 стоит на 123.255, Tower сам вызывает вас со взлётным разрешением; ничего говорить не нужно. Если вы всё же вызовете Tower («Kraków Wieża, Sierra Papa Alfa Bravo Charlie, punkt oczekiwania pas dwa pięć, gotowy do startu»), ответ тот же, а после разрешения повторный вызов остаётся без ответа.

Tower сам:

> Sierra Papa Alfa Bravo Charlie, wiatr dwieście pięćdziesiąt stopni osiem węzłów, pas dwa pięć, zezwalam startować, zgłoś z wiatrem

Пилот:

> Pas dwa pięć, zezwalasz startować, zgłoszę z wiatrem, Sierra Bravo Charlie

Взлетайте.

### Подветренная (z wiatrem) — посадка с полной остановкой

Пилот:

> Sierra Bravo Charlie, z wiatrem, z pełnym zatrzymaniem

Tower:

> Sierra Bravo Charlie, numer jeden, zgłoś prostą

Пилот:

> Numer jeden, zgłoszę prostą, Sierra Bravo Charlie

Без «z pełnym zatrzymaniem» Tower на прямой разрешит конвейер («zezwalam konwojer»).

### Прямая (prosta) — разрешение на посадку

Пилот:

> Sierra Bravo Charlie, prosta

Tower:

> Sierra Bravo Charlie, wiatr dwieście pięćdziesiąt stopni osiem węzłów, pas dwa pięć, zezwalam lądować

Пилот:

> Pas dwa pięć, zezwalasz lądować, Sierra Bravo Charlie

### После посадки — освобождение ВПП

Когда скорость упала, Tower:

> Sierra Bravo Charlie, zwolnij pas, łączność z Kraków Ground sto osiemnaście kropka jeden zero pięć, do widzenia

Пилот:

> Zwalniam pas, łączność z Kraków Ground sto osiemnaście kropka jeden zero pięć, Sierra Bravo Charlie

Освободите ВПП и переставьте COM1 на 118.105.

### Ground 118.105 — руление на стоянку

Пилот:

> Kraków Ground, Sierra Bravo Charlie, proszę o kołowanie do stanowiska

Ground:

> Sierra Bravo Charlie, kołuj do stanowiska trzy pięć drogą Charlie, Bravo cztery

Пилот:

> Kołuję do stanowiska trzy pięć drogą Charlie, Bravo cztery, Sierra Bravo Charlie

### На стоянке

Пилот:

> Kraków Ground, Sierra Bravo Charlie, na stanowisku, do widzenia

Ground:

> Sierra Bravo Charlie, do widzenia

Если самолёт ещё не на стоянке, Ground подскажет, где она: «stanowisko trzy pięć jest dwadzieścia metrów przed tobą» (przed tobą — впереди, po twojej prawej — справа, za tobą — сзади, po twojej lewej — слева).

## 4. Полезные фразы

| Пилот | Значение |
|---|---|
| `proszę powtórzyć` / `powtórz` | повторите |
| `przyjąłem` | принял (roger) |
| `zgubiłem się` / `proszę o pomoc` | заблудился, нужна помощь с рулением |
| `proszę o follow me` | прошу машину сопровождения |
| `stoję` | стою на месте |
| `przechodzę na drugi krąg` | ухожу на второй круг |

Полный список польских фраз пилота и диспетчера — `docs/phraseology/pl.md`.
