//! Hand-authored airport procedures (`procedures.toml`): circuits, transition altitude, ATC positions.

use serde::{Deserialize, Serialize};

use crate::apt::NavdataError;
use crate::model::FreqKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitSide {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CircuitDef {
    pub runway: String,
    pub side: CircuitSide,
    pub altitude_ft_msl: i32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PositionDef {
    pub role: FreqKind,
    pub callsign: String,
    pub khz: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Procedures {
    pub icao: String,
    pub source: String,
    pub airac: String,
    pub transition_altitude_ft: i32,
    pub circuits: Vec<CircuitDef>,
    pub preferred_runway: String,
    pub positions: Vec<PositionDef>,
    /// Published VFR reporting points.
    #[serde(default)]
    pub vfr_points: Vec<VfrPoint>,
    /// Names of the VFR points where holding is published.
    #[serde(default)]
    pub vfr_holds: Vec<String>,
    /// Highest altitude (ft AMSL) for VFR holding.
    #[serde(default)]
    pub vfr_hold_max_ft: Option<i32>,
    /// Neighbouring units the pilot is handed to or from.
    #[serde(default)]
    pub adjacent: Vec<AdjacentUnit>,
    /// Control area approximation.
    #[serde(default)]
    pub tma: Option<Tma>,
    /// Delivery opening hours.
    #[serde(default)]
    pub delivery_hours: Option<DeliveryHours>,
    /// IFR defaults.
    #[serde(default)]
    pub ifr: Option<IfrDefaults>,
    /// Additional approach frequencies (kHz) of the Approach position.
    #[serde(default)]
    pub approach_alt_khz: Vec<u32>,
    /// Magnetic variation in degrees, east positive (magnetic = true - variation).
    #[serde(default)]
    pub magnetic_variation_deg: f64,
}

impl Procedures {
    /// The VFR point called `name` (case-insensitive).
    pub fn vfr_point(&self, name: &str) -> Option<&VfrPoint> {
        self.vfr_points
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
}

/// A VFR reporting point.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct VfrPoint {
    /// Point name.
    pub name: String,
    /// Latitude, decimal degrees.
    pub lat: f64,
    /// Longitude, decimal degrees.
    pub lon: f64,
    /// Entry point for arrivals and departures from the south.
    #[serde(default)]
    pub south: bool,
}

/// Kind of a neighbouring unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum AdjacentKind {
    /// Area control centre.
    Acc,
    /// Flight information service.
    Fis,
    /// Aerodrome tower.
    Tower,
}

/// A neighbouring unit with its frequency.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct AdjacentUnit {
    /// Radio callsign of the unit.
    pub callsign: String,
    /// Frequency, kHz.
    pub khz: u32,
    /// Unit kind.
    pub kind: AdjacentKind,
    /// ICAO code of the aerodrome for a tower.
    #[serde(default)]
    pub airport: Option<String>,
    /// Lowest level (ft) the unit serves.
    #[serde(default)]
    pub min_level_ft: Option<i32>,
}

/// Control area approximated as a circle around the airport.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct Tma {
    /// Radius, NM.
    pub radius_nm: f64,
    /// Upper limit, ft.
    pub ceiling_ft: i32,
}

/// Delivery opening hours, UTC hours `[open, close]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct DeliveryHours {
    /// Summer time hours.
    pub summer_utc: [u8; 2],
    /// Winter time hours.
    pub winter_utc: [u8; 2],
}

/// Defaults for IFR flights.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct IfrDefaults {
    /// Default cruise altitude, ft.
    pub cruise_ft: i32,
}

/// Parses a `procedures.toml` text.
pub fn parse_procedures(text: &str) -> Result<Procedures, NavdataError> {
    toml::from_str(text).map_err(|e| NavdataError::Malformed {
        line: e
            .span()
            .map_or(0, |s| text[..s.start].matches('\n').count() + 1),
        reason: e.message().to_string(),
    })
}
