//! Airport data model.

use atc_geo::LatLon;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Airport {
    pub icao: String,
    pub name: String,
    pub elevation_ft: i32,
    pub runways: Vec<Runway>,
    pub frequencies: Vec<Frequency>,
    pub taxi: TaxiNetwork,
    pub stands: Vec<Stand>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Runway {
    pub width_m: f64,
    pub ends: [RunwayEnd; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunwayEnd {
    pub ident: String,
    pub threshold: LatLon,
    pub displaced_m: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FreqKind {
    Atis,
    Unicom,
    Delivery,
    Ground,
    Tower,
    Approach,
    Departure,
    /// Aerodrome flight information service (data only; the engine serves none yet).
    Afis,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frequency {
    pub kind: FreqKind,
    pub khz: u32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaxiNode {
    pub id: u32,
    pub pos: LatLon,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    Taxiway,
    Runway,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ZoneKind {
    Departure,
    Arrival,
    Ils,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActiveZone {
    pub kind: ZoneKind,
    pub runways: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaxiEdge {
    pub from: u32,
    pub to: u32,
    pub one_way: bool,
    pub kind: EdgeKind,
    pub name: String,
    pub active_zones: Vec<ActiveZone>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaxiNetwork {
    pub nodes: Vec<TaxiNode>,
    pub edges: Vec<TaxiEdge>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stand {
    pub name: String,
    pub pos: LatLon,
    pub heading_deg: f64,
}

/// An airport that can be named as a flight destination.
#[derive(Clone, Debug, PartialEq)]
pub struct Destination {
    /// ICAO code.
    pub icao: String,
    /// Display name.
    pub name: String,
    /// Reference position: the datum, else the midpoint of the first runway.
    pub pos: LatLon,
    /// Field elevation in feet (apt.dat header), when known.
    pub elevation_ft: Option<i32>,
}
