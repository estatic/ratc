//! Ground position: a free stand for "any stand" ("request taxi to any available spot").
//!
//! The stand must take the aircraft's category (apt.dat row 1300), must not be a de-icing
//! stand, and must be free (no other aircraft on it, no other strip cleared to it or started
//! from it). A light propeller aircraft prefers tie-down / misc stands (GA aprons) to gates,
//! any other aircraft gates; within that preference the shortest taxi route wins.

use atc_navdata::model::{Stand, StandCategory, StandKind};

use super::AtcEngine;
use crate::taxi::TaxiTarget;

/// ICAO type designator prefixes of light propeller aircraft (props stands).
const PROPS_TYPES: &[&str] = &[
    "C1", "C2", "PA", "P28", "P32", "SR2", "DA4", "DA2", "BE3", "BE9", "BE7", "M20", "TB1", "TB2",
    "DR4", "AA5",
];
/// Prefixes of `PROPS_TYPES` that are not light props: Citation jets, the Hercules.
const NOT_PROPS_TYPES: &[&str] = &["C25", "C13"];

/// The stand category an aircraft of ICAO type `aircraft_type` needs.
pub(crate) fn stand_category(aircraft_type: &str) -> StandCategory {
    let t = aircraft_type.trim().to_ascii_uppercase();
    if PROPS_TYPES.iter().any(|p| t.starts_with(p))
        && !NOT_PROPS_TYPES.iter().any(|p| t.starts_with(p))
    {
        StandCategory::Props
    } else {
        StandCategory::Jets
    }
}

/// A de-icing pad ("70 de-ice", "DEICE 2"), never given as a stand.
fn is_deice(name: &str) -> bool {
    let squashed: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_uppercase();
    squashed.contains("DEICE")
}

/// Preference of a stand of `kind` for `cat` (lower first).
fn kind_rank(kind: StandKind, cat: StandCategory) -> u8 {
    match (kind, cat) {
        (StandKind::TieDown | StandKind::Misc, StandCategory::Props) => 0,
        (StandKind::Gate, StandCategory::Props) => 1,
        (StandKind::Gate, _) => 0,
        (StandKind::TieDown | StandKind::Misc, _) => 1,
        (StandKind::Hangar, _) => 2,
    }
}

fn takes(st: &Stand, cat: StandCategory) -> bool {
    st.categories.is_empty() || st.categories.contains(&cat)
}

impl AtcEngine {
    /// The stand (index into `airport.stands`) for aircraft `i` asking for "any stand": a
    /// free, non-de-icing stand taking its category, the preferred kind first, then the
    /// shortest taxi route from where it is. `None`: no such stand reachable.
    pub(super) fn pick_stand(&self, i: usize) -> Option<usize> {
        let pos = self.last_pos(i)?;
        let cs = &self.strips[i].callsign;
        let aircraft_type = self
            .pilots
            .iter()
            .find(|p| &p.callsign == cs)
            .map_or("", |p| p.aircraft_type.as_str());
        let cat = stand_category(aircraft_type);
        self.airport
            .stands
            .iter()
            .enumerate()
            .filter(|(_, st)| takes(st, cat) && !is_deice(&st.name))
            .filter(|&(k, _)| self.stand_is_free(i, k))
            .filter_map(|(k, st)| {
                let target = TaxiTarget::Stand {
                    name: st.name.clone(),
                    at: Some(st.pos),
                };
                let route = self.graph.route(&self.airport, pos, &target).ok()?;
                Some((kind_rank(st.kind, cat), route.length_m, k))
            })
            .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
            .map(|(_, _, k)| k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_props_and_others() {
        for t in [
            "C172", "c152", "PA28", "P28A", "SR22", "DA40", "DA20", "BE36",
        ] {
            assert_eq!(stand_category(t), StandCategory::Props, "{t}");
        }
        for t in ["A320", "B738", "E190", "C25A", "C130", ""] {
            assert_eq!(stand_category(t), StandCategory::Jets, "{t}");
        }
    }

    #[test]
    fn deice_names() {
        assert!(is_deice("70 de-ice"));
        assert!(is_deice("DEICE 2"));
        assert!(!is_deice("APRON4 GA"));
    }
}
