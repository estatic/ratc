//! User overrides (`overrides.toml`) over a generated station, and detection of overrides
//! whose subject has vanished from the generated data.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use atc_navdata::procedures::{AdjacentUnit, PositionDef, Procedures, VfrPoint};
use atc_navdata::station::StationError;

const FILE: &str = "overrides.toml";

/// Top-level `Procedures` fields the user replaced, with a note on where each value came from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overrides {
    /// Overridden fields, without the reserved `[sources]` table.
    pub table: toml::Table,
    /// Field name to where the override came from.
    pub sources: BTreeMap<String, String>,
}

fn io_err(path: &Path, e: &std::io::Error) -> StationError {
    StationError::Io {
        file: path.display().to_string(),
        reason: e.to_string(),
    }
}

impl Overrides {
    /// Reads `overrides.toml` in `dir`; an absent file is empty overrides.
    pub fn load(dir: &Path) -> Result<Overrides, StationError> {
        let path = dir.join(FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Overrides::default()),
            Err(e) => return Err(io_err(&path, &e)),
        };
        let toml_err = |reason: String, span: Option<std::ops::Range<usize>>| StationError::Toml {
            file: path.display().to_string(),
            line: span.map(|s| text[..s.start].matches('\n').count() + 1),
            reason,
        };
        let mut table: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| toml_err(e.message().to_string(), e.span()))?;
        let mut sources = BTreeMap::new();
        match table.remove("sources") {
            None => {}
            Some(toml::Value::Table(t)) => {
                for (k, v) in t {
                    let toml::Value::String(v) = v else {
                        return Err(toml_err(format!("source of `{k}` must be a string"), None));
                    };
                    sources.insert(k, v);
                }
            }
            Some(_) => return Err(toml_err("`sources` must be a table".into(), None)),
        }
        Ok(Overrides { table, sources })
    }

    /// Writes `overrides.toml` atomically: keys sorted, `[sources]` last.
    pub fn save(&self, dir: &Path) -> Result<(), StationError> {
        let path = dir.join(FILE);
        let bad = |e: toml::ser::Error| StationError::Toml {
            file: path.display().to_string(),
            line: None,
            reason: e.to_string(),
        };
        // `toml::Table` is ordered by key; its serializer writes plain values before tables.
        let mut text = toml::to_string(&self.table).map_err(bad)?;
        if !self.sources.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            let sources: toml::Table = [(
                "sources".to_string(),
                toml::Value::Table(
                    self.sources
                        .iter()
                        .map(|(k, v)| (k.clone(), toml::Value::String(v.clone())))
                        .collect(),
                ),
            )]
            .into_iter()
            .collect();
            text.push_str(&toml::to_string(&sources).map_err(bad)?);
        }
        let tmp = dir.join(format!("{FILE}.tmp"));
        std::fs::create_dir_all(dir).map_err(|e| io_err(dir, &e))?;
        std::fs::write(&tmp, text).map_err(|e| io_err(&tmp, &e))?;
        std::fs::rename(&tmp, &path).map_err(|e| io_err(&path, &e))
    }

    /// Replaces the top-level field `key` with `value`, recording `source`.
    pub fn set(&mut self, key: &str, value: toml::Value, source: &str) {
        self.table.insert(key.to_string(), value);
        self.sources.insert(key.to_string(), source.to_string());
    }

    /// Drops the override of `key` together with its source.
    pub fn remove(&mut self, key: &str) {
        self.table.remove(key);
        self.sources.remove(key);
    }

    /// The override of `key` as a list of `T`; `None` when absent or malformed.
    fn list<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<Vec<T>> {
        self.table.get(key)?.clone().try_into().ok()
    }
}

fn mhz(khz: u32) -> String {
    format!("{}.{:03}", khz / 1000, khz % 1000)
}

