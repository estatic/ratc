//! OpenAIP REST types and client.

use serde::{Deserialize, Serialize};
use std::time::Duration;

const AUTH_MESSAGE: &str =
    "OpenAIP rejected the API key; get one at https://www.openaip.net (Profile → API clients)";

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ApiError {
    #[error("{}", AUTH_MESSAGE)]
    Auth,
    #[error("network error: {0}")]
    Network(String),
    #[error("OpenAIP answered with HTTP status {0}")]
    Status(u16),
    #[error("could not decode the OpenAIP response: {0}")]
    Decode(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endpoint {
    Airports,
    Airspaces,
    ReportingPoints,
}

impl Endpoint {
    pub fn path(self) -> &'static str {
        match self {
            Endpoint::Airports => "airports",
            Endpoint::Airspaces => "airspaces",
            Endpoint::ReportingPoints => "reporting-points",
        }
    }
}

/// GeoJSON point; coordinates are `[lon, lat]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub coordinates: [f64; 2],
}

impl Point {
    /// `[lat, lon]`.
    pub fn lat_lon(&self) -> [f64; 2] {
        [self.coordinates[1], self.coordinates[0]]
    }
}

/// GeoJSON polygon; rings of `[lon, lat]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    pub coordinates: Vec<Vec<[f64; 2]>>,
}

