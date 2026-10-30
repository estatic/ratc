//! Airport layout (the apt.dat part: runways, taxi network, stands). With `xplane_root` it comes
//! from the scenery X-Plane itself loads, so taxi routes name taxiways the pilot can see; the
//! repo copy in `data_dir` is the fallback. Procedures and frequencies always come from the repo.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use atc_geo::LatLon;
use atc_navdata::cifp::ProcKind;
use atc_navdata::fixes::ProcedureSet;
use atc_navdata::model::{Airport, Destination, EdgeKind};
use atc_navdata::parse_apt_dat;
use atc_navdata::procedures::{AdjacentKind, Procedures};
use atc_navdata::xplane::{
    ResolveError, apt_dat_candidates, load_destinations, load_procedure_set, resolve_airport,
};
use serde::{Deserialize, Serialize};
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

/// One apt.dat the destination scan read: a change of path, size or modification time makes
/// the cache stale.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Source {
    path: PathBuf,
    size: u64,
    /// Nanoseconds since the Unix epoch.
    mtime: u64,
}

/// An airport as the cache keeps it (before the preferred names are applied).
#[derive(Serialize, Deserialize)]
struct CachedDestination {
    icao: String,
    name: String,
    lat: f64,
    lon: f64,
    elevation_ft: Option<i32>,
}

#[derive(Serialize, Deserialize)]
struct DestinationCache {
    sources: Vec<Source>,
    destinations: Vec<CachedDestination>,
}

fn sources_of(xplane_root: &Path) -> Vec<Source> {
    apt_dat_candidates(xplane_root)
        .into_iter()
        .map(|path| {
            let meta = std::fs::metadata(&path).ok();
            let mtime = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
            Source {
                size: meta.map_or(0, |m| m.len()),
                path,
                mtime,
            }
        })
        .collect()
}

fn read_cache(path: &Path, sources: &[Source]) -> Option<Vec<CachedDestination>> {
    let cache: DestinationCache = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (cache.sources == sources).then_some(cache.destinations)
}

fn write_cache(path: &Path, cache: &DestinationCache) {
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(cache)?)?;
        std::fs::rename(&tmp, path)
    };
    if let Err(e) = write() {
        warn!("destinations: cache {} not written: {e}", path.display());
    }
}

/// Every airport of the apt.dat files of the X-Plane install at `xp_root` (no radius: any
/// airfield X-Plane knows can be a flight's destination), sorted by ICAO. `names` maps an ICAO
/// to the name it is said with; the other names are the apt.dat ones, title-cased.
///
/// The scan of the whole world takes seconds, so its result is kept in `cache` (JSON) together
/// with the size and modification time of every apt.dat read, and reused while they match.
/// Empty (with a warning) when the files cannot be read: every clearance request is then
/// answered "say again destination".
pub fn load_all_destinations(
    xp_root: &Path,
    cache: &Path,
    names: &[(String, String)],
) -> Vec<Destination> {
    let sources = sources_of(xp_root);
    let raw = match read_cache(cache, &sources) {
        Some(cached) => {
            info!(
                "destinations: {} airports from {}",
                cached.len(),
                cache.display()
            );
            cached
        }
        None => {
            let origin = LatLon { lat: 0.0, lon: 0.0 };
            let list = match load_destinations(xp_root, origin, f64::INFINITY, &[]) {
                Ok(list) => list,
                Err(e) => {
                    warn!("destinations: {e}; clearances cannot name a destination");
                    return Vec::new();
                }
            };
            info!(
                "destinations: {} airports in {} apt.dat files",
                list.len(),
                sources.len()
            );
            let destinations: Vec<CachedDestination> = list
                .into_iter()
                .map(|d| CachedDestination {
                    icao: d.icao,
                    name: d.name,
                    lat: d.pos.lat,
                    lon: d.pos.lon,
                    elevation_ft: d.elevation_ft,
                })
                .collect();
            let fresh = DestinationCache {
                sources,
                destinations,
            };
            write_cache(cache, &fresh);
            fresh.destinations
        }
    };
    raw.into_iter()
        .map(|d| Destination {
            name: names
                .iter()
                .find(|(icao, _)| *icao == d.icao)
                .map_or(d.name, |(_, n)| n.clone()),
            icao: d.icao,
            pos: LatLon {
                lat: d.lat,
                lon: d.lon,
            },
            elevation_ft: d.elevation_ft,
        })
        .collect()
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
