//! Delivery + Ground + Tower dialogue engine: pilot text and telemetry in, controller
//! messages out.
//!
//! Deterministic. The position answering a transmission is the one whose frequency it was
//! made on (`procedures.toml` positions); station names, frequencies, runways, taxiways and
//! altitudes all come from navdata or procedures.
//!
//! Per transmission: unknown frequency -> silence (Approach also answers on its alternate
//! frequencies, on the one called); callsign not heard or not registered ->
//! `SayAgainCallsign`; a pending readback is checked first (correct -> silent unless the pilot
//! also made a request; a request while it is still open -> `ReadBack(pending)`, no new
//! clearance; otherwise the correction, at most twice, then the readback is accepted;
//! a pending readback also expires once its phase has passed); then the request is
//! handled by the position ([`delivery`], [`ground`], [`tower`], [`approach`]), a request for
//! another position being answered with `Contact` for it. A transmission on the frequency of
//! an adjacent unit is answered only by that unit, once, to a flight handed to it
//! ([`approach`]). Delivery is silent while closed ([`AtcEngine::delivery_open`], from the
//! sim UTC time of the latest telemetry); Ground then issues the clearances. "Say again"
//! is answered first with the position's last transmission to the aircraft (without a
//! callsign: to the one aircraft on frequency it last spoke to); taxi help with the route again from where the aircraft is (and where
//! its stand is when close), the second time with a follow-me car ([`follow_me`]); "on
//! stand" with "good day", or where the stand is when not on it ([`stand`]). Every
//! other call from a known aircraft is answered too: "pass your message" to a bare call,
//! "say again" to anything not understood; acknowledgements (also a repeated readback)
//! stay unanswered. Greetings and farewells are returned (handovers end
//! with one: "have a nice flight" departing, "good day" arrived). An unanswered transmission
//! is joined with the next one on its frequency when the pilot finishes it there. Per
//! telemetry sample: phase tracking (circuit legs only for aircraft without a flight plan that
//! are not entering or leaving via VFR points, [`vfr`]),
//! permissions, compliance (deviations are spoken by the station owning the aircraft, none
//! after the hand-off to an adjacent unit), runway crossings, handovers (Ground -> Tower at
//! the holding point, Tower -> Approach for an IFR departure once airborne and for an IFR
//! arrival after a missed approach, Tower -> Ground after a full-stop landing) and Approach's
//! direct routing, hand-offs, radar vectors and approach clearances. An aircraft squawking
//! 7600 gets no deviation calls and nothing more from Approach.
//!
//! Language ([`LanguageMode`]): fixed, or per aircraft the language of its last understood
//! transmission; replies are rendered in it, readbacks are judged on items whatever the
//! language, and the ATIS follows the last understood transmission. Urgent instructions
//! (deviation replies) are always English (spec, "Язык"), and so is everything Approach and
//! the adjacent units say (spec 3.1, `lang_for`).

mod approach;
mod delivery;
mod follow_me;
mod ground;
mod snapshot;
mod stand;
mod tower;
mod vfr;

pub(crate) use delivery::direct_key;
pub use follow_me::{READY_S as FOLLOW_ME_READY_S, VehicleCommand, VehicleOrder};
pub use snapshot::{EngineSnapshot, SNAPSHOT_VERSION, SnapshotMismatch};

use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;

use atc_geo::{LatLon, RunwayFrame, distance_m};
use atc_navdata::fixes::ProcedureSet;
use atc_navdata::model::{Airport, Destination};
use atc_navdata::procedures::{PositionDef, Procedures};
use atc_phraseology::{
    AtcItem, AtcMessage, Callsign, CircuitLeg, ClearanceKind, Courtesy, FlightRules, Lang,
    PilotIntent, PilotMessage, Token, UnableReason, Vocabulary, normalize, parse_pilot_with,
    render,
};

use crate::atis::{AtisService, InitialContact, MAX_TAILWIND_KT, Weather, tailwind_kt};
use crate::compliance::{ComplianceMonitor, Deviation, hold_line_cross_m};
use crate::handover::FlightHandover;
use crate::phase::{FlightPhase, PhaseTracker};
use crate::readback::{ReadbackResult, Strictness, check_readback, correction};
use crate::runway::RunwayResource;
use crate::squawk::{DEFAULT_POOL, discrete_codes};
use crate::state::{AircraftState, SimTime};
use crate::strip::{FlightPlan, PilotProfile, Role, Strip};
use crate::taxi::TaxiGraph;
use delivery::RunwayChange;

/// A parked aircraft within this distance of a stand started from it.
const HOME_STAND_M: f64 = 50.0;
/// After landing, slowing below this ground speed on the runway is a full stop.
const FULL_STOP_KT: f64 = 30.0;
/// A failed readback is asked for again this many times, then accepted.
const MAX_READBACK_REQUESTS: u8 = 2;
/// An unanswered transmission is joined with the next one on its frequency within this
/// many seconds: the pilot broke it off and finished it.
const FRAGMENT_S: f64 = 30.0;

#[derive(Clone, Debug, PartialEq)]
pub struct EngineConfig {
    pub strictness: Strictness,
    /// Transponder codes assigned to aircraft; codes that are not discrete (non-octal
    /// digits, VFR, emergency) are skipped.
    pub squawk_pool: RangeInclusive<u16>,
    /// The language pilots are heard and answered in.
    pub language: LanguageMode,
}

impl EngineConfig {
    /// `strictness` with the default squawk pool, in English only.
    pub fn new(strictness: Strictness) -> Self {
        EngineConfig {
            strictness,
            squawk_pool: DEFAULT_POOL,
            language: LanguageMode::Fixed(Lang::En),
        }
    }
}

/// The language of the radio exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanguageMode {
    /// Each aircraft is answered in the language of its last understood transmission
    /// (English until it is understood in another).
    Auto,
    /// Always this language, whatever language a transmission was detected in.
    Fixed(Lang),
}

/// One controller transmission.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AtcOutput {
    pub freq_khz: u32,
    /// Transmitting station, e.g. `"Kraków Ground"`.
    pub station: String,
    pub message: AtcMessage,
    /// Rendered words for TTS.
    pub text: String,
}

/// The flight is over: [`Strip::end_flight`], keeping the contact of the station working
/// the aircraft (its next words still belong to the exchange in progress).
fn end_flight_keeping_contact(strip: &mut Strip) {
    let owner = strip.owner;
    let in_contact = strip.first_contact_done.contains(&owner);
    strip.end_flight();
    if in_contact {
        strip.first_contact_done.insert(owner);
    }
}

/// Telemetry-side state of one aircraft.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Surveillance {
    tracker: PhaseTracker,
    monitor: ComplianceMonitor,
    last: AircraftState,
    circuit_alt_ft: i32,
}

#[derive(Clone, Debug)]
pub struct AtcEngine {
    airport: Airport,
    procs: Procedures,
    cfg: EngineConfig,
    graph: TaxiGraph,
    atis: Option<AtisService>,
    /// The single runway of v1 airports.
    runway: RunwayResource,
    pilots: Vec<PilotProfile>,
    strips: Vec<Strip>,
    tracks: HashMap<Callsign, Surveillance>,
    /// The last transmission that was not understood, as (position, text, time).
    fragment: Option<(Role, String, f64)>,
    /// The last words each aircraft was told; repeating them acknowledges.
    last_told: HashMap<Callsign, String>,
    /// The last transmission of each position to each aircraft, repeated on "say again".
    last_sent: HashMap<(Callsign, Role), AtcOutput>,
    /// The aircraft each position spoke to last.
    last_addressed: HashMap<Role, Callsign>,
    /// The language of the last understood pilot transmission (`Auto`).
    last_lang: Lang,
    /// Stations that have returned an aircraft's greeting.
    greeted: HashSet<(Callsign, Role)>,
    /// The last transponder code assigned.
    last_squawk: Option<u16>,
    /// Vehicle orders not yet taken ([`AtcEngine::take_vehicle_orders`]).
    vehicle_orders: Vec<VehicleOrder>,
    /// The last vehicle order id given.
    next_vehicle: u32,
    /// The airport's procedures; `None`: no IFR.
    procedure_set: Option<ProcedureSet>,
    /// Airports a flight may be cleared to.
    destinations: Vec<Destination>,
    /// Words the parser knows besides the callsigns: VFR points, procedure fixes, places.
    vocab: Vocabulary,
    /// Sim UTC time of day (s) and day of year of the latest telemetry sample that had a
    /// time; `(None, None)` before any.
    clock: (Option<f64>, Option<u16>),
}

