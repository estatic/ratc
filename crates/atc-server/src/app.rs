//! Shared server state and the axum router (`GET /ws`).
//!
//! Locking: the network's engines, `hub` and the internal maps are `std::sync::Mutex`es, locked only for
//! short synchronous sections and never held across an `.await` (STT/TTS run in
//! `spawn_blocking` with no lock held). Where they nest, the order is `floor`, then the
//! pilot jobs or the speaker queues, then the network's engines.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use atc_core::engine::{AtcOutput, VehicleOrder};
use atc_geo::LatLon;
use atc_navdata::enroute::EnrouteUnitDef;
use atc_navdata::model::FreqKind;
use atc_navdata::procedures::Procedures;
use atc_phraseology::{Callsign, Lang, station_name};
use atc_proto::{AudioFrame, ServerMsg};
use atc_voice::{Pcm, SpeechToText, TextToSpeech, VoiceId, initial_prompt};
use axum::Router;
use axum::extract::State;
use axum::extract::ws::WebSocketUpgrade;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tokio::sync::{Notify, mpsc};

use crate::atis::AtisCache;
use crate::floor::{Floor, RadioTiming};
use crate::hub::{ClientId, Hub, PttOutcome};
use crate::network::{Network, Owner};
use crate::stations::LoadedStation;

#[derive(Clone)]
pub struct AppState {
    pub network: Arc<Network>,
    pub hub: Arc<Mutex<Hub>>,
    pub stt: Arc<dyn SpeechToText>,
    pub tts: Arc<dyn TextToSpeech>,
    pub stations: StationMap,
    /// The prompt when no owner's fits (nobody works the frequency and no station is known).
    pub stt_prompts: SttPrompts,
    /// Each station's and unit's own prompts, see [`OwnerPrompts`].
    pub owner_prompts: OwnerPrompts,
    /// Gap, margin and airtime scale of the shared radio frequencies.
    pub radio: RadioTiming,
}

/// Words of a Whisper prompt (a proxy for its 224-token window).
const PROMPT_WORDS: usize = 170;

/// `prompt`, the expected phrases `hints` (comma separated) and the controller's last words
/// `said`, within [`PROMPT_WORDS`]: the hints at the end are dropped first (stands before
/// the speaker's callsign).
fn fit_prompt(prompt: &str, mut hints: Vec<String>, said: Option<&str>) -> String {
    let words = |s: &str| s.split_whitespace().count();
    // Over the cap without any hint: the last words of the transmission matter most.
    let said_owned: Option<String> = said.map(|t| {
        let all: Vec<&str> = t.split_whitespace().collect();
        let room = PROMPT_WORDS.saturating_sub(words(prompt));
        all[all.len().saturating_sub(room)..].join(" ")
    });
    let said = said_owned.as_deref().filter(|s| !s.is_empty());
    let fixed = words(prompt) + said.map_or(0, words);
    while !hints.is_empty() && fixed + hints.iter().map(|h| words(h)).sum::<usize>() > PROMPT_WORDS
    {
        hints.pop();
    }
    let mut out = prompt.to_string();
    if !hints.is_empty() {
        out.push(' ');
        out.push_str(&hints.join(", "));
        out.push('.');
    }
    if let Some(said) = said {
        out.push(' ');
        out.push_str(said);
    }
    out
}

/// The static Whisper prompt ([`atc_voice::initial_prompt`]) in each language.
#[derive(Clone, Debug, PartialEq)]
pub struct SttPrompts {
    pub en: String,
    pub pl: String,
}

impl SttPrompts {
    /// The prompts of a place: `airport` is its name, `callsigns` the stations said there
    /// ("Kraków Ground"), each as spoken in the language of the prompt.
    pub fn build(airport: &str, callsigns: &[&str]) -> Self {
        let prompt = |lang: Lang, code: &str| {
            let names: Vec<String> = callsigns.iter().map(|c| station_name(c, lang)).collect();
            initial_prompt(airport, &names, &[], code)
        };
        SttPrompts {
            en: prompt(Lang::En, "en"),
            pl: prompt(Lang::Pl, "pl"),
        }
    }

    pub fn get(&self, lang: Lang) -> &str {
        match lang {
            Lang::En => &self.en,
            Lang::Pl => &self.pl,
        }
    }
}

