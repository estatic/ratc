//! The datarefs we subscribe to. Names, types and units are taken from
//! `Resources/plugins/DataRefs.txt` of X-Plane 11. Everything arrives over RREF as f32.

/// Index of each subscribed dataref (the RREF `index` field is the position in `ALL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Slot {
    Lat,
    Lon,
    /// metres MSL
    ElevM,
    /// true heading, degrees
    Psi,
    /// knots indicated
    Ias,
    /// metres per second
    Gs,
    /// feet per minute
    Vs,
    /// int 0/1
    OnGround,
    /// int, kHz (8.33 kHz capable). DataRefs.txt says "hertz", but X-Plane 11 sends
    /// e.g. 118105 for 118.105 MHz -- observed in a recorded track.
    Com1Khz,
    /// int, kHz, like COM1. Optional, like the COM selectors below: without them the pilot
    /// transmits and listens on COM1.
    Com2Khz,
    /// int, the radio selected for transmission on the audio panel: 6 = COM1, 7 = COM2.
    ComTx,
    /// int 0/1, COM1 selected for listening.
    ListenCom1,
    /// int 0/1, COM2 selected for listening.
    ListenCom2,
    /// seconds since sim start
    RunningTime,
    /// int 0/1
    Paused,
    /// int, the code as dialled (4201). Optional: telemetry is sent without it.
    XpdrCode,
    /// int, 0 off, 1 standby, 2 on, 3 alt, 4 test. Optional like the code.
    XpdrMode,
    /// seconds since midnight UTC. Optional: telemetry is sent without it.
    ZuluTime,
    /// int, days since 1 January (0 = 1 Jan). Optional like the time.
    LocalDateDays,
    WindDir,
    WindSpeedKt,
    BaroInhg,
    VisM,
    Temp,
    Dew,
    CloudCov0,
    CloudCov1,
    CloudCov2,
    CloudBase0,
    CloudBase1,
    CloudBase2,
}

pub const SLOT_COUNT: usize = 31;

/// Subscription table: (slot, dataref name, frequency Hz). Weather changes slowly, so it is
/// subscribed at 1 Hz (RREF frequencies are integers) and emitted every 5 s by the reader.
pub const ALL: [(Slot, &str, i32); SLOT_COUNT] = [
    (Slot::Lat, "sim/flightmodel/position/latitude", TELEMETRY_HZ),
    (
        Slot::Lon,
        "sim/flightmodel/position/longitude",
        TELEMETRY_HZ,
    ),
    (
        Slot::ElevM,
        "sim/flightmodel/position/elevation",
        TELEMETRY_HZ,
    ),
    (Slot::Psi, "sim/flightmodel/position/psi", TELEMETRY_HZ),
    (
        Slot::Ias,
        "sim/flightmodel/position/indicated_airspeed",
        TELEMETRY_HZ,
    ),
    (
        Slot::Gs,
        "sim/flightmodel/position/groundspeed",
        TELEMETRY_HZ,
    ),
    (
        Slot::Vs,
        "sim/flightmodel/position/vh_ind_fpm",
        TELEMETRY_HZ,
    ),
    (
        Slot::OnGround,
        "sim/flightmodel/failures/onground_any",
        TELEMETRY_HZ,
    ),
    (
        Slot::Com1Khz,
        "sim/cockpit2/radios/actuators/com1_frequency_hz_833",
        TELEMETRY_HZ,
    ),
    (
        Slot::Com2Khz,
        "sim/cockpit2/radios/actuators/com2_frequency_hz_833",
        TELEMETRY_HZ,
    ),
    (
        Slot::ComTx,
        "sim/cockpit2/radios/actuators/audio_com_selection",
        TELEMETRY_HZ,
    ),
    (
        Slot::ListenCom1,
        "sim/cockpit2/radios/actuators/audio_selection_com1",
        TELEMETRY_HZ,
    ),
    (
        Slot::ListenCom2,
        "sim/cockpit2/radios/actuators/audio_selection_com2",
        TELEMETRY_HZ,
    ),
    (
        Slot::RunningTime,
        "sim/time/total_running_time_sec",
        TELEMETRY_HZ,
    ),
    (Slot::Paused, "sim/time/paused", TELEMETRY_HZ),
    (
        Slot::XpdrCode,
        "sim/cockpit/radios/transponder_code",
        TELEMETRY_HZ,
    ),
    (
        Slot::XpdrMode,
        "sim/cockpit/radios/transponder_mode",
        TELEMETRY_HZ,
    ),
    (Slot::ZuluTime, "sim/time/zulu_time_sec", TELEMETRY_HZ),
    (
        Slot::LocalDateDays,
        "sim/time/local_date_days",
        TELEMETRY_HZ,
    ),
    (
        Slot::WindDir,
        "sim/weather/wind_direction_degt[0]",
        WEATHER_HZ,
    ),
    (
        Slot::WindSpeedKt,
        "sim/weather/wind_speed_kt[0]",
        WEATHER_HZ,
    ),
    (
        Slot::BaroInhg,
        "sim/weather/barometer_sealevel_inhg",
        WEATHER_HZ,
    ),
    (Slot::VisM, "sim/weather/visibility_reported_m", WEATHER_HZ),
    (Slot::Temp, "sim/weather/temperature_sealevel_c", WEATHER_HZ),
    (Slot::Dew, "sim/weather/dewpoi_sealevel_c", WEATHER_HZ),
    (Slot::CloudCov0, "sim/weather/cloud_coverage[0]", WEATHER_HZ),
    (Slot::CloudCov1, "sim/weather/cloud_coverage[1]", WEATHER_HZ),
    (Slot::CloudCov2, "sim/weather/cloud_coverage[2]", WEATHER_HZ),
    (
        Slot::CloudBase0,
        "sim/weather/cloud_base_msl_m[0]",
        WEATHER_HZ,
    ),
    (
        Slot::CloudBase1,
        "sim/weather/cloud_base_msl_m[1]",
        WEATHER_HZ,
    ),
    (
        Slot::CloudBase2,
        "sim/weather/cloud_base_msl_m[2]",
        WEATHER_HZ,
    ),
];