impl AtcEngine {
    /// An engine without IFR data ([`AtcEngine::with_procedure_set`]) or destinations
    /// ([`AtcEngine::with_destinations`]).
    pub fn new(airport: Airport, procs: Procedures, cfg: EngineConfig) -> Self {
        let vocab = Vocabulary {
            points: procs
                .vfr_points
                .iter()
                .map(|p| p.name.to_uppercase())
                .collect(),
            ..Vocabulary::default()
        };
        AtcEngine {
            graph: TaxiGraph::build(&airport.taxi),
            airport,
            procs,
            cfg,
            atis: None,
            runway: RunwayResource::default(),
            pilots: Vec::new(),
            strips: Vec::new(),
            tracks: HashMap::new(),
            fragment: None,
            last_told: HashMap::new(),
            last_sent: HashMap::new(),
            last_addressed: HashMap::new(),
            last_lang: Lang::En,
            greeted: HashSet::new(),
            last_squawk: None,
            vehicle_orders: Vec::new(),
            next_vehicle: 0,
            procedure_set: None,
            destinations: Vec::new(),
            vocab,
            clock: (None, None),
        }
    }

    /// Adds an aircraft to the frequency; registering the same callsign again is ignored.
    pub fn register_pilot(&mut self, p: PilotProfile) {
        if self.strip_index(&p.callsign).is_some() {
            return;
        }
        let mut strip = Strip::new(p.callsign.clone(), &self.runway_in_use());
        if let LanguageMode::Fixed(l) = self.cfg.language {
            strip.lang = l;
        }
        self.strips.push(strip);
        self.pilots.push(p);
    }

    /// Drops aircraft `cs` and everything the engine keeps about it (strip, surveillance,
    /// radio memory, greetings, runway claims, its broken-off transmission); a follow-me car
    /// leading it is cancelled ([`AtcEngine::take_vehicle_orders`]). Registered again, it
    /// starts afresh. `false`: not registered.
    pub fn remove_pilot(&mut self, cs: &Callsign) -> bool {
        let Some(i) = self.strip_index(cs) else {
            return false;
        };
        self.cancel_follow_me(i);
        let lang = self.strips[i].lang;
        if self
            .fragment
            .as_ref()
            .is_some_and(|(_, text, _)| self.heard(text, lang).1 == Some(i))
        {
            self.fragment = None;
        }
        self.strips.remove(i);
        self.pilots.retain(|p| &p.callsign != cs);
        self.tracks.remove(cs);
        self.last_told.remove(cs);
        self.last_sent.retain(|(c, _), _| c != cs);
        self.last_addressed.retain(|_, c| c != cs);
        self.greeted.retain(|(c, _)| c != cs);
        self.runway.release(cs);
        true
    }

    /// Hands flight `cs` out of this engine: its data for the next station, then the flight
    /// is removed completely ([`AtcEngine::remove_pilot`]). `None`: not registered.
    pub fn export_flight(&mut self, cs: &Callsign) -> Option<FlightHandover> {
        let strip = self.strip(cs)?;
        let plan = strip.plan.clone();
        let h = FlightHandover {
            callsign: cs.clone(),
            aircraft_type: self
                .pilots
                .iter()
                .find(|p| &p.callsign == cs)
                .map(|p| p.aircraft_type.clone())
                .unwrap_or_default(),
            cleared_ft: plan.as_ref().map(|p| p.cleared_ft).filter(|&ft| ft > 0),
            rules: plan.as_ref().map(|p| {
                if p.ifr {
                    FlightRules::Ifr
                } else {
                    FlightRules::Vfr
                }
            }),
            squawk: strip.squawk,
            plan,
        };
        self.remove_pilot(cs);
        Some(h)
    }

    /// Takes in a flight handed over by another station: registered afresh (replacing any
    /// strip of the same callsign) with its plan (without the departure airport's SID,
    /// runway and direct), cleared level and transponder code; its first call is answered as
    /// a flight handed over ([`Strip::handed_in`]). The owner is set to Approach (IFR) or
    /// Tower and moves to the position it first calls.
    pub fn import_flight(&mut self, h: FlightHandover) {
        self.remove_pilot(&h.callsign);
        self.register_pilot(PilotProfile {
            callsign: h.callsign.clone(),
            aircraft_type: h.aircraft_type,
        });
        let i = self.strip_index(&h.callsign).expect("registered above");
        let strip = &mut self.strips[i];
        let mut plan = h.plan;
        if let Some(p) = plan.as_mut() {
            p.sid = None;
            p.dep_runway = None;
            p.direct = None;
            if let Some(ft) = h.cleared_ft {
                p.cleared_ft = ft;
            }
        }
        let ifr = plan
            .as_ref()
            .map_or(h.rules == Some(FlightRules::Ifr), |p| p.ifr);
        strip.owner = if ifr { Role::Approach } else { Role::Tower };
        strip.plan = plan;
        strip.squawk = h.squawk;
        strip.handed_in = true;
    }

    /// The frequency (kHz) of the adjacent unit flight `cs` was handed off to, once the
    /// pilot has answered the hand-off here (its next understood transmission: the readback
    /// where one is required, else any acknowledgement) or called the unit: the flight is to
    /// be taken by the station working that frequency. `None` before, for handovers between
    /// this engine's own positions, for a release without a unit, and once the flight was
    /// taken (exported).
    pub fn handoff_target(&self, cs: &Callsign) -> Option<u32> {
        let strip = self.strip(cs)?;
        strip.handed_off_khz.filter(|_| {
            strip.handed_off.is_some() && strip.handoff_acked && strip.pending_readback.is_none()
        })
    }

    /// New weather: issues the first ATIS, or a new letter on a significant change.
    pub fn on_weather(&mut self, w: Weather) {
        match &mut self.atis {
            Some(atis) => {
                atis.update(w, &self.airport, &self.procs);
            }
            None => self.atis = Some(AtisService::new(w, &self.airport, &self.procs)),
        }
    }

    /// The configured language mode.
    pub fn language(&self) -> LanguageMode {
        self.cfg.language
    }

    /// The language the ATIS is broadcast in: the fixed language, or in `Auto` the
    /// language of the most recent understood pilot transmission (English before any).
    pub fn atis_lang(&self) -> Lang {
        match self.cfg.language {
            LanguageMode::Fixed(l) => l,
            LanguageMode::Auto => self.last_lang,
        }
    }

    /// Current ATIS letter and spoken text in [`AtcEngine::atis_lang`]; `None` before the
    /// first weather report. The airport is named as the ATIS position calls itself
    /// (`"Kraków ATIS"` -> `"Kraków"`), falling back to the navdata name.
    pub fn atis_broadcast(&self) -> Option<(char, String)> {
        let atis = self.atis.as_ref()?;
        let name = self
            .procs
            .positions
            .iter()
            .find(|p| p.role == atc_navdata::model::FreqKind::Atis)
            .and_then(|p| p.callsign.strip_suffix(" ATIS"))
            .unwrap_or(&self.airport.name);
        Some((
            atis.current().letter,
            atis.text_with(name, self.atis_lang(), self.initial_contact().as_ref()),
        ))
    }

