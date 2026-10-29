//! Builds a station (`Procedures` plus provenance) from OpenAIP and X-Plane layout data.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use atc_geo::{FT_PER_M, LatLon, M_PER_NM, bearing_deg, distance_m};
use atc_navdata::model::{Airport, FreqKind};
use atc_navdata::procedures::{
    AdjacentKind, AdjacentUnit, AirspaceDef, AirspaceKind, CircuitDef, CircuitSide, IfrDefaults,
    PositionDef, Procedures, Tma, VfrPoint,
};

use crate::defaults::{
    airspace_kind, callsign_from_name, place_name, role_of_frequency, role_order, role_word,
    title_case, transition_altitude,
};
use crate::openaip::{Limit, OaAirport, OaAirspace, OaFrequency, khz};
use crate::store::CountryData;

const OPENAIP: &str = "OpenAIP";
const XPLANE: &str = "X-Plane apt.dat";
const TOWER_RADIUS_M: f64 = 100_000.0;

/// What the user ticked in the wizard.
#[derive(Debug, Clone)]
pub struct Choice {
    /// The positions to keep.
    pub positions: Vec<PositionDef>,
}

/// A generated station with the origin of every top-level field.
#[derive(Debug, Clone)]
pub struct StationDoc {
    pub procs: Procedures,
    /// Top-level field name to `"OpenAIP"`, `"X-Plane apt.dat"` or `"default: <rule>"`.
    pub provenance: BTreeMap<String, String>,
}

fn parsed(f: &OaFrequency) -> Option<u32> {
    khz(&f.value)
}

/// One position per OpenAIP frequency that has a role, except approach.
///
/// Extra Tower, Ground, ATIS, ... frequencies stay separate positions: they are distinct
/// sector or service frequencies the user may tick individually. All approach-type frequencies
/// serve the same airspace, so they form one Approach position (the primary, else the lowest
/// frequency) and the others become `approach_alt_khz`.
pub fn offered_positions(ap: &OaAirport) -> Vec<PositionDef> {
    let mut out: Vec<PositionDef> = Vec::new();
    let mut approach: Vec<&OaFrequency> = Vec::new();
    for f in &ap.frequencies {
        let (Some(role), Some(k)) = (f.kind.and_then(role_of_frequency), parsed(f)) else {
            continue;
        };
        if role == FreqKind::Approach {
            approach.push(f);
            continue;
        }
        out.push(position(ap, role, f.name.as_deref(), k));
    }
    let primary = approach
        .iter()
        .find(|f| f.primary)
        .copied()
        .or_else(|| approach.iter().min_by_key(|f| parsed(f)).copied());
    if let Some(f) = primary {
        out.push(position(
            ap,
            FreqKind::Approach,
            f.name.as_deref(),
            parsed(f).unwrap_or(0),
        ));
    }
    out.sort_by_key(|p| (role_order(p.role), p.khz));
    out
}

fn position(ap: &OaAirport, role: FreqKind, name: Option<&str>, khz: u32) -> PositionDef {
    let callsign = match name.filter(|n| !n.trim().is_empty()) {
        Some(n) => callsign_from_name(n, &ap.name),
        None => format!("{} {}", title_case(&ap.name), role_word(role)),
    };
    PositionDef {
        role,
        callsign,
        khz,
    }
}

