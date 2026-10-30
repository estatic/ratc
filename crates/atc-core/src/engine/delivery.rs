//! Delivery position: IFR and VFR clearances, also given by Ground while Delivery is closed
//! (AIP AD 2.20), and the opening hours from the sim UTC time.
//!
//! The pilot's request is the flight plan (spec 3.2): destination (one of the airports the
//! engine was given, [`AtcEngine::with_destinations`]), flight rules and an optional level.
//! IFR: destination, SID of the departure runway towards the destination, initial climb (the
//! cruise level, lowered only by a SID ceiling below it, [`initial_climb_ft`]) and squawk. VFR:
//! destination, the VFR exit point towards it, squawk. Unknown destination: "unable, say again
//! destination"; IFR without a procedure set: "unable, IFR not available, VFR only".

use atc_geo::{LatLon, bearing_deg};
use atc_navdata::fixes::ProcedureSet;
use atc_navdata::model::Destination;
use atc_navdata::procedures::VfrPoint;
use atc_phraseology::Place;
use atc_phraseology::{
    AtcItem, FlightRules, Level, PilotIntent, PilotMessage, Token, UnableReason, normalize,
};

use super::{AtcEngine, fold};
use crate::ifr::{choose_sid, initial_climb_ft, procedure_path};
use crate::strip::{FlightPlan, Role};

/// Summer time (UTC+2 in Poland) as days of the year, 0 = 1 January: about 28 March to
/// 28 October. The last-Sunday rule is approximated (ruling).
pub(super) const SUMMER_DAYS: std::ops::RangeInclusive<u16> = 86..=300;

impl AtcEngine {
    /// Enables IFR: the airport's resolved procedures (SIDs, STARs, approaches). Their fix
    /// names are heard as procedure designators.
    pub fn with_procedure_set(mut self, set: ProcedureSet) -> Self {
        std::sync::Arc::make_mut(&mut self.vocab.fixes)
            .extend(set.fixes.keys().filter(|k| !k.starts_with("RW")).cloned());
        self.procedure_set = Some(set);
        self
    }

    /// The airports a flight may be cleared to ([`DestinationSet`], or a `Vec` of them);
    /// they are heard by ICAO code or name, and by name after "direct" (Approach clears
    /// departures direct to them). A set is built once and shared by the engines of a server.
    pub fn with_destinations(mut self, d: impl Into<DestinationSet>) -> Self {
        let d = d.into();
        self.vocab.places = d.places.clone();
        self.vocab.place_fixes = d.fixes.clone();
        self.destinations = d.list;
        self
    }

    /// Delivery is staffed at sim UTC time `utc_s` (seconds of the day) on `day_of_year`
    /// (0 = 1 January): the airport has a Delivery position and the time is within its
    /// `procedures.toml` hours, `[open, close)` UTC (summer or winter, see [`SUMMER_DAYS`]).
    /// Open when the time or the hours are unknown (Review Focus 4: old clients and replays
    /// send no time); an unknown day accepts the hours of either season.
    pub fn delivery_open(&self, utc_s: Option<f64>, day_of_year: Option<u16>) -> bool {
        if self.position(Role::Delivery).is_none() {
            return false;
        }
        let (Some(utc), Some(hours)) = (utc_s, self.procs.delivery_hours) else {
            return true;
        };
        let hour = (utc / 3600.0).rem_euclid(24.0);
        let within = |[open, close]: [u8; 2]| hour >= f64::from(open) && hour < f64::from(close);
        match day_of_year {
            Some(d) if SUMMER_DAYS.contains(&d) => within(hours.summer_utc),
            Some(_) => within(hours.winter_utc),
            None => within(hours.summer_utc) || within(hours.winter_utc),
        }
    }

    /// [`AtcEngine::delivery_open`] at the time of the latest telemetry.
    pub(super) fn delivery_open_now(&self) -> bool {
        self.delivery_open(self.clock.0, self.clock.1)
    }