    /// The unit the ATIS names for the first call of a departure, once the airport has IFR
    /// procedures: Delivery while it is open, otherwise Ground.
    fn initial_contact(&self) -> Option<InitialContact> {
        self.procedure_set.as_ref()?;
        let (role, clearance) = if self.delivery_open_now() {
            (Role::Delivery, true)
        } else {
            (Role::Ground, false)
        };
        let pos = self.position(role)?;
        Some(InitialContact {
            unit: pos
                .callsign
                .rsplit(' ')
                .next()
                .unwrap_or(&pos.callsign)
                .to_string(),
            khz: pos.khz,
            clearance,
        })
    }

    pub fn strip(&self, cs: &Callsign) -> Option<&Strip> {
        self.strip_index(cs).map(|i| &self.strips[i])
    }

    pub fn on_telemetry(&mut self, s: AircraftState) -> Vec<AtcOutput> {
        // A sample without time (old client, replay) keeps the last known time.
        if s.utc_s.is_some() {
            self.clock = (s.utc_s, s.day_of_year);
        }
        let Some(i) = self.strip_index(&s.callsign) else {
            return Vec::new();
        };
        if !self.tracks.contains_key(&s.callsign) {
            let runway = self.runway_in_use();
            let Some(sv) = self.surveillance(&runway, &s) else {
                return Vec::new();
            };
            let home = self
                .airport
                .stands
                .iter()
                .find(|st| distance_m(st.pos, s.pos) < HOME_STAND_M)
                .map(|st| st.name.clone());
            let strip = &mut self.strips[i];
            strip.runway = runway;
            strip.home_stand = home;
            self.tracks.insert(s.callsign.clone(), sv);
        }
        let clearance = self.ifr_clearance(i);
        let circuit = self.strips[i].plan.is_none() && self.strips[i].vfr.is_none();
        let sv = self.tracks.get_mut(&s.callsign).expect("tracked above");
        // An aircraft with a flight plan, or entering or leaving via points: no circuit legs.
        sv.tracker.set_circuit(circuit);
        sv.monitor.set_clearance(clearance);
        let phase = sv.tracker.update(&s);
        let strip = &mut self.strips[i];
        let prev = std::mem::replace(&mut strip.phase, phase);
        update_permissions(strip, &mut self.runway, prev, phase, sv.circuit_alt_ft);
        if taxi_done(prev, phase) {
            strip.taxi_target = None;
            strip.taxi_helps = 0;
            strip.follow_me = None;
        }
        if matches!(
            phase,
            FlightPhase::Parked | FlightPhase::TaxiOut | FlightPhase::TaxiIn
        ) {
            strip.departure_sent = false;
            strip.vfr = None;
        }
        // Parked again after a flight: the flight is over and the next departure starts
        // afresh (a new clearance, start-up and taxi clearance in full, Approach's service).
        // The station working the aircraft as it parks keeps its contact: the on-stand report
        // and the farewell still belong to the exchange in progress (abbreviated callsign).
        if phase == FlightPhase::Parked && prev != FlightPhase::Parked {
            end_flight_keeping_contact(strip);
        }
        if strip.pending_readback.is_some() && strip.pending_expired() {
            strip.readback_complete();
        }
        strip.radio_failure = s.xpdr_code == Some(approach::RADIO_FAILURE_SQUAWK);
        let deviations = sv.monitor.update(&s, phase, &strip.permissions);
        let (pos, gs_kt, t, vs_fpm) = (s.pos, s.gs_kt, s.t.0, s.vs_fpm);
        sv.last = s;
        self.note_missed_approach(i, prev, phase, pos, vs_fpm);

        let mut out = self.deviation_replies(i, deviations);
        out.extend(self.crossing_update(i, pos));
        out.extend(self.follow_me_update(i, t));
        out.extend(self.handover(i, gs_kt));
        out.extend(self.tower_call(i));
        out.extend(self.vfr_exit_update(i));
        out.extend(self.establish_check(i));
        out.extend(self.approach_update(i));
        out
    }

    /// A pilot transmission on `freq_khz` at sim time `t`, detected as `lang` (joins
    /// broken-off transmissions). `Fixed` ignores `lang`; `Auto` hears it in `lang` and
    /// answers in it when understood there, otherwise in the aircraft's language (a
    /// misdetected short phrase does not switch it).
    pub fn on_transmission(
        &mut self,
        freq_khz: u32,
        text: &str,
        lang: Lang,
        t: SimTime,
    ) -> Vec<AtcOutput> {
        let Some(role) = self.role_at(freq_khz) else {
            return self.adjacent_call(freq_khz, text, lang);
        };
        // A closed position does not answer.
        if role == Role::Delivery && !self.delivery_open_now() {
            return Vec::new();
        }
        // A broken-off transmission finished in this one is heard as a whole when the
        // whole says more: names a known aircraft, makes a request, or completes a readback.
        let joined = self
            .fragment
            .take()
            .filter(|(r, _, ft)| *r == role && (t.0 - ft).abs() <= FRAGMENT_S)
            .map(|(_, prev, _)| format!("{prev}, {text}"));
        let alone = self.hear(text, lang);
        if alone.0.intent == PilotIntent::SayAgain {
            if let Some(i) = alone.1 {
                self.understood_in(i, alone.2);
            }
            return self.repeat(role, freq_khz, alone.1, lang);
        }
        let (text, msg, i, heard_lang) = match (alone, joined.map(|j| (self.hear(&j, lang), j))) {
            ((msg, Some(i), _), Some(((jmsg, Some(ji), jl), j)))
                if ji == i && self.joined_says_more(i, &msg, &jmsg) =>
            {
                (j, jmsg, i, jl)
            }
            ((msg, Some(i), l), _) => (text.to_string(), msg, i, l),
            (_, Some(((jmsg, Some(ji), jl), j))) => (j, jmsg, ji, jl),
            _ => {
                self.fragment = Some((role, text.to_string(), t.0));
                return self.say_again_callsign(role, freq_khz, self.unknown_lang(lang));
            }
        };
        if understood(&msg) {
            self.understood_in(i, heard_lang);
            // The pilot has answered after its hand-off (read back, or acknowledged where no
            // readback is required).
            if self.strips[i].handed_off.is_some() {
                self.strips[i].handoff_acked = true;
            }
        }
        let text = text.as_str();
        if let Some(to) = self.called_elsewhere(role, &msg) {
            let mut items: Vec<AtcItem> = self
                .position(role)
                .map(|p| AtcItem::ThisIs {
                    station: p.callsign.clone(),
                })
                .into_iter()
                .collect();
            items.extend(self.redirect(i, to));
            return self.transmit(i, role, items);
        }
        if self.starts_new_flight(i, role, &msg) {
            self.strips[i].end_flight();
        }
        let strip = &mut self.strips[i];
        if role == Role::Approach {
            // Radio failure (7600): Approach no longer talks to the aircraft.
            if strip.radio_failure {
                return Vec::new();
            }
            strip.approach_khz = Some(freq_khz);
        }
        if mentions_full_stop(text, heard_lang) {
            strip.full_stop_requested = true;
        }
        // Calling the station it was sent to completes the handover; calling a station on
        // the pilot's own initiative makes it the owner.
        match strip.handoff_to {
            Some(to) if to == role => strip.readback_complete(),
            Some(_) => {}
            None => strip.owner = role,
        }
        // A flight handed in is answered as handed over by Approach; elsewhere its call is
        // served as it is.
        if role != Role::Approach {
            strip.handed_in = false;
        }
        // Taxi help replaces an open taxi clearance with the route from where it is now; a
        // follow-me request and an on-stand report are answered whatever is open.
        let help = role == Role::Ground
            && matches!(
                msg.intent,
                PilotIntent::RequestTaxiHelp
                    | PilotIntent::RequestFollowMe
                    | PilotIntent::ReportOnStand
            );
        if let Some(pending) = strip.pending_readback.clone().filter(|_| !help) {
            let result = check_readback(&pending, &msg.items);
            let request = is_request(&msg.intent);
            if result == ReadbackResult::Correct || strip.readback_requests >= MAX_READBACK_REQUESTS
            {
                // Correct, or asked for often enough: accept and move on.
                let issued = std::mem::take(&mut strip.pending_items);
                strip.readback_complete();
                if !request {
                    let mut items = self.transponder_recheck(i, role);
                    if items.is_empty() && result == ReadbackResult::Correct {
                        items = self.clearance_confirmed(i, role, &issued);
                    }
                    if items.is_empty() {
                        items = msg.farewell.into_iter().map(AtcItem::Courtesy).collect();
                    }
                    self.greet_back(i, role, msg.greeting, &mut items);
                    return self.transmit(i, role, items);
                }
            } else {
                strip.readback_requests += 1;
                let items = if request {
                    vec![AtcItem::ReadBack(pending)]
                } else {
                    // An incomplete readback may be finished in the next transmission.
                    if matches!(result, ReadbackResult::Missing(_)) {
                        self.fragment = Some((role, text.to_string(), t.0));
                    }
                    correction(&result, &strip.pending_items)
                };
                let mut items = items;
                self.greet_back(i, role, msg.greeting, &mut items);
                return self.transmit(i, role, items);
            }
        }
        // The readback of a VFR entry or exit instruction is not answered.
        if role == Role::Tower && self.vfr_readback(i, &msg) {
            return Vec::new();
        }
        let mut items = match role {
            Role::Tower if !is_request(&msg.intent) => self.transponder_recheck(i, role),
            _ => Vec::new(),
        };
        if items.is_empty() {
            items = match role {
                Role::Ground => self.ground_request(i, &msg, t.0),
                Role::Tower => self.tower_request(i, &msg),
                Role::Delivery => self.delivery_request(i, &msg),
                Role::Approach => self.approach_request(i, &msg),
            };
        }
        // A "ready for departure" after Tower already gave the departure is not answered.
        if items.is_empty() && role == Role::Tower && self.repeated_ready(i, &msg.intent) {
            return Vec::new();
        }
        // Every call gets an answer, except acknowledgements (also repeated readbacks: only
        // words it was told); a farewell is returned.
        if items.is_empty()
            && let Some(negative) = self.clearance_not_given(i, &msg)
        {
            items.push(negative);
        }
        if items.is_empty() {
            let reply = match self.content(i, &msg) {
                Content::Message => Some(AtcItem::SayAgain),
                Content::Call => Some(AtcItem::PassYourMessage),
                Content::Acknowledgement => None,
            };
            if let Some(reply) = reply {
                self.fragment = Some((role, text.to_string(), t.0));
                items.push(reply);
            } else if let Some(bye) = msg.farewell {
                items.push(AtcItem::Courtesy(bye));
            }
        }
        self.greet_back(i, role, msg.greeting, &mut items);
        self.transmit(i, role, items)
    }

