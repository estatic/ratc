//! Shared server state and the axum router (`GET /ws`).
//!
//! Locking: the network's engines, `hub` and the internal maps are `std::sync::Mutex`es, locked only for
//! short synchronous sections and never held across an `.await` (STT/TTS run in
//! `spawn_blocking` with no lock held).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use atc_core::engine::VehicleOrder;
use atc_navdata::enroute::EnrouteUnitDef;
use atc_navdata::model::FreqKind;
use atc_navdata::procedures::Procedures;
use atc_phraseology::Lang;
use atc_proto::{AudioFrame, ServerMsg};
use atc_voice::{SpeechToText, TextToSpeech, VoiceId};
use axum::Router;
use axum::extract::State;
use axum::extract::ws::WebSocketUpgrade;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tokio::sync::mpsc;

use crate::atis::AtisCache;
use crate::hub::{ClientId, Hub};
use crate::network::Network;

#[derive(Clone)]
pub struct AppState {
    pub network: Arc<Network>,
    pub hub: Arc<Mutex<Hub>>,
    pub stt: Arc<dyn SpeechToText>,
    pub tts: Arc<dyn TextToSpeech>,
    pub stations: StationMap,
    pub stt_prompts: SttPrompts,
}

/// The static Whisper prompt ([`atc_voice::initial_prompt`]) in each language.
#[derive(Clone, Debug, PartialEq)]
pub struct SttPrompts {
    pub en: String,
    pub pl: String,
}