/// Generates the station for `ap`.
pub fn generate(
    ap: &OaAirport,
    data: &CountryData,
    layout: &Airport,
    choice: &Choice,
    fetched_on: &str,
) -> StationDoc {
    let arp = ap.geometry.lat_lon();
    // AGL limits are converted with the published AIP elevation (OpenAIP), which is what the
    // limits are referenced to; the X-Plane elevation is only a fallback when OpenAIP has none.
    let elev = if ap.elevation.is_some() {
        ap.elevation_ft()
    } else {
        layout.elevation_ft
    };
    let airspaces = airspaces_around(arp, &data.airspaces, elev);
    let (ta, ta_src) = transition_altitude(&ap.country);

    let approach_khz = choice
        .positions
        .iter()
        .find(|p| p.role == FreqKind::Approach)
        .map(|p| p.khz);
    let mut alt: Vec<u32> = match approach_khz {
        Some(main) => ap
            .frequencies
            .iter()
            .filter(|f| f.kind.and_then(role_of_frequency) == Some(FreqKind::Approach))
            .filter_map(parsed)
            .filter(|&k| k != main)
            .collect(),
        None => Vec::new(),
    };
    alt.sort_unstable();
    alt.dedup();

    let procs = Procedures {
        icao: icao_code(ap).map_or_else(|| layout.icao.clone(), str::to_string),
        source: format!("OpenAIP {fetched_on}, X-Plane apt.dat"),
        airac: String::new(),
        transition_altitude_ft: ta,
        circuits: circuits(layout),
        preferred_runway: preferred_runway(layout),
        positions: choice.positions.clone(),
        vfr_points: data
            .points
            .iter()
            .filter(|p| p.airports.contains(&ap.id))
            .map(|p| {
                let [lat, lon] = p.geometry.lat_lon();
                VfrPoint {
                    name: p.name.clone(),
                    lat,
                    lon,
                    south: false,
                }
            })
            .collect(),
        vfr_holds: Vec::new(),
        vfr_hold_max_ft: None,
        adjacent: adjacent(ap, data, &airspaces),
        tma: tma(arp, &airspaces),
        delivery_hours: None,
        ifr: Some(IfrDefaults { cruise_ft: 5000 }),
        approach_alt_khz: alt,
        magnetic_variation_deg: ap.magnetic_declination.unwrap_or(0.0),
        airspaces: airspaces.iter().map(|a| a.def.clone()).collect(),
    };

    let d = |rule: &str| format!("default: {rule}");
    let provenance = [
        (
            "icao",
            if icao_code(ap).is_some() {
                OPENAIP.to_string()
            } else {
                XPLANE.to_string()
            },
        ),
        ("source", d("composed from the fetch date")),
        ("airac", d("left empty")),
        ("transition_altitude_ft", ta_src),
        ("circuits", XPLANE.to_string()),
        ("preferred_runway", d("closest to 270°")),
        ("positions", OPENAIP.to_string()),
        ("vfr_points", OPENAIP.to_string()),
        ("vfr_holds", d("none published in OpenAIP")),
        ("vfr_hold_max_ft", d("none published in OpenAIP")),
        ("adjacent", OPENAIP.to_string()),
        ("tma", OPENAIP.to_string()),
        ("delivery_hours", d("always open")),
        ("ifr", d("cruise 5000 ft")),
        ("approach_alt_khz", OPENAIP.to_string()),
        ("magnetic_variation_deg", OPENAIP.to_string()),
        ("airspaces", OPENAIP.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    StationDoc { procs, provenance }
}

/// The OpenAIP ICAO code, `None` when absent or blank.
fn icao_code(ap: &OaAirport) -> Option<&str> {
    ap.icao_code.as_deref().filter(|s| !s.trim().is_empty())
}

fn circuits(layout: &Airport) -> Vec<CircuitDef> {
    // Circuits are flown in the simulator world, so they use the X-Plane layout elevation.
    let alt = (layout.elevation_ft + 1000 + 99).div_euclid(100) * 100;
    layout
        .runways
        .iter()
        .flat_map(|r| r.ends.iter())
        .map(|e| CircuitDef {
            runway: e.ident.clone(),
            side: CircuitSide::Left,
            altitude_ft_msl: alt,
        })
        .collect()
}

fn preferred_runway(layout: &Airport) -> String {
    let mut best: Option<(f64, &str)> = None;
    for r in &layout.runways {
        for (i, e) in r.ends.iter().enumerate() {
            let hdg = bearing_deg(e.threshold, r.ends[1 - i].threshold);
            let diff = ((hdg - 270.0 + 540.0) % 360.0 - 180.0).abs();
            if best.is_none_or(|(d, _)| diff < d) {
                best = Some((diff, &e.ident));
            }
        }
    }
    best.map(|(_, id)| id.to_string()).unwrap_or_default()
}

struct Around<'a> {
    def: AirspaceDef,
    src: &'a OaAirspace,
}

pub(crate) fn limit_ft(l: &Limit, field_elev_ft: i32) -> i32 {
    let ft = match l.unit {
        6 => l.value * 100.0,
        1 => l.value,
        _ => l.value * FT_PER_M,
    }
    .round() as i32;
    if l.reference_datum == 0 && l.value > 0.0 {
        ft + field_elev_ft
    } else {
        ft
    }
}

fn airspaces_around(arp: [f64; 2], all: &[OaAirspace], elev: i32) -> Vec<Around<'_>> {
    all.iter()
        .filter_map(|a| {
            let kind = airspace_kind(a.kind)?;
            let polygon = a.geometry.outer_ring_lat_lon();
            if !contains(&polygon, arp) {
                return None;
            }
            let def = AirspaceDef {
                kind,
                name: a.name.clone(),
                class: a.icao_class.filter(|&c| c <= 6).map(|c| (b'A' + c) as char),
                lower_ft: limit_ft(&a.lower_limit, elev),
                upper_ft: limit_ft(&a.upper_limit, elev),
                polygon,
            };
            Some(Around { def, src: a })
        })
        .collect()
}

/// Ray-casting point-in-polygon on `[lat, lon]` rings (planar; the areas are small).
fn contains(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a[0] > p[0]) != (b[0] > p[0])
            && p[1] < (b[1] - a[1]) * (p[0] - a[0]) / (b[0] - a[0]) + a[1]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

fn ll(p: [f64; 2]) -> LatLon {
    LatLon {
        lat: p[0],
        lon: p[1],
    }
}

