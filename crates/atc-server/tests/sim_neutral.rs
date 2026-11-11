//! Guard: the server and the engine stay simulator-neutral. Simulator names belong to the
//! adapters (`atc-xplane-plugin`, `atc-navdata`), not to `atc-server` or `atc-core`.
//! A deliberate exception carries the `sim-neutral:` marker on its line.

use std::fs;
use std::path::Path;

const FORBIDDEN: [&str; 5] = ["xplane", "x-plane", "msfs", "simconnect", "prepar3d"];

fn rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn server_and_engine_name_no_simulator() {
    let mut files = Vec::new();
    rs_files(
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")),
        &mut files,
    );
    rs_files(
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../atc-core/src")),
        &mut files,
    );
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