impl Polygon {
    /// Outer ring as `[lat, lon]` pairs (empty when the polygon has no rings).
    pub fn outer_ring_lat_lon(&self) -> Vec<[f64; 2]> {
        self.coordinates
            .first()
            .map(|r| r.iter().map(|c| [c[1], c[0]]).collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Elevation {
    pub value: f64,
    /// 0 = metres, 1 = feet.
    pub unit: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OaFrequency {
    /// MHz as text, e.g. "121.980".
    pub value: String,
    #[serde(rename = "type")]
    pub kind: u8,
    pub name: Option<String>,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OaRunway {
    pub designator: String,
    pub true_heading: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OaAirport {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    pub icao_code: Option<String>,
    #[serde(rename = "type")]
    pub kind: u8,
    #[serde(default)]
    pub country: String,
    pub geometry: Point,
    pub elevation: Option<Elevation>,
    pub magnetic_declination: Option<f64>,
    #[serde(default)]
    pub frequencies: Vec<OaFrequency>,
    #[serde(default)]
    pub runways: Vec<OaRunway>,
    pub updated_at: String,
}

impl OaAirport {
    pub fn elevation_ft(&self) -> i32 {
        match &self.elevation {
            None => 0,
            Some(e) if e.unit == 1 => e.value.round() as i32,
            Some(e) => (e.value * 3.28084).round() as i32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit {
    pub value: f64,
    pub unit: u8,
    pub reference_datum: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OaAirspace {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: u8,
    pub icao_class: Option<u8>,
    pub lower_limit: Limit,
    pub upper_limit: Limit,
    #[serde(default)]
    pub frequencies: Vec<OaFrequency>,
    pub geometry: Polygon,
    #[serde(default)]
    pub country: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OaReportingPoint {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub compulsory: bool,
    pub geometry: Point,
    #[serde(default)]
    pub airports: Vec<String>,
}

/// `"121.980"` -> `121980` (kHz). `None` when not a decimal number.
pub fn khz(mhz: &str) -> Option<u32> {
    let v: f64 = mhz.trim().parse().ok()?;
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    Some((v * 1000.0).round() as u32)
}

/// Object-safe OpenAIP access.
pub trait OpenAipApi {
    fn airports(
        &self,
        country: &str,
        updated_after: Option<&str>,
    ) -> Result<Vec<OaAirport>, ApiError>;
    fn airspaces(
        &self,
        country: &str,
        updated_after: Option<&str>,
    ) -> Result<Vec<OaAirspace>, ApiError>;
    fn reporting_points(
        &self,
        country: &str,
        updated_after: Option<&str>,
    ) -> Result<Vec<OaReportingPoint>, ApiError>;
    fn changed_count(
        &self,
        endpoint: Endpoint,
        country: &str,
        updated_after: &str,
    ) -> Result<u64, ApiError>;
    fn search_airports(&self, query: &str) -> Result<Vec<OaAirport>, ApiError>;
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListResponse<T> {
    items: Vec<T>,
    #[serde(default)]
    total_count: u64,
    total_pages: Option<u64>,
}

impl<T> ListResponse<T> {
    /// `totalPages`, or derived from `totalCount` when the field is missing.
    fn pages(&self, limit: u64) -> u64 {
        self.total_pages
            .unwrap_or_else(|| self.total_count.div_ceil(limit))
    }
}

/// One HTTP GET: returns status and body text.
trait Transport {
    fn get(
        &self,
        url: &str,
        query: &[(&str, String)],
        key: &str,
    ) -> Result<(u16, String), ApiError>;
}

struct UreqTransport {
    agent: ureq::Agent,
}

impl Transport for UreqTransport {
    fn get(
        &self,
        url: &str,
        query: &[(&str, String)],
        key: &str,
    ) -> Result<(u16, String), ApiError> {
        let mut req = self.agent.get(url).header("x-openaip-api-key", key);
        for (k, v) in query {
            req = req.query(k, v);
        }
        let mut resp = req.call().map_err(|e| ApiError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .with_config()
            .limit(256 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| ApiError::Network(e.to_string()))?;
        Ok((status, body))
    }
}

pub struct HttpApi {
    key: String,
    base: String,
    transport: Box<dyn Transport>,
    /// Waits before retry 1, 2, 3 for HTTP 429/5xx.
    retry_waits: Vec<Duration>,
}

impl std::fmt::Debug for HttpApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpApi")
            .field("key", &"<redacted>")
            .field("base", &self.base)
            .finish()
    }
}

impl HttpApi {
    pub fn new(key: impl Into<String>) -> Self {
        Self::with_base(key, "https://api.core.openaip.net/api")
    }

    pub fn with_base(key: impl Into<String>, base: impl Into<String>) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();
        Self {
            key: key.into(),
            base: base.into(),
            transport: Box::new(UreqTransport { agent }),
            retry_waits: vec![
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8),
            ],
        }
    }

    /// GET with retries; returns the body of a 2xx answer.
    fn request(&self, path: &str, query: &[(&str, String)]) -> Result<String, ApiError> {
        let url = format!("{}/{}", self.base.trim_end_matches('/'), path);
        let mut attempt = 0;
        loop {
            let (status, body) = self.transport.get(&url, query, &self.key)?;
            match status {
                200..=299 => return Ok(body),
                401 | 403 => return Err(ApiError::Auth),
                429 | 500..=599 if attempt < self.retry_waits.len() => {
                    let wait = self.retry_waits[attempt];
                    tracing::warn!(status, attempt = attempt + 1, "OpenAIP busy, retrying");
                    attempt += 1;
                    if !wait.is_zero() {
                        std::thread::sleep(wait);
                    }
                }
                s => return Err(ApiError::Status(s)),
            }
        }
    }

    fn page<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: Endpoint,
        mut query: Vec<(&str, String)>,
        page: u64,
        limit: u64,
    ) -> Result<ListResponse<T>, ApiError> {
        query.push(("page", page.to_string()));
        query.push(("limit", limit.to_string()));
        let body = self.request(endpoint.path(), &query)?;
        serde_json::from_str(&body).map_err(|e| ApiError::Decode(e.to_string()))
    }

    fn list_all<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: Endpoint,
        country: &str,
        updated_after: Option<&str>,
    ) -> Result<Vec<T>, ApiError> {
        let mut out = Vec::new();
        let mut page = 1;
        loop {
            let mut q = vec![("country", country.to_string())];
            if let Some(u) = updated_after {
                q.push(("updatedAfter", u.to_string()));
            }
            let r: ListResponse<T> = self.page(endpoint, q, page, 1000)?;
            let last = page >= r.pages(1000);
            out.extend(r.items);
            if last {
                return Ok(out);
            }
            page += 1;
        }
    }
}

impl OpenAipApi for HttpApi {
    fn airports(&self, c: &str, u: Option<&str>) -> Result<Vec<OaAirport>, ApiError> {
        self.list_all(Endpoint::Airports, c, u)
    }
    fn airspaces(&self, c: &str, u: Option<&str>) -> Result<Vec<OaAirspace>, ApiError> {
        self.list_all(Endpoint::Airspaces, c, u)
    }
    fn reporting_points(
        &self,
        c: &str,
        u: Option<&str>,
    ) -> Result<Vec<OaReportingPoint>, ApiError> {
        self.list_all(Endpoint::ReportingPoints, c, u)
    }
    fn changed_count(
        &self,
        endpoint: Endpoint,
        country: &str,
        updated_after: &str,
    ) -> Result<u64, ApiError> {
        let q = vec![
            ("country", country.to_string()),
            ("updatedAfter", updated_after.to_string()),
        ];
        let r: ListResponse<serde::de::IgnoredAny> = self.page(endpoint, q, 1, 1)?;
        Ok(r.total_count)
    }
    fn search_airports(&self, query: &str) -> Result<Vec<OaAirport>, ApiError> {
        let q = vec![("search", query.to_string())];
        let r: ListResponse<OaAirport> = self.page(Endpoint::Airports, q, 1, 50)?;
        Ok(r.items)
    }
}

/// Canned API for tests (this crate and, with feature `fake`, dependents).
#[cfg(any(test, feature = "fake"))]
pub mod fake {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Debug, Clone, PartialEq)]
    pub struct Call {
        pub endpoint: Endpoint,
        pub country: String,
        pub updated_after: Option<String>,
    }

    #[derive(Default)]
    pub struct FakeApi {
        airports: RefCell<HashMap<String, Vec<OaAirport>>>,
        airspaces: RefCell<HashMap<String, Vec<OaAirspace>>>,
        reporting_points: RefCell<HashMap<String, Vec<OaReportingPoint>>>,
        failures: RefCell<HashMap<Endpoint, ApiError>>,
        calls: RefCell<Vec<Call>>,
    }

    impl FakeApi {
        pub fn new() -> Self {
            Self::default()
        }
        /// Replaces the canned airports of a country.
        pub fn set_airports(&self, country: &str, items: Vec<OaAirport>) {
            self.airports.borrow_mut().insert(country.into(), items);
        }
        pub fn set_airspaces(&self, country: &str, items: Vec<OaAirspace>) {
            self.airspaces.borrow_mut().insert(country.into(), items);
        }
        pub fn set_reporting_points(&self, country: &str, items: Vec<OaReportingPoint>) {
            self.reporting_points
                .borrow_mut()
                .insert(country.into(), items);
        }
        /// Makes every call to `endpoint` fail with `err`.
        pub fn fail(&self, endpoint: Endpoint, err: ApiError) {
            self.failures.borrow_mut().insert(endpoint, err);
        }
        pub fn clear_failures(&self) {
            self.failures.borrow_mut().clear();
        }
        pub fn calls(&self) -> Vec<Call> {
            self.calls.borrow().clone()
        }

        fn record(
            &self,
            endpoint: Endpoint,
            country: &str,
            u: Option<&str>,
        ) -> Result<(), ApiError> {
            self.calls.borrow_mut().push(Call {
                endpoint,
                country: country.into(),
                updated_after: u.map(String::from),
            });
            match self.failures.borrow().get(&endpoint) {
                Some(e) => Err(e.clone()),
                None => Ok(()),
            }
        }
    }

    impl OpenAipApi for FakeApi {
        fn airports(&self, c: &str, u: Option<&str>) -> Result<Vec<OaAirport>, ApiError> {
            self.record(Endpoint::Airports, c, u)?;
            Ok(self.airports.borrow().get(c).cloned().unwrap_or_default())
        }
        fn airspaces(&self, c: &str, u: Option<&str>) -> Result<Vec<OaAirspace>, ApiError> {
            self.record(Endpoint::Airspaces, c, u)?;
            Ok(self.airspaces.borrow().get(c).cloned().unwrap_or_default())
        }
        fn reporting_points(
            &self,
            c: &str,
            u: Option<&str>,
        ) -> Result<Vec<OaReportingPoint>, ApiError> {
            self.record(Endpoint::ReportingPoints, c, u)?;
            Ok(self
                .reporting_points
                .borrow()
                .get(c)
                .cloned()
                .unwrap_or_default())
        }
        fn changed_count(
            &self,
            endpoint: Endpoint,
            country: &str,
            updated_after: &str,
        ) -> Result<u64, ApiError> {
            self.record(endpoint, country, Some(updated_after))?;
            let n = match endpoint {
                Endpoint::Airports => self.airports.borrow().get(country).map_or(0, |v| v.len()),
                Endpoint::Airspaces => self.airspaces.borrow().get(country).map_or(0, |v| v.len()),
                Endpoint::ReportingPoints => self
                    .reporting_points
                    .borrow()
                    .get(country)
                    .map_or(0, |v| v.len()),
            };
            Ok(n as u64)
        }
        /// Matches case-insensitive substrings of name or ICAO across all
        /// countries. Records a call with country "" and no `updated_after`.
        fn search_airports(&self, query: &str) -> Result<Vec<OaAirport>, ApiError> {
            self.record(Endpoint::Airports, "", None)?;
            let q = query.to_lowercase();
            Ok(self
                .airports
                .borrow()
                .values()
                .flatten()
                .filter(|a| {
                    a.name.to_lowercase().contains(&q)
                        || a.icao_code
                            .as_deref()
                            .is_some_and(|i| i.to_lowercase().contains(&q))
                })
                .cloned()
                .collect())
        }
    }
}

#[cfg(any(test, feature = "fake"))]
pub use fake::FakeApi;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    const AIRPORT: &str = r#"{"_id":"aaa111","name":"TESTFIELD","icaoCode":"ZZTT","type":2,"country":"ZZ",
      "geometry":{"type":"Point","coordinates":[19.5,50.25]},
      "elevation":{"value":100,"unit":0,"referenceDatum":1},"magneticDeclination":5.5,
      "frequencies":[{"value":"121.980","unit":2,"type":14,"name":"TESTFIELD TWR","primary":true}],
      "runways":[{"designator":"07","trueHeading":72,"surface":{"x":1}}],
      "updatedAt":"2026-01-01T00:00:00.000Z","extra":1}"#;

    pub(super) fn airport() -> OaAirport {
        serde_json::from_str(AIRPORT).unwrap()
    }

    #[test]
    fn decodes_airport() {
        let a = airport();
        assert_eq!(a.id, "aaa111");
        assert_eq!(a.icao_code.as_deref(), Some("ZZTT"));
        assert_eq!(a.geometry.lat_lon(), [50.25, 19.5]);
        assert_eq!(a.elevation_ft(), 328);
        assert_eq!(a.magnetic_declination, Some(5.5));
        assert_eq!(a.frequencies[0].value, "121.980");
        assert!(a.frequencies[0].primary);
        assert_eq!(a.runways[0].true_heading, Some(72.0));
    }

    #[test]
    fn airport_defaults() {
        let a: OaAirport = serde_json::from_str(
            r#"{"_id":"b","name":"N","type":0,"country":"ZZ","geometry":{"type":"Point","coordinates":[1,2]},
            "elevation":{"value":10,"unit":1},"updatedAt":"x"}"#,
        )
        .unwrap();
        assert!(a.frequencies.is_empty() && a.runways.is_empty() && a.icao_code.is_none());
        assert_eq!(a.elevation_ft(), 10);
    }

    #[test]
    fn airport_without_elevation_or_country_decodes() {
        let a: OaAirport = serde_json::from_str(
            r#"{"_id":"b","name":"N","type":0,"geometry":{"type":"Point","coordinates":[1,2]},"updatedAt":"x"}"#,
        )
        .unwrap();
        assert_eq!(a.elevation_ft(), 0);
        assert_eq!(a.country, "");
    }

    #[test]
    fn missing_total_pages_falls_back_to_total_count() {
        let item = AIRPORT;
        let body = |n: u32| format!(r#"{{"items":[{item}],"totalCount":{n}}}"#);
        let (api, pages) = scripted(vec![(200, body(1001)), (200, body(1001))]);
        assert_eq!(api.airports("ZZ", None).unwrap().len(), 2);
        assert_eq!(*pages.borrow(), vec!["1", "2"]);
    }

    #[test]
    fn decodes_airspace() {
        let s: OaAirspace = serde_json::from_str(
            r#"{"_id":"ccc333","name":"TESTFIELD CTR","type":4,"icaoClass":3,"country":"ZZ",
            "lowerLimit":{"value":0,"unit":1,"referenceDatum":0},
            "upperLimit":{"value":2500,"unit":1,"referenceDatum":1},
            "geometry":{"type":"Polygon","coordinates":[[[19.0,50.0],[20.0,50.0],[20.0,51.0],[19.0,50.0]]]}}"#,
        )
        .unwrap();
        assert_eq!(s.icao_class, Some(3));
        assert_eq!(s.upper_limit.value, 2500.0);
        assert!(s.frequencies.is_empty());
        assert_eq!(s.geometry.outer_ring_lat_lon()[1], [50.0, 20.0]);
    }

    #[test]
    fn decodes_reporting_point() {
        let r: OaReportingPoint = serde_json::from_str(
            r#"{"_id":"ddd444","name":"ALPHA","compulsory":true,"country":"ZZ",
            "geometry":{"type":"Point","coordinates":[19.1,50.1]},"airports":["aaa111"]}"#,
        )
        .unwrap();
        assert!(r.compulsory);
        assert_eq!(r.airports, vec!["aaa111"]);
    }

    #[test]
    fn khz_cases() {
        assert_eq!(khz("121.980"), Some(121980));
        assert_eq!(khz("119.95"), Some(119950));
        assert_eq!(khz("abc"), None);
    }

    #[test]
    fn auth_error_hides_key() {
        let key = "SECRETKEY123";
        let api = HttpApi::new(key);
        assert!(ApiError::Auth.to_string().contains("openaip.net"));
        assert!(!ApiError::Auth.to_string().contains(key));
        assert!(!format!("{api:?}").contains(key));
        assert!(format!("{api:?}").contains("<redacted>"));
    }

    type Script = Rc<RefCell<VecDeque<(u16, String)>>>;
    struct Scripted(Script, Rc<RefCell<Vec<String>>>);
    impl Transport for Scripted {
        fn get(&self, _u: &str, q: &[(&str, String)], _k: &str) -> Result<(u16, String), ApiError> {
            let page = q.iter().find(|(k, _)| *k == "page").map(|(_, v)| v.clone());
            self.1.borrow_mut().push(page.unwrap_or_default());
            Ok(self.0.borrow_mut().pop_front().expect("script exhausted"))
        }
    }

    fn scripted(items: Vec<(u16, String)>) -> (HttpApi, Rc<RefCell<Vec<String>>>) {
        let pages = Rc::new(RefCell::new(Vec::new()));
        let mut api = HttpApi::new("k");
        api.transport = Box::new(Scripted(Rc::new(RefCell::new(items.into())), pages.clone()));
        api.retry_waits = vec![Duration::ZERO; 3];
        (api, pages)
    }

    fn list_body(n: usize, total_pages: u64, total: u64) -> String {
        let items = vec![AIRPORT; n].join(",");
        format!(r#"{{"items":[{items}],"totalCount":{total},"totalPages":{total_pages},"page":1}}"#)
    }

    #[test]
    fn pages_until_total_pages() {
        let (api, pages) = scripted(vec![(200, list_body(1, 2, 2)), (200, list_body(1, 2, 2))]);
        assert_eq!(api.airports("ZZ", None).unwrap().len(), 2);
        assert_eq!(*pages.borrow(), vec!["1", "2"]);
    }

    #[test]
    fn retries_then_succeeds() {
        let (api, pages) = scripted(vec![
            (429, String::new()),
            (503, String::new()),
            (200, list_body(1, 1, 1)),
        ]);
        assert_eq!(api.airports("ZZ", None).unwrap().len(), 1);
        assert_eq!(pages.borrow().len(), 3);
    }

    #[test]
    fn gives_up_after_three_retries() {
        let (api, pages) = scripted(vec![(500, String::new()); 4]);
        assert_eq!(api.airports("ZZ", None), Err(ApiError::Status(500)));
        assert_eq!(pages.borrow().len(), 4);
    }

    #[test]
    fn auth_statuses_map_to_auth() {
        let (api, _) = scripted(vec![(401, String::new())]);
        assert_eq!(api.airports("ZZ", None), Err(ApiError::Auth));
        let (api, _) = scripted(vec![(403, String::new())]);
        assert_eq!(api.search_airports("x"), Err(ApiError::Auth));
    }

    #[test]
    fn changed_count_reads_total() {
        let (api, _) = scripted(vec![(200, list_body(1, 5, 5))]);
        assert_eq!(
            api.changed_count(Endpoint::Airspaces, "ZZ", "2026-01-01")
                .unwrap(),
            5
        );
    }

    #[test]
    fn bad_json_is_decode_error() {
        let (api, _) = scripted(vec![(200, "nope".into())]);
        assert!(matches!(api.airports("ZZ", None), Err(ApiError::Decode(_))));
    }

    #[test]
    fn fake_api_behaviour() {
        let f = FakeApi::new();
        f.set_airports("ZZ", vec![airport()]);
        assert_eq!(f.airports("ZZ", Some("t")).unwrap().len(), 1);
        assert_eq!(f.search_airports("testf").unwrap().len(), 1);
        assert_eq!(f.search_airports("zztt").unwrap().len(), 1);
        assert!(f.search_airports("nothing").unwrap().is_empty());
        f.fail(Endpoint::Airspaces, ApiError::Status(500));
        assert_eq!(f.airspaces("ZZ", None), Err(ApiError::Status(500)));
        f.set_airports("ZZ", vec![]);
        assert!(f.airports("ZZ", None).unwrap().is_empty());
        assert_eq!(f.calls()[0].updated_after.as_deref(), Some("t"));
        let _dynref: &dyn OpenAipApi = &f;
    }
}
