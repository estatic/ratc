//! Ground position: where the stand is, and the "on stand" report.
//!
//! The stand is told relative to the aircraft: "stand two zero is one hundred twenty metres
//! ahead" (distance rounded to 10 m; ahead / on your right / behind you / on your left by
//! the bearing to the stand off the aircraft heading, in 90° sectors).

use atc_geo::{bearing_deg, distance_m};
use atc_phraseology::{AtcItem, Courtesy, Side};

use super::{AtcEngine, standby};

/// A stopped aircraft this close to a stand is on it.
pub(super) const ON_STAND_M: f64 = 25.0;
/// Below this ground speed the aircraft is stopped.
const STOPPED_KT: f64 = 1.0;

impl AtcEngine {
    /// "On stand": stopped on the stand it was cleared to (any stand without a taxi
    /// clearance to one) -> "good day" (a wish is returned as for any farewell); otherwise
    /// where the stand it is going to is.
    pub(super) fn report_on_stand(&mut self, i: usize, farewell: Option<Courtesy>) -> Vec<AtcItem> {
        let Some(last) = self.tracks.get(&self.strips[i].callsign).map(|t| &t.last) else {
            return standby();
        };
        let target = match &self.strips[i].taxi_target {
            Some(crate::taxi::TaxiTarget::Stand { name }) => Some(name),
            _ => None,
        };
        let on_stand = last.gs_kt < STOPPED_KT
            && self.airport.stands.iter().any(|st| {
                target.is_none_or(|t| *t == st.name) && distance_m(st.pos, last.pos) <= ON_STAND_M
            });
        if on_stand {
            return vec![AtcItem::Courtesy(farewell.unwrap_or(Courtesy::GoodDay))];
        }
        match self.stand_of(i) {
            Some(name) => self.stand_hint(i, &name).into_iter().collect(),
            None => vec![AtcItem::SayAgain],
        }
    }

    /// The stand aircraft `i` is going to: its taxi target, else the stand it started from,
    /// else the nearest one.
    fn stand_of(&self, i: usize) -> Option<String> {
        let strip = &self.strips[i];
        if let Some(crate::taxi::TaxiTarget::Stand { name }) = &strip.taxi_target {
            return Some(name.clone());
        }
        if let Some(home) = &strip.home_stand {
            return Some(home.clone());
        }
        let pos = self.last_pos(i)?;
        self.airport
            .stands
            .iter()
            .min_by(|a, b| distance_m(a.pos, pos).total_cmp(&distance_m(b.pos, pos)))
            .map(|st| st.name.clone())
    }

    /// Distance to stand `name` from aircraft `i` and "stand <name> is <N> metres <side>".
    pub(super) fn stand_hint_at(&self, i: usize, name: &str) -> Option<(f64, AtcItem)> {
        let last = &self.tracks.get(&self.strips[i].callsign)?.last;
        let st = self.airport.stands.iter().find(|st| st.name == name)?;
        let d = distance_m(last.pos, st.pos);
        let rel = (bearing_deg(last.pos, st.pos) - last.heading_deg).rem_euclid(360.0);
        let item = AtcItem::StandPosition {
            stand: name.to_string(),
            metres: ((d / 10.0).round() as u32 * 10).max(10),
            side: relative_side(rel),
        };
        Some((d, item))
    }

    fn stand_hint(&self, i: usize, name: &str) -> Option<AtcItem> {
        self.stand_hint_at(i, name).map(|(_, item)| item)
    }
}

/// Side of a relative bearing (degrees clockwise from the nose).
fn relative_side(rel: f64) -> Side {
    match rel {
        r if !(45.0..315.0).contains(&r) => Side::Ahead,
        r if r < 135.0 => Side::Right,
        r if r <= 225.0 => Side::Behind,
        _ => Side::Left,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sides() {
        assert_eq!(relative_side(0.0), Side::Ahead);
        assert_eq!(relative_side(350.0), Side::Ahead);
        assert_eq!(relative_side(90.0), Side::Right);
        assert_eq!(relative_side(180.0), Side::Behind);
        assert_eq!(relative_side(270.0), Side::Left);
        assert_eq!(relative_side(315.0), Side::Ahead);
    }
}
