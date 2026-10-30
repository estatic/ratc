//! Readback requirements and checking.
//!
//! Mapping from issued [`AtcItem`]s to required [`ReadbackItem`]s
//! ([`required_readback`]):
//!
//! | Issued                                              | Standard                          | Training            |
//! |-----------------------------------------------------|-----------------------------------|---------------------|
//! | `Taxi` to a runway holding point (`to` has "runway") | `Runway`, `Kind(Taxi)`            | same                |
//! | `Taxi` with non-empty `via`                         | `Route(via)`                      | -                   |
//! | `HoldShort(r)`                                      | `HoldShort(r)`, `Runway(r)`       | same                |
//! | `ClearedTakeoff`/`ClearedLand`/`ClearedTouchAndGo`/`LineUpAndWait`/`CrossRunway` | `Runway`, `Kind` | same |
//! | `RunwayChanged(r)`                                  | `Runway(r)`                       | same                |
//! | `HoldPosition`                                      | `Kind(HoldPosition)`              | same                |
//! | `Qnh(q)`                                            | `Qnh(q)`                          | -                   |
//! | `Squawk(c)`/`CheckSquawk(c)`                        | `Squawk(c)`                       | -                   |
//! | `Contact { khz }`                                   | `Frequency(khz)`                  | -                   |
//! | `ClearedTo { icao }`                                | `Destination(icao)`               | same                |
//! | `Sid`/`Star`                                        | `Sid`/`Star` (fix, number, letter) | same               |
//! | `ClimbTo`/`DescendTo`/`MaintainLevel`               | `Level`                           | same                |
//! | `Heading { deg }`                                   | `Heading(deg)`                    | same                |
//! | `DirectTo { fix }`                                  | `Direct(fix)`                     | same                |
//! | `ClearedApproach { kind, runway }`                  | `Approach(kind, runway)`          | same                |
//!
//! Everything else is not required. Training is a subset of Standard.
//!
//! Frequencies are compared in kHz, but a heard value within 5 kHz of the
//! required one is equal (8.33 kHz channel label vs 25 kHz label, e.g.
//! 118100 vs 118105).

