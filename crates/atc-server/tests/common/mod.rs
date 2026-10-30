//! Test harness: the real router with FakeStt/FakeTts and EPKK data on 127.0.0.1:0, and a
//! tokio-tungstenite client with timeouts on every receive.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use atc_core::engine::{AtcEngine, EngineConfig, LanguageMode};
use atc_core::readback::Strictness;
use atc_core::testkit::circuit_track;
use atc_navdata::model::{Airport, Destination};
use atc_navdata::parse_apt_dat;
use atc_navdata::procedures::{Procedures, parse_procedures};
use atc_proto::{
    AudioFrame, CallsignSource, ClientMsg, PROTOCOL_VERSION, ServerMsg, TelemetryFrame,
    WeatherFrame,
};
use atc_server::app::{AppState, OwnerPrompts, StationMap, SttPrompts, router};
use atc_server::hub::Hub;
use atc_server::network::Network;
use atc_server::stations::LoadedStation;
use atc_voice::{FakeStt, FakeTts, TextToSpeech, Transcript};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub const GROUND: u32 = 118_105;
pub const TOWER: u32 = 123_255;
pub const ATIS: u32 = 126_130;
pub const DELIVERY: u32 = 121_980;
pub const TAXI_REQUEST: &str = "Kraków Ground, SP-ABC, information Alpha, request taxi";

const APT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/airports/EPKK/apt.dat"
);
const PROCS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/airports/EPKK/procedures.toml"
);
pub const STATION_FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/stations");
const TIMEOUT: Duration = Duration::from_secs(5);

pub fn epkk() -> (Airport, Procedures) {
    let ap = parse_apt_dat(&std::fs::read_to_string(APT).unwrap(), "EPKK").unwrap();
    let procs = parse_procedures(&std::fs::read_to_string(PROCS).unwrap()).unwrap();
    (ap, procs)
}

pub fn transcript(text: &str, confidence: f32) -> Transcript {
    transcript_in(text, "en", confidence)
}

/// A transcript Whisper tagged with language `lang`.
pub fn transcript_in(text: &str, lang: &str, confidence: f32) -> Transcript {
    Transcript {
        text: text.into(),
        lang: lang.into(),
        confidence,
    }
}

pub struct TestServer {
    pub url: String,
    pub state: AppState,
    pub stt: Arc<FakeStt>,
}

/// Starts a server whose STT replies with `stt` in order; TTS fails if `tts_fail`.
pub async fn start(stt: Vec<Transcript>, tts_fail: bool) -> TestServer {
    start_with_tts(
        stt,
        Arc::new(FakeTts {
            fail: tts_fail,
            ..FakeTts::default()
        }),
    )
    .await
}

/// Like [`start`] with a caller-supplied TTS.
pub async fn start_with_tts(stt: Vec<Transcript>, tts: Arc<dyn TextToSpeech>) -> TestServer {
    start_full(stt, tts, LanguageMode::Fixed(atc_phraseology::Lang::En)).await
}

/// Like [`start`] with the engine in `language` and a working [`FakeTts`] that records its
/// calls.
pub async fn start_in(stt: Vec<Transcript>, language: LanguageMode) -> (TestServer, Arc<FakeTts>) {
    let tts = Arc::new(FakeTts::default());
    (start_full(stt, tts.clone(), language).await, tts)
}

async fn start_full(
    stt: Vec<Transcript>,
    tts: Arc<dyn TextToSpeech>,
    language: LanguageMode,
) -> TestServer {
    start_with_destinations(stt, tts, language, vec![]).await
}

/// Like [`start`], the engine knows `destinations` but has no CIFP procedure set (the
/// install lacks it).
pub async fn start_without_cifp(destinations: Vec<Destination>) -> TestServer {
    let tts = Arc::new(FakeTts::default());
    start_with_destinations(
        vec![],
        tts,
        LanguageMode::Fixed(atc_phraseology::Lang::En),
        destinations,
    )
    .await
}

