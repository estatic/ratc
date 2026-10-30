//! A flight handed from one engine (airport station) to another: what the next station
//! needs to carry it on ([`crate::engine::AtcEngine::export_flight`],
//! [`crate::engine::AtcEngine::import_flight`]).

use atc_phraseology::{Callsign, FlightRules};
use serde::{Deserialize, Serialize};

use crate::strip::FlightPlan;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlightHandover {
    pub callsign: Callsign,
    pub aircraft_type: String,
    /// The flight plan, once a clearance was issued.
    pub plan: Option<FlightPlan>,
    /// Transponder code assigned.
    pub squawk: Option<u16>,
    /// Level the flight is cleared to, in feet.
    pub cleared_ft: Option<i32>,
    /// Flight rules of the plan.
    pub rules: Option<FlightRules>,
}
