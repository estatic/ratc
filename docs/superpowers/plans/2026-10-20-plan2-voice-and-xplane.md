# План 2: голос, сервер/клиент и связь с X-Plane 11

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Пилот в X-Plane 11 нажимает PTT, говорит в микрофон, и диспетчер EPKK (ядро из плана 1) отвечает голосом на частоте COM1; всё работает локально на этой машине, архитектура — сервер + клиент, готовые к общему небу.

**Architecture:** `atc-proto` — версионированный протокол (JSON-сообщения + бинарные аудиокадры) поверх WebSocket. `atc-voice` — трейты STT/TTS, DSP (ресемплинг, радио-эффект), реализации Whisper (whisper-rs, Vulkan) и Piper (subprocess) за cargo-features. `atc-server` — tokio/axum: хаб частот («комнаты», блокировка PTT, ретрансляция), один общий `AtcEngine`, конвейер STT → engine → TTS. `atc-client` — UDP RREF к X-Plane 11 (только чтение), микрофон/динамики (cpal), PTT (кнопка джойстика через gilrs или клавиша в терминале), текстовый режим для отладки.

**Tech Stack:** Rust 1.98 (edition 2024); tokio, axum (ws), tokio-tungstenite, serde_json, cpal, gilrs, crossterm, hound, whisper-rs (feature `vulkan`), Piper 2023.11.14-2 (бинарник, subprocess), anyhow, tracing.

**Spec:** `docs/superpowers/specs/2026-10-19-virtual-atc-design.md` (§5.1–5.6). План 1 (ядро) уже влит: `crates/atc-core`, `atc-phraseology`, `atc-navdata`, `atc-geo`, `atc-replay`.

## Global Constraints

- Всё локально, без облака и интернета во время работы (интернет нужен только скриптам загрузки моделей).
- Задержка: от отпускания PTT до начала ответа < 2.5 с типично, < 3 с жёстко (spec §5.4).
- Бюджет видеопамяти на Whisper (+ LLM в плане 3) — ориентировочно ≤ 8 ГБ при запущенном X-Plane 11 на той же RX 7900 (spec §5.5).
- X-Plane 11 только читается (UDP RREF, порт 49000); в X-Plane ничего не пишем, плагин не нужен (spec §5.3).
- Сервер — единственный владелец мира; частота = комната; одновременное PTT на частоте блокируется; протокол версионирован (spec §5.6).
- Ядро решает детерминированно; в эфир не выдаётся ничего, чего нет в навданных/`procedures.toml` (наследуется из плана 1).
- `atc-proto`, логика хаба в `atc-server` и DSP в `atc-voice` — без I/O, тестируемы без моделей и без звуковой карты.
- Реальные Whisper/Piper — за cargo-features `whisper`/`piper`; `cargo test --workspace` без моделей обязан быть зелёным; тесты с моделями — `#[ignore]`, запускаются явно.
- Каждая задача: `cargo test --workspace --features atc-core/testkit` зелёный, `cargo clippy --workspace --all-targets -- -D warnings` чистый.

## Review Focus

1. X-Plane на паузе, перезапуск полёта или перестановка самолёта: время симулятора стоит или идёт назад, позиция прыгает. Ожидание — клиент не падает и не шлёт мусор, сервер не выдаёт ложных отклонений → тест `time_backwards_and_pause_are_forwarded_monotonic` в Task 9.
2. Whisper вернул пустую строку или «uh»/шум двигателя → пустой ответ без «say again» на пустоту; «say again» только на непустую неразборчивую фразу → тест `empty_and_noise_transcripts` в Task 7.
3. Клиент отвалился посреди PTT или сервер перезапустился → сервер снимает блокировку частоты; клиент переподключается с backoff → тесты `disconnect_mid_ptt_releases_lock` (Task 6), `client_reconnects_with_backoff` (Task 10).
4. TTS или STT упали (нет модели, Piper завершился с ошибкой) → пилот всё равно получает текст ответа (`AtcText`), ошибка в логе, сервер жив → тест `tts_failure_still_sends_text` в Task 7.
5. COM1 переключили во время передачи или на частоте нет станции → передача уходит в комнату той частоты, на которой нажали PTT; на «пустой» частоте — тишина без паники → тест `ptt_on_unstaffed_frequency_is_silent` в Task 7.

