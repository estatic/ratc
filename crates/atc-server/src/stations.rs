//! Every station of `data_dir`: layout, positions, procedures and an engine each.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use atc_core::engine::{AtcEngine, EngineConfig};
use atc_navdata::model::{Airport, Destination};
use atc_navdata::procedures::Procedures;

use tracing::warn;

use crate::layout::{
    adjacent_names, load_airport, load_all_destinations, load_procedure_set_for, load_procedures,
};

/// The scan of the install's airports (see [`load_all_destinations`]).
pub const DESTINATIONS_CACHE: &str = "data/cache/destinations.json";

/// One airport the server answers for. `id` is the name of its directory in `data_dir`.
pub struct LoadedStation {
    pub id: String,
    pub dir: PathBuf,
    pub procs: Procedures,
    pub airport: Airport,
    pub engine: Mutex<AtcEngine>,
}

/// A station's files, read; its engine is built once every station's names are known.
struct Parts {
    id: String,
    dir: PathBuf,
    procs: Procedures,
    airport: Airport,
}

/// Reads the station `id` from `data_dir/<id>`.
fn read_parts(data_dir: &Path, id: &str, xplane_root: Option<&Path>) -> Result<Parts> {
    let dir = data_dir.join(id);
    let procs = load_procedures(&dir)?;
    let airport = load_airport(xplane_root, &dir, id)?;
    Ok(Parts {
        id: id.to_string(),
        dir,
        procs,
        airport,
    })
}

fn build(
    parts: Parts,
    xplane_root: Option<&Path>,
    cfg: &EngineConfig,
    destinations: &Arc<[Destination]>,
) -> LoadedStation {
    let Parts {
        id,
        dir,
        procs,
        airport,
    } = parts;
    let mut engine = AtcEngine::new(airport.clone(), procs.clone(), cfg.clone())
        .with_destinations(destinations.clone());
    if let Some(set) = load_procedure_set_for(xplane_root, &id) {
        engine = engine.with_procedure_set(set);
    }
    LoadedStation {
        id,
        dir,
        procs,
        airport,
        engine: Mutex::new(engine),
    }
}

/// The destinations of every engine: all airports of the install, scanned once (cached in
/// [`DESTINATIONS_CACHE`]), the adjacent towers of all stations by their spoken names.
fn shared_destinations(xplane_root: Option<&Path>, parts: &[Parts]) -> Arc<[Destination]> {
    let Some(root) = xplane_root else {
        warn!("destinations: xplane_root not set; clearances cannot name a destination");
        return Arc::from([]);
    };
    let mut names = Vec::new();
    for p in parts {
        for n in adjacent_names(&p.procs) {
            if !names.contains(&n) {
                names.push(n);
            }
        }
    }
    load_all_destinations(root, Path::new(DESTINATIONS_CACHE), &names).into()
}

/// Loads every station directory of `data_dir` (or just `only`), sorted by id. A directory that
/// does not load is skipped with a warning naming it and the error; it is an error when none
/// loads. `enroute.toml` and other non-directories are not stations.
pub fn load_stations(
    data_dir: &Path,
    only: Option<&str>,
    xplane_root: Option<&Path>,
    cfg: &EngineConfig,
) -> Result<Vec<LoadedStation>> {
    let ids: Vec<String> = match only {
        Some(id) => vec![id.to_string()],
        None => {
            let mut ids = Vec::new();
            let entries = std::fs::read_dir(data_dir)
                .with_context(|| format!("read {}", data_dir.display()))?;
            for entry in entries {
                let entry = entry.with_context(|| format!("read {}", data_dir.display()))?;
                if entry.path().is_dir() {
                    ids.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
            ids.sort();
            ids
        }
    };
    let mut parts = Vec::new();
    let mut errors = Vec::new();
    for id in &ids {
        match read_parts(data_dir, id, xplane_root) {
            Ok(p) => parts.push(p),
            Err(e) => {
                tracing::warn!("station {} skipped: {e:#}", data_dir.join(id).display());
                errors.push(format!("{id}: {e:#}"));
            }
        }
    }
    if parts.is_empty() {
        bail!(
            "no station loaded from {}{}",
            data_dir.display(),
            if errors.is_empty() {
                String::new()
            } else {
                format!(" ({})", errors.join("; "))
            }
        );
    }
    let destinations = shared_destinations(xplane_root, &parts);
    Ok(parts
        .into_iter()
        .map(|p| build(p, xplane_root, cfg, &destinations))
        .collect())
}
