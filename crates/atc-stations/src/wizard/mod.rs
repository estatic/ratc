//! The interactive airport wizard: creates, updates, edits and removes stations from OpenAIP
//! data and the X-Plane layout, over any `BufRead`/`Write` pair.
//!
//! A station lives in `<data_dir>/<ID>/`: the generated `station.toml`, the user's
//! `overrides.toml`, `wizard.toml` (which OpenAIP airfield and country it was made from) and,
//! unless one is already there, `apt.dat` with the airport's X-Plane block so the station also
//! runs without `xplane_root`. `<ID>` is the OpenAIP ICAO code when X-Plane knows it, else the
//! ident of the nearest X-Plane airport within 300 m of the OpenAIP position.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use atc_geo::LatLon;
use serde::{Deserialize, Serialize};

pub use crate::prompt::Prompt;

use crate::openaip::{ApiError, Endpoint, OaAirport, OpenAipApi};
use crate::store::{CountryData, Store, StoreError, load_key_from, save_key_in};

mod create;
mod update;
mod values;

use create::create;
use update::{startup_check, update_all};
use values::{edit, remove};

const MENU: [&str; 6] = [
    "Create stations in an area",
    "Update data",
    "Edit a station",
    "Remove a station",
    "Set the OpenAIP API key",
    "Quit",
];
const NO_KEY: &str = "Set the OpenAIP API key first (menu 5).";
const STATION: &str = "station.toml";
const OVERRIDES: &str = "overrides.toml";
const META: &str = "wizard.toml";
const APT_DAT: &str = "apt.dat";
const PROCEDURES: &str = "procedures.toml";
/// Country queried to check a new key: any valid code works, the answer is only auth.
const KEY_CHECK_COUNTRY: &str = "PL";

/// Where the wizard reads and writes.
pub struct WizardEnv {
    /// The server's station directory (`data/airports`).
    pub data_dir: PathBuf,
    pub xplane_root: Option<PathBuf>,
    pub store: Store,
    /// Directory of the key file (`openaip.toml`).
    pub key_dir: PathBuf,
    /// Current UTC time, ISO 8601; injected so tests are deterministic.
    pub now_utc: fn() -> String,
}

impl WizardEnv {
    /// An environment on the system clock.
    pub fn new(
        data_dir: PathBuf,
        xplane_root: Option<PathBuf>,
        store: Store,
        key_dir: PathBuf,
    ) -> Self {
        Self {
            data_dir,
            xplane_root,
            store,
            key_dir,
            now_utc: now_utc_iso,
        }
    }

