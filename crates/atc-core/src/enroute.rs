//! An en-route unit (FIS or ACC, [`EnrouteUnitDef`]): it identifies the flights handed to it
//! or calling it, gives the QNH, acknowledges what they say and, when a flight leaves every
//! sector of the unit (polygon or level limits), hands it on or releases it. It gives no
//! clearances and manages no route: the level last assigned stays the flight's
//! ([`FlightHandover::cleared_ft`]).
//!
//! Where the flight goes after its sector is the caller's choice (the server knows the other
//! units and the stations): [`EnrouteUnit::on_telemetry`] reports
//! [`EnrouteEvent::LeftSector`] once, the caller answers with [`EnrouteUnit::hand_off`], and
//! [`EnrouteUnit::handoff_target`] says when the flight is to be taken: once the pilot's
//! readback on the unit's frequency repeats the new frequency, or [`HANDOFF_TIMEOUT_S`] after
//! the instruction anyway.

use std::collections::HashMap;

use atc_geo::{LatLon, point_in_polygon};
use atc_navdata::enroute::EnrouteUnitDef;
use atc_phraseology::{AtcItem, Callsign, Lang, PilotIntent, PilotMessage, parse_pilot_known};
use serde::{Deserialize, Serialize};

use crate::handover::FlightHandover;
use crate::state::SimTime;

/// A hand-off not read back is taken this long after the instruction anyway.
pub const HANDOFF_TIMEOUT_S: f64 = 30.0;

/// Hysteresis at the edge of the unit's coverage: a sample is outside only when no sector
/// contains the position with the altitude within its limits widened by this margin, and
/// the flight has left after [`LEAVE_SAMPLES`] such samples in a row (a flight hovering on a
/// boundary or a level limit is not handed to and fro).
pub const LEAVE_MARGIN_FT: f64 = 100.0;
/// Consecutive outside samples before [`EnrouteEvent::LeftSector`].
pub const LEAVE_SAMPLES: u8 = 3;
/// A released flight is forgotten this long after its release
/// ([`EnrouteUnit::prune_released`]).
pub const RELEASED_KEPT_S: f64 = 600.0;

/// What the unit reports from a telemetry sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnrouteEvent {
    /// The flight is outside every sector of the unit (reported once).
    LeftSector,
}

/// Where a flight leaving the unit goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandOff {
    /// To a unit or staffed station that takes the flight: "contact <station> <freq>".
    Contact { station: String, khz: u32 },
    /// To a station nobody answers on: "change to <station> <freq>", the flight is released.
    ChangeTo { station: String, khz: u32 },
    /// Nobody next: "frequency change approved", the flight is released.
    Release,
}