impl SttPrompts {
    pub fn get(&self, lang: Lang) -> &str {
        match lang {
            Lang::En => &self.en,
            Lang::Pl => &self.pl,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Station {
    khz: u32,
    name: String,
    voice: VoiceId,
}

/// Staffed frequencies from `procedures.positions` (ATIS, Delivery, Ground, Tower, Approach),
/// the alternate Approach frequencies (same station) and the adjacent units (Radar voice,
/// their own callsigns). Other roles are unstaffed.
#[derive(Clone, Debug, PartialEq)]
pub struct StationMap {
    stations: Vec<Station>,
}

impl StationMap {
    pub fn new(procs: &Procedures) -> Self {
        Self::union([procs])
    }

    /// The frequencies of several stations: every station's positions first, then the
    /// adjacent units they emulate; of two entries on the same kHz the first is kept (a real
    /// station's voice wins over an emulated unit's).
    pub fn union<'a>(all: impl IntoIterator<Item = &'a Procedures>) -> Self {
        Self::union_with_units(all, &[])
    }

    /// [`StationMap::union`] with the en-route units (Radar voice, FIS and ACC alike) after
    /// the positions and before the emulated adjacent units: a real unit wins over a
    /// station's emulation of it.
    pub fn union_with_units<'a>(
        all: impl IntoIterator<Item = &'a Procedures>,
        units: &[EnrouteUnitDef],
    ) -> Self {
        let all: Vec<&Procedures> = all.into_iter().collect();
        let mut stations = Vec::new();
        for procs in &all {
            stations.extend(Self::positions(procs));
        }
        stations.extend(units.iter().map(|u| Station {
            khz: u.khz,
            name: u.callsign.clone(),
            voice: VoiceId::Radar,
        }));
        for procs in &all {
            stations.extend(procs.adjacent.iter().map(|u| Station {
                khz: u.khz,
                name: u.callsign.clone(),
                voice: VoiceId::Radar,
            }));
        }
        let mut seen = std::collections::HashSet::new();
        stations.retain(|s| seen.insert(s.khz));
        StationMap { stations }
    }

    /// The staffed positions of one station and its alternate Approach frequencies.
    fn positions(procs: &Procedures) -> Vec<Station> {
        let mut stations: Vec<Station> = procs
            .positions
            .iter()
            .filter_map(|p| {
                let voice = match p.role {
                    FreqKind::Atis => VoiceId::Atis,
                    FreqKind::Delivery => VoiceId::Delivery,
                    FreqKind::Ground => VoiceId::Ground,
                    FreqKind::Tower => VoiceId::Tower,
                    FreqKind::Approach => VoiceId::Approach,
                    _ => return None,
                };
                Some(Station {
                    khz: p.khz,
                    name: p.callsign.clone(),
                    voice,
                })
            })
            .collect();
        let approach = stations
            .iter()
            .find(|s| s.voice == VoiceId::Approach)
            .map(|s| s.name.clone());
        if let Some(name) = approach {
            stations.extend(procs.approach_alt_khz.iter().map(|&khz| Station {
                khz,
                name: name.clone(),
                voice: VoiceId::Approach,
            }));
        }
        stations
    }

    /// The published name of the station on the same carrier as `freq_khz` (a 25 kHz
    /// radio tunes "118.105" as 118.100), or `freq_khz` itself when none is.
    pub fn canonical(&self, freq_khz: u32) -> u32 {
        self.stations
            .iter()
            .find(|s| atc_proto::same_channel(s.khz, freq_khz))
            .map_or(freq_khz, |s| s.khz)
    }

    fn get(&self, freq_khz: u32) -> Option<&Station> {
        self.stations.iter().find(|s| s.khz == freq_khz)
    }

    /// `None` for an unstaffed frequency.
    pub fn voice_for(&self, freq_khz: u32) -> Option<VoiceId> {
        self.get(freq_khz).map(|s| s.voice)
    }

    /// Station callsign on `freq_khz`, e.g. `"Kraków ATIS"`.
    pub fn station_name(&self, freq_khz: u32) -> Option<&str> {
        self.get(freq_khz).map(|s| s.name.as_str())
    }
}

/// What a session's writer task sends to its socket.
#[derive(Clone, Debug)]
pub(crate) enum Out {
    Msg(ServerMsg),
    Audio(AudioFrame),
    /// Close the socket and stop writing.
    Close,
}

/// Unbounded so that emitting to other clients never blocks the sender.
pub(crate) type Tx = mpsc::UnboundedSender<Out>;

/// `AppState` plus runtime bookkeeping, shared by all sessions.
pub(crate) struct Server {
    pub(crate) state: AppState,
    next_client: AtomicU64,
    next_utterance: AtomicU32,
    clients: Mutex<HashMap<ClientId, Tx>>,
    /// The client whose weather feeds the engine (the first connected one).
    pub(crate) weather_source: Mutex<Option<ClientId>>,
    pub(crate) atis: AtisCache,
    /// The controller's last words per frequency, appended to the STT prompt so a readback
    /// is heard against the clearance it repeats.
    last_said: Mutex<HashMap<u32, String>>,
}

impl Server {
    fn new(state: AppState) -> Self {
        Server {
            state,
            next_client: AtomicU64::new(1),
            // 0 is the client's own PTT stream.
            next_utterance: AtomicU32::new(1),
            clients: Mutex::new(HashMap::new()),
            weather_source: Mutex::new(None),
            atis: AtisCache::default(),
            last_said: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn remember_said(&self, freq_khz: u32, text: &str) {
        self.last_said
            .lock()
            .unwrap()
            .insert(freq_khz, text.to_string());
    }

    /// The static aviation prompt in `lang` plus the last controller transmission on
    /// `freq_khz`.
    pub(crate) fn stt_prompt(&self, freq_khz: u32, lang: Lang) -> String {
        let prompt = self.state.stt_prompts.get(lang);
        match self.last_said.lock().unwrap().get(&freq_khz) {
            Some(said) => format!("{prompt} {said}"),
            None => prompt.to_string(),
        }
    }

    /// Unique for the server's lifetime (the hub must never see an id twice).
    pub(crate) fn next_client_id(&self) -> ClientId {
        self.next_client.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn next_utterance(&self) -> u32 {
        self.next_utterance.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn add_client(&self, id: ClientId, tx: Tx) {
        self.clients.lock().unwrap().insert(id, tx);
    }

    pub(crate) fn remove_client(&self, id: ClientId) {
        self.clients.lock().unwrap().remove(&id);
    }

    /// Queues `out` for each of `ids`; unknown or disconnected ids are skipped.
    pub(crate) fn send(&self, ids: &[ClientId], out: Out) {
        let clients = self.clients.lock().unwrap();
        for id in ids {
            if let Some(tx) = clients.get(id) {
                let _ = tx.send(out.clone());
            }
        }
    }

    /// Each vehicle order to the client of its aircraft; orders for aircraft without a
    /// client are dropped.
    pub(crate) fn send_vehicle_orders(&self, orders: Vec<VehicleOrder>) {
        for o in orders {
            let client = self.state.hub.lock().unwrap().client(&o.callsign.0);
            match client {
                Some(id) => self.send(&[id], Out::Msg(crate::convert::vehicle_msg(o.command))),
                None => tracing::warn!(callsign = %o.callsign.0, "vehicle order for no client"),
            }
        }
    }

    pub(crate) fn listeners(&self, freq_khz: u32) -> Vec<ClientId> {
        self.state.hub.lock().unwrap().listeners(freq_khz)
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(Arc::new(Server::new(state)))
}

/// Browsers always send `Origin` on WebSocket handshakes; the native client does not. Refusing
/// any request with one stops web pages from driving the local server (cross-site hijacking).
async fn ws_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(server): State<Arc<Server>>,
) -> Response {
    if headers.contains_key(header::ORIGIN) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| crate::session::run(server, socket))
}

#[cfg(test)]
mod tests {
    use super::*;
    use atc_navdata::procedures::parse_procedures;

    #[test]
    fn station_map_from_epkk_procedures() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/airports/EPKK/procedures.toml"
        );
        let procs = parse_procedures(&std::fs::read_to_string(path).unwrap()).unwrap();
        let m = StationMap::new(&procs);
        assert_eq!(m.voice_for(126_130), Some(VoiceId::Atis));
        assert_eq!(m.voice_for(121_980), Some(VoiceId::Delivery));
        assert_eq!(m.voice_for(118_105), Some(VoiceId::Ground));
        assert_eq!(m.voice_for(123_255), Some(VoiceId::Tower));
        for (khz, voice, name) in [
            (121_075, VoiceId::Approach, "Kraków Approach"),
            (126_530, VoiceId::Approach, "Kraków Approach"),
            (135_405, VoiceId::Approach, "Kraków Approach"),
            (127_660, VoiceId::Radar, "Warszawa Radar"),
            (119_275, VoiceId::Radar, "Kraków Information"),
            (129_255, VoiceId::Radar, "Katowice Tower"),
        ] {
            assert_eq!(m.voice_for(khz), Some(voice), "{khz}");
            assert_eq!(m.station_name(khz), Some(name), "{khz}");
        }
        // A 25 kHz radio tuning of an 8.33 channel finds the new stations too.
        assert_eq!(m.canonical(121_075), 121_075);
        assert_eq!(m.canonical(126_525), 126_530);
        assert_eq!(m.canonical(129_250), 129_255);
        assert_eq!(m.canonical(135_400), 135_405);
        // 127.660 is a true 8.33 channel: 127.650 is another one.
        assert_eq!(m.canonical(127_650), 127_650);
        assert_eq!(m.canonical(127_660), 127_660);
        assert_eq!(m.voice_for(121_500), None);
        assert_eq!(m.station_name(126_130), Some("Kraków ATIS"));
        assert_eq!(m.station_name(121_500), None);
    }
}
