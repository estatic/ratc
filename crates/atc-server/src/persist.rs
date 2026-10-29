//! The engines' dynamic state on disk ([`EngineSnapshot`]), so a server restart keeps the
//! flights in progress. One JSON file per station under `data/cache/` (git-ignored), plus
//! `network.json` with who works each flight and the en-route units' flights
//! ([`NetworkState`]); all written atomically (temporary file, then rename) in the same tick
//! while the server runs and once more on shutdown. A file older than [`MAX_AGE`], of another
//! format or airport, or unreadable is not restored: the server starts afresh and logs why.
//! Owners naming a station or unit that is no longer there are ignored.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use atc_core::engine::EngineSnapshot;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::network::{Network, NetworkState};

/// Directory of the snapshot files, relative to the working directory (the repo root).
pub const STATE_DIR: &str = "data/cache";
/// A snapshot older than this is stale: the flights it holds are long over.
pub const MAX_AGE: Duration = Duration::from_secs(2 * 3600);
/// How often the running server writes the snapshot.
pub const SAVE_PERIOD: Duration = Duration::from_secs(2);

/// `data/cache/state-EPKK.json` for `EPKK`.
pub fn state_path(airport: &str) -> PathBuf {
    Path::new(STATE_DIR).join(format!("state-{airport}.json"))
}

/// `data/cache/network.json`: the owners and the units' flights.
pub fn network_path(dir: &Path) -> PathBuf {
    dir.join("network.json")
}

/// What [`load`] found.
#[derive(Debug)]
pub enum Loaded {
    Snapshot(Box<EngineSnapshot>),
    /// Nothing usable; the reason, for the log.
    Skipped(String),
}

/// The JSON at `path` parsed as `T` when it was written at most `max_age` before `now`; else
/// why not, for the log.
fn read_fresh<T: DeserializeOwned>(
    path: &Path,
    max_age: Duration,
    now: SystemTime,
) -> Result<T, String> {
    let skipped = |why: String| format!("{}: {why}", path.display());
    let modified = match std::fs::metadata(path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(skipped("no saved state".into()));
        }
        Err(e) => return Err(skipped(e.to_string())),
    };
    let age = now.duration_since(modified).unwrap_or_default();
    if age > max_age {
        return Err(skipped(format!(
            "saved {} min ago, older than {} min",
            age.as_secs() / 60,
            max_age.as_secs() / 60
        )));
    }
    let text = std::fs::read_to_string(path).map_err(|e| skipped(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| skipped(format!("unreadable: {e}")))
}

/// The snapshot at `path` when it was written at most `max_age` before `now`.
pub fn load(path: &Path, max_age: Duration, now: SystemTime) -> Loaded {
    match read_fresh(path, max_age, now) {
        Ok(s) => Loaded::Snapshot(Box::new(s)),
        Err(why) => Loaded::Skipped(why),
    }
}

/// Writes `json` to `path` atomically.
fn write_atomic(path: &Path, json: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Saves `value` to `path` when it differs from `last` (the JSON last written, updated
/// here). Errors are logged, not returned: a failed save must not stop the server.
pub fn save<T: Serialize>(value: &T, path: &Path, last: &mut String) {
    let json = match serde_json::to_string(value) {
        Ok(j) => j,
        Err(e) => {
            tracing::error!("state {} not serialisable: {e}", path.display());
            return;
        }
    };
    if json == *last {
        return;
    }
    match write_atomic(path, &json) {
        Ok(()) => *last = json,
        Err(e) => tracing::error!("saving state to {}: {e}", path.display()),
    }
}

/// Saves every station's snapshot and the [`NetworkState`] under `dir`; `last` holds the
/// JSON last written per file.
pub fn save_all_in(dir: &Path, network: &Network, last: &mut HashMap<String, String>) {
    let (snapshots, state) = network.persisted();
    for (id, snapshot) in snapshots {
        save(
            &snapshot,
            &dir.join(format!("state-{id}.json")),
            last.entry(id).or_default(),
        );
    }
    save(
        &state,
        &network_path(dir),
        last.entry("network.json".into()).or_default(),
    );
}

/// [`save_all_in`] for [`STATE_DIR`].
pub fn save_all(network: &Network, last: &mut HashMap<String, String>) {
    save_all_in(Path::new(STATE_DIR), network, last);
}

/// Restores every station's flights and the [`NetworkState`] from `dir` unless `fresh`; logs
/// what happened.
pub fn restore_all_in(dir: &Path, network: &Network, fresh: bool, now: SystemTime) {
    if fresh {
        tracing::info!("--fresh: saved state in {} not restored", dir.display());
        return;
    }
    for s in network.stations() {
        let path = dir.join(format!("state-{}.json", s.id));
        match load(&path, MAX_AGE, now) {
            Loaded::Snapshot(snap) => match s.engine.lock().unwrap().restore(*snap) {
                Ok(()) => tracing::info!("flights in progress restored from {}", path.display()),
                Err(e) => tracing::warn!("saved state at {} not restored: {e}", path.display()),
            },
            Loaded::Skipped(why) => tracing::info!("starting afresh ({why})"),
        }
    }
    match read_fresh::<NetworkState>(&network_path(dir), MAX_AGE, now) {
        Ok(state) => network.restore_state(state),
        Err(why) => tracing::info!("no owners restored ({why})"),
    }
}

/// [`restore_all_in`] for [`STATE_DIR`] and the present time.
pub fn restore_all(network: &Network, fresh: bool) {
    restore_all_in(Path::new(STATE_DIR), network, fresh, SystemTime::now());
}

/// Saves every [`SAVE_PERIOD`] until the task is dropped.
pub async fn save_loop(network: Arc<Network>) {
    let mut last = HashMap::new();
    let mut tick = tokio::time::interval(SAVE_PERIOD);
    loop {
        tick.tick().await;
        let network = network.clone();
        let mut prev = std::mem::take(&mut last);
        last = tokio::task::spawn_blocking(move || {
            save_all(&network, &mut prev);
            prev
        })
        .await
        .unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "atc-persist-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn reason(l: Loaded) -> String {
        match l {
            Loaded::Skipped(r) => r,
            Loaded::Snapshot(_) => panic!("expected skipped"),
        }
    }

    #[test]
    fn path_per_airport() {
        assert_eq!(state_path("EPKK"), Path::new("data/cache/state-EPKK.json"));
    }

    #[test]
    fn missing_stale_and_corrupt_files_are_skipped() {
        let d = dir();
        let p = d.join("state-EPKK.json");
        let now = SystemTime::now();
        assert!(reason(load(&p, MAX_AGE, now)).contains("no saved state"));
        write_atomic(&p, "{not json").unwrap();
        assert!(reason(load(&p, MAX_AGE, now)).contains("unreadable"));
        let later = now + MAX_AGE + Duration::from_secs(60);
        assert!(reason(load(&p, MAX_AGE, later)).contains("older than 120 min"));
        assert!(!d.join("state-EPKK.json.tmp").exists());
    }
}
