//! The engine's dynamic state on disk ([`EngineSnapshot`]), so a server restart keeps the
//! flights in progress. One JSON file per airport under `data/cache/` (git-ignored), written
//! atomically (temporary file, then rename) while the server runs and once more on shutdown.
//! A snapshot older than [`MAX_AGE`], of another format or airport, or unreadable is not
//! restored: the server starts afresh and logs why.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use atc_core::engine::{AtcEngine, EngineSnapshot};

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

/// What [`load`] found.
#[derive(Debug)]
pub enum Loaded {
    Snapshot(Box<EngineSnapshot>),
    /// Nothing usable; the reason, for the log.
    Skipped(String),
}

/// The snapshot at `path` when it was written at most `max_age` before `now`.
pub fn load(path: &Path, max_age: Duration, now: SystemTime) -> Loaded {
    let skipped = |why: String| Loaded::Skipped(format!("{}: {why}", path.display()));
    let modified = match std::fs::metadata(path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return skipped("no saved state".into());
        }
        Err(e) => return skipped(e.to_string()),
    };
    let age = now.duration_since(modified).unwrap_or_default();
    if age > max_age {
        return skipped(format!(
            "saved {} min ago, older than {} min",
            age.as_secs() / 60,
            max_age.as_secs() / 60
        ));
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => return skipped(e.to_string()),
    };
    match serde_json::from_str(&text) {
        Ok(s) => Loaded::Snapshot(Box::new(s)),
        Err(e) => skipped(format!("unreadable: {e}")),
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

/// Saves the engine's snapshot to `path` when it differs from `last` (the JSON last written,
/// updated here). Errors are logged, not returned: a failed save must not stop the server.
pub fn save(engine: &Mutex<AtcEngine>, path: &Path, last: &mut String) {
    let snapshot = engine.lock().unwrap().snapshot();
    let json = match serde_json::to_string(&snapshot) {
        Ok(j) => j,
        Err(e) => {
            tracing::error!("state snapshot not serialisable: {e}");
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

/// Saves every [`SAVE_PERIOD`] until the task is dropped.
pub async fn save_loop(engine: Arc<Mutex<AtcEngine>>, path: PathBuf) {
    let mut last = String::new();
    let mut tick = tokio::time::interval(SAVE_PERIOD);
    loop {
        tick.tick().await;
        let (engine, path) = (engine.clone(), path.clone());
        let mut prev = std::mem::take(&mut last);
        last = tokio::task::spawn_blocking(move || {
            save(&engine, &path, &mut prev);
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