async fn start_with_destinations(
    stt: Vec<Transcript>,
    tts: Arc<dyn TextToSpeech>,
    language: LanguageMode,
    destinations: Vec<Destination>,
) -> TestServer {
    let (ap, procs) = epkk();
    let cfg = EngineConfig {
        language,
        ..EngineConfig::new(Strictness::Standard)
    };
    let engine = AtcEngine::new(ap.clone(), procs.clone(), cfg).with_destinations(destinations);
    let station = LoadedStation {
        id: "EPKK".into(),
        dir: PROCS.into(),
        procs,
        airport: ap,
        engine: Mutex::new(engine),
    };
    start_network(vec![station], stt, tts).await
}

/// The synthetic station `id` of `tests/fixtures/stations` with `extra` TOML appended to its
/// `procedures.toml` (more positions, adjacent units), in English.
pub fn fixture_station(id: &str, extra: &str) -> LoadedStation {
    let dir = std::path::Path::new(STATION_FIXTURES).join(id);
    let text = std::fs::read_to_string(dir.join("procedures.toml")).unwrap() + extra;
    let procs = parse_procedures(&text).unwrap();
    let apt = std::fs::read_to_string(dir.join("apt.dat")).unwrap();
    let airport = parse_apt_dat(&apt, id).unwrap();
    let cfg = EngineConfig {
        language: LanguageMode::Fixed(atc_phraseology::Lang::En),
        ..EngineConfig::new(Strictness::Standard)
    };
    let engine = AtcEngine::new(airport.clone(), procs.clone(), cfg);
    LoadedStation {
        id: id.into(),
        dir,
        procs,
        airport,
        engine: Mutex::new(engine),
    }
}

/// A server answering for `stations`, STT replying with `stt` in order.
pub async fn start_network(
    stations: Vec<LoadedStation>,
    stt: Vec<Transcript>,
    tts: Arc<dyn TextToSpeech>,
) -> TestServer {
    start_network_with(Network::new(stations), stt, tts, false).await
}