    /// Spec 3.6: a call to `role` that begins a new flight of aircraft `i`, whose last flight
    /// ended away from home (it never parked here again): an airborne inbound call or
    /// approach request to Approach from a flight that was handed off or still holds a
    /// departure plan; a request to join (or an inbound call) to Tower from a flight that was
    /// handed off.
    fn starts_new_flight(&self, i: usize, role: Role, msg: &PilotMessage) -> bool {
        let strip = &self.strips[i];
        // A flight handed in from another station carries its plan into its first call.
        if !strip.phase.is_airborne() || strip.handed_in {
            return false;
        }
        match (role, &msg.intent) {
            (
                Role::Approach,
                PilotIntent::ReportInbound { .. } | PilotIntent::RequestApproach { .. },
            ) => self.flight_went_away(i),
            (Role::Tower, PilotIntent::RequestJoin { .. } | PilotIntent::ReportInbound { .. }) => {
                strip.handed_off.is_some()
            }
            _ => false,
        }
    }

    /// The flight of aircraft `i` was handed off to an adjacent unit, or it still holds a
    /// departure plan (a plan without an approach to this airport).
    pub(super) fn flight_went_away(&self, i: usize) -> bool {
        let strip = &self.strips[i];
        strip.handed_off.is_some() || strip.plan.as_ref().is_some_and(|p| p.approach.is_none())
    }

    /// The departure again when Tower asked aircraft `i` to put its transponder right and
    /// the pilot acknowledged: the take-off clearance, or the check repeated.
    fn transponder_recheck(&mut self, i: usize, role: Role) -> Vec<AtcItem> {
        if role != Role::Tower || !self.strips[i].transponder_check {
            return Vec::new();
        }
        self.strips[i].transponder_check = false;
        self.departure(i)
    }

    /// "Say again" from aircraft `i`: the last transmission of `role` to it, word for word.
    /// Without a callsign, the one aircraft on the frequency when it was the last one spoken
    /// to; otherwise "say again your callsign" (in `lang`, see [`AtcEngine::unknown_lang`]).
    /// Nothing said to it yet: "pass your message".
    fn repeat(&mut self, role: Role, khz: u32, i: Option<usize>, lang: Lang) -> Vec<AtcOutput> {
        let i = i.or_else(|| {
            let mut on_freq = (0..self.strips.len()).filter(|&k| {
                let s = &self.strips[k];
                s.owner == role && self.tracks.contains_key(&s.callsign)
            });
            let only = on_freq.next().filter(|_| on_freq.next().is_none())?;
            (self.last_addressed.get(&role) == Some(&self.strips[only].callsign)).then_some(only)
        });
        let Some(i) = i else {
            return self.say_again_callsign(role, khz, self.unknown_lang(lang));
        };
        match self.last_sent.get(&(self.strips[i].callsign.clone(), role)) {
            Some(last) => vec![last.clone()],
            None => self.transmit(i, role, vec![AtcItem::PassYourMessage]),
        }
    }

    /// A greeting is returned once per station, in front of the reply.
    fn greet_back(
        &mut self,
        i: usize,
        role: Role,
        greeting: Option<Courtesy>,
        items: &mut Vec<AtcItem>,
    ) {
        if items.is_empty() {
            return;
        }
        if let Some(g) = greeting
            && self.greeted.insert((self.strips[i].callsign.clone(), role))
        {
            items.insert(0, AtcItem::Courtesy(g));
        }
    }

    /// The transmission as heard, the aircraft it names and the language it was heard in.
    /// `Fixed(l)`: in `l`. `Auto`: in the detected `lang` when it is understood there,
    /// otherwise (or when it is only an acknowledgement or a courtesy that the aircraft's
    /// language understands as well, such as "roger") in the aircraft's language: a short
    /// phrase misdetected as the other language does not switch the exchange. An English
    /// aircraft switches to Polish only when the Polish words changed what was understood.
    fn hear(&self, text: &str, lang: Lang) -> (PilotMessage, Option<usize>, Lang) {
        let lang = match self.cfg.language {
            LanguageMode::Fixed(l) => l,
            LanguageMode::Auto => lang,
        };
        let (msg, i) = self.heard(text, lang);
        let Some(k) = i.filter(|_| self.cfg.language == LanguageMode::Auto) else {
            return (msg, i, lang);
        };
        let session = self.strips[k].lang;
        // Into Polish only when Polish words made a difference: the Polish parser passes
        // English words through, so English text tagged `pl` is understood there too. When
        // the English parse understands it just as well, it stays English. (The other way
        // needs no such check: Polish words mean nothing to the English parser, so an
        // English transmission in a Polish session that is understood switches back.)
        if lang == Lang::Pl && session == Lang::En {
            let (emsg, ei) = self.heard(text, Lang::En);
            if ei == Some(k) && same_understanding(&msg, &emsg) {
                return (emsg, ei, Lang::En);
            }
        }
        if session == lang || (understood(&msg) && substantive(&msg)) {
            return (msg, i, lang);
        }
        let (smsg, si) = self.heard(text, session);
        if understood(&msg) && !understood(&smsg) {
            (msg, i, lang)
        } else if si == Some(k) {
            (smsg, si, session)
        } else {
            (msg, i, session)
        }
    }