#[derive(Clone, Debug, PartialEq)]
enum Service {
    Active,
    /// Told to contact `khz` at `since`; `read_back` once the pilot repeated it.
    HandingOff {
        khz: u32,
        since: f64,
        read_back: bool,
    },
    /// Out of the unit's service since `since`; it stays silent until the pilot asks
    /// something.
    Released {
        since: f64,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct EnrouteFlight {
    data: FlightHandover,
    lang: Lang,
    identified: bool,
    last_items: Vec<AtcItem>,
    service: Service,
    /// Consecutive samples outside the unit's coverage (with the margin).
    outside: u8,
}

impl EnrouteFlight {
    fn new(data: FlightHandover) -> Self {
        EnrouteFlight {
            data,
            lang: Lang::En,
            identified: false,
            last_items: Vec::new(),
            service: Service::Active,
            outside: 0,
        }
    }

    fn released(&self) -> bool {
        matches!(self.service, Service::Released { .. })
    }
}

/// [`unit_covers`] with the level limits widened by [`LEAVE_MARGIN_FT`].
fn covers_with_margin(def: &EnrouteUnitDef, pos: LatLon, alt_ft: f64) -> bool {
    def.sectors.iter().any(|s| {
        (f64::from(s.lower_ft) - LEAVE_MARGIN_FT..=f64::from(s.upper_ft) + LEAVE_MARGIN_FT)
            .contains(&alt_ft)
            && point_in_polygon([pos.lat, pos.lon], &s.polygon)
    })
}

/// `true` when a sector of `def` contains the aircraft (position and altitude, limits
/// inclusive).
pub fn unit_covers(def: &EnrouteUnitDef, pos: LatLon, alt_ft: f64) -> bool {
    def.sectors.iter().any(|s| {
        (f64::from(s.lower_ft)..=f64::from(s.upper_ft)).contains(&alt_ft)
            && point_in_polygon([pos.lat, pos.lon], &s.polygon)
    })
}

/// A heard frequency equal to `khz` (the same 25 kHz carrier, or within 5 kHz as in the
/// readback check).
fn same_freq(heard: u32, khz: u32) -> bool {
    heard.abs_diff(khz) <= 5 || atc_proto::same_channel(heard, khz)
}

/// A readback or an acknowledgement: something understood that asks for nothing.
fn acknowledges(msg: &PilotMessage) -> bool {
    msg.intent == PilotIntent::Readback
        || (msg.intent == PilotIntent::Unknown
            && (msg.ack || !msg.items.is_empty() || msg.farewell.is_some()))
}

/// The pilot asks for something (a request or a report, not a readback or "say again").
fn asks(msg: &PilotMessage) -> bool {
    !matches!(
        msg.intent,
        PilotIntent::Unknown | PilotIntent::Readback | PilotIntent::SayAgain
    )
}

/// What a restart keeps of a flight a unit works: its data (with the level last assigned),
/// the language it speaks and whether the unit has identified it already.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitFlightState {
    pub data: FlightHandover,
    pub lang: Lang,
    pub identified: bool,
}

pub struct EnrouteUnit {
    def: EnrouteUnitDef,
    flights: HashMap<Callsign, EnrouteFlight>,
}

impl EnrouteUnit {
    pub fn new(def: EnrouteUnitDef) -> Self {
        EnrouteUnit {
            def,
            flights: HashMap::new(),
        }
    }

    pub fn def(&self) -> &EnrouteUnitDef {
        &self.def
    }

    /// `true` while the unit works flight `cs` (not released, not handed away).
    pub fn serves(&self, cs: &Callsign) -> bool {
        self.flights.get(cs).is_some_and(|f| !f.released())
    }

    /// `true` while the unit works flight `cs` with no hand-off in progress.
    pub fn active(&self, cs: &Callsign) -> bool {
        self.flights
            .get(cs)
            .is_some_and(|f| f.service == Service::Active)
    }

    /// The language the flight last spoke to the unit in (English before).
    pub fn lang_of(&self, cs: &Callsign) -> Lang {
        self.flights.get(cs).map_or(Lang::En, |f| f.lang)
    }

    /// The flights the unit works (not those released), by callsign, for [`restore_flights`]
    /// after a restart. A hand-off in progress is not kept: the flight comes back as worked normally, so a
    /// sector-exit hand-off is issued again, but one for entering a CTR/TMA is not (the flight
    /// stays with the unit until it leaves its sectors). A strip a station created for the
    /// flight (traffic awareness) may reappear after a restart as ghost traffic.
    ///
    /// [`restore_flights`]: EnrouteUnit::restore_flights
    pub fn flight_states(&self) -> Vec<UnitFlightState> {
        let mut out: Vec<UnitFlightState> = self
            .flights
            .values()
            .filter(|f| !f.released())
            .map(|f| UnitFlightState {
                data: f.data.clone(),
                lang: f.lang,
                identified: f.identified,
            })
            .collect();
        out.sort_by(|a, b| a.data.callsign.0.cmp(&b.data.callsign.0));
        out
    }

    /// Takes the flights of [`EnrouteUnit::flight_states`] back into service.
    pub fn restore_flights(&mut self, flights: Vec<UnitFlightState>) {
        for st in flights {
            let mut f = EnrouteFlight::new(st.data);
            f.lang = st.lang;
            f.identified = st.identified;
            self.flights.insert(f.data.callsign.clone(), f);
        }
    }

