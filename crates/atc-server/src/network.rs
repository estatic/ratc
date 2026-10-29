//! The network dispatcher: every station's engine behind one interface. Transmissions are
//! routed by frequency, telemetry fans out to the engines near the aircraft, and a flight
//! handed off to another station's frequency moves to that station with its plan
//! ([`FlightHandover`]).
//!
//! Routing of a transmission from aircraft `from` on `freq_khz`:
//! 1. a frequency of a station position (or of an en-route unit): that station; when several
//!    stations work it (one shared unit), the owner of the flight if it is one of them, else
//!    the one nearest the aircraft. A flight owned by another station moves on this call;
//! 2. otherwise a frequency of an adjacent unit some engine emulates
//!    ([`Procedures::adjacent`]): the owning engine if it emulates it, else the nearest one
//!    that does (the flight moves);
//! 3. otherwise nobody answers.
//!
//! Locking: `owners` is held for a whole dispatch, engines are locked one at a time inside it
//! and never the other way round; `pilots` is a leaf lock.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use atc_core::atis::Weather;
use atc_core::engine::{AtcEngine, AtcOutput, EngineSnapshot, LanguageMode, VehicleOrder};
use atc_core::state::{AircraftState, SimTime};
use atc_core::strip::{PilotProfile, Role};
use atc_geo::{LatLon, distance_m};
use atc_navdata::model::{Airport, FreqKind};
use atc_phraseology::{Callsign, Lang};

#[cfg(doc)]
use atc_core::handover::FlightHandover;
#[cfg(doc)]
use atc_navdata::procedures::Procedures;

use crate::stations::LoadedStation;

const M_PER_NM: f64 = 1852.0;
/// Telemetry also goes to every engine whose airport is this close to the aircraft.
pub const FAN_OUT_NM: f64 = 30.0;

/// Who works a flight: a station (index into [`Network::stations`]) or an en-route unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    Station(usize),
    /// En-route units are not loaded yet; such an owner never answers.
    Unit(usize),
}

impl Owner {
    fn station(self) -> Option<usize> {
        match self {
            Owner::Station(i) => Some(i),
            Owner::Unit(_) => None,
        }
    }
}

#[derive(Default)]
struct Pilot {
    profile: Option<PilotProfile>,
    pos: Option<LatLon>,
    /// Stations that handed the flight away to another owner: they do not register it
    /// again from telemetry (no ghost strip) until it comes back.
    handed_away: HashSet<usize>,
}

pub struct Network {
    stations: Vec<LoadedStation>,
    /// Each station's reference point (midpoint of its first runway).
    refs: Vec<Option<LatLon>>,
    owners: Mutex<HashMap<Callsign, Owner>>,
    pilots: Mutex<HashMap<Callsign, Pilot>>,
    /// Who works each staffed position frequency (positions the engine answers, alternate
    /// Approach frequencies), in station order. ATIS, Unicom, Departure and AFIS
    /// frequencies own nothing: a call there moves no flight.
    freq_index: HashMap<u32, Vec<Owner>>,
    /// The stations emulating each adjacent unit frequency.
    emulated: HashMap<u32, Vec<usize>>,
    /// The station of each ATIS frequency (the first one on a shared frequency).
    atis: HashMap<u32, usize>,
}

/// Midpoint of the first runway.
fn reference_point(ap: &Airport) -> Option<LatLon> {
    let [a, b] = &ap.runways.first()?.ends;
    Some(LatLon {
        lat: (a.threshold.lat + b.threshold.lat) / 2.0,
        lon: (a.threshold.lon + b.threshold.lon) / 2.0,
    })
}

/// The entry of `map` for `khz`, or for another name of the same 25 kHz carrier (the lowest
/// such key, so the result does not depend on the map's order).
fn on_channel<V>(map: &HashMap<u32, V>, khz: u32) -> Option<&V> {
    map.get(&khz).or_else(|| {
        map.iter()
            .filter(|(k, _)| atc_proto::same_channel(**k, khz))
            .min_by_key(|(k, _)| **k)
            .map(|(_, v)| v)
    })
}