    /// Aircraft `i` was understood in `lang`: it is answered in it from now on, and the
    /// ATIS follows it (`Auto`; in `Fixed` it is the fixed language anyway).
    fn understood_in(&mut self, i: usize, lang: Lang) {
        self.strips[i].lang = lang;
        self.last_lang = lang;
    }

    /// The language to answer a transmission naming no known aircraft in: the fixed one, or
    /// the transmission's own.
    fn unknown_lang(&self, lang: Lang) -> Lang {
        match self.cfg.language {
            LanguageMode::Fixed(l) => l,
            LanguageMode::Auto => lang,
        }
    }

    /// The transmission parsed in `lang` and the registered aircraft it names. The parser
    /// knows the registered callsigns, so letters around one go back to the route. When the
    /// parsed callsign is no registered aircraft (another letter group taken for it, a letter
    /// lost), every letter group of the text is tried: as said, then with letters missing.
    fn heard(&self, text: &str, lang: Lang) -> (PilotMessage, Option<usize>) {
        let vocab = Vocabulary {
            callsigns: self.strips.iter().map(|s| s.callsign.clone()).collect(),
            ..self.vocab.clone()
        };
        let msg = parse_pilot_with(text, lang, &vocab);
        let i = msg
            .callsign
            .as_deref()
            .and_then(|h| self.resolve_callsign(h))
            .or_else(|| {
                let groups = letter_groups(text, lang);
                let unique = |hit: &dyn Fn(&Strip, &str) -> bool| {
                    let mut hits = (0..self.strips.len())
                        .filter(|&i| groups.iter().any(|g| hit(&self.strips[i], g)));
                    let first = hits.next()?;
                    hits.next().is_none().then_some(first)
                };
                unique(&|s, g| s.callsign.matches(g))
                    .or_else(|| unique(&|s, g| letters_lost(&s.callsign.0, g)))
            });
        (msg, i)
    }

    /// The transmission joined with the fragment before it is a request or a correct
    /// readback where the transmission alone is not.
    fn joined_says_more(&self, i: usize, alone: &PilotMessage, joined: &PilotMessage) -> bool {
        if let Some(pending) = &self.strips[i].pending_readback {
            let correct =
                |m: &PilotMessage| check_readback(pending, &m.items) == ReadbackResult::Correct;
            if correct(joined) && !correct(alone) {
                return true;
            }
        }
        alone.intent == PilotIntent::Unknown && is_request(&joined.intent)
    }

    /// The correction when the pilot reads back a take-off or landing clearance aircraft
    /// `i` does not have.
    fn clearance_not_given(&self, i: usize, msg: &PilotMessage) -> Option<AtcItem> {
        let (kinds, perm) = (&msg.items.kinds, &self.strips[i].permissions);
        if kinds.contains(&ClearanceKind::Takeoff) && !perm.may_take_off {
            Some(AtcItem::NotClearedForTakeoff)
        } else if (kinds.contains(&ClearanceKind::Land)
            || kinds.contains(&ClearanceKind::TouchAndGo))
            && !perm.may_land
        {
            Some(AtcItem::NotClearedToLand)
        } else {
            None
        }
    }

    /// "Station calling <station>, say again": a transmission too unclear to read, in the
    /// fixed language or (`Auto`) the speaker's `lang` (its aircraft's language), never
    /// another aircraft's. Nothing for a frequency no position uses.
    pub fn say_again(&self, freq_khz: u32, lang: Lang) -> Vec<AtcOutput> {
        let Some(pos) = self
            .procs
            .positions
            .iter()
            .find(|p| p.khz == freq_khz)
            .or_else(|| {
                self.role_at(freq_khz)
                    .filter(|r| *r == Role::Approach)
                    .and_then(|r| self.position(r))
            })
        else {
            return Vec::new();
        };
        if pos.role == atc_navdata::model::FreqKind::Delivery && !self.delivery_open_now() {
            return Vec::new();
        }
        let lang = self.unknown_lang(lang);
        let message = AtcMessage {
            to: None,
            from: pos.callsign.clone(),
            items: vec![AtcItem::SayAgainStation {
                station: pos.callsign.clone(),
            }],
            lang: Role::from_freq_kind(pos.role).map_or(lang, |r| lang_for(r, lang)),
        };
        let text = render(&message, true);
        vec![AtcOutput {
            freq_khz,
            station: pos.callsign.clone(),
            message,
            text,
        }]
    }

    /// What an unanswered transmission of aircraft `i` says besides the callsign: a
    /// message, only a call (station name and greetings), or an acknowledgement (including
    /// a repeat of what it was last told). The parser sorts the words; the station names
    /// and the last transmission to the aircraft are known only here. In Polish a word
    /// repeats a told word in another form ("kołuję" for "kołuj", see [`inflected`]).
    fn content(&self, i: usize, msg: &PilotMessage) -> Content {
        let words = |text: &str| -> Vec<String> {
            normalize(text)
                .into_iter()
                .filter_map(|t| match t {
                    Token::Word(w) => Some(fold(&w)),
                    _ => None,
                })
                .collect()
        };
        let told = self
            .last_told
            .get(&self.strips[i].callsign)
            .map_or_else(Vec::new, |t| words(t));
        let polish = self.strips[i].lang == Lang::Pl;
        let was_told = |w: &str| told.iter().any(|t| t == w || (polish && inflected(t, w)));

        let station: Vec<String> = self
            .procs
            .positions
            .iter()
            .flat_map(|p| words(&p.callsign))
            .collect();
        let (mut called, mut greeted, mut acked) = (false, false, false);
        let sorted = msg
            .ack_words
            .iter()
            .map(|w| (w, Some(true)))
            .chain(msg.greeting_words.iter().map(|w| (w, Some(false))))
            .chain(msg.residual.iter().map(|w| (w, None)));
        for (w, kind) in sorted {
            let w = fold(w);
            if station.contains(&w) {
                called = true;
            } else if kind == Some(true) || was_told(&w) {
                acked = true;
            } else if kind == Some(false) {
                greeted = true;
            } else {
                return Content::Message;
            }
        }
        // A bare callsign acknowledges; naming the station or greeting it is a call.
        if called || (greeted && !acked) {
            Content::Call
        } else {
            Content::Acknowledgement
        }
    }

    fn strip_index(&self, cs: &Callsign) -> Option<usize> {
        self.strips.iter().position(|s| &s.callsign == cs)
    }

    /// The one registered aircraft matching the heard callsign (full or abbreviated).
    fn resolve_callsign(&self, heard: &str) -> Option<usize> {
        let mut hits = (0..self.strips.len()).filter(|&i| self.strips[i].callsign.matches(heard));
        let first = hits.next()?;
        hits.next().is_none().then_some(first)
    }

    fn runway_in_use(&self) -> String {
        self.atis.as_ref().map_or_else(
            || self.procs.preferred_runway.clone(),
            |a| a.current().runway.clone(),
        )
    }

    /// The departure runway of aircraft `i`: the one it asked for and was given, else the
    /// runway in use.
    pub(super) fn departure_runway(&self, i: usize) -> String {
        self.strips[i]
            .requested_runway
            .clone()
            .unwrap_or_else(|| self.runway_in_use())
    }