fn tma(arp: [f64; 2], around: &[Around<'_>]) -> Option<Tma> {
    use atc_navdata::procedures::AirspaceKind::{Ctr, Tma as TmaKind};
    let smallest = |kind| {
        around
            .iter()
            .filter(|a| a.def.kind == kind)
            .min_by(|a, b| area(&a.def.polygon).total_cmp(&area(&b.def.polygon)))
    };
    let a = smallest(TmaKind).or_else(|| smallest(Ctr))?;
    let ring = &a.def.polygon;
    // A closed ring repeats its first vertex.
    let n = ring.len() - usize::from(ring.len() > 1 && ring.first() == ring.last());
    let mean = ring[..n]
        .iter()
        .map(|&v| distance_m(ll(arp), ll(v)))
        .sum::<f64>()
        / n as f64;
    Some(Tma {
        radius_nm: (mean / M_PER_NM).round(),
        ceiling_ft: a.def.upper_ft,
    })
}

pub(crate) fn first_khz(freqs: &[OaFrequency]) -> Option<(u32, Option<&str>)> {
    freqs
        .iter()
        .find_map(|f| parsed(f).map(|k| (k, f.name.as_deref())))
}

/// Sector name without the `FIS ` prefix and a trailing directional suffix.
pub(crate) fn fis_name(sector: &str) -> String {
    const DIRS: [&str; 16] = [
        "ZACHOD",
        "ZACHÓD",
        "WSCHOD",
        "WSCHÓD",
        "POLNOC",
        "PÓŁNOC",
        "POLUDNIE",
        "POŁUDNIE",
        "NORTH",
        "SOUTH",
        "EAST",
        "WEST",
        "N",
        "S",
        "E",
        "W",
    ];
    let mut s = sector.trim();
    if s.len() >= 4 && s.is_char_boundary(4) && s[..4].eq_ignore_ascii_case("FIS ") {
        s = s[4..].trim();
    }
    if let Some(i) = s.rfind(['-', ' ']) {
        let tail = &s[i + 1..];
        if DIRS.iter().any(|d| d.to_uppercase() == tail.to_uppercase()) {
            s = s[..i].trim();
        }
    }
    title_case(s)
}

fn adjacent(ap: &OaAirport, data: &CountryData, around: &[Around<'_>]) -> Vec<AdjacentUnit> {
    let arp = ll(ap.geometry.lat_lon());
    let mut out = Vec::new();
    for a in around {
        let Some((k, name)) = first_khz(&a.src.frequencies) else {
            continue;
        };
        match a.def.kind {
            AirspaceKind::Fis => out.push(AdjacentUnit {
                callsign: format!("{} Information", fis_name(&a.src.name)),
                khz: k,
                kind: AdjacentKind::Fis,
                airport: None,
                min_level_ft: None,
            }),
            AirspaceKind::Acc => out.push(AdjacentUnit {
                callsign: title_case(name.unwrap_or(&a.src.name)),
                khz: k,
                kind: AdjacentKind::Acc,
                airport: None,
                min_level_ft: Some(a.def.lower_ft),
            }),
            _ => {}
        }
    }
    let mut towers: Vec<(f64, AdjacentUnit)> = data
        .airports
        .iter()
        .filter(|o| o.id != ap.id)
        .filter_map(|o| {
            let dist = distance_m(arp, ll(o.geometry.lat_lon()));
            let twr = o.frequencies.iter().filter(|f| f.kind == Some(14));
            let f = twr.clone().find(|f| f.primary).or(twr.clone().next())?;
            let k = parsed(f)?;
            let callsign = match f.name.as_deref().filter(|n| !n.trim().is_empty()) {
                Some(n) => callsign_from_name(n, &o.name),
                None => format!("{} Tower", place_name(&o.name)),
            };
            (dist <= TOWER_RADIUS_M).then(|| {
                (
                    dist,
                    AdjacentUnit {
                        callsign,
                        khz: k,
                        kind: AdjacentKind::Tower,
                        airport: icao_code(o).map(str::to_string),
                        min_level_ft: None,
                    },
                )
            })
        })
        .collect();
    towers.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.extend(towers.into_iter().map(|(_, u)| u));
    out
}

/// Writes the station as TOML, each top-level key preceded by `# source: <provenance>`.
pub fn write_station(doc: &StationDoc, path: &Path) -> io::Result<()> {
    let bad = |e: &dyn std::fmt::Display| io::Error::new(io::ErrorKind::InvalidData, e.to_string());
    let text = toml::to_string(&doc.procs).map_err(|e| bad(&e))?;
    let mut ed: toml_edit::DocumentMut = text.parse().map_err(|e| bad(&e))?;
    for (mut key, item) in ed.iter_mut() {
        let comment = match doc.provenance.get(key.get()) {
            Some(src) => format!("# source: {src}\n"),
            None => continue,
        };
        match item {
            toml_edit::Item::Table(t) => t.decor_mut().set_prefix(comment),
            toml_edit::Item::ArrayOfTables(a) => {
                if let Some(t) = a.iter_mut().next() {
                    t.decor_mut().set_prefix(comment);
                }
            }
            _ => key.leaf_decor_mut().set_prefix(comment),
        }
    }
    write_atomic(path, ed.to_string().as_bytes())
}

/// Writes `bytes` to `<path>.tmp`, then renames it over `path`, so a reader never sees a
/// half-written file.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}