    /// Takes in a flight handed over by a station or another unit (replacing any flight of
    /// the same callsign); its first call is answered "identified".
    pub fn import_flight(&mut self, h: FlightHandover) {
        self.flights
            .insert(h.callsign.clone(), EnrouteFlight::new(h));
    }

    /// Hands flight `cs` out: its data as handed in (the level last assigned kept), then the
    /// unit forgets it. `None`: unknown here.
    pub fn export_flight(&mut self, cs: &Callsign) -> Option<FlightHandover> {
        self.flights.remove(cs).map(|f| f.data)
    }

    /// A transmission from `from` on the unit's frequency, heard as `lang`: the first call
    /// is "identified" with the QNH when known; then a request gets "roger", a readback or an
    /// acknowledgement nothing, "say again" the last transmission again, and what is not
    /// understood "say again". A flight told to contact the next unit reads the frequency
    /// back (nothing more is said) or hears the instruction again. A released flight is
    /// silent unless it asks something (the unit then works it again). An unknown aircraft
    /// is taken into service.
    pub fn on_transmission(
        &mut self,
        from: &Callsign,
        text: &str,
        lang: Lang,
        qnh: Option<u16>,
    ) -> Vec<AtcItem> {
        let msg = parse_pilot_known(text, lang, std::slice::from_ref(from));
        let f = self.flights.entry(from.clone()).or_insert_with(|| {
            EnrouteFlight::new(FlightHandover {
                callsign: from.clone(),
                aircraft_type: String::new(),
                plan: None,
                squawk: None,
                cleared_ft: None,
                rules: None,
            })
        });
        f.lang = lang;
        let repeat = msg.intent == PilotIntent::SayAgain && !f.last_items.is_empty();
        let items = match &mut f.service {
            Service::Released { .. } => {
                if !asks(&msg) && !msg.station_called {
                    return Vec::new();
                }
                f.service = Service::Active;
                f.identified = false;
                Self::active_reply(f, &msg, qnh)
            }
            Service::HandingOff { khz, read_back, .. } => {
                if msg.items.freqs_khz.iter().any(|&h| same_freq(h, *khz)) {
                    *read_back = true;
                    return Vec::new();
                }
                if repeat || asks(&msg) || acknowledges(&msg) {
                    f.last_items.clone()
                } else {
                    vec![AtcItem::SayAgain]
                }
            }
            Service::Active if repeat => f.last_items.clone(),
            Service::Active => Self::active_reply(f, &msg, qnh),
        };
        if !items.is_empty() && items != [AtcItem::SayAgain] {
            f.last_items = items.clone();
        }
        items
    }

    fn active_reply(f: &mut EnrouteFlight, msg: &PilotMessage, qnh: Option<u16>) -> Vec<AtcItem> {
        if !f.identified {
            f.identified = true;
            let mut items = vec![AtcItem::Identified];
            items.extend(qnh.map(AtcItem::Qnh));
            return items;
        }
        if acknowledges(msg) {
            Vec::new()
        } else if asks(msg) || msg.station_called || msg.greeting.is_some() {
            vec![AtcItem::Roger]
        } else {
            vec![AtcItem::SayAgain]
        }
    }

    /// A telemetry sample of flight `cs`: [`EnrouteEvent::LeftSector`] once a flight the
    /// unit works (no hand-off in progress) has been outside all its sectors for
    /// [`LEAVE_SAMPLES`] samples in a row, with the [`LEAVE_MARGIN_FT`] level margin.
    pub fn on_telemetry(
        &mut self,
        cs: &Callsign,
        pos: LatLon,
        alt_ft: f64,
    ) -> Option<EnrouteEvent> {
        let f = self.flights.get_mut(cs)?;
        if f.service != Service::Active || covers_with_margin(&self.def, pos, alt_ft) {
            f.outside = 0;
            return None;
        }
        f.outside = f.outside.saturating_add(1);
        (f.outside >= LEAVE_SAMPLES).then_some(EnrouteEvent::LeftSector)
    }

