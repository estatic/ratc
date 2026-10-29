//! Local per-country OpenAIP store and the API key file.

use crate::openaip::{ApiError, Endpoint, OaAirport, OaAirspace, OaReportingPoint, OpenAipApi};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const AIRPORTS: &str = "airports.json";
const AIRSPACES: &str = "airspaces.json";
const POINTS: &str = "reporting-points.json";
const SYNC: &str = "sync.toml";
const KEY_FILE: &str = "openaip.toml";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("{}: {message}", path.display())]
    Decode { path: PathBuf, message: String },
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Everything downloaded for one country.
#[derive(Debug, Clone, PartialEq)]
pub struct CountryData {
    pub airports: Vec<OaAirport>,
    pub airspaces: Vec<OaAirspace>,
    pub points: Vec<OaReportingPoint>,
    /// ISO UTC time of the download.
    pub synced_at: String,
}

/// Number of records changed upstream since the last sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeCount {
    pub airports: u64,
    pub airspaces: u64,
    pub points: u64,
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `<data dir>/openaip`, or `None` when there is no home directory.
    pub fn default_location() -> Option<Self> {
        let dirs = directories::ProjectDirs::from("", "", "vatc")?;
        Some(Self::at(dirs.data_dir().join("openaip")))
    }

    fn dir(&self, cc: &str) -> PathBuf {
        self.root.join(cc.to_uppercase())
    }

    /// Downloads all three endpoints and replaces the stored files. Nothing is
    /// written unless every request succeeded, nor when all three lists are empty (an unknown
    /// country code, which would otherwise be re-checked forever).
    pub fn fetch_country(
        &self,
        api: &dyn OpenAipApi,
        cc: &str,
        now_utc: &str,
    ) -> Result<CountryData, StoreError> {
        let cc = cc.to_uppercase();
        let airports = api.airports(&cc, None)?;
        let airspaces = api.airspaces(&cc, None)?;
        let points = api.reporting_points(&cc, None)?;
        let data = CountryData {
            airports,
            airspaces,
            points,
            synced_at: now_utc.to_string(),
        };
        if data.airports.is_empty() && data.airspaces.is_empty() && data.points.is_empty() {
            return Ok(data);
        }

        let dir = self.dir(&cc);
        std::fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        let sync = format!("synced_at = {}\n", toml_string(now_utc));
        let files: [(&str, Vec<u8>); 4] = [
            (AIRPORTS, json_bytes(&data.airports, &dir.join(AIRPORTS))?),
            (
                AIRSPACES,
                json_bytes(&data.airspaces, &dir.join(AIRSPACES))?,
            ),
            (POINTS, json_bytes(&data.points, &dir.join(POINTS))?),
            (SYNC, sync.into_bytes()),
        ];
        let tmp_of = |name: &str| dir.join(format!("{name}.tmp"));
        let written = (|| {
            for (name, bytes) in &files {
                let tmp = tmp_of(name);
                std::fs::write(&tmp, bytes).map_err(io_err(&tmp))?;
            }
            Ok::<(), StoreError>(())
        })();
        if let Err(e) = written {
            for (name, _) in &files {
                let _ = std::fs::remove_file(tmp_of(name));
            }
            return Err(e);
        }
        for (name, _) in &files {
            let tmp = tmp_of(name);
            let dst = dir.join(name);
            std::fs::rename(&tmp, &dst).map_err(io_err(&dst))?;
        }
        Ok(data)
    }

    /// `None` when the country has never been downloaded.
    pub fn load_country(&self, cc: &str) -> Result<Option<CountryData>, StoreError> {
        let dir = self.dir(cc);
        let sync_path = dir.join(SYNC);
        let sync_text = match std::fs::read_to_string(&sync_path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_err(&sync_path)(e)),
        };
        let table: toml::Table =
            sync_text
                .parse()
                .map_err(|e: toml::de::Error| StoreError::Decode {
                    path: sync_path.clone(),
                    message: e.to_string(),
                })?;
        let synced_at = table
            .get("synced_at")
            .and_then(|v| v.as_str())
            .ok_or_else(|| StoreError::Decode {
                path: sync_path.clone(),
                message: "missing synced_at".into(),
            })?
            .to_string();
        Ok(Some(CountryData {
            airports: read_json(&dir.join(AIRPORTS))?,
            airspaces: read_json(&dir.join(AIRSPACES))?,
            points: read_json(&dir.join(POINTS))?,
            synced_at,
        }))
    }

    /// Upper-case codes of the downloaded countries, sorted.
    pub fn countries(&self) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut out: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().join(SYNC).is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        out.sort();
        out
    }

    /// Upstream changes since the last sync; `None` when the country was never
    /// downloaded.
    pub fn changes(
        &self,
        api: &dyn OpenAipApi,
        cc: &str,
    ) -> Result<Option<ChangeCount>, StoreError> {
        let Some(data) = self.load_country(cc)? else {
            return Ok(None);
        };
        let cc = cc.to_uppercase();
        let since = data.synced_at.as_str();
        Ok(Some(ChangeCount {
            airports: api.changed_count(Endpoint::Airports, &cc, since)?,
            airspaces: api.changed_count(Endpoint::Airspaces, &cc, since)?,
            points: api.changed_count(Endpoint::ReportingPoints, &cc, since)?,
        }))
    }
}

