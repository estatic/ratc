//! Runway as a shared resource: one aircraft at a time, with landing reservations.

use atc_phraseology::Callsign;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RunwayUse {
    Takeoff,
    Land,
    Cross,
    LineUp,
}

/// State of one runway. The same callsign never blocks itself.
///
/// Clearance rules (`can_clear`), where "other" means a callsign different from the requester:
/// - `Takeoff` and `LineUp`: blocked by another aircraft occupying the runway, another
///   crossing it, or another holding the landing reservation.
/// - `Land`: blocked by another occupying or crossing; a landing reservation held by
///   another also blocks (one landing at a time).
/// - `Cross`: blocked by another occupying or by another's landing reservation. A
///   crossing by another does not block (the single `crossing` slot is then replaced).
///
/// When several blockers apply, the reported one is the first of occupying, crossing, landing.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunwayResource {
    /// Aircraft lined up, taking off or rolling out on the runway.
    pub occupied_by: Option<Callsign>,
    /// Aircraft holding the landing reservation (cleared to land, not yet vacated).
    pub landing: Option<Callsign>,
    /// Aircraft currently crossing.
    pub crossing: Option<Callsign>,
}

impl RunwayResource {
    /// Checks whether `who` may be cleared for `use_`; `Err` names the aircraft in the way.
    pub fn can_clear(&self, who: &Callsign, use_: RunwayUse) -> Result<(), Callsign> {
        let other = |c: &Option<Callsign>| c.clone().filter(|c| c != who);
        let (occupied, crossing, landing) = (
            other(&self.occupied_by),
            other(&self.crossing),
            other(&self.landing),
        );
        let blocker = match use_ {
            RunwayUse::Takeoff | RunwayUse::LineUp | RunwayUse::Land => {
                occupied.or(crossing).or(landing)
            }
            RunwayUse::Cross => occupied.or(landing),
        };
        blocker.map_or(Ok(()), Err)
    }

    /// Records `who` using the runway as `use_` if `can_clear` allows it.
    /// `Takeoff`/`LineUp` set `occupied_by`, `Land` sets `landing`, `Cross` sets `crossing`.
    pub fn occupy(&mut self, who: &Callsign, use_: RunwayUse) -> Result<(), Callsign> {
        self.can_clear(who, use_)?;
        let slot = match use_ {
            RunwayUse::Takeoff | RunwayUse::LineUp => &mut self.occupied_by,
            RunwayUse::Land => &mut self.landing,
            RunwayUse::Cross => &mut self.crossing,
        };
        *slot = Some(who.clone());
        Ok(())
    }

    /// Drops every claim `who` holds on the runway.
    pub fn release(&mut self, who: &Callsign) {
        for slot in [&mut self.occupied_by, &mut self.landing, &mut self.crossing] {
            if slot.as_ref() == Some(who) {
                *slot = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(s: &str) -> Callsign {
        Callsign::new(s)
    }

    #[test]
    fn runway_blocks_takeoff_when_occupied() {
        let mut r = RunwayResource::default();
        r.occupy(&cs("SPABC"), RunwayUse::LineUp).unwrap();
        assert_eq!(
            r.can_clear(&cs("SPXYZ"), RunwayUse::Takeoff),
            Err(cs("SPABC"))
        );
        assert_eq!(
            r.can_clear(&cs("SPXYZ"), RunwayUse::LineUp),
            Err(cs("SPABC"))
        );
        assert_eq!(r.can_clear(&cs("SPXYZ"), RunwayUse::Land), Err(cs("SPABC")));
        assert_eq!(
            r.can_clear(&cs("SPXYZ"), RunwayUse::Cross),
            Err(cs("SPABC"))
        );
        // Never blocks itself.
        assert_eq!(r.can_clear(&cs("SPABC"), RunwayUse::Takeoff), Ok(()));
        r.release(&cs("SPABC"));
        assert_eq!(r.can_clear(&cs("SPXYZ"), RunwayUse::Takeoff), Ok(()));
    }

    #[test]
    fn crossing_blocked_when_landing_reserved() {
        let mut r = RunwayResource::default();
        r.occupy(&cs("SPABC"), RunwayUse::Land).unwrap();
        assert_eq!(r.landing, Some(cs("SPABC")));
        assert_eq!(
            r.can_clear(&cs("SPXYZ"), RunwayUse::Cross),
            Err(cs("SPABC"))
        );
        assert_eq!(
            r.can_clear(&cs("SPXYZ"), RunwayUse::Takeoff),
            Err(cs("SPABC"))
        );
        assert_eq!(r.can_clear(&cs("SPXYZ"), RunwayUse::Land), Err(cs("SPABC")));
        assert_eq!(r.can_clear(&cs("SPABC"), RunwayUse::Cross), Ok(()));
    }

    #[test]
    fn crossing_aircraft_blocks_takeoff_and_landing_but_not_another_cross() {
        let mut r = RunwayResource::default();
        r.occupy(&cs("SPABC"), RunwayUse::Cross).unwrap();
        assert_eq!(r.crossing, Some(cs("SPABC")));
        assert_eq!(
            r.can_clear(&cs("SPXYZ"), RunwayUse::LineUp),
            Err(cs("SPABC"))
        );
        assert_eq!(r.can_clear(&cs("SPXYZ"), RunwayUse::Land), Err(cs("SPABC")));
        assert_eq!(r.can_clear(&cs("SPXYZ"), RunwayUse::Cross), Ok(()));
    }

    #[test]
    fn failed_occupy_changes_nothing_and_release_clears_all_claims() {
        let mut r = RunwayResource::default();
        r.occupy(&cs("SPABC"), RunwayUse::Takeoff).unwrap();
        let before = r.clone();
        assert_eq!(r.occupy(&cs("SPXYZ"), RunwayUse::Land), Err(cs("SPABC")));
        assert_eq!(r, before);
        r.release(&cs("SPXYZ"));
        assert_eq!(r, before);
        r.release(&cs("SPABC"));
        assert_eq!(r, RunwayResource::default());
    }
}