---

### Task 1: Протокол `atc-proto`

**Files:**
- Create: `crates/atc-proto/Cargo.toml` (deps: serde, serde_json, thiserror), `crates/atc-proto/src/lib.rs`
- Test: `crates/atc-proto/src/lib.rs` (модуль `tests`)

**Interfaces:**
- Produces:
  - `pub const PROTOCOL_VERSION: u16 = 1;`
  - `#[derive(Serialize, Deserialize)] #[serde(tag = "type", rename_all = "snake_case")] pub enum ClientMsg { Hello { protocol: u16, callsign: String, aircraft_type: String }, Telemetry(TelemetryFrame), Weather(WeatherFrame), PttStart { freq_khz: u32 }, PttEnd, TextTransmission { freq_khz: u32, text: String } }`
  - `pub struct TelemetryFrame { pub t: f64, pub lat: f64, pub lon: f64, pub alt_ft_msl: f64, pub heading_deg: f64, pub ias_kt: f64, pub gs_kt: f64, pub vs_fpm: f64, pub on_ground: bool, pub com1_khz: u32, pub paused: bool }`
  - `pub struct WeatherFrame { pub wind_dir_deg: u16, pub wind_speed_kt: u16, pub visibility_m: u32, pub clouds: Vec<CloudLayer>, pub temp_c: i16, pub dewpoint_c: i16, pub qnh_hpa: u16 }`, `pub struct CloudLayer { pub cover: String /* "few"|"sct"|"bkn"|"ovc" */, pub base_ft_agl: u32 }`
  - `#[serde(tag = "type", rename_all = "snake_case")] pub enum ServerMsg { Welcome { protocol: u16, session: String }, Rejected { reason: String }, AtcText { station: String, freq_khz: u32, text: String, utterance: u32 }, AtcAudioStart { utterance: u32, station: String, freq_khz: u32, sample_rate: u32, looping: bool }, AtcAudioEnd { utterance: u32 }, PilotTranscript { freq_khz: u32, callsign: Option<String>, text: String, lang: String }, FrequencyBlocked { freq_khz: u32 }, RelayStart { from: String, freq_khz: u32, sample_rate: u32 }, RelayEnd { from: String } }`
  - Бинарные кадры: `pub struct AudioFrame { pub stream: u32 /* utterance id или 0 = свой PTT */, pub seq: u32, pub pcm: Vec<i16> }` с `pub fn encode(&self) -> Vec<u8>` (заголовок: магия `b"ATCA"`, u32 stream LE, u32 seq LE, затем i16 LE) и `pub fn decode(bytes: &[u8]) -> Result<AudioFrame, ProtoError>`.
  - Решение (моё): в v1 аудио — PCM s16le моно (клиент → сервер 16 кГц, сервер → клиент частота голоса Piper, обычно 22050). Opus — в версии для интернета; поле `sample_rate` и магия кадра позволяют добавить кодек без поломки протокола.

