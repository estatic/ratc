//! Flight strip: what the controllers know about and have told one aircraft.

use std::collections::HashSet;

use atc_geo::LatLon;
use atc_navdata::model::FreqKind;
use atc_phraseology::{ApproachKind, AtcItem, Callsign, Lang, ReadbackItem, TaxiTo};

use crate::compliance::Permissions;
use crate::phase::FlightPhase;
use crate::readback::{Strictness, required_readback};
use crate::taxi::TaxiTarget;

/// Controller position handled by the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Role {
    Ground,
    Tower,
    /// Clearance delivery.
    Delivery,
    /// Radar approach control.
    Approach,
}

impl Role {
    /// The `procedures.toml` position kind of this role.
    pub fn freq_kind(self) -> FreqKind {
        match self {
            Role::Ground => FreqKind::Ground,
            Role::Tower => FreqKind::Tower,
            Role::Delivery => FreqKind::Delivery,
            Role::Approach => FreqKind::Approach,
        }
    }

    /// Role of a position kind; `None` for positions the engine does not staff.
    pub fn from_freq_kind(kind: FreqKind) -> Option<Role> {
        match kind {
            FreqKind::Ground => Some(Role::Ground),
            FreqKind::Tower => Some(Role::Tower),
            FreqKind::Delivery => Some(Role::Delivery),
            FreqKind::Approach => Some(Role::Approach),
            // AFIS is data only for now: the engine does not staff it.
            FreqKind::Atis | FreqKind::Unicom | FreqKind::Departure | FreqKind::Afis => None,
        }
    }
}

/// The flight plan of an aircraft: what it asked for and was cleared (the request is the
/// flight plan, spec 3.2). There is no off-block time, so the AIP rule "clearance not earlier
/// than 30 min before EOBT" is not checked.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FlightPlan {
    /// Flight rules: IFR, else VFR.
    pub ifr: bool,
    /// Destination ICAO code.
    pub destination: String,
    /// Destination as spoken ("Katowice").
    pub dest_name: String,
    /// Requested cruise level in feet; 0 for a VFR flight that named none.
    pub cruise_ft: i32,
    /// Ident of the cleared SID (`"SUDO2J"`).
    pub sid: Option<String>,
    /// The departure runway the clearance was given for; `None` for a clearance without one
    /// (an arrival, an older snapshot).
    #[serde(default)]
    pub dep_runway: Option<String>,
    /// Ident of the cleared STAR.
    pub star: Option<String>,
    /// Expected or cleared approach and its runway.
    pub approach: Option<(ApproachKind, String)>,
    /// On radar vectors.
    pub vectors: bool,
    /// Level the aircraft is cleared to, in feet (0 when none was given).
    pub cleared_ft: i32,
    /// The fix (or destination) the aircraft was cleared direct to, leaving its SID (an
    /// arrival: the IAF of the full procedure).
    pub direct: Option<String>,
    /// Assigned heading (magnetic) while on radar vectors.
    pub heading: Option<u16>,
    /// The approach has been cleared ("cleared ILS approach").
    pub approach_cleared: bool,
    /// Vectored around a pattern (downwind, base) onto the final: the current leg.
    pub pattern: Option<PatternLeg>,
    /// "Check established" has been said for the current approach clearance.
    pub established_check: bool,
}

/// Leg of the radar pattern flown by an arrival that comes from the far side of the final.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PatternLeg {
    /// Parallel to the final, outbound, abeam it.
    Downwind,
    /// Perpendicular to the final, towards it.
    Base,
}

/// Where a VFR flight is in its entry to, or exit from, the control zone via the published
/// points (AIP AD 2.22 §3). `None` on the strip: not entering or leaving, a circuit flight
/// flies the circuit.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VfrState {
    /// Told to enter via `point` and to report there.
    Entering { point: String },
    /// Told to hold over `hold` (the runway is busy) before entering via `entry`.
    Holding { entry: String, hold: String },
    /// Told to report at `point` on the way out.
    Leaving { point: String },
    /// Left the zone via its exit point (handed to the next unit).
    Left,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PilotProfile {
    pub callsign: Callsign,
    pub aircraft_type: String,
}

/// A runway crossing on the cleared taxi route, cleared separately on approach.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RouteCrossing {
    /// Runway as it is spoken (`"25"`, or the edge label such as `"07/25"`).
    pub runway: String,
    /// Route node before the runway, and the first one beyond it.
    pub entry: LatLon,
    pub exit: LatLon,
    /// `CrossRunway` has been issued.
    pub cleared: bool,
}