/// The speech-to-text prompt of every station (from its own airport and positions) and of
/// every en-route unit (its callsign and the nearest stations'): a transmission is heard
/// against the names of whoever works its frequency, not against every station of the
/// server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OwnerPrompts {
    stations: Vec<SttPrompts>,
    units: Vec<SttPrompts>,
}

/// How many of the nearest stations a unit's prompt names.
const UNIT_PROMPT_STATIONS: usize = 2;

impl OwnerPrompts {
    pub fn for_network(network: &Network) -> Self {
        let stations: Vec<&LoadedStation> = network.stations().iter().collect();
        let units = network
            .units()
            .iter()
            .map(|def| {
                let nearest = network.nearest_stations(centre(def), UNIT_PROMPT_STATIONS);
                let airport = nearest
                    .first()
                    .map_or("", |&i| stations[i].airport.name.as_str());
                let mut callsigns = vec![def.callsign.as_str()];
                for &i in &nearest {
                    callsigns.extend(position_names(stations[i]));
                }
                SttPrompts::build(airport, &callsigns)
            })
            .collect();
        OwnerPrompts {
            stations: stations
                .iter()
                .map(|s| SttPrompts::build(&s.airport.name, &position_names(s)))
                .collect(),
            units,
        }
    }

    pub fn get(&self, owner: Owner) -> Option<&SttPrompts> {
        match owner {
            Owner::Station(i) => self.stations.get(i),
            Owner::Unit(k) => self.units.get(k),
        }
    }
}

fn position_names(s: &LoadedStation) -> Vec<&str> {
    s.procs
        .positions
        .iter()
        .map(|p| p.callsign.as_str())
        .collect()
}

/// The mean of the corners of the unit's sectors.
fn centre(def: &EnrouteUnitDef) -> Option<LatLon> {
    let corners: Vec<&[f64; 2]> = def.sectors.iter().flat_map(|s| &s.polygon).collect();
    if corners.is_empty() {
        return None;
    }
    let n = corners.len() as f64;
    Some(LatLon {
        lat: corners.iter().map(|c| c[0]).sum::<f64>() / n,
        lon: corners.iter().map(|c| c[1]).sum::<f64>() / n,
    })
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

/// Words a controller speaks per second: the airtime of a transmission without audio.
const WORDS_PER_S: f64 = 2.5;

/// The length of `pcm`; `None` when it has no usable sample rate.
fn audio_airtime(pcm: &Pcm) -> Option<Duration> {
    if pcm.sample_rate == 0 {
        return None;
    }
    Duration::try_from_secs_f64(pcm.samples.len() as f64 / f64::from(pcm.sample_rate)).ok()
}

/// How long `text` takes to say, from its word count.
fn words_airtime(text: &str) -> Duration {
    let words = text.split_whitespace().count() as f64;
    Duration::try_from_secs_f64(words / WORDS_PER_S).unwrap_or(Duration::ZERO)
}

/// A controller transmission's synthesized speech: its utterance id and audio (`None` when
/// TTS failed or the frequency has no voice, so the text goes alone).
pub(crate) struct Speech {
    pub(crate) utterance: u32,
    pub(crate) pcm: Option<Pcm>,
}

/// A controller transmission waiting for its turn on its frequency.
struct Queued {
    /// Identifies the item while the speaker synthesizes it outside the lock.
    seq: u64,
    /// A reply to a pilot transmission: ahead of every other item.
    reply: bool,
    out: AtcOutput,
    /// Synthesized ahead of its turn.
    speech: Option<Speech>,
}

/// The front of a frequency's queue, as the speaker sees it.
pub(crate) struct Front {
    pub(crate) seq: u64,
    /// The text and language still to synthesize; `None` once synthesized.
    pub(crate) unsynthesized: Option<(String, atc_phraseology::Lang)>,
}

/// What [`Server::take_on_air`] did with the front item.
pub(crate) enum OnAir {
    /// The item holds the frequency until `end`: send it now.
    Speak {
        out: Box<AtcOutput>,
        speech: Speech,
        end: Instant,
    },
    /// A readback reminder that no longer applies: skipped.
    Dropped,
    /// The floor or the queue front changed: look again.
    Retry,
}

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
    /// Who may speak on each frequency.
    pub(crate) floor: Mutex<Floor>,
    /// Transmissions of each client per frequency granted and not yet handled: the pilot's
    /// floor is released when the last is done (a re-key during processing is another one).
    pilot_jobs: Mutex<HashMap<(u32, ClientId), u32>>,
    /// Controller transmissions waiting per frequency, replies first.
    queues: Mutex<HashMap<u32, VecDeque<Queued>>>,
    next_queued: AtomicU64,
    /// The wake-up of each frequency's speaker task (queue or floor changed); an entry
    /// exists once the task runs.
    speakers: Mutex<HashMap<u32, Arc<Notify>>>,
}