- [ ] **Step 1: Тесты**: `client_msgs_roundtrip_json` (каждый вариант → JSON → обратно равен), `hello_json_shape` (`{"type":"hello","protocol":1,...}`), `audio_frame_roundtrip`, `audio_frame_rejects_bad_magic_and_odd_length`.
- [ ] **Step 2: `cargo test -p atc-proto`** — FAIL.
- [ ] **Step 3: Реализовать.**
- [ ] **Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(proto): versioned client/server protocol and audio frames`

### Task 2: `atc-voice` — трейты, PCM, ресемплинг, радио-эффект, WAV

**Files:**
- Create: `crates/atc-voice/Cargo.toml` (deps: thiserror, hound; features `whisper = ["dep:whisper-rs"]`, `piper = []`), `src/lib.rs`, `src/pcm.rs`, `src/dsp.rs`, `src/wav.rs`
- Test: в модулях

**Interfaces:**
- Produces:
  - `#[derive(Clone)] pub struct Pcm { pub sample_rate: u32, pub samples: Vec<i16> }` с `pub fn duration_s(&self) -> f64`
  - `pub fn resample(input: &Pcm, to_rate: u32) -> Pcm` (линейная интерполяция; для 48000/44100/22050 → 16000 достаточно)
  - `pub struct Transcript { pub text: String, pub lang: String, pub confidence: f32 }`
  - `pub trait SpeechToText: Send + Sync { fn transcribe(&self, pcm16k: &Pcm, prompt: &str) -> Result<Transcript, VoiceError>; }`
  - `#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)] pub enum VoiceId { Ground, Tower, Atis }`
  - `pub trait TextToSpeech: Send + Sync { fn synthesize(&self, voice: VoiceId, text: &str) -> Result<Pcm, VoiceError>; }`
  - `pub fn radio_effect(pcm: &Pcm, seed: u64) -> Pcm` — полосовой фильтр 300–3400 Гц (два биквада: HPF 300, LPF 3400, Q 0.707), мягкая компрессия/клиппинг, белый шум −30 dBFS (детерминированный ГПСЧ от `seed`).
  - `pub fn read_wav(path) -> Result<Pcm>`, `pub fn write_wav(path, &Pcm) -> Result<()>` (моно s16).
  - `pub struct FakeStt { pub reply: Mutex<VecDeque<Result<Transcript, VoiceError>>> }`, `pub struct FakeTts { pub fail: bool }` (FakeTts возвращает 0.1 с тишины на 22050 Гц на каждое слово) — `#[cfg(any(test, feature = "fakes"))]`, feature `fakes` для тестов сервера.
  - `VoiceError` — thiserror: `ModelMissing(String)`, `Engine(String)`, `Io(String)`.

- [ ] **Step 1: Тесты**: `resample_48k_to_16k_preserves_duration` (±1 мс), `resample_sine_keeps_frequency` (1 кГц синус после ресемплинга: число переходов через ноль ±2 %), `radio_effect_attenuates_low_and_high` (энергия 100 Гц и 6 кГц синусов падает ≥ 12 дБ, 1 кГц — не более 3 дБ), `radio_effect_deterministic_for_seed`, `wav_roundtrip`.
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(voice): pcm, resampling, radio effect, stt/tts traits`

### Task 3: Piper TTS

**Files:**
- Create: `tools/fetch-piper.sh` — скачивает `piper_linux_x86_64.tar.gz` из релиза `rhasspy/piper` `2023.11.14-2` в `models/piper/` и голоса из `huggingface.co/rhasspy/piper-voices` (`.onnx` + `.onnx.json`) в `models/piper/voices/`: три разных английских голоса medium (по одному на Ground, Tower, ATIS — выбор исполнителя, записать в `models/README.md`).
- Create: `crates/atc-voice/src/piper.rs` (`#[cfg(feature = "piper")]`)
- Create: `config/voices.toml` (сопоставление `VoiceId` → файл голоса, `length_scale` для темпа)
- Test: `crates/atc-voice/tests/piper_live.rs` (`#[ignore]`)

**Interfaces:**
- Produces: `pub struct PiperTts { … }` с `pub fn new(piper_bin: PathBuf, voices: HashMap<VoiceId, PiperVoice>) -> Result<Self, VoiceError>` (проверяет существование файлов → `ModelMissing`), `impl TextToSpeech` (subprocess: `piper --model <onnx> --output_raw --length_scale <x>`, текст в stdin, сырой s16le со stdout; `sample_rate` из `.onnx.json` → `audio.sample_rate`); `pub struct PiperVoice { pub model: PathBuf, pub length_scale: f32 }`; `pub fn load_voices_config(text: &str, base: &Path) -> Result<HashMap<VoiceId, PiperVoice>, VoiceError>`.