/// Like [`start_network`] for a prepared `network`; with `per_owner`, every station and unit
/// has its own speech-to-text prompt as in the real server (the fixed one stays the fallback).
pub async fn start_network_with(
    network: Network,
    stt: Vec<Transcript>,
    tts: Arc<dyn TextToSpeech>,
    per_owner: bool,
) -> TestServer {
    let stations = StationMap::union(network.stations().iter().map(|s| &s.procs));
    let fake_stt = Arc::new(FakeStt::default());
    fake_stt
        .reply
        .lock()
        .unwrap()
        .extend(stt.into_iter().map(Ok));
    let state_network = Arc::new(network);
    let state = AppState {
        network: state_network.clone(),
        hub: Arc::new(Mutex::new(Hub::default())),
        stt: fake_stt.clone(),
        tts,
        stations,
        stt_prompts: SttPrompts {
            en: "Kraków Ground".into(),
            pl: "Kraków, lotnisko".into(),
        },
        owner_prompts: if per_owner {
            OwnerPrompts::for_network(&state_network)
        } else {
            OwnerPrompts::default()
        },
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(state.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    TestServer {
        url: format!("ws://{addr}/ws"),
        state,
        stt: fake_stt,
    }
}

impl TestServer {
    /// Polls the hub until `freq_khz` has `n` listeners (cross-connection ordering).
    pub async fn wait_listeners(&self, freq_khz: u32, n: usize) {
        tokio::time::timeout(TIMEOUT, async {
            while self.state.hub.lock().unwrap().listeners(freq_khz).len() != n {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("listeners");
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum In {
    Msg(ServerMsg),
    Audio(AudioFrame),
    Closed,
}

pub struct Client {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl Client {
    pub async fn connect(url: &str) -> Client {
        let (ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        Client { ws }
    }

    /// Connects, says hello as `callsign` and expects `Welcome`.
    pub async fn hello(url: &str, callsign: &str) -> Client {
        let mut c = Client::connect(url).await;
        c.send(ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            callsign: callsign.into(),
            aircraft_type: "C172".into(),
            callsign_source: CallsignSource::Config,
        })
        .await;
        match c.recv().await {
            In::Msg(ServerMsg::Welcome { protocol, .. }) => assert_eq!(protocol, PROTOCOL_VERSION),
            other => panic!("expected welcome, got {other:?}"),
        }
        match c.recv().await {
            In::Msg(ServerMsg::Identified { .. }) => {}
            other => panic!("expected identified, got {other:?}"),
        }
        c
    }

    pub async fn send(&mut self, m: ClientMsg) {
        let s = serde_json::to_string(&m).unwrap();
        self.ws.send(Message::text(s)).await.unwrap();
    }

    pub async fn send_audio(&mut self, seq: u32, pcm: Vec<i16>) {
        let f = AudioFrame {
            stream: 0,
            seq,
            pcm,
        };
        self.ws.send(Message::binary(f.encode())).await.unwrap();
    }

    /// Next message (5 s timeout); pings and pongs are skipped.
    pub async fn recv(&mut self) -> In {
        loop {
            let next = tokio::time::timeout(TIMEOUT, self.ws.next())
                .await
                .expect("timed out waiting for the server");
            match next {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return In::Closed,
                Some(Ok(Message::Text(t))) => return In::Msg(serde_json::from_str(&t).unwrap()),
                Some(Ok(Message::Binary(b))) => return In::Audio(AudioFrame::decode(&b).unwrap()),
                Some(Ok(_)) => {}
            }
        }
    }

    /// Receives until `stop` matches; returns everything received, including the match.
    pub async fn recv_until(&mut self, stop: impl Fn(&In) -> bool) -> Vec<In> {
        let mut got = Vec::new();
        loop {
            let m = self.recv().await;
            let done = stop(&m) || m == In::Closed;
            got.push(m);
            if done {
                return got;
            }
        }
    }

    /// Weather, then telemetry parked at the testkit's stand on `com1_khz`.
    pub async fn weather_and_parked(&mut self, com1_khz: u32) {
        self.send(ClientMsg::Weather(weather())).await;
        self.send(ClientMsg::Telemetry(parked(com1_khz))).await;
    }
}

pub fn weather() -> WeatherFrame {
    WeatherFrame {
        wind_dir_deg: 250,
        wind_speed_kt: 8,
        visibility_m: 10_000,
        clouds: vec![],
        temp_c: 15,
        dewpoint_c: 10,
        qnh_hpa: 1013,
    }
}

/// The testkit track's first sample: parked at a stand with a route to runway 25.
pub fn parked(com1_khz: u32) -> TelemetryFrame {
    let (ap, procs) = epkk();
    let s = circuit_track(&ap, &procs, "25", 1.0).remove(0);
    TelemetryFrame {
        alt_ft_msl: s.alt_ft_msl,
        heading_deg: s.heading_deg,
        ..parked_at(com1_khz, s.pos.lat, s.pos.lon)
    }
}

/// Standing still on the ground at `lat`/`lon` on `com1_khz`.
pub fn parked_at(com1_khz: u32, lat: f64, lon: f64) -> TelemetryFrame {
    TelemetryFrame {
        t: 10.0,
        lat,
        lon,
        alt_ft_msl: 100.0,
        heading_deg: 0.0,
        ias_kt: 0.0,
        gs_kt: 0.0,
        vs_fpm: 0.0,
        on_ground: true,
        com1_khz,
        listen_khz: Vec::new(),
        xpdr_code: None,
        xpdr_mode: None,
        utc_s: None,
        day_of_year: None,
        paused: false,
    }
}

pub fn is_atc_text(m: &In) -> bool {
    matches!(m, In::Msg(ServerMsg::AtcText { .. }))
}

pub fn is_audio_end(m: &In) -> bool {
    matches!(m, In::Msg(ServerMsg::AtcAudioEnd { .. }))
}

pub fn atc_texts(msgs: &[In]) -> Vec<(String, String)> {
    msgs.iter()
        .filter_map(|m| match m {
            In::Msg(ServerMsg::AtcText { station, text, .. }) => {
                Some((station.clone(), text.clone()))
            }
            _ => None,
        })
        .collect()
}