impl Network {
    /// Panics without stations ([`crate::stations::load_stations`] never returns none).
    pub fn new(stations: Vec<LoadedStation>) -> Self {
        assert!(!stations.is_empty(), "a network needs a station");
        let mut freq_index: HashMap<u32, Vec<Owner>> = HashMap::new();
        let mut emulated: HashMap<u32, Vec<usize>> = HashMap::new();
        let mut atis = HashMap::new();
        for (i, s) in stations.iter().enumerate() {
            let p = &s.procs;
            let has_approach = p.positions.iter().any(|q| q.role == FreqKind::Approach);
            let alt = p.approach_alt_khz.iter().filter(|_| has_approach);
            let staffed = p
                .positions
                .iter()
                .filter(|q| Role::from_freq_kind(q.role).is_some())
                .map(|q| q.khz);
            for khz in staffed.chain(alt.copied()) {
                let owners = freq_index.entry(khz).or_default();
                if !owners.contains(&Owner::Station(i)) {
                    owners.push(Owner::Station(i));
                }
            }
            for u in &p.adjacent {
                let at = emulated.entry(u.khz).or_default();
                if !at.contains(&i) {
                    at.push(i);
                }
            }
            for q in p.positions.iter().filter(|q| q.role == FreqKind::Atis) {
                atis.entry(q.khz).or_insert(i);
            }
        }
        Network {
            refs: stations
                .iter()
                .map(|s| reference_point(&s.airport))
                .collect(),
            stations,
            owners: Mutex::new(HashMap::new()),
            pilots: Mutex::new(HashMap::new()),
            freq_index,
            emulated,
            atis,
        }
    }

    /// The stations, sorted by id.
    pub fn stations(&self) -> &[LoadedStation] {
        &self.stations
    }

    /// Who works `cs` now; `None` before its first transmission.
    pub fn owner(&self, cs: &Callsign) -> Option<Owner> {
        self.owners.lock().unwrap().get(cs).copied()
    }

    /// A new aircraft; each engine registers it the first time it hears of it.
    pub fn register_pilot(&self, p: PilotProfile) {
        let cs = p.callsign.clone();
        self.pilots.lock().unwrap().entry(cs).or_default().profile = Some(p);
    }

    /// The engine language mode (the same for every station).
    pub fn language(&self) -> LanguageMode {
        self.stations[0].engine.lock().unwrap().language()
    }

    /// The language of aircraft `from` calling on `freq_khz`: its strip's at the station
    /// that would answer, else that station's ATIS language.
    pub fn pilot_lang(&self, from: &Callsign, freq_khz: u32) -> Lang {
        let owner = self.owner(from);
        let i = self
            .route(owner, freq_khz, self.pos(from))
            .or_else(|| owner.and_then(Owner::station))
            .unwrap_or(0);
        let engine = self.stations[i].engine.lock().unwrap();
        engine
            .strip(from)
            .map_or_else(|| engine.atis_lang(), |s| s.lang)
    }

    /// One telemetry sample: to the owning engine and every engine within [`FAN_OUT_NM`];
    /// an aircraft nobody owns yet is also sent to the nearest station, which speaks for it.
    /// Only the owner's (or that nearest station's) outputs are returned. A hand-off the
    /// pilot has acknowledged then moves the flight.
    pub fn on_telemetry(&self, s: AircraftState) -> Vec<AtcOutput> {
        let cs = s.callsign.clone();
        let pos = s.pos;
        self.pilots
            .lock()
            .unwrap()
            .entry(cs.clone())
            .or_default()
            .pos = Some(pos);
        let mut owners = self.owners.lock().unwrap();
        let speaker = match owners.get(&cs) {
            Some(o) => o.station(),
            None => self.nearest(0..self.stations.len(), Some(pos)),
        };
        let mut out = Vec::new();
        for (i, st) in self.stations.iter().enumerate() {
            let near = self.refs[i].is_some_and(|r| distance_m(r, pos) <= FAN_OUT_NM * M_PER_NM);
            if !near && speaker != Some(i) {
                continue;
            }
            let mut engine = st.engine.lock().unwrap();
            self.ensure_registered(i, &mut engine, &cs);
            let o = engine.on_telemetry(s.clone());
            if speaker == Some(i) {
                out = o;
            }
        }
        self.check_handoff(&mut owners, &cs, Some(pos));
        out
    }