    /// A request on the Delivery frequency: clearances here, the rest to Ground or Tower.
    pub(super) fn delivery_request(&mut self, i: usize, msg: &PilotMessage) -> Vec<AtcItem> {
        let via = msg.items.points.first().map(String::as_str);
        match &msg.intent {
            PilotIntent::RequestClearance {
                destination,
                rules,
                level_ft,
            } => match rules {
                None => vec![AtcItem::SayFlightRules],
                Some(rules) => self.clearance(
                    i,
                    destination.as_deref(),
                    *rules == FlightRules::Ifr,
                    *level_ft,
                    via,
                ),
            },
            PilotIntent::RequestDepartureRunway { runway }
                if self.runway_request_is_ready(i, runway) =>
            {
                self.redirect(i, Role::Tower)
            }
            PilotIntent::RequestDepartureRunway { runway } => {
                self.request_departure_runway(i, runway)
            }
            PilotIntent::RequestStartUp { .. }
            | PilotIntent::RequestTaxi { .. }
            | PilotIntent::RequestTaxiToStand { .. }
            | PilotIntent::RequestTaxiHelp
            | PilotIntent::RequestFollowMe => self.redirect(i, Role::Ground),
            PilotIntent::ReadyForDeparture { .. }
            | PilotIntent::ReportLeg(_)
            | PilotIntent::RequestFullStop => self.redirect(i, Role::Tower),
            _ => Vec::new(),
        }
    }

    /// A clearance request to a position other than Delivery: "contact Delivery" while it
    /// is open, otherwise the clearance itself.
    pub(super) fn clearance_elsewhere(
        &mut self,
        i: usize,
        destination: Option<&str>,
        ifr: bool,
        level_ft: Option<i32>,
        via: Option<&str>,
    ) -> Vec<AtcItem> {
        if self.delivery_open_now() {
            self.redirect(i, Role::Delivery)
        } else {
            self.clearance(i, destination, ifr, level_ft, via)
        }
    }

    /// The clearance of aircraft `i` to `destination` (ICAO, as heard), to be read back. A
    /// VFR flight leaves via the point it asked for (`via`) when that point may be used
    /// towards the destination, else via the point towards it.
    pub(super) fn clearance(
        &mut self,
        i: usize,
        destination: Option<&str>,
        ifr: bool,
        level_ft: Option<i32>,
        via: Option<&str>,
    ) -> Vec<AtcItem> {
        let Some(dest) = self.destination(destination) else {
            return vec![AtcItem::Unable(UnableReason::UnknownDestination)];
        };
        let (mut items, plan) = if ifr {
            match self.ifr_departure(i, &dest, level_ft) {
                Some((items, plan)) => (items, plan),
                None => return vec![AtcItem::Unable(UnableReason::NoIfrData)],
            }
        } else {
            let exit = self
                .vfr_exit(dest.pos, via)
                .map(|point| AtcItem::LeaveControlZoneVia { point });
            let plan = FlightPlan {
                ifr: false,
                destination: dest.icao.clone(),
                dest_name: dest.name.clone(),
                cruise_ft: level_ft.unwrap_or(0),
                sid: None,
                dep_runway: Some(self.departure_runway(i)),
                star: None,
                approach: None,
                vectors: false,
                cleared_ft: 0,
                direct: None,
                heading: None,
                approach_cleared: false,
                pattern: None,
                established_check: false,
            };
            (exit.into_iter().collect(), plan)
        };
        items.insert(0, cleared_to(&dest));
        self.clearance_squawk(i, &mut items);
        let exit = items.iter().find_map(|it| match it {
            AtcItem::LeaveControlZoneVia { point } => Some(point.clone()),
            _ => None,
        });
        let strip = &mut self.strips[i];
        strip.vfr_exit = exit;
        strip.plan = Some(plan);
        strip.expect_readback(&items, self.cfg.strictness);
        items
    }

    /// One of the engine's destinations by ICAO code (as heard).
    pub(super) fn destination(&self, icao: Option<&str>) -> Option<Destination> {
        let icao = icao?;
        self.destinations
            .iter()
            .find(|d| d.icao.eq_ignore_ascii_case(icao))
            .cloned()
    }

    /// The squawk of a clearance to aircraft `i` added to `items` (none when the code pool
    /// is exhausted).
    pub(super) fn clearance_squawk(&mut self, i: usize, items: &mut Vec<AtcItem>) {
        if let Some(code) = self.squawk(i) {
            items.push(AtcItem::Squawk(code));
            self.strips[i].code_with_clearance = true;
        }
    }

    /// SID and initial climb of aircraft `i` towards `dest` from its departure runway; `None`
    /// without a procedure set, a cruise level or a SID for the runway.
    fn ifr_departure(
        &self,
        i: usize,
        dest: &Destination,
        level_ft: Option<i32>,
    ) -> Option<(Vec<AtcItem>, FlightPlan)> {
        let cruise_ft = level_ft.or(self.procs.ifr.map(|d| d.cruise_ft))?;
        let runway = self.departure_runway(i);
        let (sid, ident, climb_ft) = self.sid_from(&runway, dest.pos, cruise_ft)?;
        let items = vec![sid, AtcItem::ClimbTo(self.level(climb_ft))];
        let plan = FlightPlan {
            ifr: true,
            destination: dest.icao.clone(),
            dest_name: dest.name.clone(),
            cruise_ft,
            sid: Some(ident),
            dep_runway: Some(runway),
            star: None,
            approach: None,
            vectors: false,
            cleared_ft: climb_ft,
            direct: None,
            heading: None,
            approach_cleared: false,
            pattern: None,
            established_check: false,
        };
        Some((items, plan))
    }