/// Overrides that name a VFR point, hold, position or adjacent unit absent from `generated`.
pub fn orphans(generated: &Procedures, ov: &Overrides) -> Vec<String> {
    const GONE: &str = "is not in OpenAIP any more";
    let mut out = BTreeSet::new();
    let own_points: Vec<VfrPoint> = ov.list("vfr_points").unwrap_or_default();
    for p in &own_points {
        if generated.vfr_point(&p.name).is_none() {
            out.insert(format!("vfr_points: {} {GONE}", p.name));
        }
    }
    for name in ov.list::<String>("vfr_holds").unwrap_or_default() {
        let known = generated.vfr_point(&name).is_some()
            || generated
                .vfr_holds
                .iter()
                .any(|h| h.eq_ignore_ascii_case(&name))
            || own_points
                .iter()
                .any(|p| p.name.eq_ignore_ascii_case(&name));
        if !known {
            out.insert(format!("vfr_holds: {name} {GONE}"));
        }
    }
    for p in ov.list::<PositionDef>("positions").unwrap_or_default() {
        if !generated.positions.iter().any(|g| g.khz == p.khz) {
            out.insert(format!("positions: {} ({}) {GONE}", mhz(p.khz), p.callsign));
        }
    }
    for a in ov.list::<AdjacentUnit>("adjacent").unwrap_or_default() {
        if !generated
            .adjacent
            .iter()
            .any(|g| g.callsign.eq_ignore_ascii_case(&a.callsign))
        {
            out.insert(format!("adjacent: {} {GONE}", a.callsign));
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("vatc-ov-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn generated() -> Procedures {
        toml::from_str(
            r#"
icao = "ZZTT"
source = "test"
airac = "2601"
transition_altitude_ft = 6000
preferred_runway = "07"
circuits = []
vfr_holds = ["ALFA"]

[[positions]]
role = "Tower"
callsign = "Testfield Tower"
khz = 118100

[[vfr_points]]
name = "ALFA"
lat = 50.0
lon = 19.0

[[adjacent]]
callsign = "Testfield Info"
khz = 119000
kind = "Fis"
"#,
        )
        .unwrap()
    }

    fn val(text: &str) -> toml::Value {
        let t: toml::Table = format!("v = {text}").parse().unwrap();
        t["v"].clone()
    }

    #[test]
    fn save_load_round_trip_keeps_sources() {
        let dir = tmp("rt");
        let mut ov = Overrides::default();
        ov.set("preferred_runway", val("\"25\""), "user");
        ov.set(
            "tma",
            val("{ radius_nm = 12.5, ceiling_ft = 4500 }"),
            "wizard",
        );
        ov.save(&dir).unwrap();
        let text = std::fs::read_to_string(dir.join("overrides.toml")).unwrap();
        assert!(
            text.rfind("[sources]").unwrap() > text.rfind("[tma]").unwrap(),
            "{text}"
        );
        assert!(text.find("preferred_runway").unwrap() < text.find("[tma]").unwrap());
        assert!(!dir.join("overrides.toml.tmp").exists());
        let back = Overrides::load(&dir).unwrap();
        assert_eq!(back, ov);
        assert_eq!(back.sources["tma"], "wizard");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_absent_is_empty_and_bad_file_names_file() {
        let dir = tmp("bad");
        assert_eq!(Overrides::load(&dir).unwrap(), Overrides::default());
        std::fs::write(dir.join("overrides.toml"), "a = [1\n").unwrap();
        let e = Overrides::load(&dir).unwrap_err().to_string();
        assert!(e.contains("overrides.toml"), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn set_then_merge_wins() {
        let mut ov = Overrides::default();
        ov.set("preferred_runway", val("\"25\""), "user");
        ov.set("preferred_runway", val("\"07\""), "user");
        ov.set("transition_altitude_ft", val("5000"), "user");
        let base: toml::Table = toml::to_string(&generated()).unwrap().parse().unwrap();
        let merged = atc_navdata::station::merge_overrides(base, &ov.table);
        let p: Procedures = merged.try_into().unwrap();
        assert_eq!(p.preferred_runway, "07");
        assert_eq!(p.transition_altitude_ft, 5000);
        ov.remove("preferred_runway");
        assert!(!ov.table.contains_key("preferred_runway"));
        assert!(!ov.sources.contains_key("preferred_runway"));
    }

    #[test]
    fn orphaned_vfr_point_reported() {
        let mut ov = Overrides::default();
        ov.set(
            "vfr_points",
            val("[{ name = \"ALFA\", lat = 50.0, lon = 19.0 }, { name = \"ECHO\", lat = 50.1, lon = 19.1 }]"),
            "user",
        );
        ov.set("vfr_holds", val("[\"ALFA\", \"ECHO\", \"GONE\"]"), "user");
        assert_eq!(
            orphans(&generated(), &ov),
            vec![
                "vfr_holds: GONE is not in OpenAIP any more".to_string(),
                "vfr_points: ECHO is not in OpenAIP any more".to_string(),
            ]
        );
    }

    #[test]
    fn orphaned_position_reported() {
        let mut ov = Overrides::default();
        ov.set(
            "positions",
            val("[{ role = \"Tower\", callsign = \"Testfield Tower\", khz = 118100 }, { role = \"Tower\", callsign = \"Old Tower\", khz = 118200 }]"),
            "user",
        );
        ov.set(
            "adjacent",
            val("[{ callsign = \"Testfield Info\", khz = 119000, kind = \"Fis\" }, { callsign = \"Gone Radar\", khz = 125000, kind = \"Acc\" }]"),
            "user",
        );
        assert_eq!(
            orphans(&generated(), &ov),
            vec![
                "adjacent: Gone Radar is not in OpenAIP any more".to_string(),
                "positions: 118.200 (Old Tower) is not in OpenAIP any more".to_string(),
            ]
        );
    }

    #[test]
    fn no_orphans_for_plain_overrides() {
        let mut ov = Overrides::default();
        ov.set("preferred_runway", val("\"25\""), "user");
        assert!(orphans(&generated(), &ov).is_empty());
    }
}
