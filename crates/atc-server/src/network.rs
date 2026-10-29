//! The network dispatcher: every station's engine and every en-route unit behind one
//! interface. Transmissions are routed by frequency, telemetry fans out to the engines near
//! the aircraft, and a flight handed off to another station's or unit's frequency moves there
//! with its plan ([`FlightHandover`]).
//!
//! Routing of a transmission from aircraft `from` on `freq_khz`:
//! 1. a frequency of a station position or of an en-route unit: that station or unit; when
//!    several stations work it (one shared unit), the owner of the flight if it is one of
//!    them, else the station nearest the aircraft (a station before a unit on the same
//!    frequency). A flight owned by someone else moves on this call;
//! 2. otherwise a frequency of an adjacent unit some engine emulates
//!    ([`Procedures::adjacent`]): the owning engine if it emulates it, else the nearest one
//!    that does (the flight moves). A real en-route unit on that frequency wins (rule 1);
//! 3. otherwise nobody answers.
//!
//! En-route units ([`EnrouteUnit`]): when a flight a unit works leaves all its sectors, the
//! unit hands it to the next unit containing the aircraft, else to the station whose CTR or
//! TMA contains it (its Approach, else Tower, else another staffed position: "contact"), a
//! station without controllers hears "change to" its published frequency, and with nobody
//! next the flight is released ("frequency change approved"). A "contact" moves the flight
//! once the pilot reads the frequency back on the unit's frequency, after
//! [`atc_core::enroute::HANDOFF_TIMEOUT_S`] anyway, or when the pilot calls the next owner
//! directly. A released flight has no owner.
//!
//! Locking: `owners` is held for a whole dispatch, engines and units are locked one at a time
//! inside it and never the other way round; `pilots` and `qnh` are leaf locks.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use atc_core::atis::Weather;
use atc_core::engine::{AtcEngine, AtcOutput, EngineSnapshot, LanguageMode, VehicleOrder};
use atc_core::enroute::{EnrouteEvent, EnrouteUnit, HandOff, unit_covers};
use atc_core::state::{AircraftState, SimTime};
use atc_core::strip::{PilotProfile, Role};
use atc_geo::{LatLon, distance_m, point_in_polygon};
use atc_navdata::enroute::EnrouteUnitDef;
use atc_navdata::model::{Airport, FreqKind};
use atc_navdata::procedures::AirspaceKind;
use atc_phraseology::{AtcItem, AtcMessage, Callsign, Lang, render};

#[cfg(doc)]
use atc_core::handover::FlightHandover;
#[cfg(doc)]
use atc_navdata::procedures::Procedures;

use crate::stations::LoadedStation;

const M_PER_NM: f64 = 1852.0;
/// Telemetry also goes to every engine whose airport is this close to the aircraft.
pub const FAN_OUT_NM: f64 = 30.0;