- [ ] **Step 1: Тесты**: `missing_model_is_model_missing` (без `#[ignore]`, работает без моделей), `voices_config_parses`; `#[ignore] piper_speaks_atc_phrase` (синтез «Sierra Bravo Charlie, cleared for take-off» → длительность 1–6 с, ненулевая энергия).
- [ ] **Step 2: FAIL. Step 3: Реализовать; запустить `tools/fetch-piper.sh`.**
- [ ] **Step 4: `cargo test -p atc-voice --features piper -- --include-ignored`** — PASS.
- [ ] **Step 5: Commit** `feat(voice): piper text-to-speech` (модели в git не коммитить — `/models/` уже в `.gitignore`)

### Task 4: Whisper STT

**Files:**
- Create: `tools/fetch-whisper.sh` — `ggml-large-v3-turbo-q5_0.bin` и `ggml-small.en-q5_1.bin` (запасная) из `huggingface.co/ggerganov/whisper.cpp` в `models/whisper/`.
- Create: `crates/atc-voice/src/whisper.rs` (`#[cfg(feature = "whisper")]`), workspace-dep `whisper-rs` с feature `vulkan`.
- Create: `crates/atc-voice/src/prompt.rs` — `pub fn initial_prompt(airport_name: &str, stations: &[String], callsigns: &[String]) -> String` (короткая строка с фразеологией: «Kraków Ground, Kraków Tower, runway two five, holding point, hold short, QNH, cleared for take-off, Sierra Papa Alpha Bravo Charlie…»; ≤ 200 токенов).
- Test: `crates/atc-voice/tests/whisper_live.rs` (`#[ignore]`)

**Interfaces:**
- Produces: `pub struct WhisperStt` с `pub fn new(model: &Path, language: Option<&str> /* None = автоопределение */) -> Result<Self, VoiceError>` и `impl SpeechToText` (вход 16 кГц, greedy, `no_context = true`, `single_segment = false`, язык — автоопределение, `lang` в `Transcript`, `confidence` = средняя вероятность токенов).

- [ ] **Step 1: Тесты**: `prompt_contains_stations_and_is_short` (без моделей); `#[ignore] tts_to_stt_roundtrip` — Piper синтезирует 5 фраз пилота из `scenarios/epkk-circuit-vmc.toml`, ресемплинг в 16 кГц, Whisper распознаёт, затем `atc_phraseology::parse_pilot` даёт тот же `intent` и те же `items`, что и исходный текст (4 из 5 минимум); `#[ignore] stt_rejects_silence` (3 с тишины → пустой текст или confidence < 0.3).
- [ ] **Step 2: FAIL. Step 3: Реализовать; `tools/fetch-whisper.sh`; сборка с Vulkan.**
- [ ] **Step 4: `cargo test -p atc-voice --features whisper,piper -- --include-ignored`** — PASS. Если Vulkan-сборка не собирается — остановиться и описать ошибку (NEEDS_CONTEXT), не переходить на CPU молча.
- [ ] **Step 5: Commit** `feat(voice): whisper speech-to-text with aviation prompt`

### Task 5: Замер задержки и видеопамяти

**Files:**
- Create: `crates/atc-voice/src/bin/voice-bench.rs` (`required-features = ["whisper", "piper"]`)
- Create: `docs/benchmarks/2026-10-voice.md` (результаты)

**Interfaces:**
- CLI: `voice-bench --whisper <model> --piper-config config/voices.toml [--runs 10]` печатает таблицу: модель, медиана/p95 времени STT на фразу 3 с, время TTS на реплику диспетчера, VRAM до/после загрузки модели. VRAM AMD читать из `/sys/class/drm/card*/device/mem_info_vram_used` (байты; выбрать карту с наибольшим `mem_info_vram_total`).

- [ ] **Step 1: Реализовать бинарник** (тестов нет — это измерительный инструмент; проверка — запуск).
- [ ] **Step 2: Запустить без X-Plane** для обеих моделей Whisper, записать в `docs/benchmarks/2026-10-voice.md`.
- [ ] **Step 3: Попросить пользователя запустить X-Plane 11 на EPKK** и повторить замер; записать. Правило выбора модели по умолчанию: large-v3-turbo, если STT p95 ≤ 1.2 с и VRAM X-Plane + Whisper оставляет ≥ 6 ГБ свободных; иначе small.en (и отметить, что польский/русский тогда потребуют multilingual small). Выбор — в `config/server.toml` (Task 8) и в документ.
- [ ] **Step 4: Commit** `docs: voice latency and VRAM benchmark`