/// A string dataref (a byte array), read one byte per RREF slot as `name[i]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrDataref {
    pub name: &'static str,
    /// How many bytes are read; real values are far shorter than the dataref's 40.
    pub len: usize,
    /// RREF index of byte 0; byte `i` is at `first_index + i`.
    pub first_index: usize,
}

pub const ICAO_LEN: usize = 8;
pub const TAIL_LEN: usize = 12;

/// Aircraft type, e.g. `C172`. Its RREF indices follow the numeric slots.
pub const AIRCRAFT_ICAO: StrDataref = StrDataref {
    name: "sim/aircraft/view/acf_ICAO",
    len: ICAO_LEN,
    first_index: SLOT_COUNT,
};
/// Tail number, e.g. `SP-ABC`.
pub const AIRCRAFT_TAIL: StrDataref = StrDataref {
    name: "sim/aircraft/view/acf_tailnum",
    len: TAIL_LEN,
    first_index: SLOT_COUNT + ICAO_LEN,
};
pub const STRINGS: [StrDataref; 2] = [AIRCRAFT_ICAO, AIRCRAFT_TAIL];

/// Numeric slots plus every string byte: the number of subscriptions.
pub const SUBSCRIPTION_COUNT: usize = SLOT_COUNT + ICAO_LEN + TAIL_LEN;

/// Every subscription as (RREF index, dataref name, frequency Hz): the numeric table `ALL`
/// first, then the string bytes at 1 Hz.
pub fn subscriptions() -> impl Iterator<Item = (i32, String, i32)> {
    let numeric = ALL
        .into_iter()
        .map(|(slot, name, hz)| (slot as i32, name.to_string(), hz));
    let bytes = STRINGS.into_iter().flat_map(|d| {
        (0..d.len).map(move |i| {
            (
                (d.first_index + i) as i32,
                format!("{}[{i}]", d.name),
                WEATHER_HZ,
            )
        })
    });
    numeric.chain(bytes)
}

pub const TELEMETRY_HZ: i32 = 5;
pub const WEATHER_HZ: i32 = 1;

impl Slot {
    /// Telemetry slots come first, up to the sim date.
    pub fn is_telemetry(self) -> bool {
        (self as usize) <= Slot::LocalDateDays as usize
    }
}