    fn now(&self) -> String {
        (self.now_utc)()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WizardError {
    #[error("console output failed: {0}")]
    Output(#[from] std::io::Error),
}

/// What the wizard remembers about a station it made.
#[derive(Debug, Serialize, Deserialize)]
struct Meta {
    openaip_id: String,
    country: String,
    positions_khz: Vec<u32>,
    /// Every position OpenAIP offered when the station was last written; `None` in files
    /// from before it was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offered_khz: Option<Vec<u32>>,
}

/// Runs the wizard until Quit or end of input. Problems with data, files or OpenAIP are
/// reported on the console and the menu continues; only a failing console is an error.
pub fn run<R: BufRead, W: Write>(
    env: &WizardEnv,
    api_factory: impl Fn(&str) -> Box<dyn OpenAipApi>,
    prompt: &mut Prompt<R, W>,
) -> Result<(), WizardError> {
    let mut api = load_key_from(&env.key_dir).map(|k| api_factory(&k));
    if let Some(a) = api.as_deref() {
        startup_check(env, a, prompt);
    }
    while !prompt.eof() {
        match prompt.menu("Airport wizard", &MENU) {
            0 | 6 => break,
            5 => {
                if let Some(new) = set_key(env, &api_factory, prompt) {
                    api = Some(new);
                }
            }
            n => match api.as_deref() {
                None => prompt.say(NO_KEY),
                Some(a) => match n {
                    1 => create(env, a, prompt),
                    2 => update_all(env, a, prompt),
                    3 => edit(env, prompt),
                    _ => remove(env, prompt),
                },
            },
        }
    }
    match prompt.take_write_error() {
        Some(e) => Err(e.into()),
        None => Ok(()),
    }
}

// ---------------------------------------------------------------- key

fn set_key<R: BufRead, W: Write>(
    env: &WizardEnv,
    factory: &impl Fn(&str) -> Box<dyn OpenAipApi>,
    p: &mut Prompt<R, W>,
) -> Option<Box<dyn OpenAipApi>> {
    loop {
        let key = p.ask("OpenAIP API key (Enter = cancel)", None);
        if key.is_empty() {
            return None;
        }
        let api = factory(&key);
        match api.changed_count(Endpoint::Airports, KEY_CHECK_COUNTRY, &env.now()) {
            Ok(_) => {}
            Err(ApiError::Auth) => {
                p.say(ApiError::Auth.to_string());
                if p.confirm("Try another key?", true) {
                    continue;
                }
                return None;
            }
            Err(e) => {
                p.say(format!("Could not check the key: {e}"));
                if !p.confirm("Save it anyway?", false) {
                    return None;
                }
            }
        }
        return match save_key_in(&env.key_dir, &key) {
            Ok(path) => {
                p.say(format!("Key saved to {}.", path.display()));
                Some(api)
            }
            Err(e) => {
                p.say(format!("Could not save the key: {e}"));
                None
            }
        };
    }
}

// ---------------------------------------------------------------- helpers

struct StationDir {
    id: String,
    dir: PathBuf,
    meta: Option<Meta>,
}

/// Subdirectories of the data dir, sorted by name.
fn stations(data_dir: &Path) -> Vec<StationDir> {
    let Ok(rd) = std::fs::read_dir(data_dir) else {
        return Vec::new();
    };
    let mut out: Vec<StationDir> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let id = e.file_name().into_string().ok()?;
            let dir = e.path();
            let meta = std::fs::read_to_string(dir.join(META))
                .ok()
                .and_then(|t| toml::from_str(&t).ok());
            Some(StationDir { id, dir, meta })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn write_meta(dir: &Path, meta: &Meta) -> Result<(), String> {
    let text = toml::to_string(meta).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(META), text).map_err(|e| e.to_string())
}

/// Downloads `cc` into the store, printing progress per page and the unreadable records left
/// out.
fn download_country<R: BufRead, W: Write>(
    env: &WizardEnv,
    api: &dyn OpenAipApi,
    cc: &str,
    p: &mut Prompt<R, W>,
) -> Result<CountryData, StoreError> {
    let cc = cc.to_uppercase();
    let fetched = env
        .store
        .fetch_country_with(api, &cc, &env.now(), &mut |e, n, of| {
            p.say(format!("{cc} {}: page {n}/{of}", e.path()));
        })?;
    let s = fetched.skipped;
    for (e, n) in [
        (Endpoint::Airports, s.airports),
        (Endpoint::Airspaces, s.airspaces),
        (Endpoint::ReportingPoints, s.points),
    ] {
        if n > 0 {
            let records = if n == 1 { "record" } else { "records" };
            p.say(format!(
                "{cc} {}: {n} {records} skipped (unreadable)",
                e.path()
            ));
        }
    }
    Ok(fetched.data)
}

/// Every downloaded country in one data set, so neighbours' airspaces and towers count too.
fn combined(store: &Store) -> CountryData {
    let mut all = CountryData {
        airports: Vec::new(),
        airspaces: Vec::new(),
        points: Vec::new(),
        synced_at: String::new(),
    };
    for cc in store.countries() {
        if let Ok(Some(d)) = store.load_country(&cc) {
            all.airports.extend(d.airports);
            all.airspaces.extend(d.airspaces);
            all.points.extend(d.points);
        }
    }
    all
}

/// The date the country was downloaded, else today.
fn fetched_on(store: &Store, cc: &str, now: &str) -> String {
    match store.load_country(cc) {
        Ok(Some(d)) => date_of(&d.synced_at),
        _ => date_of(now),
    }
}

fn date_of(iso: &str) -> String {
    iso.chars().take(10).collect()
}

fn lat_lon(ap: &OaAirport) -> LatLon {
    let [lat, lon] = ap.geometry.lat_lon();
    LatLon { lat, lon }
}

fn mhz(khz: u32) -> String {
    format!("{}.{:03}", khz / 1000, khz % 1000)
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_utc_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    iso_from_unix(secs)
}

fn iso_from_unix(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to (year, month, day); H. Hinnant's `civil_from_days`.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_time_from_unix_seconds() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(1_761_696_000), "2025-10-29T00:00:00Z");
        assert_eq!(iso_from_unix(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_from_unix(4_107_542_399), "2100-02-28T23:59:59Z");
    }
}