impl Server {
    fn new(state: AppState) -> Self {
        let floor = Mutex::new(Floor::new(state.radio.gap));
        Server {
            state,
            next_client: AtomicU64::new(1),
            // 0 is the client's own PTT stream.
            next_utterance: AtomicU32::new(1),
            clients: Mutex::new(HashMap::new()),
            weather_source: Mutex::new(None),
            atis: AtisCache::default(),
            last_said: Mutex::new(HashMap::new()),
            floor,
            pilot_jobs: Mutex::new(HashMap::new()),
            queues: Mutex::new(HashMap::new()),
            next_queued: AtomicU64::new(0),
            speakers: Mutex::new(HashMap::new()),
        }
    }

    /// Replies to a pilot transmission, in batch order: on air after earlier replies and
    /// after items already queued for the same aircraft (generated before its transmission
    /// was handled, e.g. a deviation call the reply refers to), before everything else.
    pub(crate) fn enqueue_front(self: &Arc<Self>, outputs: Vec<AtcOutput>) {
        self.enqueue(outputs, true);
    }

    /// Controller transmissions of their own (telemetry): after everything queued.
    pub(crate) fn enqueue_back(self: &Arc<Self>, outputs: Vec<AtcOutput>) {
        self.enqueue(outputs, false);
    }

    fn enqueue(self: &Arc<Self>, outputs: Vec<AtcOutput>, reply: bool) {
        let mut freqs = Vec::new();
        {
            let mut queues = self.queues.lock().unwrap();
            for out in outputs {
                let freq = out.freq_khz;
                let q = queues.entry(freq).or_default();
                let item = Queued {
                    seq: self.next_queued.fetch_add(1, Ordering::Relaxed),
                    reply,
                    out,
                    speech: None,
                };
                if reply {
                    let to = &item.out.message.to;
                    let at = q
                        .iter()
                        .rposition(|e| e.reply || (to.is_some() && e.out.message.to == *to))
                        .map_or(0, |i| i + 1);
                    q.insert(at, item);
                } else {
                    q.push_back(item);
                }
                if !freqs.contains(&freq) {
                    freqs.push(freq);
                }
            }
        }
        for freq in freqs {
            self.wake_speaker(freq);
        }
    }

    /// Wakes the speaker of `freq`, starting it on first use.
    fn wake_speaker(self: &Arc<Self>, freq_khz: u32) {
        let mut speakers = self.speakers.lock().unwrap();
        let wake = speakers.entry(freq_khz).or_insert_with(|| {
            let wake = Arc::new(Notify::new());
            tokio::spawn(crate::pipeline::speaker(
                self.clone(),
                freq_khz,
                wake.clone(),
            ));
            wake
        });
        wake.notify_one();
    }

    /// The floor of `freq` changed: its speaker (if any) looks again.
    fn floor_changed(&self, freq_khz: u32) {
        if let Some(wake) = self.speakers.lock().unwrap().get(&freq_khz) {
            wake.notify_one();
        }
    }

    /// The front item of `freq`'s queue.
    pub(crate) fn queue_front(&self, freq_khz: u32) -> Option<Front> {
        let queues = self.queues.lock().unwrap();
        let e = queues.get(&freq_khz)?.front()?;
        Some(Front {
            seq: e.seq,
            unsynthesized: e
                .speech
                .is_none()
                .then(|| (e.out.text.clone(), e.out.message.lang)),
        })
    }

    /// Stores the speech of queued item `seq`.
    pub(crate) fn store_speech(&self, freq_khz: u32, seq: u64, speech: Speech) {
        let mut queues = self.queues.lock().unwrap();
        if let Some(e) = queues
            .get_mut(&freq_khz)
            .and_then(|q| q.iter_mut().find(|e| e.seq == seq))
        {
            e.speech = Some(speech);
        }
    }