### Task 6: Хаб частот (чистая логика)

**Files:**
- Create: `crates/atc-server/Cargo.toml` (deps: atc-proto, atc-core с feature testkit для dev, atc-voice с `fakes` для dev, tokio, axum (ws), serde, toml, anyhow, tracing), `crates/atc-server/src/lib.rs`, `src/hub.rs`
- Test: `crates/atc-server/src/hub.rs`

**Interfaces:**
- Produces:
  - `pub type ClientId = u64;`
  - `pub struct Hub` с `pub fn join(&mut self, id: ClientId, callsign: String) -> Result<(), HubError /* DuplicateCallsign */>`, `pub fn leave(&mut self, id) -> Vec<HubEvent>`, `pub fn tune(&mut self, id, freq_khz: u32)`, `pub fn ptt_start(&mut self, id, freq_khz) -> PttOutcome /* Granted | Blocked */`, `pub fn ptt_end(&mut self, id) -> Option<u32 /* freq */>`, `pub fn listeners(&self, freq_khz: u32) -> Vec<ClientId>` (кто настроен на частоту), `pub fn callsign(&self, id) -> Option<&str>`.
  - `pub enum HubEvent { PttReleased { freq_khz: u32, by: ClientId } }`
  - Правила: одна передача на частоту одновременно (второй → `Blocked`); PTT держит частоту, на которой нажали, даже если COM1 переключили; `leave` снимает PTT; позывные уникальны.