    /// A pilot transmission, routed as the module documentation says; the answering
    /// station owns the flight from then on.
    pub fn on_transmission(
        &self,
        from: &Callsign,
        freq_khz: u32,
        text: &str,
        lang: Lang,
        t: SimTime,
    ) -> Vec<AtcOutput> {
        let mut owners = self.owners.lock().unwrap();
        let owner = owners.get(from).copied();
        let pos = self.pos(from);
        let Some(target) = self.route(owner, freq_khz, pos) else {
            return Vec::new();
        };
        if let Some(old) = owner.and_then(Owner::station)
            && old != target
        {
            self.move_flight(old, target, from);
        }
        owners.insert(from.clone(), Owner::Station(target));
        let out = {
            let mut engine = self.stations[target].engine.lock().unwrap();
            self.ensure_registered(target, &mut engine, from);
            engine.on_transmission(freq_khz, text, lang, t)
        };
        self.check_handoff(&mut owners, from, pos);
        out
    }

    /// "Say again" in `lang` from the station that would answer `from` on `freq_khz`
    /// (nothing moves); nothing on a frequency nobody works.
    pub fn say_again(&self, from: &Callsign, freq_khz: u32, lang: Lang) -> Vec<AtcOutput> {
        let owner = self.owner(from);
        match self.route(owner, freq_khz, self.pos(from)) {
            Some(i) => self.stations[i]
                .engine
                .lock()
                .unwrap()
                .say_again(freq_khz, lang),
            None => Vec::new(),
        }
    }