use atc_phraseology::types::{AtcItem, ClearanceKind, HeardItems, Level, ReadbackItem, TaxiTo};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strictness {
    Training,
    Standard,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadbackResult {
    Correct,
    /// Required item paired with what was heard instead.
    Wrong(Vec<(ReadbackItem, String)>),
    Missing(Vec<ReadbackItem>),
}

fn item_readback(item: &AtcItem, strictness: Strictness) -> Vec<ReadbackItem> {
    use ReadbackItem as R;
    let standard = strictness == Strictness::Standard;
    let mut out = Vec::new();
    let clearance = |out: &mut Vec<R>, runway: &str, kind| {
        out.push(R::Runway(runway.to_string()));
        out.push(R::Kind(kind));
    };
    match item {
        AtcItem::Taxi { to, via, .. } => {
            if let TaxiTo::HoldingPoint { runway } = to {
                clearance(&mut out, &runway.to_uppercase(), ClearanceKind::Taxi);
            }
            if standard && !via.is_empty() {
                out.push(R::Route(via.clone()));
            }
        }
        AtcItem::HoldShort(r) => {
            out.push(R::HoldShort(r.clone()));
            out.push(R::Runway(r.clone()));
        }
        AtcItem::ClearedTakeoff { runway, .. } => {
            clearance(&mut out, runway, ClearanceKind::Takeoff)
        }
        AtcItem::ClearedLand { runway, .. } => clearance(&mut out, runway, ClearanceKind::Land),
        AtcItem::ClearedTouchAndGo { runway, .. } => {
            clearance(&mut out, runway, ClearanceKind::TouchAndGo)
        }
        AtcItem::LineUpAndWait(r) => clearance(&mut out, r, ClearanceKind::LineUp),
        AtcItem::CrossRunway(r) => clearance(&mut out, r, ClearanceKind::Cross),
        AtcItem::RunwayChanged(r) => out.push(R::Runway(r.clone())),
        AtcItem::HoldPosition => out.push(R::Kind(ClearanceKind::HoldPosition)),
        AtcItem::Qnh(q) if standard => out.push(R::Qnh(*q)),
        AtcItem::Squawk(c) | AtcItem::CheckSquawk(c) if standard => out.push(R::Squawk(*c)),
        AtcItem::Contact { khz, .. } if standard => out.push(R::Frequency(*khz)),
        AtcItem::ClearedTo { icao, .. } => out.push(R::Destination(icao.clone())),
        AtcItem::Sid {
            fix,
            number,
            letter,
        } => out.push(R::Sid(fix.clone(), *number, *letter)),
        AtcItem::Star {
            fix,
            number,
            letter,
        } => out.push(R::Star(fix.clone(), *number, *letter)),
        AtcItem::ClimbTo(l) | AtcItem::DescendTo(l) | AtcItem::MaintainLevel(l) => {
            out.push(R::Level(*l))
        }
        AtcItem::Heading { deg, .. } => out.push(R::Heading(*deg)),
        AtcItem::DirectTo { fix } => out.push(R::Direct(fix.clone())),
        AtcItem::ClearedApproach { kind, runway } => out.push(R::Approach(*kind, runway.clone())),
        _ => {}
    }
    out
}

/// Items the pilot must read back for `items`, deduplicated, in issue order.
pub fn required_readback(items: &[AtcItem], strictness: Strictness) -> Vec<ReadbackItem> {
    let mut out: Vec<ReadbackItem> = Vec::new();
    for r in items.iter().flat_map(|i| item_readback(i, strictness)) {
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

enum Outcome {
    Ok,
    Missing,
    Wrong(String),
}

fn judge<T: ToString>(heard: &[T], ok: impl Fn(&T) -> bool) -> Outcome {
    judge_shown(heard, ok, T::to_string)
}

/// [`judge`] for values without `ToString`: `show` renders what was heard.
fn judge_shown<T>(heard: &[T], ok: impl Fn(&T) -> bool, show: impl Fn(&T) -> String) -> Outcome {
    if heard.iter().any(ok) {
        Outcome::Ok
    } else if heard.is_empty() {
        Outcome::Missing
    } else {
        Outcome::Wrong(heard.iter().map(show).collect::<Vec<_>>().join(", "))
    }
}

/// A procedure designator as said: "SUDOL 2 J".
fn designator((fix, number, letter): &(String, u8, char)) -> String {
    format!("{fix} {number} {letter}")
}

/// A heard level: altitudes in feet and flight levels, each judged as one list.
fn judge_level(want: Level, heard: &HeardItems) -> Outcome {
    let said: Vec<Level> = heard
        .altitudes_ft
        .iter()
        .map(|&a| Level::Altitude(a))
        .chain(heard.flight_levels.iter().map(|&f| Level::FlightLevel(f)))
        .collect();
    judge_shown(
        &said,
        |h| *h == want,
        |h| match h {
            Level::Altitude(a) => format!("{a} ft"),
            Level::FlightLevel(f) => format!("FL{f}"),
        },
    )
}

fn check_one(req: &ReadbackItem, heard: &HeardItems) -> Outcome {
    match req {
        ReadbackItem::Runway(r) => {
            // `runway X` inside `hold short runway X` is reported in
            // `hold_short` only, so both lists count.
            let all: Vec<String> = heard
                .runways
                .iter()
                .chain(&heard.hold_short)
                .cloned()
                .collect();
            judge(&all, |h| h == r)
        }
        ReadbackItem::HoldShort(r) => judge(&heard.hold_short, |h| h == r),
        ReadbackItem::Route(route) => {
            if heard.taxiways.is_empty() {
                Outcome::Missing
            } else if heard.taxiways == *route {
                Outcome::Ok
            } else {
                Outcome::Wrong(heard.taxiways.join(" "))
            }
        }
        ReadbackItem::Kind(k) => {
            if heard.kinds.is_empty() {
                Outcome::Missing
            } else if heard.kinds.contains(k) {
                Outcome::Ok
            } else {
                Outcome::Wrong(format!("{:?}", heard.kinds))
            }
        }
        ReadbackItem::Altitude(a) => judge(&heard.altitudes_ft, |h| h == a),
        ReadbackItem::Heading(a) => judge(&heard.headings, |h| h == a),
        ReadbackItem::Frequency(f) => judge(&heard.freqs_khz, |h| h.abs_diff(*f) <= 5),
        ReadbackItem::Qnh(q) => judge(&heard.qnh, |h| h == q),
        ReadbackItem::Squawk(s) => judge(&heard.squawk, |h| h == s),
        // A designator said without "departure" or "arrival" is listed as a SID, so
        // either list satisfies either requirement.
        ReadbackItem::Sid(f, n, l) | ReadbackItem::Star(f, n, l) => {
            let said: Vec<_> = heard.sids.iter().chain(&heard.stars).cloned().collect();
            judge_shown(
                &said,
                |(hf, hn, hl)| hf.eq_ignore_ascii_case(f) && hn == n && hl == l,
                designator,
            )
        }
        ReadbackItem::Level(level) => judge_level(*level, heard),
        ReadbackItem::Direct(fix) => judge(&heard.directs, |h| {
            crate::engine::direct_key(h) == crate::engine::direct_key(fix)
        }),
        ReadbackItem::Approach(kind, runway) => judge_shown(
            &heard.approaches,
            |(hk, hr)| hk == kind && hr == runway,
            |(hk, hr)| format!("{hk:?} {hr}"),
        ),
        ReadbackItem::Destination(icao) => match &heard.destination {
            None => Outcome::Missing,
            Some(d) if d.eq_ignore_ascii_case(icao) => Outcome::Ok,
            Some(d) => Outcome::Wrong(d.clone()),
        },
    }
}

/// Compares the pilot's readback with what was required. A same-type value
/// that differs is `Wrong`; no value of that type is `Missing`. `Wrong` takes
/// priority over `Missing`. Route order matters.
pub fn check_readback(required: &[ReadbackItem], heard: &HeardItems) -> ReadbackResult {
    let mut wrong = Vec::new();
    let mut missing = Vec::new();
    for req in required {
        match check_one(req, heard) {
            Outcome::Ok => {}
            Outcome::Missing => missing.push(req.clone()),
            Outcome::Wrong(h) => wrong.push((req.clone(), h)),
        }
    }
    if !wrong.is_empty() {
        ReadbackResult::Wrong(wrong)
    } else if !missing.is_empty() {
        ReadbackResult::Missing(missing)
    } else {
        ReadbackResult::Correct
    }
}

/// ATC reply to a failed readback. `Wrong`: "negative" plus the original
/// issued items that produced the wrong readback items ("check squawk" repeated as
/// "squawk"). `Missing`: a
/// `ReadBack` request for the missing items. `Correct`: nothing.
pub fn correction(result: &ReadbackResult, issued: &[AtcItem]) -> Vec<AtcItem> {
    match result {
        ReadbackResult::Correct => Vec::new(),
        ReadbackResult::Missing(m) => vec![AtcItem::ReadBack(m.clone())],
        ReadbackResult::Wrong(w) => {
            let mut out = vec![AtcItem::Negative];
            for item in issued {
                let produced = item_readback(item, Strictness::Standard);
                if w.iter().any(|(r, _)| produced.contains(r)) {
                    out.push(match item {
                        AtcItem::CheckSquawk(c) => AtcItem::Squawk(*c),
                        other => other.clone(),
                    });
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atc_phraseology::types::{ApproachKind, Wind};

    const WIND: Wind = Wind {
        dir_deg: 250,
        speed_kt: 8,
        calm: false,
    };

    fn taxi() -> AtcItem {
        AtcItem::Taxi {
            to: TaxiTo::HoldingPoint {
                runway: "25".into(),
            },
            via: vec!["A".into(), "B".into()],
            crossings: vec![],
        }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn runway_change_requires_runway_and_sid() {
        let items = [
            AtcItem::RunwayChanged("07".into()),
            AtcItem::Sid {
                fix: "SUDOL".into(),
                number: 1,
                letter: 'F',
            },
        ];
        let want = vec![
            ReadbackItem::Runway("07".into()),
            ReadbackItem::Sid("SUDOL".into(), 1, 'F'),
        ];
        assert_eq!(required_readback(&items, Strictness::Standard), want);
        assert_eq!(required_readback(&items, Strictness::Training), want);
    }

    #[test]
    fn correct_taxi_readback() {
        let req = required_readback(&[taxi()], Strictness::Standard);
        assert_eq!(
            req,
            vec![
                ReadbackItem::Runway("25".into()),
                ReadbackItem::Kind(ClearanceKind::Taxi),
                ReadbackItem::Route(s(&["A", "B"])),
            ]
        );
        let heard = HeardItems {
            runways: s(&["25"]),
            taxiways: s(&["A", "B"]),
            kinds: vec![ClearanceKind::Taxi],
            ..Default::default()
        };
        assert_eq!(check_readback(&req, &heard), ReadbackResult::Correct);
    }

    #[test]
    fn wrong_runway_is_wrong_not_missing() {
        let req = required_readback(
            &[AtcItem::ClearedLand {
                runway: "25".into(),
                wind: WIND,
            }],
            Strictness::Standard,
        );
        let heard = HeardItems {
            runways: s(&["07"]),
            kinds: vec![ClearanceKind::Land],
            ..Default::default()
        };
        assert_eq!(
            check_readback(&req, &heard),
            ReadbackResult::Wrong(vec![(ReadbackItem::Runway("25".into()), "07".into())])
        );
    }

    #[test]
    fn missing_hold_short() {
        let req = required_readback(&[AtcItem::HoldShort("25".into())], Strictness::Standard);
        let r = check_readback(&req, &HeardItems::default());
        assert_eq!(
            r,
            ReadbackResult::Missing(vec![
                ReadbackItem::HoldShort("25".into()),
                ReadbackItem::Runway("25".into())
            ])
        );
        // hold short heard (runway counted under hold_short only) is enough
        let heard = HeardItems {
            hold_short: s(&["25"]),
            ..Default::default()
        };
        assert_eq!(check_readback(&req, &heard), ReadbackResult::Correct);
    }

    #[test]
    fn training_ignores_qnh() {
        let items = [
            AtcItem::Qnh(1013),
            AtcItem::Contact {
                station: "Tower".into(),
                khz: 118100,
            },
        ];
        assert!(required_readback(&items, Strictness::Training).is_empty());
        assert_eq!(
            required_readback(&items, Strictness::Standard),
            vec![ReadbackItem::Qnh(1013), ReadbackItem::Frequency(118100)]
        );
        let t = required_readback(&[taxi()], Strictness::Training);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn squawk_is_a_standard_item() {
        let items = [AtcItem::Squawk(4201)];
        assert!(required_readback(&items, Strictness::Training).is_empty());
        let req = required_readback(&items, Strictness::Standard);
        assert_eq!(req, vec![ReadbackItem::Squawk(4201)]);
        let heard = HeardItems {
            squawk: vec![4210],
            ..Default::default()
        };
        let wrong = check_readback(&req, &heard);
        assert_eq!(
            wrong,
            ReadbackResult::Wrong(vec![(ReadbackItem::Squawk(4201), "4210".into())])
        );
        // "check squawk" is read back like the code; its correction repeats the code.
        let check = [AtcItem::CheckSquawk(4201), AtcItem::SquawkCharlie];
        assert!(required_readback(&check, Strictness::Training).is_empty());
        assert_eq!(required_readback(&check, Strictness::Standard), req);
        assert_eq!(
            correction(&wrong, &check),
            vec![AtcItem::Negative, AtcItem::Squawk(4201)]
        );
    }

    #[test]
    fn route_order_matters() {
        let req = vec![ReadbackItem::Route(s(&["A", "B"]))];
        let heard = HeardItems {
            taxiways: s(&["B", "A"]),
            ..Default::default()
        };
        assert_eq!(
            check_readback(&req, &heard),
            ReadbackResult::Wrong(vec![(req[0].clone(), "B A".into())])
        );
    }

    #[test]
    fn frequency_tolerance_5khz() {
        let req = vec![ReadbackItem::Frequency(118100)];
        let near = HeardItems {
            freqs_khz: vec![118105],
            ..Default::default()
        };
        assert_eq!(check_readback(&req, &near), ReadbackResult::Correct);
        let far = HeardItems {
            freqs_khz: vec![118125],
            ..Default::default()
        };
        assert!(matches!(
            check_readback(&req, &far),
            ReadbackResult::Wrong(_)
        ));
    }

    #[test]
    fn wrong_beats_missing() {
        let req = vec![ReadbackItem::Qnh(1013), ReadbackItem::Squawk(1234)];
        let heard = HeardItems {
            qnh: vec![1012],
            ..Default::default()
        };
        assert!(matches!(
            check_readback(&req, &heard),
            ReadbackResult::Wrong(_)
        ));
    }

    #[test]
    fn correction_for_wrong_repeats_item() {
        let issued = [
            AtcItem::HoldShort("07".into()),
            AtcItem::ClearedLand {
                runway: "25".into(),
                wind: WIND,
            },
        ];
        let r = ReadbackResult::Wrong(vec![(ReadbackItem::Runway("25".into()), "07".into())]);
        assert_eq!(
            correction(&r, &issued),
            vec![AtcItem::Negative, issued[1].clone()]
        );
    }

    #[test]
    fn hold_position_needs_readback() {
        let req = required_readback(&[AtcItem::HoldPosition], Strictness::Training);
        assert_eq!(req, vec![ReadbackItem::Kind(ClearanceKind::HoldPosition)]);
        let heard = HeardItems {
            kinds: vec![ClearanceKind::HoldPosition],
            ..Default::default()
        };
        assert_eq!(check_readback(&req, &heard), ReadbackResult::Correct);
    }

    #[test]
    fn correction_for_missing_asks_readback() {
        let m = vec![ReadbackItem::Qnh(1013)];
        assert_eq!(
            correction(&ReadbackResult::Missing(m.clone()), &[]),
            vec![AtcItem::ReadBack(m)]
        );
        assert!(correction(&ReadbackResult::Correct, &[]).is_empty());
    }

    #[test]
    fn clearance_items_require_readback() {
        let items = vec![
            AtcItem::ClearedTo {
                destination: "Katowice".into(),
                icao: "EPKT".into(),
            },
            AtcItem::Sid {
                fix: "SUDOL".into(),
                number: 2,
                letter: 'J',
            },
            AtcItem::ClimbTo(Level::Altitude(5000)),
            AtcItem::ClearedApproach {
                kind: ApproachKind::Ils,
                runway: "25".into(),
            },
        ];
        assert_eq!(
            required_readback(&items, Strictness::Training),
            vec![
                ReadbackItem::Destination("EPKT".into()),
                ReadbackItem::Sid("SUDOL".into(), 2, 'J'),
                ReadbackItem::Level(Level::Altitude(5000)),
                ReadbackItem::Approach(ApproachKind::Ils, "25".into()),
            ]
        );
    }

    fn ifr_clearance() -> Vec<AtcItem> {
        vec![
            AtcItem::ClearedTo {
                destination: "Katowice".into(),
                icao: "EPKT".into(),
            },
            AtcItem::Sid {
                fix: "SUDOL".into(),
                number: 2,
                letter: 'J',
            },
            AtcItem::ClimbTo(Level::Altitude(5000)),
            AtcItem::Squawk(4201),
        ]
    }

    fn heard(text: &str) -> HeardItems {
        use atc_phraseology::{Lang, Vocabulary, parse_pilot_with};
        let vocab = Vocabulary {
            fixes: std::sync::Arc::new(s(&["SUDOL", "NUMBA"]).into_iter().collect()),
            places: vec![atc_phraseology::Place::new("katowice", "EPKT")].into(),
            ..Vocabulary::default()
        };
        parse_pilot_with(text, Lang::En, &vocab).items
    }

    #[test]
    fn spoken_destination_readback_matches_icao() {
        let req = required_readback(&ifr_clearance(), Strictness::Standard);
        assert!(req.contains(&ReadbackItem::Destination("EPKT".into())));
        let said = heard(
            "cleared to Katowice, Sudol two J departure, climb five thousand, squawk four two zero one",
        );
        assert_eq!(check_readback(&req, &said), ReadbackResult::Correct);
    }

    #[test]
    fn correct_sid_readback() {
        let req = required_readback(&ifr_clearance(), Strictness::Standard);
        let said = heard(
            "cleared to Katowice, Sudol two Juliett departure, climb five thousand, squawk four two zero one",
        );
        assert_eq!(check_readback(&req, &said), ReadbackResult::Correct);
        // Without the word "departure", and as a star, the designator still counts.
        let bare = heard(
            "Sudol two Juliett, climb altitude five thousand feet, to Katowice, squawk four two zero one",
        );
        assert_eq!(check_readback(&req, &bare), ReadbackResult::Correct);
    }

    #[test]
    fn wrong_sid_is_negative_with_the_sid_repeated() {
        let issued = ifr_clearance();
        let req = required_readback(&issued, Strictness::Standard);
        let said = heard(
            "cleared to Katowice, Numba two Juliett departure, climb five thousand, squawk four two zero one",
        );
        let result = check_readback(&req, &said);
        assert_eq!(
            result,
            ReadbackResult::Wrong(vec![(
                ReadbackItem::Sid("SUDOL".into(), 2, 'J'),
                "NUMBA 2 J".into()
            )])
        );
        assert_eq!(
            correction(&result, &issued),
            vec![AtcItem::Negative, issued[1].clone()]
        );
        // The right fix with the wrong number is wrong too.
        let number = heard("Sudol three Juliett departure");
        assert!(matches!(
            check_readback(&[ReadbackItem::Sid("SUDOL".into(), 2, 'J')], &number),
            ReadbackResult::Wrong(_)
        ));
    }

    #[test]
    fn missing_level_is_asked_again() {
        let req = required_readback(&ifr_clearance(), Strictness::Standard);
        let said =
            heard("cleared to Katowice, Sudol two Juliett departure, squawk four two zero one");
        assert_eq!(
            check_readback(&req, &said),
            ReadbackResult::Missing(vec![ReadbackItem::Level(Level::Altitude(5000))])
        );
    }

    #[test]
    fn level_destination_direct_and_approach_are_compared() {
        let fl = ReadbackItem::Level(Level::FlightLevel(80));
        assert_eq!(
            check_readback(
                std::slice::from_ref(&fl),
                &heard("descend flight level eight zero")
            ),
            ReadbackResult::Correct
        );
        assert!(matches!(
            check_readback(&[fl], &heard("descend flight level seven zero")),
            ReadbackResult::Wrong(_)
        ));
        // An altitude is not a flight level.
        assert!(matches!(
            check_readback(
                &[ReadbackItem::Level(Level::Altitude(5000))],
                &heard("climb flight level five zero")
            ),
            ReadbackResult::Wrong(_)
        ));
        let dest = ReadbackItem::Destination("EPKT".into());
        assert!(matches!(
            check_readback(
                std::slice::from_ref(&dest),
                &heard("to Echo Papa Whiskey Alpha")
            ),
            ReadbackResult::Wrong(_)
        ));
        assert_eq!(
            check_readback(&[dest], &heard("squawk four two zero one")),
            ReadbackResult::Missing(vec![ReadbackItem::Destination("EPKT".into())])
        );
        let direct = ReadbackItem::Direct("SUDOL".into());
        assert_eq!(
            check_readback(
                std::slice::from_ref(&direct),
                &heard("proceed direct Sudol")
            ),
            ReadbackResult::Correct
        );
        assert!(matches!(
            check_readback(&[direct], &heard("proceed direct Numba")),
            ReadbackResult::Wrong(_)
        ));
        let appr = ReadbackItem::Approach(ApproachKind::Ils, "25".into());
        assert_eq!(
            check_readback(
                std::slice::from_ref(&appr),
                &heard("cleared ILS approach runway two five")
            ),
            ReadbackResult::Correct
        );
        assert!(matches!(
            check_readback(&[appr], &heard("cleared R NAV approach runway two five")),
            ReadbackResult::Wrong(_)
        ));
    }
}
