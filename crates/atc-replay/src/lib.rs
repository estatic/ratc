//! Scenario replay: drives the ATC engine with a scripted flight and prints the radio transcript.
//!
//! A scenario (TOML) names an airport, a pilot, the weather, a synthetic track and the lines the
//! pilot says (`[[say]]`), each fired at a flight phase or at a sim time. An optional
//! `transponder = { code, mode }` sets the transponder from the start, and on a `[[say]]` the
//! pilot sets it just before speaking. The output is deterministic: one
//! `[{t:7.1}] {WHO}: {text}` line per transmission, and a `VEHICLE` line per vehicle order
//! (follow-me car sent or cancelled).
//!
//! Language: the top-level `language = "auto" | "en" | "pl"` sets the engine's language mode
//! (default `"en"`), and `lang = "en" | "pl"` on a `[[say]]` is the language the speech
//! recogniser detected the line in (default `"en"`).
//!
//! COM1 of every sample is the first position's frequency until the pilot speaks, then the
//! frequency of the last line spoken, or of the station the pilot was told to contact once the
//! readback is said; Tower calls an aircraft only when COM1 is on its frequency.
//!
//! Tracks: `track = "synthetic-circuit:<runway>"`, `track_file = "<csv>"`, or `track = "path"`
//! with a `[path]` table (`start = { stand = "35" }` or `{ lat, lon, alt, hdg }`, `runway` to
//! take off from, `waypoints` of `{ fix | point | lat+lon | hdg+nm, alt, kt }`, `land`; see
//! [`atc_core::testkit::path_track`]). `navdata = "xplane"`: the procedure set and the
//! destinations come from the X-Plane install ([`xplane_root`]), loaded as the server loads
//! them. `utc = "HH:MM"` (with `day`, default 200) gives the samples a sim UTC time.
//!
//! Triggers of a `[[say]]`: `at = "phase:<Name>"` (the first sample of that phase),
//! `at = "utc:HH:MM"`, `at = "heard:<words>"` (the first controller transmission containing
//! the words, lower case, after the line before it was said) or `t = <seconds>`. `freq` names
//! a position (`delivery`, `ground`, `tower`, `approach`, `atis`), an adjacent unit (`radar`,
//! `information`) or an adjacent tower by airport (`tower:EPKT`). `ATC_REPLAY_TRACE=1` prints
//! every sample on stderr.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use atc_core::atis::Weather;
use atc_core::engine::{
    AtcEngine, AtcOutput, EngineConfig, LanguageMode, VehicleCommand, VehicleOrder,
};
use atc_core::phase::FlightPhase;
use atc_core::readback::Strictness;
use atc_core::state::{AircraftState, SimTime, TransponderMode};
use atc_core::strip::PilotProfile;
use atc_core::testkit::{PathSpec, PathStart, Waypoint, WaypointPos, circuit_track, path_track};
use atc_geo::LatLon;
use atc_navdata::fixes::ProcedureSet;
use atc_navdata::model::{Airport, Destination, FreqKind};
use atc_navdata::parse_apt_dat;
use atc_navdata::procedures::{AdjacentKind, AdjacentUnit, Procedures};
use atc_navdata::xplane::{load_destinations, load_procedure_set};
use atc_phraseology::{AtcItem, Callsign, CircuitLeg, Lang, Wind};
use serde::Deserialize;

/// Sample interval of the synthetic track, seconds.
const SAMPLE_DT_S: f64 = 1.0;
/// Below this ground speed the aircraft counts as standing still.
const STOPPED_KT: f64 = 0.5;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    airport: String,
    pilot: PilotDef,
    weather: WeatherDef,
    #[serde(default = "default_strictness")]
    strictness: StrictnessDef,
    track: Option<String>,
    track_file: Option<String>,
    #[serde(default, rename = "say")]
    says: Vec<Say>,
    #[serde(default, rename = "event")]
    events: Vec<Event>,
    transponder: Option<TransponderDef>,
    #[serde(default = "default_language")]
    language: LanguageDef,
    /// `track = "path"`: the flight.
    path: Option<PathDef>,
    /// `"xplane"`: the procedure set and destinations come from the X-Plane install
    /// ([`xplane_root`]).
    navdata: Option<NavdataDef>,
    /// Sim UTC time of the first sample, `"HH:MM"`; without it samples carry no time.
    utc: Option<String>,
    /// Day of the year (0 = 1 January) the samples carry with `utc`.
    #[serde(default = "default_day")]
    day: u16,
}

fn default_day() -> u16 {
    200
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum NavdataDef {
    Xplane,
}

/// `[path]` of a `track = "path"` scenario (see [`atc_core::testkit::PathSpec`]).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathDef {
    start: StartDef,
    runway: Option<String>,
    #[serde(default)]
    waypoints: Vec<WaypointDef>,
    land: Option<String>,
}

/// `{ stand = "35" }` or `{ lat, lon, alt, hdg }` (true heading).
#[derive(Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum StartDef {
    Stand {
        stand: String,
    },
    Air {
        lat: f64,
        lon: f64,
        alt: f64,
        hdg: f64,
    },
}