    /// The SID from `runway` towards `dest` as spoken, its ident and the initial climb for
    /// `cruise_ft`; `None` without a procedure set or a SID for the runway.
    fn sid_from(
        &self,
        runway: &str,
        dest: LatLon,
        cruise_ft: i32,
    ) -> Option<(AtcItem, String, i32)> {
        let set = self.procedure_set.as_ref()?;
        let from = self.threshold(runway).or_else(|| self.airport_ref())?;
        let sid = choose_sid(set, runway, from, dest)?;
        let climb_ft = initial_climb_ft(&procedure_path(sid), cruise_ft);
        let (fix, number, letter) = crate::ifr::spoken_designator(sid);
        let item = AtcItem::Sid {
            fix,
            number,
            letter,
        };
        Some((item, sid.procedure.ident.clone(), climb_ft))
    }

    /// The clearance of aircraft `i` moved to departure `runway`: unchanged without a
    /// clearance for a departure runway or when it is that runway; else "runway changed"
    /// with, for an IFR flight, the SID from the new runway and the initial climb when it
    /// changed, and the amended plan (not yet stored on the strip).
    pub(super) fn runway_change(&self, i: usize, runway: &str) -> RunwayChange {
        let Some(plan) = self.strips[i].plan.as_ref() else {
            return RunwayChange::Same;
        };
        let Some(cleared) = plan.dep_runway.as_deref().filter(|r| *r != runway) else {
            return RunwayChange::Same;
        };
        let mut plan = plan.clone();
        let mut items = vec![AtcItem::RunwayChanged(runway.to_string())];
        if plan.ifr {
            let dest = self
                .destinations
                .iter()
                .find(|d| d.icao == plan.destination)
                .map(|d| d.pos);
            let Some((sid, ident, climb_ft)) =
                dest.and_then(|pos| self.sid_from(runway, pos, plan.cruise_ft))
            else {
                return RunwayChange::NoSid {
                    cleared: cleared.to_string(),
                };
            };
            items.push(sid);
            if climb_ft != plan.cleared_ft {
                items.push(AtcItem::ClimbTo(self.level(climb_ft)));
            }
            plan.sid = Some(ident);
            plan.cleared_ft = climb_ft;
        }
        plan.dep_runway = Some(runway.to_string());
        RunwayChange::Amended { items, plan }
    }

    /// `ft` as an altitude up to the transition altitude, a flight level above it.
    pub(super) fn level(&self, ft: i32) -> Level {
        if ft > self.procs.transition_altitude_ft {
            Level::FlightLevel(u16::try_from(ft / 100).unwrap_or(0))
        } else {
            Level::Altitude(ft)
        }
    }