    /// New weather to every engine; returns the ATIS frequencies whose broadcast changed.
    pub fn on_weather(&self, w: Weather) -> Vec<u32> {
        let changed: Vec<usize> = self
            .stations
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let mut engine = s.engine.lock().unwrap();
                let before = engine.atis_broadcast().map(|a| a.0);
                engine.on_weather(w.clone());
                (engine.atis_broadcast().map(|a| a.0) != before).then_some(i)
            })
            .collect();
        let mut khz: Vec<u32> = self
            .atis
            .iter()
            .filter(|(_, i)| changed.contains(i))
            .map(|(k, _)| *k)
            .collect();
        khz.sort_unstable();
        khz
    }

    /// Every station's ATIS frequency, ascending.
    pub fn atis_khzs(&self) -> Vec<u32> {
        let mut khz: Vec<u32> = self.atis.keys().copied().collect();
        khz.sort_unstable();
        khz
    }

    /// The current ATIS letter, language and text on `freq_khz`.
    pub fn atis_broadcast(&self, freq_khz: u32) -> Option<(char, Lang, String)> {
        let i = *self.atis.get(&freq_khz)?;
        let engine = self.stations[i].engine.lock().unwrap();
        let (letter, text) = engine.atis_broadcast()?;
        Some((letter, engine.atis_lang(), text))
    }

    /// Whose speech-to-text prompt fits a transmission on `freq_khz` from `pos`: the
    /// station (or unit) working the frequency, nearest first; unowned, the nearest
    /// station.
    pub fn stt_prompt_owner(&self, freq_khz: u32, pos: Option<LatLon>) -> Option<Owner> {
        match on_channel(&self.freq_index, freq_khz) {
            Some(at) => self
                .nearest(at.iter().filter_map(|o| o.station()), pos)
                .map(Owner::Station)
                .or_else(|| at.first().copied()),
            None => self
                .nearest(0..self.stations.len(), pos)
                .map(Owner::Station),
        }
    }

    /// Each station's id and engine snapshot, taken under the ownership lock so that no
    /// flight is half-way between two engines (and the owners -> engine lock order holds).
    pub fn snapshots(&self) -> Vec<(String, EngineSnapshot)> {
        let _owners = self.owners.lock().unwrap();
        self.stations
            .iter()
            .map(|s| (s.id.clone(), s.engine.lock().unwrap().snapshot()))
            .collect()
    }

    /// The vehicle orders of every engine, drained. Taking non-owners' orders too is right:
    /// only the owner orders a car (orders come from transmissions it answered), while an
    /// engine that exported a flight queues the cancellation of a car leading it, which must
    /// still reach the aircraft. Each order names its aircraft, so whoever drains it delivers
    /// it to the right client.
    pub fn take_vehicle_orders(&self) -> Vec<VehicleOrder> {
        self.stations
            .iter()
            .flat_map(|s| s.engine.lock().unwrap().take_vehicle_orders())
            .collect()
    }

    fn pos(&self, cs: &Callsign) -> Option<LatLon> {
        self.pilots.lock().unwrap().get(cs).and_then(|p| p.pos)
    }

    /// Registers `cs` in `engine` (station `i`) unless it has a strip there or station `i`
    /// handed it away.
    fn ensure_registered(&self, i: usize, engine: &mut AtcEngine, cs: &Callsign) {
        if engine.strip(cs).is_some() {
            return;
        }
        let profile = {
            let pilots = self.pilots.lock().unwrap();
            let p = pilots.get(cs);
            if p.is_some_and(|p| p.handed_away.contains(&i)) {
                return;
            }
            p.and_then(|p| p.profile.clone())
        };
        engine.register_pilot(profile.unwrap_or_else(|| PilotProfile {
            callsign: cs.clone(),
            aircraft_type: String::new(),
        }));
    }

    /// The station answering a call on `freq_khz` (rules 1 and 2 of the module docs).
    fn route(&self, owner: Option<Owner>, freq_khz: u32, pos: Option<LatLon>) -> Option<usize> {
        if let Some(at) = on_channel(&self.freq_index, freq_khz) {
            if let Some(o) = owner.filter(|o| at.contains(o)) {
                return o.station();
            }
            return self.nearest(at.iter().filter_map(|o| o.station()), pos);
        }
        let at = on_channel(&self.emulated, freq_khz)?;
        if let Some(i) = owner.and_then(Owner::station).filter(|i| at.contains(i)) {
            return Some(i);
        }
        self.nearest(at.iter().copied(), pos)
    }

    /// The candidate station nearest `pos` (the first on a tie, or without a position).
    fn nearest(
        &self,
        candidates: impl Iterator<Item = usize>,
        pos: Option<LatLon>,
    ) -> Option<usize> {
        let dist = |i: usize| match (self.refs[i], pos) {
            (Some(r), Some(p)) => distance_m(r, p),
            _ => f64::INFINITY,
        };
        candidates.min_by(|&a, &b| dist(a).total_cmp(&dist(b)))
    }

    /// Hands flight `cs` from station `from` to station `to`.
    fn move_flight(&self, from: usize, to: usize, cs: &Callsign) {
        let handover = self.stations[from].engine.lock().unwrap().export_flight(cs);
        {
            let mut pilots = self.pilots.lock().unwrap();
            let away = &mut pilots.entry(cs.clone()).or_default().handed_away;
            away.insert(from);
            away.remove(&to);
        }
        if let Some(h) = handover {
            tracing::info!(callsign = %cs.0, from = %self.stations[from].id, to = %self.stations[to].id, "flight handed over");
            self.stations[to].engine.lock().unwrap().import_flight(h);
        }
    }

    /// The owner's acknowledged hand-off to another station's frequency moves the flight
    /// there. A frequency the owner works too (a shared unit) or one no station works (an
    /// emulated adjacent unit) keeps it.
    fn check_handoff(
        &self,
        owners: &mut HashMap<Callsign, Owner>,
        cs: &Callsign,
        pos: Option<LatLon>,
    ) {
        let Some(i) = owners.get(cs).and_then(|o| o.station()) else {
            return;
        };
        let Some(khz) = self.stations[i].engine.lock().unwrap().handoff_target(cs) else {
            return;
        };
        let Some(at) = on_channel(&self.freq_index, khz) else {
            return;
        };
        if at.contains(&Owner::Station(i)) {
            return;
        }
        let Some(j) = self.nearest(at.iter().filter_map(|o| o.station()), pos) else {
            return;
        };
        self.move_flight(i, j, cs);
        owners.insert(cs.clone(), Owner::Station(j));
    }
}