    /// Hands flight `cs` on at sim time `t`: the items the unit says. `Contact` waits for the
    /// readback ([`EnrouteUnit::handoff_target`]); `ChangeTo` and `Release` end the unit's
    /// service at once. Nothing for a flight the unit does not work.
    pub fn hand_off(&mut self, cs: &Callsign, to: HandOff, t: SimTime) -> Vec<AtcItem> {
        let Some(f) = self.flights.get_mut(cs).filter(|f| !f.released()) else {
            return Vec::new();
        };
        let items = match to {
            HandOff::Contact { station, khz } => {
                f.service = Service::HandingOff {
                    khz,
                    since: t.0,
                    read_back: false,
                };
                vec![AtcItem::Contact { station, khz }]
            }
            HandOff::ChangeTo { station, khz } => {
                f.service = Service::Released { since: t.0 };
                vec![AtcItem::ChangeTo { station, khz }]
            }
            HandOff::Release => {
                f.service = Service::Released { since: t.0 };
                vec![AtcItem::FrequencyChangeApproved]
            }
        };
        f.last_items = items.clone();
        items
    }

    /// The frequency flight `cs` is to be taken on at sim time `now`: once read back, or
    /// [`HANDOFF_TIMEOUT_S`] after the instruction. `None` without a hand-off in progress.
    pub fn handoff_target(&self, cs: &Callsign, now: SimTime) -> Option<u32> {
        match self.flights.get(cs)?.service {
            Service::HandingOff {
                khz,
                since,
                read_back,
            } if read_back || now.0 - since >= HANDOFF_TIMEOUT_S => Some(khz),
            _ => None,
        }
    }

    /// Ends the unit's service of flight `cs` at `t` without a word (the hand-off target is
    /// gone).
    pub fn release(&mut self, cs: &Callsign, t: SimTime) {
        if let Some(f) = self.flights.get_mut(cs) {
            f.service = Service::Released { since: t.0 };
        }
    }

    /// Forgets the flights released [`RELEASED_KEPT_S`] or more before `now`.
    pub fn prune_released(&mut self, now: SimTime) {
        self.flights.retain(|_, f| match f.service {
            Service::Released { since } => now.0 - since < RELEASED_KEPT_S,
            _ => true,
        });
    }

    /// Forgets flight `cs` altogether, in service or released (the flight ended). `true`:
    /// the unit knew it.
    pub fn remove(&mut self, cs: &Callsign) -> bool {
        self.flights.remove(cs).is_some()
    }

    /// Forgets flight `cs` if the unit released it (someone else took it).
    pub fn forget_released(&mut self, cs: &Callsign) {
        if self.flights.get(cs).is_some_and(EnrouteFlight::released) {
            self.flights.remove(cs);
        }
    }

    /// `true` when the unit still remembers flight `cs` (in service or released).
    pub fn knows(&self, cs: &Callsign) -> bool {
        self.flights.contains_key(cs)
    }
}

#[cfg(test)]
mod tests {
    use atc_navdata::enroute::{EnrouteKind, SectorDef};

    use super::*;
    use crate::strip::FlightPlan;

    const KHZ: u32 = 120_500;

    /// One sector, 50..51 N 20..21 E, GND to 9500 ft.
    fn unit() -> EnrouteUnit {
        EnrouteUnit::new(EnrouteUnitDef {
            callsign: "Testland Information".into(),
            khz: KHZ,
            kind: EnrouteKind::Fis,
            sectors: vec![SectorDef {
                name: "FIS TESTLAND".into(),
                lower_ft: 0,
                upper_ft: 9500,
                polygon: vec![[50.0, 20.0], [50.0, 21.0], [51.0, 21.0], [51.0, 20.0]],
            }],
        })
    }

    fn cs() -> Callsign {
        Callsign::new("SPTST")
    }

    fn pos(lat: f64, lon: f64) -> LatLon {
        LatLon { lat, lon }
    }