- [ ] **Step 1: Тесты**: `second_ptt_on_same_freq_is_blocked`, `ptt_on_other_freq_is_granted`, `ptt_keeps_original_freq_after_retune`, **`disconnect_mid_ptt_releases_lock`**, `listeners_follow_tuning`, `duplicate_callsign_rejected`.
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(server): frequency hub with PTT locking`

### Task 7: Сервер — WebSocket и конвейер речи (с подделками)

**Files:**
- Create: `crates/atc-server/src/session.rs` (обработка одного клиента), `src/pipeline.rs` (STT → engine → TTS), `src/atis.rs` (рассылка ATIS), `src/app.rs` (`pub fn router(state: AppState) -> axum::Router`, маршрут `GET /ws`)
- Test: `crates/atc-server/tests/ws.rs` (tokio-tungstenite клиент против `router` на `127.0.0.1:0`)

**Interfaces:**
- Consumes: `atc_core::engine::AtcEngine` (`register_pilot`, `on_weather`, `on_telemetry`, `on_transmission`), `AtcOutput { freq_khz, station, message, text }`, `atc_proto::*`, `atc_voice::{SpeechToText, TextToSpeech, VoiceId, radio_effect, resample}`, `Hub`.
- Produces:
  - `pub struct AppState { pub engine: Arc<Mutex<AtcEngine>>, pub hub: Arc<Mutex<Hub>>, pub stt: Arc<dyn SpeechToText>, pub tts: Arc<dyn TextToSpeech>, pub stations: StationMap, pub stt_prompt: String }`
  - `pub struct StationMap` — из `procedures.positions`: `fn voice_for(&self, freq_khz) -> Option<VoiceId>` (Ground/Tower/ATIS), `fn atis_khz(&self) -> Option<u32>`.
- Поведение:
  - `Hello` с другим `protocol` → `Rejected` и закрытие; иначе `Welcome`, `hub.join`, `engine.register_pilot`.
  - `Telemetry` → `hub.tune(com1)`; → `TelemetryFrame` в `AircraftState` (`SimTime(t)`), `engine.on_telemetry`; ответы → `emit`. Кадр с `paused = true` в ядро не передаётся.
  - `Weather` → `engine.on_weather` (источник погоды — первый подключившийся клиент; решение моё, для общего неба один источник).
  - `PttStart` → `hub.ptt_start`; `Blocked` → `FrequencyBlocked`; бинарные `AudioFrame` (stream 0) копятся в буфер и ретранслируются (`RelayStart`/кадры/`RelayEnd`) другим слушателям частоты; `PttEnd` → буфер 16 кГц → `stt.transcribe` в `spawn_blocking` → `PilotTranscript` отправителю → если текст пустой или confidence < 0.3 — ничего; иначе `engine.on_transmission(freq, text, t)` → `emit`.
  - `TextTransmission` — тот же путь без STT (для отладки и тестов).
  - `emit(output)`: всем `listeners(freq)` — `AtcText`; затем `tts.synthesize(voice_for(freq), text)` → `radio_effect` → `AtcAudioStart` + кадры (stream = utterance) + `AtcAudioEnd`. Ошибка TTS → только текст + `tracing::error!`.
  - ATIS: при смене буквы или при настройке клиента на частоту ATIS — синтез текста ATIS один раз на букву (кеш), `AtcAudioStart { looping: true }`; клиент сам крутит петлю, пока настроен.
  - Логирование задержек `tracing::info!(stt_ms, engine_ms, tts_ms)` на каждую передачу.

- [ ] **Step 1: Тесты** (FakeStt/FakeTts, реальные данные EPKK): `hello_wrong_protocol_rejected`, `text_taxi_request_gets_text_and_audio` (`TextTransmission` «Kraków Ground, SP-ABC, information Alpha, request taxi…» после погоды и телеметрии на стоянке → `AtcText` от «Kraków Ground» с «taxi to holding point» + `AtcAudioStart`…`AtcAudioEnd`), `ptt_audio_goes_through_stt` (FakeStt возвращает ту же фразу), **`empty_and_noise_transcripts`** (FakeStt → "" и "uh" с confidence 0.1 → ни одного `AtcText`; непустая неразборчивая фраза с confidence 0.9 → ответ «say again» от ядра, если ядро так отвечает, иначе тишина — зафиксировать фактическое поведение ядра), **`tts_failure_still_sends_text`**, **`ptt_on_unstaffed_frequency_is_silent`** (PTT на 121.500 → нет `AtcText`, нет паники), `second_client_hears_relay_and_atc` (два клиента на 118.105: второй получает `RelayStart` от первого и `AtcText`), `atis_loops_on_atis_freq` (телеметрия с com1 = 126130 → `AtcAudioStart { looping: true, station: "Kraków ATIS"… }` — имя станции из `procedures.toml`).
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(server): websocket sessions and speech pipeline`

### Task 8: Бинарник сервера и конфигурация

**Files:**
- Create: `crates/atc-server/src/main.rs`, `config/server.toml`
- Test: `crates/atc-server/src/config.rs` (модуль `tests`)

**Interfaces:**
- `config/server.toml`: `listen = "127.0.0.1:8740"`, `airport = "EPKK"`, `data_dir = "data/airports"`, `strictness = "standard"|"training"`, `[whisper] model = "models/whisper/…"`, `[piper] bin = "models/piper/piper/piper"`, `voices = "config/voices.toml"`.
- `pub struct ServerConfig` + `pub fn load_config(text: &str) -> Result<ServerConfig>`.
- `main`: читает `--config` (по умолчанию `config/server.toml`), грузит навданные и процедуры, создаёт `AtcEngine`, `WhisperStt`, `PiperTts` (features включены в бинарнике по умолчанию: `default = ["whisper", "piper"]` у бинарника; для тестов workspace — без них через `--no-default-features` не требуется, т.к. тесты используют библиотеку), `axum::serve`. Отсутствующая модель → понятная ошибка с подсказкой `tools/fetch-*.sh` и выход с кодом 2.

- [ ] **Step 1: Тесты**: `config_parses_example`, `missing_model_error_mentions_fetch_script`.
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS; ручной запуск `cargo run -p atc-server --release` — в логе «listening on 127.0.0.1:8740».**
- [ ] **Step 5: Commit** `feat(server): binary and configuration`

### Task 9: Клиент — X-Plane 11 UDP RREF