/// Who works a flight: a station (index into [`Network::stations`]) or an en-route unit
/// (index into [`Network::units`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    Station(usize),
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
    /// The en-route units, each behind its own lock ([`Owner::Unit`] indexes them).
    units: Vec<Mutex<EnrouteUnit>>,
    /// Their definitions, for coverage queries without a lock.
    unit_defs: Vec<EnrouteUnitDef>,
    /// The QNH of the last weather given to the engines (units give it on first contact).
    qnh: Mutex<Option<u16>>,
    language: LanguageMode,
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
        let language = stations[0].engine.lock().unwrap().language();
        Network {
            units: Vec::new(),
            unit_defs: Vec::new(),
            qnh: Mutex::new(None),
            language,
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

    /// The en-route units (`enroute.toml`), owners of their frequencies after the stations'
    /// positions on the same frequency.
    pub fn with_units(mut self, defs: Vec<EnrouteUnitDef>) -> Self {
        for def in defs {
            let k = self.units.len();
            self.freq_index
                .entry(def.khz)
                .or_default()
                .push(Owner::Unit(k));
            self.units.push(Mutex::new(EnrouteUnit::new(def.clone())));
            self.unit_defs.push(def);
        }
        self
    }

    /// The stations, sorted by id.
    pub fn stations(&self) -> &[LoadedStation] {
        &self.stations
    }

    /// The en-route units' definitions, in [`Owner::Unit`] order.
    pub fn units(&self) -> &[EnrouteUnitDef] {
        &self.unit_defs
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
        self.language
    }

    /// The language a unit answers a transmission heard as `lang` in.
    fn unit_lang(&self, lang: Lang) -> Lang {
        match self.language {
            LanguageMode::Fixed(l) => l,
            LanguageMode::Auto => lang,
        }
    }

    /// The language of aircraft `from` calling on `freq_khz`: its strip's at the station
    /// that would answer, else that station's ATIS language.
    pub fn pilot_lang(&self, from: &Callsign, freq_khz: u32) -> Lang {
        let owner = self.owner(from);
        let i = match self.route(owner, freq_khz, self.pos(from)).or(owner) {
            Some(Owner::Unit(k)) => return self.units[k].lock().unwrap().lang_of(from),
            Some(Owner::Station(i)) => i,
            None => 0,
        };
        let engine = self.stations[i].engine.lock().unwrap();
        engine
            .strip(from)
            .map_or_else(|| engine.atis_lang(), |s| s.lang)
    }

    /// One telemetry sample: to the owning engine and every engine within [`FAN_OUT_NM`];
    /// an aircraft nobody owns yet is also sent to the nearest station, which speaks for it.
    /// Only the owner's (or that nearest station's) outputs are returned; an en-route unit
    /// owning the flight checks its sectors and hands it on or releases it. A hand-off the
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
        if let Some(&Owner::Unit(k)) = owners.get(&cs) {
            out.extend(self.unit_telemetry(&mut owners, k, &s));
        }
        self.check_handoff(&mut owners, &cs, Some(pos), s.t);
        out
    }

    /// A pilot transmission, routed as the module documentation says; the answering
    /// station or unit owns the flight from then on (a unit not working the flight, one it
    /// released, does not answer an acknowledgement and takes nothing).
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
        if let Some(old) = owner
            && old != target
        {
            self.move_flight(old, target, from);
        }
        let out = match target {
            Owner::Station(i) => {
                owners.insert(from.clone(), target);
                // The pilot calls it: in contact again even if it handed the flight away.
                if let Some(p) = self.pilots.lock().unwrap().get_mut(from) {
                    p.handed_away.remove(&i);
                }
                let mut engine = self.stations[i].engine.lock().unwrap();
                self.ensure_registered(i, &mut engine, from);
                engine.on_transmission(freq_khz, text, lang, t)
            }
            Owner::Unit(k) => {
                let lang = self.unit_lang(lang);
                let qnh = *self.qnh.lock().unwrap();
                let (items, serves) = {
                    let mut unit = self.units[k].lock().unwrap();
                    let items = unit.on_transmission(from, text, lang, qnh);
                    (items, unit.serves(from))
                };
                if serves {
                    owners.insert(from.clone(), target);
                } else {
                    owners.remove(from);
                }
                self.unit_output(k, Some(from), items, lang)
            }
        };
        self.check_handoff(&mut owners, from, pos, t);
        out
    }

    /// "Say again" in `lang` from the station that would answer `from` on `freq_khz`
    /// (nothing moves); nothing on a frequency nobody works.
    pub fn say_again(&self, from: &Callsign, freq_khz: u32, lang: Lang) -> Vec<AtcOutput> {
        let owner = self.owner(from);
        match self.route(owner, freq_khz, self.pos(from)) {
            Some(Owner::Station(i)) => self.stations[i]
                .engine
                .lock()
                .unwrap()
                .say_again(freq_khz, lang),
            Some(Owner::Unit(k)) => {
                let lang = self.unit_lang(lang);
                if self.units[k].lock().unwrap().serves(from) {
                    self.unit_output(k, Some(from), vec![AtcItem::SayAgain], lang)
                } else {
                    let station = self.unit_defs[k].callsign.clone();
                    self.unit_output(k, None, vec![AtcItem::SayAgainStation { station }], lang)
                }
            }
            None => Vec::new(),
        }
    }

    /// New weather to every engine; returns the ATIS frequencies whose broadcast changed.
    pub fn on_weather(&self, w: Weather) -> Vec<u32> {
        *self.qnh.lock().unwrap() = Some(w.qnh_hpa);
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

    /// Who answers a call on `freq_khz` (rules 1 and 2 of the module docs).
    fn route(&self, owner: Option<Owner>, freq_khz: u32, pos: Option<LatLon>) -> Option<Owner> {
        if let Some(at) = on_channel(&self.freq_index, freq_khz) {
            if let Some(o) = owner.filter(|o| at.contains(o)) {
                return Some(o);
            }
            return self.taker(at, pos);
        }
        let at = on_channel(&self.emulated, freq_khz)?;
        if let Some(i) = owner.and_then(Owner::station).filter(|i| at.contains(i)) {
            return Some(Owner::Station(i));
        }
        self.nearest(at.iter().copied(), pos).map(Owner::Station)
    }

    /// Of the owners of one frequency, who takes a flight not theirs: the station nearest
    /// `pos`, else the first unit.
    fn taker(&self, at: &[Owner], pos: Option<LatLon>) -> Option<Owner> {
        self.nearest(at.iter().filter_map(|o| o.station()), pos)
            .map(Owner::Station)
            .or_else(|| at.iter().copied().find(|o| matches!(o, Owner::Unit(_))))
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

    /// Hands flight `cs` from `from` to `to` (stations or units).
    fn move_flight(&self, from: Owner, to: Owner, cs: &Callsign) {
        let handover = match from {
            Owner::Station(i) => self.stations[i].engine.lock().unwrap().export_flight(cs),
            Owner::Unit(k) => self.units[k].lock().unwrap().export_flight(cs),
        };
        let aircraft_type = {
            let mut pilots = self.pilots.lock().unwrap();
            let p = pilots.entry(cs.clone()).or_default();
            if let Owner::Station(i) = from {
                p.handed_away.insert(i);
            }
            if let Owner::Station(j) = to {
                p.handed_away.remove(&j);
            }
            p.profile.as_ref().map(|p| p.aircraft_type.clone())
        };
        let Some(mut h) = handover else {
            return;
        };
        // A unit that took an aircraft by its call knows no type: the client's.
        if h.aircraft_type.is_empty() {
            h.aircraft_type = aircraft_type.unwrap_or_default();
        }
        tracing::info!(callsign = %cs.0, from = %self.owner_name(from), to = %self.owner_name(to), "flight handed over");
        match to {
            Owner::Station(j) => self.stations[j].engine.lock().unwrap().import_flight(h),
            Owner::Unit(k) => self.units[k].lock().unwrap().import_flight(h),
        }
    }

    fn owner_name(&self, o: Owner) -> &str {
        match o {
            Owner::Station(i) => &self.stations[i].id,
            Owner::Unit(k) => &self.unit_defs[k].callsign,
        }
    }

    /// The owner's acknowledged hand-off to another frequency moves the flight to whoever
    /// works it. From a station: a frequency the station works too (a shared unit) or one
    /// nobody works (an emulated adjacent unit) keeps it. From a unit: the target is taken
    /// once read back or timed out at `t`; with nobody on it the flight is released.
    fn check_handoff(
        &self,
        owners: &mut HashMap<Callsign, Owner>,
        cs: &Callsign,
        pos: Option<LatLon>,
        t: SimTime,
    ) {
        let Some(from) = owners.get(cs).copied() else {
            return;
        };
        let to = match from {
            Owner::Station(i) => {
                let Some(khz) = self.stations[i].engine.lock().unwrap().handoff_target(cs) else {
                    return;
                };
                let Some(at) = on_channel(&self.freq_index, khz) else {
                    return;
                };
                if at.contains(&from) {
                    return;
                }
                self.taker(at, pos)
            }
            Owner::Unit(k) => {
                let Some(khz) = self.units[k].lock().unwrap().handoff_target(cs, t) else {
                    return;
                };
                let to = on_channel(&self.freq_index, khz)
                    .and_then(|at| self.taker(at, pos))
                    .filter(|&o| o != from);
                if to.is_none() {
                    self.units[k].lock().unwrap().release(cs);
                    owners.remove(cs);
                }
                to
            }
        };
        if let Some(to) = to {
            self.move_flight(from, to, cs);
            owners.insert(cs.clone(), to);
        }
    }

    /// A telemetry sample `s` for unit `k`, which works the flight: leaving its sectors, the
    /// flight is handed on or released (module docs); what the unit says.
    fn unit_telemetry(
        &self,
        owners: &mut HashMap<Callsign, Owner>,
        k: usize,
        s: &AircraftState,
    ) -> Vec<AtcOutput> {
        let cs = &s.callsign;
        let event = self.units[k]
            .lock()
            .unwrap()
            .on_telemetry(cs, s.pos, s.alt_ft_msl);
        if event != Some(EnrouteEvent::LeftSector) {
            return Vec::new();
        }
        let to = self.next_after(k, s.pos, s.alt_ft_msl);
        let released = !matches!(to, HandOff::Contact { .. });
        tracing::info!(callsign = %cs.0, unit = %self.unit_defs[k].callsign, next = ?to, "left sector");
        let (items, lang) = {
            let mut unit = self.units[k].lock().unwrap();
            (unit.hand_off(cs, to, s.t), unit.lang_of(cs))
        };
        if released {
            owners.remove(cs);
        }
        self.unit_output(k, Some(cs), items, lang)
    }

    /// Where a flight leaving unit `k` at `pos`, `alt_ft` goes (module docs).
    fn next_after(&self, k: usize, pos: LatLon, alt_ft: f64) -> HandOff {
        let here = self.unit_defs[k].khz;
        if let Some(d) = self
            .unit_defs
            .iter()
            .enumerate()
            .find(|&(j, d)| j != k && d.khz != here && unit_covers(d, pos, alt_ft))
            .map(|(_, d)| d)
        {
            return HandOff::Contact {
                station: d.callsign.clone(),
                khz: d.khz,
            };
        }
        let inside = (0..self.stations.len()).filter(|&i| self.in_terminal_area(i, pos, alt_ft));
        let Some(i) = self.nearest(inside, Some(pos)) else {
            return HandOff::Release;
        };
        let positions = &self.stations[i].procs.positions;
        let of = |kind| positions.iter().find(|q| q.role == kind);
        let staffed = of(FreqKind::Approach)
            .or_else(|| of(FreqKind::Tower))
            .or_else(|| {
                positions
                    .iter()
                    .find(|q| Role::from_freq_kind(q.role).is_some())
            });
        if let Some(q) = staffed {
            return HandOff::Contact {
                station: q.callsign.clone(),
                khz: q.khz,
            };
        }
        // No controller: its published frequency (Unicom, AFIS, ...), if any.
        match positions.iter().find(|q| q.role != FreqKind::Atis) {
            Some(q) => HandOff::ChangeTo {
                station: q.callsign.clone(),
                khz: q.khz,
            },
            None => HandOff::Release,
        }
    }

    /// `true` when a CTR or TMA of station `i` contains the aircraft.
    fn in_terminal_area(&self, i: usize, pos: LatLon, alt_ft: f64) -> bool {
        self.stations[i].procs.airspaces.iter().any(|a| {
            matches!(a.kind, AirspaceKind::Ctr | AirspaceKind::Tma)
                && (f64::from(a.lower_ft)..=f64::from(a.upper_ft)).contains(&alt_ft)
                && point_in_polygon([pos.lat, pos.lon], &a.polygon)
        })
    }

    /// Unit `k`'s transmission of `items` to `to` (nothing without items); the callsign in
    /// full on the first contact.
    fn unit_output(
        &self,
        k: usize,
        to: Option<&Callsign>,
        items: Vec<AtcItem>,
        lang: Lang,
    ) -> Vec<AtcOutput> {
        if items.is_empty() {
            return Vec::new();
        }
        let def = &self.unit_defs[k];
        let first = items.contains(&AtcItem::Identified);
        let message = AtcMessage {
            to: to.cloned(),
            from: def.callsign.clone(),
            items,
            lang,
        };
        let text = render(&message, first);
        vec![AtcOutput {
            freq_khz: def.khz,
            station: def.callsign.clone(),
            message,
            text,
        }]
    }
}