    fn handover(cleared_ft: Option<i32>) -> FlightHandover {
        FlightHandover {
            callsign: cs(),
            aircraft_type: "C172".into(),
            plan: Some(FlightPlan {
                ifr: false,
                destination: "ZZZY".into(),
                dest_name: "Testville".into(),
                cruise_ft: 4500,
                sid: None,
                dep_runway: None,
                star: None,
                approach: None,
                vectors: false,
                cleared_ft: 4500,
                direct: None,
                heading: None,
                approach_cleared: false,
                pattern: None,
                established_check: false,
            }),
            squawk: Some(7000),
            cleared_ft,
            rules: None,
        }
    }

    fn say(u: &mut EnrouteUnit, text: &str) -> Vec<AtcItem> {
        u.on_transmission(&cs(), text, Lang::En, Some(1013))
    }

    #[test]
    fn first_call_identified_with_qnh() {
        let mut u = unit();
        u.import_flight(handover(Some(4500)));
        assert_eq!(
            say(
                &mut u,
                "Testland Information, SPTST, four thousand five hundred feet"
            ),
            vec![AtcItem::Identified, AtcItem::Qnh(1013)]
        );
        // The QNH read back: nothing more.
        assert_eq!(say(&mut u, "QNH one zero one three, SPTST"), vec![]);
        // A request: acknowledged, never cleared.
        assert_eq!(say(&mut u, "SPTST, request taxi"), vec![AtcItem::Roger]);
        assert_eq!(say(&mut u, "blah blah"), vec![AtcItem::SayAgain]);
        assert_eq!(say(&mut u, "say again, SPTST"), vec![AtcItem::Roger]);
    }

    #[test]
    fn unknown_aircraft_calling_is_identified_without_qnh_when_unknown() {
        let mut u = unit();
        let items = u.on_transmission(&cs(), "Testland Information, SPTST", Lang::En, None);
        assert_eq!(items, vec![AtcItem::Identified]);
        assert!(u.serves(&cs()));
    }

    #[test]
    fn left_sector_by_polygon_or_level_once() {
        let mut u = unit();
        u.import_flight(handover(None));
        assert_eq!(u.on_telemetry(&cs(), pos(50.5, 20.5), 4500.0), None);
        // Within the 100 ft margin above the upper limit: still covered.
        for _ in 0..5 {
            assert_eq!(u.on_telemetry(&cs(), pos(50.5, 20.5), 9590.0), None);
        }
        // More than 100 ft above, three samples in a row.
        assert_eq!(u.on_telemetry(&cs(), pos(50.5, 20.5), 9700.0), None);
        assert_eq!(u.on_telemetry(&cs(), pos(50.5, 20.5), 9700.0), None);
        assert_eq!(
            u.on_telemetry(&cs(), pos(50.5, 20.5), 9700.0),
            Some(EnrouteEvent::LeftSector)
        );
        // Laterally outside: the same three samples.
        let mut u = unit();
        u.import_flight(handover(None));
        assert_eq!(u.on_telemetry(&cs(), pos(51.2, 20.5), 4500.0), None);
        assert_eq!(u.on_telemetry(&cs(), pos(51.2, 20.5), 4500.0), None);
        assert_eq!(
            u.on_telemetry(&cs(), pos(51.2, 20.5), 4500.0),
            Some(EnrouteEvent::LeftSector)
        );
        let items = u.hand_off(&cs(), HandOff::Release, SimTime(10.0));
        assert_eq!(items, vec![AtcItem::FrequencyChangeApproved]);
        assert!(!u.serves(&cs()));
        for _ in 0..5 {
            assert_eq!(u.on_telemetry(&cs(), pos(51.2, 20.5), 4500.0), None);
        }
        // Unknown flights report nothing.
        assert_eq!(
            u.on_telemetry(&Callsign::new("SPXYZ"), pos(52.0, 20.5), 4500.0),
            None
        );
    }

    /// Hovering on the boundary (out, in, out, ...) never leaves.
    #[test]
    fn boundary_hover_does_not_leave() {
        let mut u = unit();
        u.import_flight(handover(None));
        for k in 0..20 {
            let lat = if k % 2 == 0 { 51.001 } else { 50.999 };
            assert_eq!(u.on_telemetry(&cs(), pos(lat, 20.5), 4500.0), None, "{k}");
        }
    }