fn toml_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn json_bytes<T: Serialize>(items: &[T], path: &Path) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec(items).map_err(|e| StoreError::Decode {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>, StoreError> {
    let bytes = std::fs::read(path).map_err(io_err(path))?;
    serde_json::from_slice(&bytes).map_err(|e| StoreError::Decode {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn config_dir() -> Option<PathBuf> {
    Some(
        directories::ProjectDirs::from("", "", "vatc")?
            .config_dir()
            .to_path_buf(),
    )
}

/// The saved OpenAIP API key, if any.
pub fn load_key() -> Option<String> {
    load_key_from(&config_dir()?)
}

/// Saves the key to the config directory; returns the file path.
pub fn save_key(key: &str) -> io::Result<PathBuf> {
    let dir = config_dir().ok_or_else(|| io::Error::other("no home directory"))?;
    save_key_in(&dir, key)
}

pub fn load_key_from(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(KEY_FILE)).ok()?;
    let table: toml::Table = text.parse().ok()?;
    let key = table.get("key")?.as_str()?.trim();
    (!key.is_empty()).then(|| key.to_string())
}

pub fn save_key_in(dir: &Path, key: &str) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(KEY_FILE);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(format!("key = {}\n", toml_string(key.trim())).as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openaip::{ApiError, Endpoint, FakeApi, OaAirport, OaAirspace, OaReportingPoint};
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d =
            std::env::temp_dir().join(format!("vatc-store-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn airport(id: &str) -> OaAirport {
        serde_json::from_str(&format!(
            r#"{{"_id":"{id}","name":"TESTFIELD","icaoCode":"ZZTT","type":2,"country":"ZZ",
            "geometry":{{"type":"Point","coordinates":[19.5,50.25]}},
            "elevation":{{"value":100,"unit":0}},"magneticDeclination":5.5,
            "frequencies":[{{"value":"121.980","type":14,"name":"TWR","primary":true}}],
            "runways":[{{"designator":"07","trueHeading":72}}],
            "updatedAt":"2026-01-01T00:00:00.000Z"}}"#
        ))
        .unwrap()
    }

    fn airspace(id: &str) -> OaAirspace {
        serde_json::from_str(&format!(
            r#"{{"_id":"{id}","name":"TESTFIELD CTR","type":4,"icaoClass":3,
            "lowerLimit":{{"value":0,"unit":1,"referenceDatum":0}},
            "upperLimit":{{"value":2500,"unit":1,"referenceDatum":1}},
            "geometry":{{"type":"Polygon","coordinates":[[[19.0,50.0],[20.0,50.0],[20.0,51.0],[19.0,50.0]]]}},
            "country":"ZZ"}}"#
        ))
        .unwrap()
    }

    fn point(id: &str) -> OaReportingPoint {
        serde_json::from_str(&format!(
            r#"{{"_id":"{id}","name":"NOVEMBER","compulsory":true,
            "geometry":{{"type":"Point","coordinates":[19.6,50.3]}},"airports":["a1"]}}"#
        ))
        .unwrap()
    }

    fn seeded() -> FakeApi {
        let api = FakeApi::new();
        api.set_airports("ZZ", vec![airport("a1")]);
        api.set_airspaces("ZZ", vec![airspace("s1")]);
        api.set_reporting_points("ZZ", vec![point("p1")]);
        api
    }

    #[test]
    fn fetch_then_load_round_trips() {
        let root = tmp("rt");
        let store = Store::at(&root);
        assert_eq!(store.load_country("zz").unwrap(), None);
        let got = store
            .fetch_country(&seeded(), "zz", "2026-02-02T10:00:00Z")
            .unwrap();
        assert_eq!(got.airports, vec![airport("a1")]);
        assert_eq!(got.synced_at, "2026-02-02T10:00:00Z");
        assert!(root.join("ZZ/airports.json").is_file());
        assert!(root.join("ZZ/airspaces.json").is_file());
        assert!(root.join("ZZ/reporting-points.json").is_file());
        assert!(root.join("ZZ/sync.toml").is_file());
        let loaded = store.load_country("ZZ").unwrap().unwrap();
        assert_eq!(loaded, got);
        assert_eq!(loaded.airspaces, vec![airspace("s1")]);
        assert_eq!(loaded.points, vec![point("p1")]);
        assert_eq!(store.countries(), vec!["ZZ".to_string()]);
        let leftovers = std::fs::read_dir(root.join("ZZ"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn failed_fetch_keeps_previous_store() {
        let root = tmp("fail");
        let store = Store::at(&root);
        let api = seeded();
        let first = store
            .fetch_country(&api, "ZZ", "2026-02-02T10:00:00Z")
            .unwrap();
        let before = std::fs::read(root.join("ZZ/airports.json")).unwrap();
        api.set_airports("ZZ", vec![airport("a2")]);
        api.fail(Endpoint::Airspaces, ApiError::Status(500));
        let err = store
            .fetch_country(&api, "ZZ", "2026-03-03T10:00:00Z")
            .unwrap_err();
        assert!(matches!(err, StoreError::Api(ApiError::Status(500))));
        assert_eq!(
            std::fs::read(root.join("ZZ/airports.json")).unwrap(),
            before
        );
        let loaded = store.load_country("ZZ").unwrap().unwrap();
        assert_eq!(loaded, first);
        assert_eq!(loaded.synced_at, "2026-02-02T10:00:00Z");
    }

    #[test]
    fn changes_uses_synced_at() {
        let root = tmp("chg");
        let store = Store::at(&root);
        let api = seeded();
        assert_eq!(store.changes(&api, "ZZ").unwrap(), None);
        store
            .fetch_country(&api, "ZZ", "2026-02-02T10:00:00Z")
            .unwrap();
        api.clear_failures();
        let n = store.changes(&api, "ZZ").unwrap().unwrap();
        assert_eq!((n.airports, n.airspaces, n.points), (1, 1, 1));
        let calls = api.calls();
        let last3 = &calls[calls.len() - 3..];
        assert!(
            last3
                .iter()
                .all(|c| c.updated_after.as_deref() == Some("2026-02-02T10:00:00Z"))
        );
    }

    #[test]
    fn key_file_round_trip_and_mode() {
        let dir = tmp("key");
        assert_eq!(load_key_from(&dir), None);
        let path = save_key_in(&dir, "  secret-key  ").unwrap();
        assert_eq!(path, dir.join("openaip.toml"));
        assert_eq!(load_key_from(&dir).as_deref(), Some("secret-key"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode =
                |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            save_key_in(&dir, "other\"key").unwrap();
            assert_eq!(mode(&path), 0o600);
            assert_eq!(load_key_from(&dir).as_deref(), Some("other\"key"));
        }
    }

    #[test]
    fn empty_country_is_not_stored() {
        let root = std::env::temp_dir().join(format!(
            "vatc-store-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = Store::at(&root);
        let d = store
            .fetch_country(&FakeApi::new(), "QQ", "2026-02-02T10:00:00Z")
            .unwrap();
        assert!(d.airports.is_empty());
        assert!(store.countries().is_empty());
        assert!(store.load_country("QQ").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