**Files:**
- Create: `crates/atc-client/Cargo.toml` (deps: atc-proto, tokio, serde, toml, anyhow, tracing), `src/lib.rs`, `src/xplane.rs`, `src/datarefs.rs`
- Test: `crates/atc-client/src/xplane.rs` + `crates/atc-client/tests/xplane_fake.rs` (фальшивый X-Plane на локальном UDP-сокете)

**Interfaces:**
- Протокол RREF: запрос — `b"RREF\0"` + i32 freq_hz LE + i32 index LE + имя dataref, дополненное нулями до 400 байт, на `xplane_host:49000`; ответ — `b"RREF"` + 1 байт + повторы (i32 index LE, f32 value LE). Отписка — тот же запрос с freq 0 (при выходе).
- `datarefs.rs`: список подписок (индекс → назначение). **Имена проверить по `Resources/plugins/DataRefs.txt` установки X-Plane 11** (`/home/estatic/.local/share/Steam/steamapps/common/X-Plane 11`), ожидаемые: `sim/flightmodel/position/latitude`, `…/longitude`, `…/elevation` (м), `…/psi` (истинный курс), `…/indicated_airspeed` (kt), `…/groundspeed` (м/с), `…/vh_ind_fpm`, `sim/flightmodel/failures/onground_any`, `sim/cockpit2/radios/actuators/com1_frequency_hz_833` (кГц), `sim/time/total_running_time_sec`, `sim/time/paused`, погода: `sim/weather/wind_direction_degt[0]`, `sim/weather/wind_speed_kt[0]`, `sim/weather/barometer_sealevel_inhg`, `sim/weather/visibility_reported_m`, `sim/weather/cloud_coverage[0..2]`, `sim/weather/cloud_base_msl_m[0..2]`, `sim/weather/temperature_sealevel_c`, `sim/weather/dewpoi_sealevel_c`. Если имя или единицы отличаются — использовать то, что в DataRefs.txt, и записать в отчёт.
- Produces:
  - `pub fn encode_rref_request(freq_hz: i32, index: i32, name: &str) -> [u8; 413]`
  - `pub fn decode_rref(packet: &[u8]) -> Result<Vec<(i32, f32)>, XpError>`
  - `pub struct XPlaneState` с `pub fn apply(&mut self, values: &[(i32, f32)])`, `pub fn telemetry(&self) -> Option<TelemetryFrame>` (None, пока не пришли все обязательные значения), `pub fn weather(&self, field_elev_ft: f64) -> Option<WeatherFrame>` (QNH = inHg × 33.8639, округление; облака: coverage → few/sct/bkn/ovc, base MSL м → ft AGL).
  - `pub async fn run_xplane(cfg: XpConfig, tx: mpsc::Sender<XpUpdate>) -> anyhow::Result<()>` — подписка 5 Гц на телеметрию, 0.2 Гц на погоду; переподписка, если 5 с нет пакетов (X-Plane перезапущен).
  - Время: `t` в `TelemetryFrame` — монотонное время клиента, которое продвигается только когда `paused == false` и `total_running_time_sec` растёт; если оно пошло назад (перезапуск полёта) — `t` продолжает расти от последнего значения, а скачок позиции обработает ядро как разрыв телеметрии.