    #[test]
    fn released_flights_are_pruned_after_ten_minutes() {
        let mut u = unit();
        u.import_flight(handover(None));
        u.hand_off(&cs(), HandOff::Release, SimTime(100.0));
        u.prune_released(SimTime(699.0));
        assert!(u.knows(&cs()));
        u.prune_released(SimTime(700.0));
        assert!(!u.knows(&cs()));
        // Taken elsewhere: forgotten at once; a flight in service is kept.
        u.import_flight(handover(None));
        u.forget_released(&cs());
        assert!(u.knows(&cs()));
        u.release(&cs(), SimTime(800.0));
        u.forget_released(&cs());
        assert!(!u.knows(&cs()));
    }

    #[test]
    fn contact_waits_for_frequency_readback_or_timeout() {
        let mut u = unit();
        u.import_flight(handover(None));
        say(&mut u, "Testland Information, SPTST");
        let to = HandOff::Contact {
            station: "Other Radar".into(),
            khz: 127_660,
        };
        let items = u.hand_off(&cs(), to, SimTime(100.0));
        assert_eq!(
            items,
            vec![AtcItem::Contact {
                station: "Other Radar".into(),
                khz: 127_660
            }]
        );
        assert_eq!(u.on_telemetry(&cs(), pos(52.0, 20.5), 4500.0), None);
        assert_eq!(u.handoff_target(&cs(), SimTime(101.0)), None);
        // Not repeating the frequency: the instruction again.
        assert_eq!(say(&mut u, "roger, SPTST"), items);
        assert_eq!(say(&mut u, "one two seven decimal six six, SPTST"), vec![]);
        assert_eq!(u.handoff_target(&cs(), SimTime(102.0)), Some(127_660));

        let mut u = unit();
        u.import_flight(handover(None));
        let to = HandOff::Contact {
            station: "Other Radar".into(),
            khz: 127_660,
        };
        u.hand_off(&cs(), to, SimTime(100.0));
        assert_eq!(u.handoff_target(&cs(), SimTime(129.0)), None);
        assert_eq!(u.handoff_target(&cs(), SimTime(130.0)), Some(127_660));
    }

    #[test]
    fn released_flight_is_silent_until_it_asks() {
        let mut u = unit();
        u.import_flight(handover(None));
        let to = HandOff::ChangeTo {
            station: "Testville Radio".into(),
            khz: 122_800,
        };
        assert_eq!(
            u.hand_off(&cs(), to, SimTime(0.0)),
            vec![AtcItem::ChangeTo {
                station: "Testville Radio".into(),
                khz: 122_800
            }]
        );
        assert!(!u.serves(&cs()));
        assert_eq!(say(&mut u, "one two two decimal eight, SPTST"), vec![]);
        assert_eq!(say(&mut u, "frequency change approved, SPTST"), vec![]);
        assert!(!u.serves(&cs()));
        assert_eq!(
            u.hand_off(&cs(), HandOff::Release, SimTime(1.0)),
            vec![],
            "released once"
        );
        // Asking again brings it back into service.
        assert_eq!(
            say(&mut u, "Testland Information, SPTST, request taxi"),
            vec![AtcItem::Identified, AtcItem::Qnh(1013)]
        );
        assert!(u.serves(&cs()));
    }

    #[test]
    fn level_kept_from_handover() {
        let mut u = unit();
        u.import_flight(handover(Some(6500)));
        say(&mut u, "Testland Information, SPTST");
        say(&mut u, "SPTST, request climb flight level eight zero");
        let h = u.export_flight(&cs()).expect("exported");
        assert_eq!(h.cleared_ft, Some(6500));
        assert_eq!(h.plan.unwrap().destination, "ZZZY");
        assert_eq!(h.squawk, Some(7000));
        assert!(u.export_flight(&cs()).is_none());
        assert!(!u.serves(&cs()));
    }
}