/// One of `fix`, `point`, `lat`+`lon` or `hdg`+`nm` (magnetic heading flown for `nm`), with
/// the altitude reached there and the ground speed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaypointDef {
    fix: Option<String>,
    point: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    hdg: Option<f64>,
    nm: Option<f64>,
    alt: f64,
    kt: f64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LanguageDef {
    Auto,
    En,
    Pl,
}

fn default_language() -> LanguageDef {
    LanguageDef::En
}

impl LanguageDef {
    fn mode(self) -> LanguageMode {
        match self {
            LanguageDef::Auto => LanguageMode::Auto,
            LanguageDef::En => LanguageMode::Fixed(Lang::En),
            LanguageDef::Pl => LanguageMode::Fixed(Lang::Pl),
        }
    }
}

/// A language the speech recogniser can tag a line with.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LangDef {
    #[default]
    En,
    Pl,
}

impl LangDef {
    fn lang(self) -> Lang {
        match self {
            LangDef::En => Lang::En,
            LangDef::Pl => Lang::Pl,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PilotDef {
    callsign: String,
    #[serde(rename = "type")]
    aircraft_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WeatherDef {
    wind_dir_deg: u16,
    wind_speed_kt: u16,
    visibility_m: u32,
    temp_c: i16,
    dewpoint_c: i16,
    qnh_hpa: u16,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum StrictnessDef {
    Training,
    Standard,
}

fn default_strictness() -> StrictnessDef {
    StrictnessDef::Standard
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Say {
    /// `"phase:<Name>"`, `"utc:HH:MM"` or `"heard:<words>"` (the first controller
    /// transmission containing the words, after the previous line was said); mutually
    /// exclusive with `t`.
    at: Option<String>,
    /// Sim time in seconds.
    t: Option<f64>,
    freq: String,
    text: String,
    /// Set before the line is said.
    transponder: Option<TransponderDef>,
    /// The language the line was detected in.
    #[serde(default)]
    lang: LangDef,
}

/// The transponder as the pilot sets it; a field left out keeps its value.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransponderDef {
    /// As dialled (`4201`).
    code: Option<u16>,
    /// As X-Plane reports it: 0 off, 1 standby, 2 on, 3 alt, 4 test.
    mode: Option<u8>,
}

/// Transponder state the scenario imposes on the track; unset fields leave the track's own.
#[derive(Clone, Copy, Debug, Default)]
struct Transponder {
    code: Option<u16>,
    mode: Option<TransponderMode>,
}

impl Transponder {
    fn set(&mut self, def: &TransponderDef) -> Result<()> {
        if let Some(code) = def.code {
            self.code = Some(code);
        }
        if let Some(m) = def.mode {
            let mode = TransponderMode::from_xplane(m)
                .ok_or_else(|| anyhow!("bad transponder mode {m} (expected 0 to 3)"))?;
            self.mode = Some(mode);
        }
        Ok(())
    }

    fn apply(self, s: &mut AircraftState) {
        if self.code.is_some() {
            s.xpdr_code = self.code;
        }
        if self.mode.is_some() {
            s.xpdr_mode = self.mode;
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    kind: EventKind,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum EventKind {
    /// The aircraft never stops at the holding point and rolls onto the runway uncleared.
    SkipHoldShort,
}

#[derive(Clone, Debug, PartialEq)]
enum Trigger {
    Phase(FlightPhase),
    Time(f64),
    Heard(String),
}

fn parse_phase(name: &str) -> Result<FlightPhase> {
    Ok(match name {
        "Parked" => FlightPhase::Parked,
        "TaxiOut" => FlightPhase::TaxiOut,
        "HoldingShort" => FlightPhase::HoldingShort,
        "OnRunway" => FlightPhase::OnRunway,
        "TakeoffRoll" => FlightPhase::TakeoffRoll,
        "Upwind" => FlightPhase::Circuit(CircuitLeg::Upwind),
        "Crosswind" => FlightPhase::Circuit(CircuitLeg::Crosswind),
        "Downwind" => FlightPhase::Circuit(CircuitLeg::Downwind),
        "Base" => FlightPhase::Circuit(CircuitLeg::Base),
        "Final" => FlightPhase::Circuit(CircuitLeg::Final),
        "Airborne" => FlightPhase::Airborne,
        "Landed" => FlightPhase::Landed,
        "TaxiIn" => FlightPhase::TaxiIn,
        other => bail!("unknown flight phase `{other}`"),
    })
}

/// `"HH:MM"` as seconds of the day.
fn parse_utc(s: &str) -> Result<f64> {
    let (h, m) = s
        .split_once(':')
        .ok_or_else(|| anyhow!("time `{s}` must look like `HH:MM`"))?;
    let (h, m): (u32, u32) = (
        h.parse().with_context(|| format!("bad hour in `{s}`"))?,
        m.parse().with_context(|| format!("bad minute in `{s}`"))?,
    );
    if h > 23 || m > 59 {
        bail!("time `{s}` out of range");
    }
    Ok(f64::from(h * 3600 + m * 60))
}

/// `start_utc`: the scenario's `utc`, needed by `utc:` triggers.
fn trigger_of(say: &Say, start_utc: Option<f64>) -> Result<Trigger> {
    match (&say.at, say.t) {
        (Some(at), None) => {
            if let Some(words) = at.strip_prefix("heard:") {
                return Ok(Trigger::Heard(words.to_lowercase()));
            }
            if let Some(time) = at.strip_prefix("utc:") {
                let start = start_utc
                    .ok_or_else(|| anyhow!("trigger `{at}` needs the scenario's `utc`"))?;
                return Ok(Trigger::Time(
                    (parse_utc(time)? - start).rem_euclid(86_400.0),
                ));
            }
            let name = at.strip_prefix("phase:").ok_or_else(|| {
                anyhow!(
                    "trigger `{at}` must look like `phase:<Name>`, `utc:HH:MM` or `heard:<words>`"
                )
            })?;
            Ok(Trigger::Phase(parse_phase(name)?))
        }
        (None, Some(t)) => Ok(Trigger::Time(t)),
        _ => bail!("say \"{}\" needs exactly one of `at` and `t`", say.text),
    }
}

/// The frequency (kHz) a `[[say]]` names: a position (`delivery`, `ground`, `tower`,
/// `approach`, `atis`), an adjacent unit (`radar`, `information`) or the adjacent tower of an
/// airport (`tower:<ICAO>`).
fn freq_khz(procs: &Procedures, name: &str) -> Result<u32> {
    let adjacent = |f: &dyn Fn(&AdjacentUnit) -> bool| {
        procs
            .adjacent
            .iter()
            .find(|u| f(u))
            .map(|u| u.khz)
            .ok_or_else(|| anyhow!("no adjacent unit for `{name}`"))
    };
    let kind = match name {
        "delivery" => FreqKind::Delivery,
        "ground" => FreqKind::Ground,
        "tower" => FreqKind::Tower,
        "approach" => FreqKind::Approach,
        "atis" => FreqKind::Atis,
        "radar" => return adjacent(&|u| u.kind == AdjacentKind::Acc),
        "information" => return adjacent(&|u| u.kind == AdjacentKind::Fis),
        other => {
            if let Some(icao) = other.strip_prefix("tower:") {
                return adjacent(&|u| {
                    u.kind == AdjacentKind::Tower
                        && u.airport
                            .as_deref()
                            .is_some_and(|a| a.eq_ignore_ascii_case(icao))
                });
            }
            bail!(
                "unknown frequency `{other}` (expected delivery, ground, tower, approach, atis, \
                 radar, information or tower:<ICAO>)"
            )
        }
    };
    procs
        .positions
        .iter()
        .find(|p| p.role == kind)
        .map(|p| p.khz)
        .ok_or_else(|| anyhow!("no `{name}` position"))
}

fn data_dir(icao: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/airports")
        .join(icao)
}

fn load_airport(icao: &str) -> Result<(Airport, Procedures)> {
    let dir = data_dir(icao);
    let read = |name: &str| {
        let path = dir.join(name);
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
    };
    let airport = parse_apt_dat(&read("apt.dat")?, icao)?;
    let procs = atc_navdata::station::load_station(&dir)
        .with_context(|| format!("loading the station {}", dir.display()))?;
    Ok((airport, procs))
}

/// Radius around the home airport searched for clearance destinations, NM (as the server).
const DESTINATION_RADIUS_NM: f64 = 100.0;

/// The X-Plane install for `navdata = "xplane"` scenarios: `ATC_XPLANE_ROOT`, else the
/// default Steam path; `None` when that directory does not exist.
pub fn xplane_root() -> Option<PathBuf> {
    let root = std::env::var_os("ATC_XPLANE_ROOT").map_or_else(
        || {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local/share/Steam/steamapps/common/X-Plane 11")
        },
        PathBuf::from,
    );
    root.is_dir().then_some(root)
}

/// Whether the scenario at `path` needs the X-Plane install (`navdata = "xplane"`).
pub fn needs_xplane(path: &Path) -> Result<bool> {
    let src =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let sc: Scenario =
        toml::from_str(&src).with_context(|| format!("parsing {}", path.display()))?;
    Ok(sc.navdata == Some(NavdataDef::Xplane))
}

/// The procedure set and destinations of `airport` from the X-Plane install, loaded as the
/// server loads them: destinations within 100 NM of the first runway's midpoint, adjacent
/// towers named as their callsign says ("Katowice Tower" at EPKT is "Katowice").
fn xplane_navdata(
    airport: &Airport,
    procs: &Procedures,
) -> Result<(ProcedureSet, Vec<Destination>)> {
    let root = xplane_root().ok_or_else(|| {
        anyhow!("scenario needs the X-Plane install (ATC_XPLANE_ROOT or the default Steam path)")
    })?;
    let set = load_procedure_set(&root, &airport.icao)
        .with_context(|| format!("CIFP procedures of {}", airport.icao))?;
    let names: Vec<(String, String)> = procs
        .adjacent
        .iter()
        .filter(|u| u.kind == AdjacentKind::Tower)
        .filter_map(|u| {
            let icao = u.airport.clone()?;
            let city = u.callsign.strip_suffix(" Tower").unwrap_or(&u.callsign);
            Some((icao, city.to_string()))
        })
        .collect();
    let [a, b] = &airport
        .runways
        .first()
        .ok_or_else(|| anyhow!("{} has no runway", airport.icao))?
        .ends;
    let around = LatLon {
        lat: (a.threshold.lat + b.threshold.lat) / 2.0,
        lon: (a.threshold.lon + b.threshold.lon) / 2.0,
    };
    let destinations =
        load_destinations(&root, around, DESTINATION_RADIUS_NM, &names).context("destinations")?;
    Ok((set, destinations))
}

/// The track with the stop at the first holding point removed: the aircraft keeps rolling onto
/// the runway. Samples after the removed stop are moved earlier so that time stays continuous.
fn skip_hold_short(track: Vec<AircraftState>) -> Vec<AircraftState> {
    let standing = |s: &AircraftState| s.on_ground && s.gs_kt < STOPPED_KT;
    let Some(moved) = track.iter().position(|s| !standing(s)) else {
        return track;
    };
    let Some(stop_start) = track[moved..].iter().position(standing).map(|i| moved + i) else {
        return track;
    };
    let stop_len = track[stop_start..]
        .iter()
        .take_while(|s| standing(s))
        .count();
    let gap = SAMPLE_DT_S * stop_len as f64;
    track
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !(stop_start..stop_start + stop_len).contains(i))
        .map(|(i, mut s)| {
            if i >= stop_start + stop_len {
                s.t = SimTime(s.t.0 - gap);
            }
            s
        })
        .collect()
}

/// Column order of a `track_file` CSV (header row required).
pub const TRACK_CSV_HEADER: &str =
    "t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz,xpdr_code,xpdr_mode";
/// Columns of recordings made before the transponder was recorded.
const TRACK_CSV_HEADER_V1: &str = "t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz";

/// Parses a track CSV (see [`TRACK_CSV_HEADER`]): `t` in s, `alt_ft` MSL, `hdg` true degrees,
/// `ias`/`gs` knots, `vs` ft/min, `on_ground` `1`/`0`/`true`/`false`, `com1_khz` integer,
/// `xpdr_code` as dialled (`4201`) and `xpdr_mode` as X-Plane reports it (0 off, 1 standby,
/// 2 on, 3 alt, 4 test), each empty when unknown. The header without the transponder
/// columns is accepted too (transponder unknown). Blank lines are skipped. The callsign is
/// left empty (the scenario's pilot sets it).
pub fn read_track_csv(src: &str) -> Result<Vec<AircraftState>> {
    let mut lines = src
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty());
    let header: Vec<&str> = lines
        .next()
        .map(|(_, l)| l.split(',').map(str::trim).collect())
        .unwrap_or_default();
    let Some(columns) = [TRACK_CSV_HEADER, TRACK_CSV_HEADER_V1]
        .into_iter()
        .map(|h| h.split(',').collect::<Vec<_>>())
        .find(|c| *c == header)
    else {
        bail!(
            "track CSV header must be `{TRACK_CSV_HEADER}`, got `{}`",
            header.join(",")
        );
    };
    lines
        .map(|(i, line)| parse_track_row(line, &columns).with_context(|| format!("line {}", i + 1)))
        .collect()
}

fn parse_track_row(line: &str, columns: &[&str]) -> Result<AircraftState> {
    let fields: Vec<&str> = line.split(',').map(str::trim).collect();
    if fields.len() != columns.len() {
        bail!("expected {} fields, got {}", columns.len(), fields.len());
    }
    let num = |i: usize| -> Result<f64> {
        fields[i]
            .parse()
            .with_context(|| format!("bad `{}` value `{}`", columns[i], fields[i]))
    };
    // An optional integer column: absent or empty is unknown.
    let opt = |i: usize| -> Result<Option<u16>> {
        match fields.get(i) {
            None | Some(&"") => Ok(None),
            Some(v) => v
                .parse()
                .map(Some)
                .with_context(|| format!("bad `{}` value `{v}`", columns[i])),
        }
    };
    let on_ground = match fields[8] {
        "1" | "true" => true,
        "0" | "false" => false,
        other => bail!("bad `on_ground` value `{other}` (expected 1, 0, true or false)"),
    };
    let com1_khz = fields[9]
        .parse()
        .with_context(|| format!("bad `com1_khz` value `{}`", fields[9]))?;
    let xpdr_mode = match opt(11)? {
        None => None,
        Some(m) => Some(
            u8::try_from(m)
                .ok()
                .and_then(TransponderMode::from_xplane)
                .ok_or_else(|| anyhow!("bad `xpdr_mode` value `{m}` (expected 0 to 3)"))?,
        ),
    };
    Ok(AircraftState {
        callsign: Callsign(String::new()),
        t: SimTime(num(0)?),
        pos: LatLon {
            lat: num(1)?,
            lon: num(2)?,
        },
        alt_ft_msl: num(3)?,
        heading_deg: num(4)?,
        ias_kt: num(5)?,
        gs_kt: num(6)?,
        vs_fpm: num(7)?,
        on_ground,
        com1_khz,
        xpdr_code: opt(10)?,
        xpdr_mode,
        utc_s: None,
        day_of_year: None,
    })
}

/// The flight of `[path]`, its names checked against the data.
fn path_spec(
    def: &PathDef,
    airport: &Airport,
    procs: &Procedures,
    set: Option<&ProcedureSet>,
) -> Result<PathSpec> {
    let runway_known = |r: &str| {
        airport
            .runways
            .iter()
            .any(|rw| rw.ends.iter().any(|e| e.ident == r))
    };
    for r in def.runway.iter().chain(&def.land) {
        if !runway_known(r) {
            bail!("path: no runway {r}");
        }
    }
    let start = match &def.start {
        StartDef::Stand { stand } => {
            if !airport.stands.iter().any(|s| s.name == *stand) {
                bail!("path: no stand {stand}");
            }
            if def.runway.is_none() {
                bail!("path: a start on a stand needs `runway`");
            }
            PathStart::Stand(stand.clone())
        }
        StartDef::Air { lat, lon, alt, hdg } => PathStart::Air {
            pos: LatLon {
                lat: *lat,
                lon: *lon,
            },
            alt_ft: *alt,
            hdg: *hdg,
        },
    };
    let waypoints = def
        .waypoints
        .iter()
        .enumerate()
        .map(|(k, w)| {
            let at = match (&w.fix, &w.point, w.lat, w.lon, w.hdg, w.nm) {
                (Some(fix), None, None, None, None, None) => {
                    if set.and_then(|s| s.fix(fix)).is_none() {
                        bail!("waypoint {}: no fix {fix} in the procedures", k + 1);
                    }
                    WaypointPos::Fix(fix.clone())
                }
                (None, Some(point), None, None, None, None) => {
                    if !procs
                        .vfr_points
                        .iter()
                        .any(|p| p.name.eq_ignore_ascii_case(point))
                    {
                        bail!("waypoint {}: no VFR point {point}", k + 1);
                    }
                    WaypointPos::Point(point.clone())
                }
                (None, None, Some(lat), Some(lon), None, None) => {
                    WaypointPos::LatLon(LatLon { lat, lon })
                }
                (None, None, None, None, Some(deg), Some(nm)) => WaypointPos::Heading { deg, nm },
                _ => bail!(
                    "waypoint {} needs exactly one of `fix`, `point`, `lat`+`lon`, `hdg`+`nm`",
                    k + 1
                ),
            };
            // A leg at no speed would never end.
            if w.kt.is_nan() || w.kt <= 0.0 {
                bail!("waypoint {}: `kt` must be above 0, got {}", k + 1, w.kt);
            }
            Ok(Waypoint {
                at,
                alt_ft: w.alt,
                kt: w.kt,
            })
        })
        .collect::<Result<_>>()?;
    Ok(PathSpec {
        start,
        runway: def.runway.clone(),
        waypoints,
        land: def.land.clone(),
    })
}

/// `track_file` paths are relative to the scenario file's directory.
fn build_track(
    sc: &Scenario,
    scenario_dir: &Path,
    airport: &Airport,
    procs: &Procedures,
    set: Option<&ProcedureSet>,
) -> Result<Vec<AircraftState>> {
    let mut track = match (&sc.track, &sc.track_file) {
        (Some(spec), None) if spec == "path" => {
            let def = sc
                .path
                .as_ref()
                .ok_or_else(|| anyhow!("`track = \"path\"` needs a `[path]` table"))?;
            path_track(
                airport,
                procs,
                set,
                &path_spec(def, airport, procs, set)?,
                SAMPLE_DT_S,
            )
        }
        (Some(spec), None) => {
            let runway = spec.strip_prefix("synthetic-circuit:").ok_or_else(|| {
                anyhow!("unknown track `{spec}` (expected `synthetic-circuit:<runway>` or `path`)")
            })?;
            circuit_track(airport, procs, runway, SAMPLE_DT_S)
        }
        (None, Some(file)) => {
            let path = scenario_dir.join(file);
            let src = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            read_track_csv(&src).with_context(|| format!("parsing {}", path.display()))?
        }
        _ => bail!("exactly one of `track` and `track_file` is required"),
    };
    let callsign = Callsign::new(&sc.pilot.callsign);
    let start_utc = sc.utc.as_deref().map(parse_utc).transpose()?;
    for s in &mut track {
        s.callsign = callsign.clone();
        if let Some(utc) = start_utc {
            s.utc_s = Some((utc + s.t.0).rem_euclid(86_400.0));
            s.day_of_year = Some(sc.day);
        }
    }
    for ev in &sc.events {
        match ev.kind {
            EventKind::SkipHoldShort => track = skip_hold_short(track),
        }
    }
    Ok(track)
}

struct Transcript(String);

impl Transcript {
    fn line(&mut self, t: f64, who: &str, text: &str) {
        self.0.push_str(&format!("[{t:7.1}] {who}: {text}\n"));
    }

    fn controller(&mut self, t: f64, out: &[AtcOutput]) {
        for o in out {
            self.line(t, &o.station, &o.text);
        }
    }

    fn vehicles(&mut self, t: f64, orders: Vec<VehicleOrder>) {
        for o in orders {
            let text = match o.command {
                VehicleCommand::FollowMe {
                    id,
                    path,
                    end_heading_deg,
                } => {
                    let end = path
                        .last()
                        .copied()
                        .unwrap_or(LatLon { lat: 0.0, lon: 0.0 });
                    format!(
                        "follow-me {id} for {}, {} points to {:.5} {:.5} heading {end_heading_deg:.0}",
                        o.callsign.0,
                        path.len(),
                        end.lat,
                        end.lon
                    )
                }
                VehicleCommand::FollowMeCancel { id } => format!("follow-me {id} cancelled"),
            };
            self.line(t, "VEHICLE", &text);
        }
    }
}

/// The frequency of the last "contact" in `out`.
fn contact_khz(out: &[AtcOutput]) -> Option<u32> {
    out.iter()
        .flat_map(|o| &o.message.items)
        .filter_map(|i| match i {
            AtcItem::Contact { khz, .. } => Some(*khz),
            _ => None,
        })
        .next_back()
}

/// Replays the scenario at `path` and returns its transcript.
pub fn run_scenario(path: &Path) -> Result<String> {
    let src =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let sc: Scenario =
        toml::from_str(&src).with_context(|| format!("parsing {}", path.display()))?;
    let (airport, procs) = load_airport(&sc.airport)?;
    let navdata = match sc.navdata {
        Some(NavdataDef::Xplane) => Some(xplane_navdata(&airport, &procs)?),
        None => None,
    };
    let start_utc = sc.utc.as_deref().map(parse_utc).transpose()?;

    // (trigger, frequency in kHz, text, language, transponder, done), in file order.
    let mut pending = Vec::new();
    for say in &sc.says {
        let khz = freq_khz(&procs, &say.freq)
            .with_context(|| format!("{}: say \"{}\"", sc.airport, say.text))?;
        pending.push((
            trigger_of(say, start_utc)?,
            khz,
            say.text.as_str(),
            say.lang.lang(),
            say.transponder.as_ref(),
            false,
        ));
    }

    let scenario_dir = path.parent().unwrap_or(Path::new("."));
    let set = navdata.as_ref().map(|(set, _)| set);
    let track = build_track(&sc, scenario_dir, &airport, &procs, set)?;
    let strictness = match sc.strictness {
        StrictnessDef::Training => Strictness::Training,
        StrictnessDef::Standard => Strictness::Standard,
    };
    let cfg = EngineConfig {
        language: sc.language.mode(),
        ..EngineConfig::new(strictness)
    };
    let procs_first_khz = procs.positions.first().map_or(0, |p| p.khz);
    let mut engine = AtcEngine::new(airport, procs, cfg);
    if let Some((set, destinations)) = navdata {
        engine = engine
            .with_procedure_set(set)
            .with_destinations(destinations);
    }
    let callsign = Callsign::new(&sc.pilot.callsign);
    engine.register_pilot(PilotProfile {
        callsign: callsign.clone(),
        aircraft_type: sc.pilot.aircraft_type.clone(),
    });
    let w = &sc.weather;
    engine.on_weather(Weather {
        wind: Wind {
            dir_deg: w.wind_dir_deg,
            speed_kt: w.wind_speed_kt,
            calm: w.wind_speed_kt == 0,
        },
        visibility_m: w.visibility_m,
        clouds: vec![],
        temp_c: w.temp_c,
        dewpoint_c: w.dewpoint_c,
        qnh_hpa: w.qnh_hpa,
    });

    let mut xpdr = Transponder::default();
    if let Some(def) = &sc.transponder {
        xpdr.set(def)?;
    }
    let mut transcript = Transcript(String::new());
    // COM1 of the samples: the first position until the pilot speaks, then the frequency of
    // the last line spoken, or the one Ground / Tower told it to contact (tuned after the
    // readback, which is still spoken on the old frequency).
    let mut com1 = procs_first_khz;
    let mut contact: Option<u32> = None;
    let mut seen_phases: Vec<FlightPhase> = Vec::new();
    let mut heard: Vec<String> = Vec::new();
    // `ATC_REPLAY_TRACE=1`: every sample on stderr, for writing path tracks.
    let trace = std::env::var_os("ATC_REPLAY_TRACE").is_some();
    for mut sample in track {
        let t = sample.t.0;
        xpdr.apply(&mut sample);
        sample.com1_khz = com1;
        let out = engine.on_telemetry(sample.clone());
        contact = contact_khz(&out).or(contact);
        transcript.controller(t, &out);
        transcript.vehicles(t, engine.take_vehicle_orders());
        // What the controllers said since the last pilot line, for `heard:` triggers.
        heard.extend(out.iter().map(|o| o.text.to_lowercase()));

        let phase = engine.strip(&callsign).map(|s| s.phase);
        if trace {
            eprintln!(
                "{t:7.1} {:.5} {:.5} {:6.0} ft {:3.0} deg {:3.0} kt {:6.0} fpm {phase:?}",
                sample.pos.lat,
                sample.pos.lon,
                sample.alt_ft_msl,
                sample.heading_deg,
                sample.gs_kt,
                sample.vs_fpm
            );
        }
        let new_phase = phase.filter(|p| !seen_phases.contains(p));
        seen_phases.extend(new_phase);
        let mut all_before_done = true;
        for (trigger, khz, text, lang, set_xpdr, done) in &mut pending {
            let due = match trigger {
                Trigger::Phase(p) => new_phase == Some(*p),
                Trigger::Time(at) => t >= *at,
                Trigger::Heard(words) => {
                    all_before_done && heard.iter().any(|h| h.contains(words.as_str()))
                }
            };
            if *done || !due {
                all_before_done &= *done;
                continue;
            }
            *done = true;
            heard.clear();
            if let Some(def) = set_xpdr {
                // The sim reports the new setting before the pilot speaks.
                xpdr.set(def)?;
                xpdr.apply(&mut sample);
                let out = engine.on_telemetry(sample.clone());
                contact = contact_khz(&out).or(contact);
                transcript.controller(t, &out);
            }
            transcript.line(t, "PILOT", text);
            let reply = engine.on_transmission(*khz, text, *lang, SimTime(t));
            contact = contact_khz(&reply).or(contact);
            com1 = contact.take().unwrap_or(*khz);
            transcript.controller(t, &reply);
            heard.extend(reply.iter().map(|o| o.text.to_lowercase()));
            transcript.vehicles(t, engine.take_vehicle_orders());
        }
    }
    for (trigger, _, text, _, _, done) in &pending {
        if !done {
            bail!(
                "pilot line \"{text}\" never fired ({trigger:?}); transcript so far:\n{}",
                transcript.0
            );
        }
    }
    Ok(transcript.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_track_file_csv() {
        let csv = "t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz\n\
                   0.0,50.0761,19.7961,791.0,257.0,0.0,0.0,0.0,1,118105\n\
                   \n\
                   0.2, 50.0762 ,19.7962,791.5,258.5,5.0,4.5,-10.0,true,118105\n\
                   0.4,50.0763,19.7963,850.0,259.0,60.0,58.0,500.0,0,123255\n";
        let track = read_track_csv(csv).unwrap();
        assert_eq!(track.len(), 3);
        let s = &track[1];
        assert_eq!(s.t, SimTime(0.2));
        assert_eq!((s.pos.lat, s.pos.lon), (50.0762, 19.7962));
        assert_eq!((s.alt_ft_msl, s.heading_deg), (791.5, 258.5));
        assert_eq!((s.ias_kt, s.gs_kt, s.vs_fpm), (5.0, 4.5, -10.0));
        assert!(s.on_ground);
        assert_eq!(s.com1_khz, 118_105);
        assert!(track[0].on_ground && !track[2].on_ground);
        assert_eq!(track[2].com1_khz, 123_255);
    }

    #[test]
    fn track_csv_with_transponder() {
        let csv = "t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz,xpdr_code,xpdr_mode\n\
                   0.0,50.0761,19.7961,791.0,257.0,0.0,0.0,0.0,1,118105,0421,1\n\
                   0.2,50.0761,19.7961,791.0,257.0,0.0,0.0,0.0,1,118105,,\n";
        let track = read_track_csv(csv).unwrap();
        assert_eq!(track[0].xpdr_code, Some(421));
        assert_eq!(track[0].xpdr_mode, Some(TransponderMode::Standby));
        assert_eq!((track[1].xpdr_code, track[1].xpdr_mode), (None, None));
        let bad = csv.replace(",1\n0.2", ",7\n0.2");
        let e = format!("{:#}", read_track_csv(&bad).unwrap_err());
        assert!(e.contains("line 2") && e.contains("xpdr_mode"), "{e}");
        // Old recordings have no transponder columns.
        let old = "t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz\n\
                   0.0,50.0761,19.7961,791.0,257.0,0.0,0.0,0.0,1,118105\n";
        let track = read_track_csv(old).unwrap();
        assert_eq!((track[0].xpdr_code, track[0].xpdr_mode), (None, None));
        let mixed = format!("{old}0.2,50.0761,19.7961,791.0,257.0,0.0,0.0,0.0,1,118105,4201,3\n");
        assert!(read_track_csv(&mixed).is_err());
    }

    /// Writes `src` as a scenario in a fresh directory and replays it.
    fn replay(name: &str, src: &str) -> Result<String> {
        let dir = std::env::temp_dir().join(format!("atc-replay-unit-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scenario.toml");
        std::fs::write(&path, src).unwrap();
        run_scenario(&path)
    }

    const HEAD: &str = r#"
airport = "EPKK"
pilot = { callsign = "SPABC", type = "C172" }
weather = { wind_dir_deg = 250, wind_speed_kt = 8, visibility_m = 10000, temp_c = 15, dewpoint_c = 10, qnh_hpa = 1013 }
track = "path"
"#;

    const PATH: &str = r#"
[path]
start = { lat = 50.15, lon = 19.70, alt = 2000, hdg = 120 }
waypoints = [
  { point = "ZULU", alt = 2000, kt = 90 },
  { hdg = 170, nm = 2, alt = 2000, kt = 90 },
]
"#;

    /// `utc` gives the samples a time and `utc:` triggers fire at it: Delivery answers
    /// before 22:00 UTC in summer and is closed after; `heard:` fires on the controller's
    /// words after the line before it.
    #[test]
    fn utc_and_heard_triggers() {
        let src = format!(
            r#"{HEAD}utc = "21:59"
day = 200
{PATH}
[[say]]
at = "utc:21:59"
freq = "delivery"
text = "Krakow Delivery, Sierra Papa Alpha Bravo Charlie, request start up"

[[say]]
at = "heard:contact kraków ground"
freq = "delivery"
text = "Ground one one eight decimal one zero five, Sierra Bravo Charlie"

[[say]]
at = "utc:22:01"
freq = "delivery"
text = "Krakow Delivery, Sierra Papa Alpha Bravo Charlie, request start up"
"#
        );
        let out = replay("utc", &src).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 4, "{out}");
        assert!(lines[0].starts_with("[    0.0] PILOT:"), "{out}");
        assert!(
            lines[1].contains("Kraków Delivery") && lines[1].contains("Ground"),
            "{out}"
        );
        assert!(lines[2].starts_with("[    0.0] PILOT: Ground"), "{out}");
        // 22:01 is 120 s after the start; Delivery is closed then.
        assert!(lines[3].starts_with("[  120.0] PILOT:"), "{out}");
    }

    #[test]
    fn path_and_frequency_errors() {
        let say = "\n[[say]]\nt = 1.0\nfreq = \"tower\"\ntext = \"hello\"\n";
        let fix = PATH.replace(r#"point = "ZULU""#, r#"fix = "KK599""#);
        let e = format!(
            "{:#}",
            replay("fix", &format!("{HEAD}{fix}{say}")).unwrap_err()
        );
        assert!(e.contains("no fix KK599"), "{e}");
        let both = PATH.replace(
            r#"point = "ZULU""#,
            r#"point = "ZULU", lat = 50.0, lon = 19.0"#,
        );
        let e = format!(
            "{:#}",
            replay("both", &format!("{HEAD}{both}{say}")).unwrap_err()
        );
        assert!(e.contains("exactly one of"), "{e}");
        let freq = say.replace("tower", "director");
        let e = format!(
            "{:#}",
            replay("freq", &format!("{HEAD}{PATH}{freq}")).unwrap_err()
        );
        assert!(e.contains("unknown frequency `director`"), "{e}");
        let stop = PATH.replace("kt = 90 },\n  { hdg", "kt = 0 },\n  { hdg");
        assert_ne!(stop, PATH);
        let e = format!(
            "{:#}",
            replay("kt", &format!("{HEAD}{stop}{say}")).unwrap_err()
        );
        assert!(e.contains("waypoint 1: `kt` must be above 0"), "{e}");
        let none = format!("{HEAD}{say}");
        let e = format!("{:#}", replay("none", &none).unwrap_err());
        assert!(e.contains("[path]"), "{e}");
        // The adjacent units by kind, and an adjacent tower by its airport.
        let src = format!(
            "{HEAD}{PATH}{}{}{}",
            say.replace("tower", "radar"),
            say.replace("tower", "information"),
            say.replace("\"tower\"", "\"tower:EPKT\"")
        );
        assert!(replay("adjacent", &src).is_ok());
    }

    #[test]
    fn track_csv_errors_name_the_line() {
        let bad_header = "t,lat\n0,1\n";
        assert!(format!("{:#}", read_track_csv(bad_header).unwrap_err()).contains("header"));
        let bad_value = "t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz\n\
                         0,50,19,791,257,0,0,0,maybe,118105\n";
        let e = format!("{:#}", read_track_csv(bad_value).unwrap_err());
        assert!(e.contains("line 2") && e.contains("on_ground"), "{e}");
        let short = "t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz\n0,50,19\n";
        let e = format!("{:#}", read_track_csv(short).unwrap_err());
        assert!(e.contains("line 2"), "{e}");
    }
}