    /// "Request departure runway ...", on Delivery or Ground while parked or taxiing out:
    /// approved when its tailwind is within [`MAX_TAILWIND_KT`], else (or an unknown runway)
    /// "unable, runway in use ...". The runway given is kept for the flight, also when it is
    /// the runway in use. A flight already cleared from another runway also gets the runway
    /// change (with the new SID), to be read back; an IFR flight without a SID from the
    /// requested runway keeps its runway ("unable, runway in use" naming it). A flight
    /// taxiing to another runway is also re-cleared to the new holding point
    /// ([`AtcEngine::retaxi_to_runway`]).
    /// A departure runway request that can only mean the departure, so Ground and Delivery
    /// answer it like ready for departure: at the holding point or on the runway, or taxiing
    /// to the runway asked for.
    pub(super) fn runway_request_is_ready(&self, i: usize, runway: &str) -> bool {
        let strip = &self.strips[i];
        match strip.phase {
            FlightPhase::HoldingShort | FlightPhase::OnRunway => true,
            FlightPhase::TaxiOut => strip.runway == runway,
            _ => false,
        }
    }

    pub(super) fn request_departure_runway(&mut self, i: usize, runway: &str) -> Vec<AtcItem> {
        if !matches!(
            self.strips[i].phase,
            FlightPhase::Parked | FlightPhase::TaxiOut
        ) {
            return Vec::new();
        }
        let Some(wind) = self.wind() else {
            return standby();
        };
        let in_use = self.runway_in_use();
        if runway != in_use
            && !tailwind_kt(&wind, &self.airport, runway).is_some_and(|t| t <= MAX_TAILWIND_KT)
        {
            return vec![AtcItem::Unable(UnableReason::RunwayInUse {
                runway: in_use,
            })];
        }
        let mut items = vec![AtcItem::RunwayApproved(runway.to_string())];
        let change = self.runway_change(i, runway);
        if self.strips[i].phase == FlightPhase::TaxiOut
            && runway != self.strips[i].runway
            && !matches!(change, RunwayChange::NoSid { .. })
        {
            return self.retaxi_to_runway(i, runway, items);
        }
        match change {
            RunwayChange::Same => {}
            RunwayChange::Amended {
                items: change,
                plan,
            } => {
                items.extend(change);
                let strip = &mut self.strips[i];
                strip.plan = Some(plan);
                strip.expect_readback(&items, self.cfg.strictness);
            }
            RunwayChange::NoSid { cleared } => {
                tracing::info!(
                    "{}: no SID from requested runway {runway}, keeps runway {cleared}",
                    self.strips[i].callsign.0
                );
                return vec![AtcItem::Unable(UnableReason::RunwayInUse {
                    runway: cleared,
                })];
            }
        }
        self.strips[i].requested_runway = Some(runway.to_string());
        items
    }

    fn position(&self, role: Role) -> Option<&PositionDef> {
        self.procs
            .positions
            .iter()
            .find(|p| p.role == role.freq_kind())
    }

    /// The transponder code of aircraft `i`, assigned at first need: the pool's next code
    /// after the last one assigned (wrapping around) that no other aircraft holds. `None`
    /// when every code of the pool is taken.
    fn squawk(&mut self, i: usize) -> Option<u16> {
        if let Some(code) = self.strips[i].squawk {
            return Some(code);
        }
        let codes: Vec<u16> = discrete_codes(&self.cfg.squawk_pool).collect();
        let start = self
            .last_squawk
            .and_then(|last| codes.iter().position(|&c| c == last))
            .map_or(0, |k| k + 1);
        let code = (0..codes.len())
            .map(|k| codes[(start + k) % codes.len()])
            .find(|&c| self.strips.iter().all(|s| s.squawk != Some(c)))?;
        self.last_squawk = Some(code);
        self.strips[i].squawk = Some(code);
        Some(code)
    }

    fn last_pos(&self, i: usize) -> Option<LatLon> {
        self.tracks
            .get(&self.strips[i].callsign)
            .map(|t| t.last.pos)
    }

    /// Phase tracker and compliance monitor for `runway`; `None` if navdata or procedures
    /// lack the runway, its circuit or its holding points.
    fn surveillance(&self, runway: &str, s: &AircraftState) -> Option<Surveillance> {
        let rw = self
            .airport
            .runways
            .iter()
            .find(|r| r.ends.iter().any(|e| e.ident == runway))?;
        let k = rw.ends.iter().position(|e| e.ident == runway)?;
        let (thr, opp) = (rw.ends[k].threshold, rw.ends[1 - k].threshold);
        let frame = RunwayFrame::new(thr, opp);
        let (length, half_width) = (distance_m(thr, opp), rw.width_m / 2.0);
        let circuit = self.procs.circuits.iter().find(|c| c.runway == runway)?;
        let holds: Vec<LatLon> = self
            .graph
            .holding_points(&self.airport, runway)
            .into_iter()
            .filter_map(|id| self.graph.node_pos(id))
            .collect();
        let cross = hold_line_cross_m(&frame, &holds)?;
        Some(Surveillance {
            tracker: PhaseTracker::new(
                frame,
                length,
                half_width,
                circuit.side,
                f64::from(self.airport.elevation_ft),
                holds,
                self.airport.stands.iter().map(|st| st.pos).collect(),
            ),
            monitor: ComplianceMonitor::new(
                runway,
                frame,
                length,
                half_width,
                cross,
                f64::from(circuit.altitude_ft_msl),
            )
            .with_magnetic_variation(self.procs.magnetic_variation_deg),
            last: s.clone(),
            circuit_alt_ft: circuit.altitude_ft_msl,
        })
    }

    /// Sends `items` from `role` to aircraft `i` in its language (Approach: English) and logs
    /// them on its strip. The callsign is in full on the first message from that role,
    /// abbreviated afterwards.
    fn transmit(&mut self, i: usize, role: Role, items: Vec<AtcItem>) -> Vec<AtcOutput> {
        let lang = lang_for(role, self.strips[i].lang);
        self.transmit_in(i, role, items, lang)
    }

    /// [`AtcEngine::transmit`] in `lang`, whatever the aircraft's language. Approach speaks on
    /// the frequency the aircraft last called it on.
    fn transmit_in(
        &mut self,
        i: usize,
        role: Role,
        items: Vec<AtcItem>,
        lang: Lang,
    ) -> Vec<AtcOutput> {
        if items.is_empty() {
            return Vec::new();
        }
        let Some(pos) = self.position(role) else {
            return Vec::new();
        };
        let (station, mut freq_khz) = (pos.callsign.clone(), pos.khz);
        let strip = &mut self.strips[i];
        if role == Role::Approach
            && let Some(khz) = strip.approach_khz
        {
            freq_khz = khz;
        }
        let first_contact = strip.first_contact_done.insert(role);
        strip.issued.extend(items.iter().cloned());
        let message = AtcMessage {
            to: Some(strip.callsign.clone()),
            from: station.clone(),
            items,
            lang,
        };
        let text = render(&message, first_contact);
        let cs = strip.callsign.clone();
        self.last_told.insert(cs.clone(), text.clone());
        let out = AtcOutput {
            freq_khz,
            station,
            message,
            text,
        };
        self.last_sent.insert((cs.clone(), role), out.clone());
        self.last_addressed.insert(role, cs);
        vec![out]
    }

    /// "Say again your callsign" from `role` on `freq_khz`, the frequency called.
    fn say_again_callsign(&self, role: Role, freq_khz: u32, lang: Lang) -> Vec<AtcOutput> {
        let Some(pos) = self.position(role) else {
            return Vec::new();
        };
        let lang = lang_for(role, lang);
        let message = AtcMessage {
            to: None,
            from: pos.callsign.clone(),
            items: vec![AtcItem::SayAgainCallsign {
                station: pos.callsign.clone(),
            }],
            lang,
        };
        let text = render(&message, true);
        vec![AtcOutput {
            freq_khz,
            station: pos.callsign.clone(),
            message,
            text,
        }]
    }

