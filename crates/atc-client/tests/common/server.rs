//! The real `atc_server` router with FakeStt/FakeTts and EPKK data on 127.0.0.1.

use std::sync::{Arc, Mutex};

use atc_core::engine::{AtcEngine, EngineConfig};
use atc_core::readback::Strictness;
use atc_core::state::AircraftState;
use atc_core::testkit::circuit_track;
use atc_navdata::parse_apt_dat;
use atc_navdata::procedures::parse_procedures;
use atc_server::app::{AppState, StationMap, SttPrompts, router};
use atc_server::hub::Hub;
use atc_server::network::Network;
use atc_server::stations::LoadedStation;
use atc_voice::{FakeStt, FakeTts};

pub const GROUND: u32 = 118_105;
pub const TAXI_REQUEST: &str = "Kraków Ground, SP-ABC, information Alpha, request taxi";

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/airports/EPKK");

fn epkk() -> (
    atc_navdata::model::Airport,
    atc_navdata::procedures::Procedures,
) {
    let read = |f: &str| std::fs::read_to_string(format!("{DATA}/{f}")).unwrap();
    (
        parse_apt_dat(&read("apt.dat"), "EPKK").unwrap(),
        parse_procedures(&read("procedures.toml")).unwrap(),
    )
}

/// The testkit's first sample: parked at a stand with a route to runway 25.
pub fn parked() -> AircraftState {
    let (ap, procs) = epkk();
    circuit_track(&ap, &procs, "25", 1.0).remove(0)
}

pub struct TestServer {
    pub url: String,
    pub state: AppState,
}

/// Serves on `listener` (STT never used: text mode only).
pub async fn start_on(listener: tokio::net::TcpListener) -> TestServer {
    let (ap, procs) = epkk();
    let stations = StationMap::new(&procs);
    let engine = AtcEngine::new(
        ap.clone(),
        procs.clone(),
        EngineConfig::new(Strictness::Standard),
    );
    let station = LoadedStation {
        id: "EPKK".into(),
        dir: DATA.into(),
        procs,
        airport: ap,
        engine: Mutex::new(engine),
    };
    let state = AppState {
        network: Arc::new(Network::new(vec![station])),
        hub: Arc::new(Mutex::new(Hub::default())),
        stt: Arc::new(FakeStt::default()),
        tts: Arc::new(FakeTts::default()),
        stations,
        stt_prompts: SttPrompts {
            en: "Kraków Ground".into(),
            pl: "Kraków, lotnisko".into(),
        },
    };
    let addr = listener.local_addr().unwrap();
    let app = router(state.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    TestServer {
        url: format!("ws://{addr}/ws"),
        state,
    }
}

pub async fn start() -> TestServer {
    start_on(tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap()).await
}