- [ ] **Step 1: Тесты**: `rref_request_layout` (длина 413, нули после имени), `decode_rref_multiple_values`, `decode_rejects_short_or_wrong_header`, `state_needs_all_required_values`, `qnh_from_inhg` (29.92 → 1013), `cloud_coverage_mapping`, **`time_backwards_and_pause_are_forwarded_monotonic`**, `fake_xplane_end_to_end` (фальшивый UDP-сервер отвечает на подписки пакетами для EPKK стоянки → `run_xplane` выдаёт `TelemetryFrame` с com1 118105).
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(client): X-Plane 11 UDP dataref reader`

### Task 10: Клиент — звук, PTT, соединение, текстовый режим

**Files:**
- Create: `crates/atc-client/src/audio.rs` (cpal: захват → 16 кГц моно кадры 20 мс; воспроизведение очереди `Pcm`, петля ATIS), `src/ptt.rs` (gilrs кнопка джойстика по номеру из конфига; клавиша пробел в терминале через crossterm raw mode), `src/connection.rs` (WebSocket с переподключением), `src/main.rs`, `config/client.toml`
- Test: `crates/atc-client/src/connection.rs`, `crates/atc-client/tests/client_server.rs` (клиентская логика против настоящего `atc_server::app::router` с FakeStt/FakeTts и фальшивым X-Plane)

**Interfaces:**
- `config/client.toml`: `server = "ws://127.0.0.1:8740/ws"`, `callsign = "SP-ABC"`, `aircraft_type = "C172"`, `xplane = "127.0.0.1:49000"`, `[ptt] joystick_button = 0`, `key = "space"`, `input_device`/`output_device` (опционально, по имени).
- `connection.rs`: `pub struct Backoff` (0.5 → 1 → 2 → 4 → 8 с, максимум 8 с, сброс после успешного `Welcome`), `pub async fn run_connection(cfg, from_ui: mpsc::Receiver<ClientMsg+audio>, to_ui: mpsc::Sender<ServerEvent>)`; после переподключения — повторный `Hello`.
- Воспроизведение: `AtcAudio*` для частоты ≠ текущей COM1 — не играть (сервер шлёт по частотам, но COM1 мог смениться); петля ATIS останавливается при перенастройке.
- Текстовый режим: `atc-client --text` — строки из stdin уходят как `TextTransmission` на текущую COM1; ответы печатаются (`[Kraków Ground] …`). PTT, микрофон и динамики в этом режиме не открываются.
- Запись трека: `atc-client --record <file.csv>` — пишет телеметрию в CSV формата `atc-replay` (`t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz`); в этой же задаче реализовать `track_file` в `crates/atc-replay` (сейчас «not supported yet»).

- [ ] **Step 1: Тесты**: `backoff_sequence_and_reset`, **`client_reconnects_with_backoff`** (сервер закрыт → клиент пытается по расписанию; сервер поднят → `Welcome`, повторный `Hello`), `text_mode_roundtrip` (клиент в текстовом режиме + фальшивый X-Plane + сервер с подделками: «request taxi» → напечатан ответ Ground), `audio_for_other_freq_not_played` (логика выбора, без звуковой карты), `replay_track_file_csv` (в `atc-replay`: CSV из 3 строк читается в `AircraftState`).
- [ ] **Step 2: FAIL. Step 3: Реализовать. Step 4: PASS.**
- [ ] **Step 5: Commit** `feat(client): audio, push-to-talk, reconnecting connection, text mode`

### Task 11: Приёмка в X-Plane 11 и документация запуска

**Files:**
- Create: `docs/RUNNING.md` (установка моделей, настройка X-Plane: Settings → Network не нужен для RREF; запуск сервера и клиента; назначение PTT; текстовый режим; запись трека)
- Create: `scenarios/recorded/` (записанные треки, если пользователь согласен их коммитить)

- [ ] **Step 1: Текстовый режим с реальным X-Plane**: пользователь ставит C172 на стоянку EPKK; `atc-client --text`; пройти круг, набирая реплики пилота текстом. Проверка: ответы Ground/Tower совпадают по смыслу с golden-транскриптом `scenarios/epkk-circuit-vmc.expected.txt`, нет ложных отклонений.
- [ ] **Step 2: Голосовой режим**: тот же круг голосом с PTT. Записать в `docs/benchmarks/2026-10-voice.md` фактическую задержку (из логов сервера `stt_ms + engine_ms + tts_ms`) для 10 передач; цель < 2.5 с типично, < 3 с максимум. Если цель не достигнута — записать, что именно медленное, и сообщить пользователю (не подгонять цифры).
- [ ] **Step 3: Запись трека** этого полёта `--record`, прогон через `atc-replay` с `track_file` — транскрипт сохраняется как новый golden, если пользователь согласен.
- [ ] **Step 4: Commit** `docs: running guide and acceptance results`