    /// When the controller may start on `freq` (see [`Floor::atc_may_start`]).
    pub(crate) fn atc_may_start(&self, freq_khz: u32) -> Option<Instant> {
        self.floor
            .lock()
            .unwrap()
            .atc_may_start(freq_khz, Instant::now())
    }

    /// Puts queued item `seq` on the air if the floor is free and it is still the
    /// synthesized front item. A readback reminder that no longer applies is dropped
    /// instead. On air the frequency is busy for the engines.
    pub(crate) fn take_on_air(&self, freq_khz: u32, seq: u64) -> OnAir {
        let mut floor = self.floor.lock().unwrap();
        let now = Instant::now();
        if floor.atc_may_start(freq_khz, now).is_some() {
            return OnAir::Retry;
        }
        let item = {
            let mut queues = self.queues.lock().unwrap();
            let Some(q) = queues.get_mut(&freq_khz) else {
                return OnAir::Retry;
            };
            match q.front() {
                Some(e) if e.seq == seq && e.speech.is_some() => q.pop_front().unwrap(),
                _ => return OnAir::Retry,
            }
        };
        let network = &self.state.network;
        if let Some(r) = &item.out.reminder
            && !network.reminder_still_due(r)
        {
            network.reminder_dropped(r);
            return OnAir::Dropped;
        }
        let speech = item.speech.expect("checked above");
        let airtime = speech
            .pcm
            .as_ref()
            .and_then(audio_airtime)
            .unwrap_or_else(|| words_airtime(&item.out.text));
        floor.atc_start(freq_khz, now, airtime, &self.state.radio);
        network.set_frequency_busy(freq_khz, true);
        OnAir::Speak {
            end: floor.atc_end_at(freq_khz).unwrap_or(now),
            out: Box::new(item.out),
            speech,
        }
    }

    /// The controller's transmission on `freq` has ended: free for the engines unless a
    /// pilot keyed meanwhile.
    pub(crate) fn atc_done(&self, freq_khz: u32) {
        let floor = self.floor.lock().unwrap();
        if !floor.is_busy(freq_khz, Instant::now()) {
            self.state.network.set_frequency_busy(freq_khz, false);
        }
    }

    /// Client `id` starts a transmission on `freq` (a PTT press or typed text). Granted, it
    /// holds the floor until [`Server::pilot_done`] for it.
    pub(crate) fn pilot_press(&self, freq_khz: u32, id: ClientId) -> PttOutcome {
        let mut floor = self.floor.lock().unwrap();
        let outcome = floor.pilot_press(freq_khz, id, Instant::now());
        if outcome == PttOutcome::Granted {
            *self
                .pilot_jobs
                .lock()
                .unwrap()
                .entry((freq_khz, id))
                .or_default() += 1;
            self.state.network.set_frequency_busy(freq_khz, true);
        }
        outcome
    }

    /// The pilot let go of the PTT: the floor stays theirs while it is processed.
    pub(crate) fn pilot_release(&self, freq_khz: u32, id: ClientId) {
        self.floor.lock().unwrap().pilot_release(freq_khz, id);
    }

    /// One of client `id`'s transmissions on `freq` is handled (or discarded). After the
    /// last, the floor is free: the engines learn it and the speaker looks again.
    pub(crate) fn pilot_done(&self, freq_khz: u32, id: ClientId) {
        let freed = {
            let mut floor = self.floor.lock().unwrap();
            let mut jobs = self.pilot_jobs.lock().unwrap();
            let Some(n) = jobs.get_mut(&(freq_khz, id)) else {
                return;
            };
            *n -= 1;
            if *n > 0 {
                return;
            }
            jobs.remove(&(freq_khz, id));
            drop(jobs);
            let freed = floor.pilot_done(freq_khz, id, Instant::now());
            if freed {
                self.state.network.set_frequency_busy(freq_khz, false);
            }
            freed
        };
        if freed {
            self.floor_changed(freq_khz);
        }
    }

