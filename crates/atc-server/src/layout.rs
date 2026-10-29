//! Airport layout (the apt.dat part: runways, taxi network, stands). With `xplane_root` it comes
//! from the scenery X-Plane itself loads, so taxi routes name taxiways the pilot can see; the
//! repo copy in `data_dir` is the fallback. Procedures and frequencies always come from the repo.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use atc_geo::LatLon;
use atc_navdata::cifp::ProcKind;
use atc_navdata::fixes::ProcedureSet;
use atc_navdata::model::{Airport, Destination, EdgeKind};
use atc_navdata::parse_apt_dat;
use atc_navdata::procedures::{AdjacentKind, Procedures};
use atc_navdata::xplane::{ResolveError, load_destinations, load_procedure_set, resolve_airport};
use tracing::{info, warn};

/// Taxiway names of the network, sorted and deduplicated (unnamed and runway edges skipped).
pub fn taxiway_names(ap: &Airport) -> Vec<String> {
    let names: BTreeSet<String> = ap
        .taxi
        .edges
        .iter()
        .filter_map(|edge| match edge.kind {
            EdgeKind::Taxiway => Some(&edge.name),
            EdgeKind::Runway => None,
        })
        .filter(|name| !name.is_empty())
        .cloned()
        .collect();
    names.into_iter().collect()
}

/// The layout for `icao`: from the X-Plane install at `xplane_root` when given and the airport
/// resolves there, else `repo_dir/apt.dat` (with a warning saying why).
pub fn load_airport(xplane_root: Option<&Path>, repo_dir: &Path, icao: &str) -> Result<Airport> {
    if let Some(root) = xplane_root {
        match from_xplane(root, icao) {
            Ok((ap, source)) => {
                log_layout(&ap, &source, "X-Plane scenery");
                return Ok(ap);
            }
            Err(e) => {
                warn!(
                    "airport layout: X-Plane scenery unusable ({e:#}); falling back to the repo \
                     copy, taxi routes may name taxiways that are not in the sim"
                );
            }
        }
    } else {
        warn!(
            "airport layout: xplane_root not set; using the repo copy, taxi routes may not \
             match the sim scenery"
        );
    }
    let path = repo_dir.join("apt.dat");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let ap = parse_apt_dat(&text, icao).with_context(|| format!("parse {}", path.display()))?;
    log_layout(&ap, &path, "repo copy");
    Ok(ap)
}

fn from_xplane(root: &Path, icao: &str) -> Result<(Airport, PathBuf)> {
    let resolved = resolve_airport(root, icao)
        .with_context(|| format!("resolve {icao} in {}", root.display()))?;
    let ap = resolved
        .parse(icao)
        .with_context(|| format!("parse {icao} from {}", resolved.source.display()))?;
    Ok((ap, resolved.source))
}

fn log_layout(ap: &Airport, source: &Path, origin: &str) {
    let unnamed = ap
        .taxi
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Taxiway && e.name.is_empty())
        .count();
    info!(
        "airport layout: {} ({origin}): {} runways, {} taxi nodes, {} taxi edges ({unnamed} \
         unnamed taxiway edges), {} stands; taxiways: {}",
        source.display(),
        ap.runways.len(),
        ap.taxi.nodes.len(),
        ap.taxi.edges.len(),
        ap.stands.len(),
        taxiway_names(ap).join(", ")
    );
}

/// Radius around the home airport searched for clearance destinations, NM.
const DESTINATION_RADIUS_NM: f64 = 100.0;

/// Airport ICAO to the name it is said with, for the adjacent towers: "Katowice Tower" at
/// EPKT is "Katowice".
pub fn adjacent_names(procs: &Procedures) -> Vec<(String, String)> {
    procs
        .adjacent
        .iter()
        .filter(|u| u.kind == AdjacentKind::Tower)
        .filter_map(|u| {
            let icao = u.airport.clone()?;
            let city = u.callsign.strip_suffix(" Tower").unwrap_or(&u.callsign);
            Some((icao, city.to_string()))
        })
        .collect()
}

/// The airports a flight from `airport` may be cleared to: those of the apt.dat files of the
/// install within 100 NM. Empty (with a warning) without `xplane_root` or when they cannot be
/// read, so every clearance request is answered "say again destination".
pub fn load_destinations_near(
    xplane_root: Option<&Path>,
    airport: &Airport,
    names: &[(String, String)],
) -> Vec<Destination> {
    let Some(root) = xplane_root else {
        warn!("destinations: xplane_root not set; clearances cannot name a destination");
        return Vec::new();
    };
    let Some(around) = airport_position(airport) else {
        warn!("destinations: {} has no runway; none loaded", airport.icao);
        return Vec::new();
    };
    match load_destinations(root, around, DESTINATION_RADIUS_NM, names) {
        Ok(list) => {
            info!(
                "destinations: {} airports within {DESTINATION_RADIUS_NM} NM of {}",
                list.len(),
                airport.icao
            );
            list
        }
        Err(e) => {
            warn!("destinations: {e}; clearances cannot name a destination");
            Vec::new()
        }
    }
}

/// Midpoint of the first runway.
fn airport_position(ap: &Airport) -> Option<LatLon> {
    let [a, b] = &ap.runways.first()?.ends;
    Some(LatLon {
        lat: (a.threshold.lat + b.threshold.lat) / 2.0,
        lon: (a.threshold.lon + b.threshold.lon) / 2.0,
    })
}

/// The CIFP procedures of `icao` from the install at `xplane_root`, or `None` with one
/// warning when `xplane_root` is unset, the install has no CIFP for the airport, or it cannot
/// be used. The server then answers IFR clearance requests "unable IFR"; VFR and circuit work
/// as before.
pub fn load_procedure_set_for(xplane_root: Option<&Path>, icao: &str) -> Option<ProcedureSet> {
    let Some(root) = xplane_root else {
        warn!(
            "procedures: xplane_root not set, no CIFP procedures for {icao}; IFR clearances \
             will be answered \"unable IFR\", VFR and circuit work as before"
        );
        return None;
    };
    match load_procedure_set(root, icao) {
        Ok(set) => {
            let count = |k| {
                set.procedures
                    .iter()
                    .filter(|p| p.procedure.kind == k)
                    .count()
            };
            info!(
                "procedures: {icao} from CIFP of {}: cycle {}, {} SIDs, {} STARs, {} approaches, \
                 {} fixes",
                root.display(),
                set.cycle,
                count(ProcKind::Sid),
                count(ProcKind::Star),
                count(ProcKind::Approach),
                set.fixes.len()
            );
            Some(set)
        }
        Err(e @ ResolveError::CifpNotFound { .. }) => {
            warn!(
                "procedures: {e}; IFR clearances will be answered \"unable IFR\", VFR and \
                 circuit work as before"
            );
            None
        }
        Err(e) => {
            warn!(
                "procedures: CIFP of {icao} unusable ({e}); IFR clearances will be answered \
                 \"unable IFR\", VFR and circuit work as before"
            );
            None
        }
    }
}

/// The station's positions and procedures: `station.toml` (+ `overrides.toml`) when present,
/// else `procedures.toml`. Errors name the offending file.
pub fn load_procedures(dir: &Path) -> Result<Procedures> {
    Ok(atc_navdata::station::load_station(dir)?)
}