/// A follow-me car sent to lead the aircraft to its taxi target.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FollowMe {
    /// Vehicle order id (see `engine::VehicleCommand`).
    pub id: u32,
    /// Sim time the car was sent ("follow-me car is on the way").
    pub sent_s: f64,
    /// "Follow the follow-me car" has been said.
    pub leading: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Strip {
    pub callsign: Callsign,
    pub owner: Role,
    pub phase: FlightPhase,
    /// Everything said to this aircraft, in order.
    pub issued: Vec<AtcItem>,
    pub pending_readback: Option<Vec<ReadbackItem>>,
    pub permissions: Permissions,
    pub first_contact_done: HashSet<Role>,
    pub full_stop_requested: bool,
    /// The clearance awaiting readback (used for corrections).
    pub pending_items: Vec<AtcItem>,
    /// Station the aircraft was told to contact, until the handover completes.
    pub handoff_to: Option<Role>,
    /// Runway in use for this aircraft (re-read from the ATIS before each taxi-out clearance).
    pub runway: String,
    /// Stand the aircraft started from, the default taxi-in target.
    pub home_stand: Option<String>,
    /// Crossings of the current taxi route not yet completed.
    pub crossings: Vec<RouteCrossing>,
    /// Where the aircraft was last cleared to taxi, until it holds short there, enters a
    /// runway or parks.
    pub taxi_target: Option<TaxiTarget>,
    /// A landing without clearance was reported and the aircraft is still on the runway.
    pub uncleared_landing: bool,
    /// Times the pending readback has been asked for again.
    pub readback_requests: u8,
    /// Transponder code assigned to the aircraft, once one was needed.
    pub squawk: Option<u16>,
    /// Tower asked to put the transponder right at departure; the pilot's next
    /// acknowledgement re-runs the departure.
    pub transponder_check: bool,
    /// Tower has transmitted the departure (check squawk, clearance or hold position) in this
    /// departure cycle; it ends when the aircraft taxis again or is parked.
    pub departure_sent: bool,
    /// Taxi help requests since the last taxi clearance; the second brings a follow-me car.
    pub taxi_helps: u8,
    /// The follow-me car leading the aircraft, until it parks or holds short.
    pub follow_me: Option<FollowMe>,
    /// The language the aircraft is answered in: the fixed language, or (auto) that of its
    /// last understood transmission, English before any.
    pub lang: Lang,
    /// The flight plan, once a clearance was issued.
    pub plan: Option<FlightPlan>,
    /// The adjacent unit (callsign) the flight was handed off to; empty: released with a
    /// frequency change, no unit to contact.
    pub handed_off: Option<String>,
    /// Approach has identified the aircraft on radar.
    pub identified: bool,
    /// The adjacent unit has answered the aircraft's call ("identified", once).
    pub adjacent_identified: bool,
    /// The transponder shows 7600 (radio failure, AIP AD 2.22 §2.1): logged here; Approach
    /// stops issuing instructions and no deviation is called while it is set.
    pub radio_failure: bool,
    /// The arrival went around or flew a missed approach; Tower hands it back to Approach,
    /// which vectors it again once it calls.
    pub missed_approach: bool,
    /// The Approach frequency the aircraft last called on (the unit has several); Approach
    /// answers on it.
    pub approach_khz: Option<u32>,
    /// Entry to or exit from the control zone via the VFR points, until the flight joins the
    /// circuit; it ends with the departure cycle.
    pub vfr: Option<VfrState>,
    /// The exit point of the VFR clearance last issued (`None`: an IFR clearance or no
    /// point); Tower asks for the report at it after take-off.
    pub vfr_exit: Option<String>,
    /// Tower has just given a VFR instruction and the pilot has not answered yet: a
    /// transmission naming the point is then a readback, not a position report.
    pub vfr_unacked: bool,
    /// The QNH given with the start-up of this departure (not repeated in the taxi
    /// clearance); cleared when the aircraft parks again.
    pub qnh_with_start_up: Option<u16>,
    /// The transponder code was given with this departure's clearance (not repeated in the
    /// taxi clearance); cleared when the aircraft parks again.
    pub code_with_clearance: bool,
    /// The departure runway the pilot asked for and was given instead of the runway in use;
    /// `None`: the runway in use. Cleared when the flight ends.
    #[serde(default)]
    pub requested_runway: Option<String>,
    /// The frequency (kHz) of the adjacent unit the flight was told to contact with its
    /// hand-off (`handed_off`); `None` without one (also a release without a unit).
    #[serde(default)]
    pub handed_off_khz: Option<u32>,
    /// The flight was handed in from another station's engine and has not called yet: its
    /// first call is answered as a flight handed over.
    #[serde(default)]
    pub handed_in: bool,
    // Every field above that belongs to one flight is reset by `Strip::end_flight`.
}