    /// The position answering on `khz`: the one with that frequency, or Approach on one of
    /// its alternate frequencies (`approach_alt_khz`, the same unit).
    fn role_at(&self, khz: u32) -> Option<Role> {
        self.procs
            .positions
            .iter()
            .find(|p| p.khz == khz)
            .and_then(|p| Role::from_freq_kind(p.role))
            .or_else(|| {
                let alt = self
                    .procs
                    .approach_alt_khz
                    .iter()
                    .any(|&a| atc_proto::same_channel(a, khz));
                (alt && self.position(Role::Approach).is_some()).then_some(Role::Approach)
            })
    }

    /// Immediate replies to deviations, spoken by the station owning the aircraft. An
    /// uncleared landing is also an incursion: only the landing is spoken. "Hold position"
    /// must be read back; it replaces any open readback and cancels an open handover.
    /// Always in English, whatever the aircraft's language: the spec ("Язык") keeps urgent
    /// instructions in English; the aircraft's language is left as it is.
    fn deviation_replies(&mut self, i: usize, deviations: Vec<Deviation>) -> Vec<AtcOutput> {
        // Handed off to an adjacent unit: nobody here watches it any more. Radio failure
        // (7600): no deviation calls (AIP AD 2.22 §2.1, the airspace is kept clear instead).
        if self.strips[i].handed_off.is_some() || self.strips[i].radio_failure {
            return Vec::new();
        }
        let off_circuit = self.strips[i].phase == FlightPhase::Airborne;
        let levels: Vec<_> = deviations
            .iter()
            .map(|d| match d {
                Deviation::AltitudeDeviation { assigned, .. } => Some(self.level(*assigned)),
                _ => None,
            })
            .collect();
        let strip = &mut self.strips[i];
        if deviations.contains(&Deviation::LandingWithoutClearance) {
            strip.uncleared_landing = true;
        }
        let mut items = Vec::new();
        for (d, level) in deviations.into_iter().zip(levels) {
            match d {
                Deviation::RunwayIncursion { runway } => {
                    if !strip.uncleared_landing {
                        items.push(AtcItem::HoldPosition);
                        items.push(AtcItem::RunwayIncursion { runway });
                    }
                }
                Deviation::TakeoffWithoutClearance => items.push(AtcItem::TakenOffWithoutClearance),
                Deviation::LandingWithoutClearance => items.push(AtcItem::LandedWithoutClearance),
                Deviation::AltitudeDeviation { assigned, .. } => match level {
                    Some(level) if off_circuit => items.push(AtcItem::CheckLevel(level)),
                    _ => items.push(AtcItem::CheckAltitude {
                        circuit_ft: assigned.unsigned_abs(),
                    }),
                },
                Deviation::OffTrack { metres, .. } => items.push(AtcItem::OffTrack {
                    nm: ((f64::from(metres) / atc_geo::M_PER_NM).round() as u16).max(1),
                }),
                Deviation::HeadingDeviation { assigned, .. } => {
                    items.push(AtcItem::CheckHeading(assigned))
                }
            }
        }
        if items.contains(&AtcItem::HoldPosition) {
            strip.handoff_to = None;
            strip.expect_readback(&items, self.cfg.strictness);
        }
        let owner = strip.owner;
        // Spec, "Язык": urgent instructions are always English.
        self.transmit_in(i, owner, items, Lang::En)
    }

    /// Ground -> Tower once holding short. Tower -> Approach for an IFR flight once airborne
    /// (AIP AD 2.22 §1: VFR flights stay on Tower). Tower -> Ground after a full-stop landing
    /// (full stop requested, or slowing below 30 kt on the runway): with `Vacate` while still
    /// on the runway, `Contact` only once vacated. Once per handover, never while another
    /// readback is open.
    fn handover(&mut self, i: usize, gs_kt: f64) -> Vec<AtcOutput> {
        let strip = &self.strips[i];
        if strip.pending_readback.is_some() || strip.handoff_to.is_some() {
            return Vec::new();
        }
        let (from, to, mut items) = match (strip.phase, strip.owner) {
            (FlightPhase::HoldingShort, Role::Ground) => (Role::Ground, Role::Tower, Vec::new()),
            (FlightPhase::Landed, Role::Tower)
                if strip.full_stop_requested || gs_kt < FULL_STOP_KT =>
            {
                (
                    Role::Tower,
                    Role::Ground,
                    vec![AtcItem::Vacate { via: None }],
                )
            }
            (FlightPhase::TaxiIn, Role::Tower) => (Role::Tower, Role::Ground, Vec::new()),
            (FlightPhase::Airborne, Role::Tower)
                if strip
                    .plan
                    .as_ref()
                    .is_some_and(|p| p.ifr && (p.approach.is_none() || strip.missed_approach)) =>
            {
                (Role::Tower, Role::Approach, Vec::new())
            }
            _ => return Vec::new(),
        };
        let Some(pos) = self.position(to) else {
            return Vec::new();
        };
        items.push(AtcItem::Contact {
            station: pos.callsign.clone(),
            khz: pos.khz,
        });
        // Departing: a good flight; arrived: a good day. Approach carries on with the flight.
        match to {
            Role::Tower => items.push(AtcItem::Courtesy(Courtesy::HaveANiceFlight)),
            Role::Ground => items.push(AtcItem::Courtesy(Courtesy::GoodDay)),
            Role::Delivery | Role::Approach => {}
        }
        let strip = &mut self.strips[i];
        strip.handoff_to = Some(to);
        strip.expect_readback(&items, self.cfg.strictness);
        self.transmit(i, from, items)
    }

    /// Before a departure is cleared to taxi: re-reads its departure runway (the one it
    /// asked for, else the runway in use from the ATIS) and, if it changed, restarts phase
    /// tracking and compliance for the new runway. A flight cleared from another runway gets
    /// the runway change to announce and its amended plan, stored by the caller once the
    /// taxi clearance is given ([`AtcEngine::runway_change`]); an IFR flight without a SID
    /// from the new runway keeps the runway it was cleared for.
    fn refresh_runway(&mut self, i: usize) -> Option<(Vec<AtcItem>, FlightPlan)> {
        let mut runway = self.departure_runway(i);
        let amended = match self.runway_change(i, &runway) {
            RunwayChange::Same => None,
            RunwayChange::Amended { items, plan } => Some((items, plan)),
            RunwayChange::NoSid { cleared } => {
                tracing::info!(
                    "{}: no SID from runway {runway}, keeps runway {cleared}",
                    self.strips[i].callsign.0
                );
                runway = cleared;
                None
            }
        };
        if runway != self.strips[i].runway {
            let cs = self.strips[i].callsign.clone();
            let sv = self
                .tracks
                .get(&cs)
                .and_then(|t| self.surveillance(&runway, &t.last));
            if let Some(sv) = sv {
                self.tracks.insert(cs, sv);
                self.strips[i].runway = runway;
            }
        }
        amended
    }

    /// A call naming another open position of this airport ("Krakow Approach, ..." on the
    /// Tower frequency): that position. A readback naming a station carries its frequency and
    /// is not a call.
    fn called_elsewhere(&self, role: Role, msg: &PilotMessage) -> Option<Role> {
        if !msg.station_called || !msg.items.freqs_khz.is_empty() {
            return None;
        }
        let to = match msg.addressee.as_deref()? {
            "ground" => Role::Ground,
            "tower" => Role::Tower,
            "approach" => Role::Approach,
            "delivery" => Role::Delivery,
            _ => return None,
        };
        let open = to != Role::Delivery || self.delivery_open_now();
        (to != role && open && self.position(to).is_some()).then_some(to)
    }