    /// Client `id` disconnected: every floor it held or was processed on is free.
    pub(crate) fn floor_leave(&self, id: ClientId) {
        let freed = {
            let mut floor = self.floor.lock().unwrap();
            self.pilot_jobs
                .lock()
                .unwrap()
                .retain(|&(_, client), _| client != id);
            let freed = floor.leave(id);
            for &freq in &freed {
                self.state.network.set_frequency_busy(freq, false);
            }
            freed
        };
        for freq in freed {
            self.floor_changed(freq);
        }
    }

    pub(crate) fn remember_said(&self, freq_khz: u32, text: &str) {
        self.last_said
            .lock()
            .unwrap()
            .insert(freq_khz, text.to_string());
    }

    /// The aviation prompt of whoever works `freq_khz` (a station or unit; unowned, the
    /// station nearest the speaking aircraft `from`) in `lang`, plus the last controller
    /// transmission on `freq_khz`.
    pub(crate) fn stt_prompt(&self, freq_khz: u32, lang: Lang, from: &Callsign) -> String {
        let network = &self.state.network;
        let prompt = network
            .stt_prompt_owner(freq_khz, network.pilot_pos(from))
            .and_then(|o| self.state.owner_prompts.get(o))
            .unwrap_or(&self.state.stt_prompts)
            .get(lang);
        let said = self.last_said.lock().unwrap().get(&freq_khz).cloned();
        let hints = network.stt_hints(freq_khz, from);
        fit_prompt(prompt, hints, said.as_deref())
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

    /// Ends the flight `cs` of client `id` (see `Network::end_flight`). The cancel of its
    /// follow-me car goes to `id` itself: the hub may already list the client under its new
    /// callsign, so a lookup by `cs` would find nobody.
    pub(crate) fn end_flight(&self, id: ClientId, cs: &atc_phraseology::Callsign) {
        let network = &self.state.network;
        network.end_flight(cs);
        let (own, others): (Vec<_>, Vec<_>) = network
            .take_vehicle_orders()
            .into_iter()
            .partition(|o| o.callsign == *cs);
        for o in own {
            self.send(&[id], Out::Msg(crate::convert::vehicle_msg(o.command)));
        }
        self.send_vehicle_orders(others);
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

    #[test]
    fn prompt_drops_stands_before_callsigns() {
        let hints: Vec<String> = ["November one Sierra Papa", "November Sierra Papa"]
            .into_iter()
            .map(String::from)
            .chain((0..60).map(|i| format!("apron {i} Golf Alpha")))
            .collect();
        let p = fit_prompt(
            "Kraków Ground",
            hints.clone(),
            Some("taxi to holding point"),
        );
        assert!(p.starts_with("Kraków Ground November one Sierra Papa, November Sierra Papa"));
        assert!(p.ends_with("taxi to holding point"));
        assert!(p.split_whitespace().count() <= PROMPT_WORDS, "{p}");
        assert!(p.contains("apron 0 Golf Alpha"));
        assert!(!p.contains("apron 59 Golf Alpha"));
        let long = "word ".repeat(PROMPT_WORDS);
        let p = fit_prompt(&long, hints, None);
        assert!(!p.contains("Sierra"));
    }

    #[test]
    fn prompt_truncates_long_transmission_to_its_last_words() {
        let said = (0..200)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let p = fit_prompt(
            "Kraków Ground",
            vec!["November Sierra Papa".into()],
            Some(&said),
        );
        assert!(p.split_whitespace().count() <= PROMPT_WORDS, "{p}");
        assert!(p.ends_with("w199") && !p.contains("w0 "), "{p}");
        assert!(!p.contains("Sierra"));
        // No room left for any of it: the prompt alone, without a trailing space.
        let full = vec!["word"; PROMPT_WORDS].join(" ");
        assert_eq!(fit_prompt(&full, Vec::new(), Some(&said)), full);
    }

    #[test]
    fn airtime_from_audio_or_words() {
        let pcm = |sample_rate, n| Pcm {
            sample_rate,
            samples: vec![0; n],
        };
        assert_eq!(
            audio_airtime(&pcm(22_050, 44_100)),
            Some(Duration::from_secs(2))
        );
        assert_eq!(audio_airtime(&pcm(0, 100)), None);
        assert_eq!(audio_airtime(&pcm(16_000, 0)), Some(Duration::ZERO));
        assert_eq!(
            words_airtime("one two three four five"),
            Duration::from_secs(2)
        );
        assert_eq!(words_airtime(""), Duration::ZERO);
    }
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