impl Strip {
    pub fn new(callsign: Callsign, runway: &str) -> Self {
        Strip {
            callsign,
            owner: Role::Ground,
            phase: FlightPhase::Parked,
            issued: Vec::new(),
            pending_readback: None,
            permissions: Permissions {
                may_enter_runway: false,
                may_take_off: false,
                may_land: false,
                assigned_alt_ft: None,
            },
            first_contact_done: HashSet::new(),
            full_stop_requested: false,
            pending_items: Vec::new(),
            handoff_to: None,
            runway: runway.to_string(),
            home_stand: None,
            crossings: Vec::new(),
            taxi_target: None,
            uncleared_landing: false,
            readback_requests: 0,
            squawk: None,
            transponder_check: false,
            departure_sent: false,
            taxi_helps: 0,
            follow_me: None,
            lang: Lang::En,
            plan: None,
            handed_off: None,
            identified: false,
            adjacent_identified: false,
            radio_failure: false,
            missed_approach: false,
            approach_khz: None,
            vfr: None,
            vfr_exit: None,
            vfr_unacked: false,
            qnh_with_start_up: None,
            code_with_clearance: false,
            requested_runway: None,
            handed_off_khz: None,
            handed_in: false,
        }
    }

    /// The flight is over (parked again, or a new flight begins away from home, spec 3.6):
    /// every per-flight field returns to its initial state, so the next flight is served in
    /// full. Kept: the callsign, the owner, the phase, the log of what was said, the runway
    /// and home stand, the language, the transponder code (the aircraft keeps its code for
    /// the session) and the radio failure (read from every sample).
    pub fn end_flight(&mut self) {
        self.pending_readback = None;
        self.pending_items.clear();
        self.readback_requests = 0;
        self.handoff_to = None;
        self.permissions.may_enter_runway = false;
        self.permissions.may_take_off = false;
        self.permissions.may_land = false;
        self.first_contact_done.clear();
        self.full_stop_requested = false;
        self.crossings.clear();
        self.taxi_target = None;
        self.uncleared_landing = false;
        self.transponder_check = false;
        self.departure_sent = false;
        self.taxi_helps = 0;
        self.follow_me = None;
        self.plan = None;
        self.handed_off = None;
        self.identified = false;
        self.adjacent_identified = false;
        self.missed_approach = false;
        self.approach_khz = None;
        self.vfr = None;
        self.vfr_exit = None;
        self.vfr_unacked = false;
        self.qnh_with_start_up = None;
        self.code_with_clearance = false;
        self.requested_runway = None;
        self.handed_off_khz = None;
        self.handed_in = false;
    }

    /// Records a clearance just issued and the readback it requires. A clearance
    /// requiring no readback is complete at once.
    pub fn expect_readback(&mut self, items: &[AtcItem], strictness: Strictness) {
        let required = required_readback(items, strictness);
        self.pending_items = items.to_vec();
        self.readback_requests = 0;
        if required.is_empty() {
            self.readback_complete();
        } else {
            self.pending_readback = Some(required);
        }
    }

    /// The pending clearance is acknowledged: a pending handover takes effect.
    pub fn readback_complete(&mut self) {
        self.pending_readback = None;
        self.pending_items.clear();
        self.readback_requests = 0;
        if let Some(role) = self.handoff_to.take() {
            self.owner = role;
        }
    }

    /// The phase the pending clearance belongs to has passed: taxi / hold short / crossing
    /// once on the runway or airborne (a taxi to a stand once parked), take-off / line-up
    /// once airborne, landing / touch and go once landed, hold position once airborne.
    /// `Contact` never expires here.
    pub fn pending_expired(&self) -> bool {
        use FlightPhase as P;
        let phase = self.phase;
        self.pending_items.iter().any(|item| match item {
            AtcItem::Taxi {
                to: TaxiTo::Stand { .. },
                ..
            } => phase == P::Parked,
            AtcItem::Taxi { .. } | AtcItem::HoldShort(_) | AtcItem::CrossRunway(_) => {
                matches!(phase, P::OnRunway | P::TakeoffRoll) || phase.is_airborne()
            }
            AtcItem::ClearedTakeoff { .. }
            | AtcItem::LineUpAndWait(_)
            | AtcItem::HoldPosition
            | AtcItem::CheckSquawk(_)
            | AtcItem::SquawkCharlie => phase.is_airborne(),
            AtcItem::ClearedLand { .. } | AtcItem::ClearedTouchAndGo { .. } => {
                matches!(phase, P::Landed | P::TaxiIn)
            }
            _ => false,
        })
    }
}
