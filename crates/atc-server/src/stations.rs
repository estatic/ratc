//! Every station of `data_dir`: layout, positions, procedures and an engine each.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use atc_core::engine::{AtcEngine, EngineConfig};
use atc_navdata::model::Airport;
use atc_navdata::procedures::Procedures;

use crate::layout::{
    adjacent_names, load_airport, load_destinations_near, load_procedure_set_for, load_procedures,
};

/// One airport the server answers for. `id` is the name of its directory in `data_dir`.
pub struct LoadedStation {
    pub id: String,
    pub dir: PathBuf,
    pub procs: Procedures,
    pub airport: Airport,
    pub engine: Mutex<AtcEngine>,
}

/// Loads the station `id` from `data_dir/<id>`.
fn load_one(
    data_dir: &Path,
    id: &str,
    xplane_root: Option<&Path>,
    cfg: &EngineConfig,
) -> Result<LoadedStation> {
    let dir = data_dir.join(id);
    let procs = load_procedures(&dir)?;
    let airport = load_airport(xplane_root, &dir, id)?;
    let destinations = load_destinations_near(xplane_root, &airport, &adjacent_names(&procs));
    let mut engine =
        AtcEngine::new(airport.clone(), procs.clone(), cfg.clone()).with_destinations(destinations);
    if let Some(set) = load_procedure_set_for(xplane_root, id) {
        engine = engine.with_procedure_set(set);
    }
    Ok(LoadedStation {
        id: id.to_string(),
        dir,
        procs,
        airport,
        engine: Mutex::new(engine),
    })
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
    let mut loaded = Vec::new();
    let mut errors = Vec::new();
    for id in &ids {
        match load_one(data_dir, id, xplane_root, cfg) {
            Ok(s) => loaded.push(s),
            Err(e) => {
                tracing::warn!("station {} skipped: {e:#}", data_dir.join(id).display());
                errors.push(format!("{id}: {e:#}"));
            }
        }
    }
    if loaded.is_empty() {
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
    Ok(loaded)
}