    /// The VFR point closest in bearing (from the airport) to `toward`; towards the south
    /// (bearing between 090 and 270) only the points marked `south` when there are any
    /// (AIP AD 2.22: from the south only via the southern points).
    fn vfr_exit(&self, toward: LatLon, requested: Option<&str>) -> Option<String> {
        let home = self.airport_ref()?;
        let want = bearing_deg(home, toward);
        let southward = want > 90.0 && want < 270.0;
        let any_south = self.procs.vfr_points.iter().any(|p| p.south);
        let usable = |p: &&VfrPoint| !(southward && any_south) || p.south;
        if let Some(p) = requested
            .and_then(|name| self.procs.vfr_point(name))
            .filter(|p| usable(p))
        {
            return Some(p.name.to_uppercase());
        }
        self.procs
            .vfr_points
            .iter()
            .filter(usable)
            .map(|p| {
                let pos = LatLon {
                    lat: p.lat,
                    lon: p.lon,
                };
                let gap = ((bearing_deg(home, pos) - want + 540.0).rem_euclid(360.0) - 180.0).abs();
                (gap, p)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p.name.to_uppercase())
    }

    /// Threshold of `runway`.
    fn threshold(&self, runway: &str) -> Option<LatLon> {
        self.airport
            .runways
            .iter()
            .flat_map(|r| r.ends.iter())
            .find(|e| e.ident == runway)
            .map(|e| e.threshold)
    }

    /// Reference point of the airport: the middle of its first runway.
    pub(super) fn airport_ref(&self) -> Option<LatLon> {
        let [a, b] = &self.airport.runways.first()?.ends;
        Some(LatLon {
            lat: (a.threshold.lat + b.threshold.lat) / 2.0,
            lon: (a.threshold.lon + b.threshold.lon) / 2.0,
        })
    }

    /// The pending clearance (`issued`) was accepted: "readback correct", and from Delivery
    /// "contact Ground". Nothing for other clearances.
    pub(super) fn clearance_confirmed(
        &mut self,
        i: usize,
        role: Role,
        issued: &[AtcItem],
    ) -> Vec<AtcItem> {
        if !issued
            .iter()
            .any(|it| matches!(it, AtcItem::ClearedTo { .. }))
        {
            return Vec::new();
        }
        let mut items = vec![AtcItem::ReadbackCorrect];
        if role == Role::Delivery {
            items.extend(self.redirect(i, Role::Ground));
        }
        items
    }

    /// Aircraft `i` says it flies IFR but has no IFR clearance yet.
    pub(super) fn needs_ifr_clearance(&self, i: usize, text: &str) -> bool {
        mentions_ifr(text) && self.strips[i].plan.as_ref().is_none_or(|p| !p.ifr)
    }
}

/// "Cleared to `dest`".
pub(super) fn cleared_to(dest: &Destination) -> AtcItem {
    AtcItem::ClearedTo {
        destination: dest.name.clone(),
        icao: dest.icao.clone(),
    }
}

/// A departure runway other than the one in the flight's clearance ([`AtcEngine::runway_change`]).
pub(super) enum RunwayChange {
    /// No clearance for another runway: nothing to announce.
    Same,
    /// The announcement ("runway changed", SID, climb) and the amended plan.
    Amended {
        items: Vec<AtcItem>,
        plan: FlightPlan,
    },
    /// An IFR flight has no SID from the new runway: it keeps the `cleared` runway.
    NoSid { cleared: String },
}

/// "IFR" (also spelled "I F R") or "instrument" in `text`.
pub(super) fn mentions_ifr(text: &str) -> bool {
    let toks = normalize(text);
    toks.iter().enumerate().any(|(k, t)| match t {
        Token::Word(w) => w == "ifr" || w == "instrument",
        Token::Letter('I') => {
            matches!(toks.get(k + 1), Some(Token::Letter('F')))
                && matches!(toks.get(k + 2), Some(Token::Letter('R')))
        }
        _ => false,
    })
}

/// The destinations of an engine and what the parser needs to hear them: built once (the
/// vocabulary of the world's airports is large) and cloned cheaply into each engine.
#[derive(Clone, Debug)]
pub struct DestinationSet {
    pub(super) list: std::sync::Arc<[Destination]>,
    places: std::sync::Arc<[Place]>,
    fixes: std::sync::Arc<std::collections::HashSet<String>>,
}

impl DestinationSet {
    pub fn new(list: Vec<Destination>) -> Self {
        DestinationSet {
            places: list
                .iter()
                .map(|d| Place {
                    name: ascii_fold(&d.name.to_lowercase()),
                    icao: d.icao.clone(),
                    pos: Some((d.pos.lat, d.pos.lon)),
                })
                .collect(),
            fixes: std::sync::Arc::new(list.iter().map(|d| direct_key(&d.name)).collect()),
            list: list.into(),
        }
    }
}

impl From<Vec<Destination>> for DestinationSet {
    fn from(list: Vec<Destination>) -> Self {
        DestinationSet::new(list)
    }
}

/// A name as the parser hears it after "direct": uppercase ASCII letters and digits only
/// ("Bielsko-Biała" -> "BIELSKOBIALA"); also how a "direct" readback is compared.
pub(crate) fn direct_key(name: &str) -> String {
    ascii_fold(&name.to_lowercase())
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Lowercase `s` without diacritics: Polish ([`fold`]) and common Latin ones ("faréville" ->
/// "fareville"), as the parser's place names expect.
fn ascii_fold(s: &str) -> String {
    fold(s)
        .chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' | 'č' => 'c',
            'è' | 'é' | 'ê' | 'ë' | 'ě' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' | 'ň' => 'n',
            'ò' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
            'ř' => 'r',
            'š' => 's',
            'ù' | 'ú' | 'û' | 'ü' | 'ů' => 'u',
            'ý' | 'ÿ' => 'y',
            'ž' => 'z',
            c => c,
        })
        .collect()
}
