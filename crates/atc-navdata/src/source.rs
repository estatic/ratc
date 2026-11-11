//! The boundary between the ATC server and the simulator's own data (airports, procedures).
//! Server code talks to [`SimDataSource`]; each simulator provides one implementation.

use std::path::PathBuf;
use std::sync::Arc;

use atc_geo::LatLon;

use crate::apt::{NavdataError, parse_apt_dat};
use crate::fixes::ProcedureSet;
use crate::model::{Airport, Destination};
use crate::xplane::{ResolveError, XPlaneSource};

/// One airport's layout in apt.dat text (the repo's layout format), from a simulator's data.
#[derive(Debug, Clone, PartialEq)]
pub struct AirportLayout {
    /// Where the layout came from, for logs.
    pub origin: String,
    /// Airport id on the header row; differs from the ICAO when matched via `1302 icao_code`.
    pub ident: String,
    /// The apt.dat block.
    pub apt_dat: String,
}

impl AirportLayout {
    /// Parses the block; `Airport::icao` is set to `icao`.
    pub fn parse(&self, icao: &str) -> Result<Airport, NavdataError> {
        let mut airport = parse_apt_dat(&self.apt_dat, &self.ident)?;
        airport.icao = icao.to_string();
        Ok(airport)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// The source has no such airport / no procedures for it.
    #[error("{0}")]
    NotFound(String),
    /// The data exists but cannot be read or used.
    #[error("{0}")]
    Unusable(String),
}

impl From<ResolveError> for SourceError {
    fn from(e: ResolveError) -> Self {
        match e {
            ResolveError::NotFound { .. } | ResolveError::CifpNotFound { .. } => {
                SourceError::NotFound(e.to_string())
            }
            _ => SourceError::Unusable(e.to_string()),
        }
    }
}

pub trait SimDataSource: Send + Sync {
    /// For logs: "X-Plane scenery at <root>".
    fn label(&self) -> String;
    fn airport_layout(&self, icao: &str) -> Result<AirportLayout, SourceError>;
    fn procedure_set(&self, icao: &str) -> Result<ProcedureSet, SourceError>;
    /// Airports within `radius_nm` of `around` (f64::INFINITY = all), sorted by ICAO.
    fn airports(&self, around: LatLon, radius_nm: f64) -> Result<Vec<Destination>, SourceError>;
    /// Files whose size/mtime change makes a cached airport scan stale.
    fn cache_inputs(&self) -> Vec<PathBuf>;
    /// Warnings about the simulator's own navigation data for these airports (e.g. the
    /// aircraft GPS having no procedures there), one line each, for the server log.
    fn gps_warnings(&self, _icaos: &[String]) -> Vec<String> {
        Vec::new()
    }
}

/// Which simulator's data the server reads.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "sim", rename_all = "snake_case", deny_unknown_fields)]
pub enum SimDataConfig {
    Xplane { root: PathBuf },
}

impl SimDataConfig {
    /// The old top-level `xplane_root` key.
    pub fn from_legacy_root(root: PathBuf) -> Self {
        SimDataConfig::Xplane { root }
    }

    pub fn open(&self) -> Arc<dyn SimDataSource> {
        match self {
            SimDataConfig::Xplane { root } => Arc::new(XPlaneSource::new(root.clone())),
        }
    }
}
