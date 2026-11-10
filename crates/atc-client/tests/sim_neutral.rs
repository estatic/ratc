//! Guard: the client core stays simulator-neutral. Simulator names belong to the adapters
//! under `src/sim/<name>/` and to the entry point and config (`src/main.rs`, `src/config.rs`).
//! A deliberate exception carries the `sim-neutral:` marker on its line.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: [&str; 5] = ["xplane", "x-plane", "msfs", "simconnect", "prepar3d"];

fn rs_files(dir: &Path, skip: &[PathBuf], out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if skip.contains(&path) {
            continue;
        }
        if path.is_dir() {
            rs_files(&path, skip, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn client_core_names_no_simulator() {
    let src = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
    let skip = [
        src.join("sim").join("xplane"),
        src.join("main.rs"),
        src.join("config.rs"),
    ];
    let mut files = Vec::new();
    rs_files(src, &skip, &mut files);
    let mut hits = Vec::new();
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        for (i, line) in text.lines().enumerate() {
            if line.trim() == "#[cfg(test)]" {
                break;
            }
            let lower = line.to_lowercase();
            if FORBIDDEN.iter().any(|w| lower.contains(w)) && !lower.contains("sim-neutral:") {
                hits.push(format!("{}:{}: {}", path.display(), i + 1, line));
            }
        }
    }
    assert!(hits.is_empty(), "{}", hits.join("\n"));
}