    /// A request made to the wrong position: "contact" the one that handles it.
    fn redirect(&mut self, i: usize, to: Role) -> Vec<AtcItem> {
        let Some(pos) = self.position(to) else {
            return Vec::new();
        };
        let items = vec![AtcItem::Contact {
            station: pos.callsign.clone(),
            khz: pos.khz,
        }];
        let strip = &mut self.strips[i];
        strip.handoff_to = Some(to);
        strip.expect_readback(&items, self.cfg.strictness);
        items
    }

    /// Wind of the current ATIS; `None` before the first weather report.
    fn wind(&self) -> Option<atc_phraseology::Wind> {
        self.atis.as_ref().map(|a| a.current().weather.wind)
    }
}

/// Permissions that follow from the phase alone (see `compliance_pipeline.rs`): runway and
/// takeoff permissions end once airborne (in the circuit or not), the landing permission once vacated (`TaxiIn`),
/// the circuit altitude is assigned on downwind.
fn update_permissions(
    strip: &mut Strip,
    runway: &mut RunwayResource,
    prev: FlightPhase,
    phase: FlightPhase,
    circuit_alt_ft: i32,
) {
    let p = &mut strip.permissions;
    p.assigned_alt_ft =
        (phase == FlightPhase::Circuit(CircuitLeg::Downwind)).then_some(circuit_alt_ft);
    match phase {
        _ if phase.is_airborne() && !prev.is_airborne() => {
            p.may_enter_runway = false;
            p.may_take_off = false;
            p.may_land = false;
            strip.uncleared_landing = false;
            runway.release(&strip.callsign);
        }
        FlightPhase::TaxiIn if prev != FlightPhase::TaxiIn => {
            p.may_land = false;
            strip.uncleared_landing = false;
            runway.release(&strip.callsign);
        }
        FlightPhase::Parked if prev == FlightPhase::TaxiIn => strip.full_stop_requested = false,
        _ => {}
    }
}

/// The taxi clearance is used up: holding short, on the runway or airborne, or parked
/// after taxiing in.
fn taxi_done(prev: FlightPhase, phase: FlightPhase) -> bool {
    matches!(
        phase,
        FlightPhase::HoldingShort
            | FlightPhase::OnRunway
            | FlightPhase::TakeoffRoll
            | FlightPhase::Circuit(_)
            | FlightPhase::Airborne
    ) || (phase == FlightPhase::Parked && prev == FlightPhase::TaxiIn)
}

/// The transmission says something the engine handles: an intent, readback items, an
/// acknowledgement or a courtesy. A callsign with other words only is not understood.
fn understood(msg: &PilotMessage) -> bool {
    substantive(msg) || msg.ack || msg.greeting.is_some() || msg.farewell.is_some()
}

/// Both parses understood the same: intent, readback items, acknowledgement, courtesies.
fn same_understanding(a: &PilotMessage, b: &PilotMessage) -> bool {
    a.intent == b.intent
        && a.items == b.items
        && a.ack == b.ack
        && a.greeting == b.greeting
        && a.farewell == b.farewell
}

/// An intent or readback items: more than words both languages share ("roger").
fn substantive(msg: &PilotMessage) -> bool {
    msg.intent != PilotIntent::Unknown || !msg.items.is_empty()
}

fn is_request(intent: &PilotIntent) -> bool {
    matches!(
        intent,
        PilotIntent::RequestTaxi { .. }
            | PilotIntent::RequestTaxiHelp
            | PilotIntent::ReadyForDeparture { .. }
            | PilotIntent::ReportLeg(_)
            | PilotIntent::RequestFullStop
            | PilotIntent::RequestTaxiToStand { .. }
            | PilotIntent::RequestFollowMe
            | PilotIntent::ReportOnStand
            | PilotIntent::RequestClearance { .. }
            | PilotIntent::RequestStartUp { .. }
            | PilotIntent::ReportDeparture
            | PilotIntent::ReportInbound { .. }
            | PilotIntent::RequestJoin { .. }
            | PilotIntent::RequestLeave { .. }
            | PilotIntent::RequestApproach { .. }
            | PilotIntent::ReportEstablished
            | PilotIntent::RequestDepartureRunway { .. }
    )
}

/// The language `role` speaks to an aircraft whose language is `lang`: Approach (and the
/// adjacent units) always English (spec 3.1), the others the aircraft's.
fn lang_for(role: Role, lang: Lang) -> Lang {
    match role {
        Role::Approach => Lang::En,
        Role::Ground | Role::Tower | Role::Delivery => lang,
    }
}

enum Content {
    Message,
    Call,
    Acknowledgement,
}

/// Lowercase word without Polish diacritics ("kraków" -> "krakow").
fn fold(w: &str) -> String {
    w.chars()
        .map(|c| match c {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            c => c,
        })
        .collect()
}

/// Two folded Polish words are forms of one word: the shorter (at least 3 letters) begins
/// the longer ("pas" / "pasem", "koluj" / "koluje"), or they share their first 5 letters
/// ("oczekuje" / "oczekiwania"). Loose on purpose: it only decides whether a repeat of a
/// clearance needs an answer.
fn inflected(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let common = short
        .iter()
        .zip(long.iter())
        .take_while(|(x, y)| x == y)
        .count();
    (short.len() >= 3 && common == short.len()) || common >= 5
}

/// Every run of consecutive letters and digits in `text` heard in `lang` (pause commas
/// inside a run ignored), as strings: "Sierra Papa, Alpha Bravo" -> "S", "SP", "SPA", ...,
/// "BC"; in Polish "trzy dwa jeden" is "321".
fn letter_groups(text: &str, lang: Lang) -> Vec<String> {
    let tokens = match lang {
        Lang::En => normalize(text),
        Lang::Pl => atc_phraseology::pl::normalize::normalize(text),
    };
    let mut runs: Vec<Vec<String>> = vec![Vec::new()];
    for t in tokens {
        match t {
            Token::Letter(c) => runs.last_mut().unwrap().push(c.to_string()),
            Token::Number(d) => runs.last_mut().unwrap().push(d),
            Token::Break => {}
            Token::Word(_) => runs.push(Vec::new()),
        }
    }
    let mut out = Vec::new();
    for run in runs {
        for a in 0..run.len() {
            for b in a + 1..=run.len() {
                out.push(run[a..b].concat());
            }
        }
    }
    out
}

/// `heard` is `full` with letters lost in the middle: at least 4 characters, the same
/// first one and the same last two ("SPBC" for "SPABC").
fn letters_lost(full: &str, heard: &str) -> bool {
    let n = heard.chars().count();
    if n < 4 || n >= full.chars().count() {
        return false;
    }
    let (h, f): (Vec<char>, Vec<char>) = (heard.chars().collect(), full.chars().collect());
    if h[0] != f[0] || h[n - 2..] != f[f.len() - 2..] {
        return false;
    }
    let mut rest = f.iter();
    h.iter().all(|c| rest.any(|x| x == c))
}

/// "full stop" anywhere in the transmission (also inside a position report), heard in
/// `lang` ("z pełnym zatrzymaniem" in Polish).
fn mentions_full_stop(text: &str, lang: Lang) -> bool {
    let tokens = match lang {
        Lang::En => normalize(text),
        Lang::Pl => atc_phraseology::pl::normalize::normalize(text),
    };
    tokens.windows(2).any(|w| {
        matches!((&w[0], &w[1]), (Token::Word(a), Token::Word(b)) if a == "full" && b == "stop")
    })
}

/// "standby": the information needed for the clearance (weather, position) is not known yet.
fn standby() -> Vec<AtcItem> {
    vec![AtcItem::Standby]
}
