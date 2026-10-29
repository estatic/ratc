//! Loading an airport's `Procedures` from a generated `station.toml` plus user `overrides.toml`.

use std::path::Path;

use crate::procedures::Procedures;

/// Failure to load an airport's procedures.
#[derive(Debug, thiserror::Error)]
pub enum StationError {
    #[error("cannot read {file}: {reason}")]
    Io { file: String, reason: String },
    #[error("{file}: line {line}: {reason}")]
    Toml {
        file: String,
        line: usize,
        reason: String,
    },
}

/// Merges `overrides` over `base`: tables merge recursively, arrays and scalars are replaced
/// whole. The reserved `[sources]` table of the overrides is not merged.
pub fn merge_overrides(mut base: toml::Table, overrides: &toml::Table) -> toml::Table {
    merge_into(&mut base, overrides, true);
    base
}

fn merge_into(base: &mut toml::Table, overrides: &toml::Table, top: bool) {
    for (key, value) in overrides {
        if top && key == "sources" {
            continue;
        }
        match (base.get_mut(key), value) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge_into(b, o, false),
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

fn read(path: &Path) -> Result<Option<String>, StationError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StationError::Io {
            file: path.display().to_string(),
            reason: e.to_string(),
        }),
    }
}

fn toml_error(path: &Path, text: &str, e: &toml::de::Error) -> StationError {
    StationError::Toml {
        file: path.display().to_string(),
        line: e
            .span()
            .map_or(0, |s| text[..s.start].matches('\n').count() + 1),
        reason: e.message().to_string(),
    }
}

/// Loads `station.toml` merged with `overrides.toml` (absent = empty) when `station.toml`
/// exists, else `procedures.toml` alone.
pub fn load_station(dir: &Path) -> Result<Procedures, StationError> {
    let station_path = dir.join("station.toml");
    let Some(station_text) = read(&station_path)? else {
        let path = dir.join("procedures.toml");
        let text = read(&path)?.ok_or_else(|| StationError::Io {
            file: path.display().to_string(),
            reason: "file not found".into(),
        })?;
        return toml::from_str(&text).map_err(|e| toml_error(&path, &text, &e));
    };
    let base: toml::Table = station_text
        .parse()
        .map_err(|e| toml_error(&station_path, &station_text, &e))?;
    let over_path = dir.join("overrides.toml");
    let merged = match read(&over_path)? {
        Some(text) => {
            let over: toml::Table = text
                .parse()
                .map_err(|e| toml_error(&over_path, &text, &e))?;
            merge_overrides(base, &over)
        }
        None => base,
    };
    // The merged table has no source text; line numbers are only known for the files above.
    merged
        .try_into()
        .map_err(|e: toml::de::Error| StationError::Toml {
            file: station_path.display().to_string(),
            line: 0,
            reason: e.message().to_string(),
        })
}
