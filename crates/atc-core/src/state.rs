//! Aircraft telemetry as seen by the core.

use atc_geo::LatLon;
use atc_phraseology::Callsign;

/// Simulation time in seconds.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct SimTime(pub f64);

/// One telemetry sample of the controlled aircraft.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AircraftState {
    pub callsign: Callsign,
    pub t: SimTime,
    pub pos: LatLon,
    pub alt_ft_msl: f64,
    pub heading_deg: f64,
    pub ias_kt: f64,
    pub gs_kt: f64,
    pub vs_fpm: f64,
    pub on_ground: bool,
    pub com1_khz: u32,
    /// Transponder code as dialled (`4201`); `None` when unknown.
    pub xpdr_code: Option<u16>,
    /// `None` when unknown.
    pub xpdr_mode: Option<TransponderMode>,
    /// Sim UTC time of day in seconds (`sim/time/zulu_time_sec`); `None` when unknown.
    pub utc_s: Option<f64>,
    /// Sim day of the year, 0 = 1 January (`sim/time/local_date_days`); `None` when unknown.
    pub day_of_year: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransponderMode {
    Off,
    Standby,
    /// Replying; X-Plane has no separate ALT, the ALT knob position reports `On` too.
    On,
    Test,
}

impl TransponderMode {
    /// The mode of X-Plane 11's `transponder_mode` dataref (0 off, 1 standby, 2 on, 3 test,
    /// as in `DataRefs.txt`); `None` for other values.
    pub fn from_xplane(mode: u8) -> Option<Self> {
        match mode {
            0 => Some(Self::Off),
            1 => Some(Self::Standby),
            2 => Some(Self::On),
            3 => Some(Self::Test),
            _ => None,
        }
    }

    /// The transponder does not reply: off or on standby.
    pub fn is_silent(self) -> bool {
        matches!(self, Self::Off | Self::Standby)
    }
}
