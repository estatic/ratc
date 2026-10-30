//! The engine's dynamic state, saved by the server so a restart does not drop the flights in
//! progress: strips (clearances, plans, phases, open readbacks), surveillance, the runway, the
//! ATIS and the radio memory. Everything built from the configuration (airport, procedures,
//! taxi graph, destinations, vocabulary) is rebuilt at start and not part of it.

use std::collections::{HashMap, HashSet};

use atc_phraseology::{Callsign, Lang};
use serde::{Deserialize, Serialize};

use super::{AtcEngine, AtcOutput, Surveillance};
use crate::atis::AtisService;
use crate::runway::RunwayResource;
use crate::strip::{PilotProfile, Role, Strip};

/// Format version; a snapshot of another version is not restored.
pub const SNAPSHOT_VERSION: u32 = 1;

/// Maps with non-string keys are kept as pairs (JSON object keys are strings).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineSnapshot {
    pub version: u32,
    /// ICAO of the airport the snapshot was taken at.
    pub airport: String,
    atis: Option<AtisService>,
    runway: RunwayResource,
    pilots: Vec<PilotProfile>,
    strips: Vec<Strip>,
    tracks: Vec<(Callsign, Surveillance)>,
    fragment: Option<(Role, String, f64)>,
    last_told: Vec<(Callsign, String)>,
    last_sent: Vec<((Callsign, Role), AtcOutput)>,
    last_addressed: Vec<(Role, Callsign)>,
    last_lang: Lang,
    greeted: Vec<(Callsign, Role)>,
    last_squawk: Option<u16>,
    next_vehicle: u32,
    clock: (Option<f64>, Option<u16>),
}

/// Why a snapshot was not restored.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotMismatch {
    #[error("snapshot format {found}, this server reads {SNAPSHOT_VERSION}")]
    Version { found: u32 },
    #[error("snapshot taken at {found}, this server runs {expected}")]
    Airport { found: String, expected: String },
}

impl AtcEngine {
    /// The dynamic state, to be restored with [`AtcEngine::restore`].
    pub fn snapshot(&self) -> EngineSnapshot {
        fn pairs<K: Clone, V: Clone>(m: &HashMap<K, V>) -> Vec<(K, V)> {
            m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        }
        EngineSnapshot {
            version: SNAPSHOT_VERSION,
            airport: self.airport.icao.clone(),
            atis: self.atis.clone(),
            runway: self.runway.clone(),
            pilots: self.pilots.clone(),
            strips: self.strips.clone(),
            tracks: pairs(&self.tracks),
            fragment: self.fragment.clone(),
            last_told: pairs(&self.last_told),
            last_sent: pairs(&self.last_sent),
            last_addressed: pairs(&self.last_addressed),
            last_lang: self.last_lang,
            greeted: self.greeted.iter().cloned().collect(),
            last_squawk: self.last_squawk,
            next_vehicle: self.next_vehicle,
            clock: self.clock,
        }
    }

    /// Replaces the dynamic state with `s`, taken by [`AtcEngine::snapshot`] of an engine for
    /// the same airport. Pending vehicle orders are not kept (the plugin's vehicles do not
    /// survive a restart either).
    pub fn restore(&mut self, s: EngineSnapshot) -> Result<(), SnapshotMismatch> {
        if s.version != SNAPSHOT_VERSION {
            return Err(SnapshotMismatch::Version { found: s.version });
        }
        if s.airport != self.airport.icao {
            return Err(SnapshotMismatch::Airport {
                found: s.airport,
                expected: self.airport.icao.clone(),
            });
        }
        self.atis = s.atis;
        self.runway = s.runway;
        self.pilots = s.pilots;
        self.strips = s.strips;
        // Clearances from before the restart are not reminded.
        for strip in &mut self.strips {
            strip.readback_due_s = None;
        }
        self.tracks = s.tracks.into_iter().collect();
        self.fragment = s.fragment;
        self.last_told = s.last_told.into_iter().collect();
        self.last_sent = s.last_sent.into_iter().collect();
        self.last_addressed = s.last_addressed.into_iter().collect();
        self.last_lang = s.last_lang;
        self.greeted = s.greeted.into_iter().collect::<HashSet<_>>();
        self.last_squawk = s.last_squawk;
        self.next_vehicle = s.next_vehicle;
        self.clock = s.clock;
        self.vehicle_orders.clear();
        Ok(())
    }
}
