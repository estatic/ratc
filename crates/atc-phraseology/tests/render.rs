use atc_phraseology::*;

const WIND: Wind = Wind {
    dir_deg: 240,
    speed_kt: 10,
    calm: false,
};

fn msg(to: Option<&str>, from: &str, items: Vec<AtcItem>) -> AtcMessage {
    AtcMessage {
        to: to.map(Callsign::new),
        from: from.to_string(),
        items,
        lang: Lang::En,
    }
}

fn ground(items: Vec<AtcItem>) -> AtcMessage {
    msg(Some("SP-ABC"), "Kraków Ground", items)
}

#[test]
fn taxi_clearance_text() {
    let m = ground(vec![
        AtcItem::Taxi {
            to: TaxiTo::HoldingPoint {
                runway: "25".into(),
            },
            via: vec!["A".into(), "B".into()],
            crossings: vec![],
        },
        AtcItem::HoldShort("25".into()),
        AtcItem::Qnh(1013),
    ]);
    assert_eq!(
        render(&m, true),
        "Sierra Papa Alpha Bravo Charlie, taxi to holding point runway two five via Alpha, Bravo, hold short runway two five, QNH one zero one three"
    );
}

#[test]
fn takeoff_text() {
    let m = msg(
        Some("SPABC"),
        "Kraków Tower",
        vec![
            AtcItem::ClearedTakeoff {
                runway: "25".into(),
                wind: WIND,
            },
            AtcItem::Report(CircuitLeg::Downwind),
        ],
    );
    assert_eq!(
        render(&m, false),
        "Sierra Bravo Charlie, wind two four zero degrees, one zero knots, runway two five, cleared for take-off, report downwind"
    );
}

#[test]
fn negative_correction() {
    let m = ground(vec![AtcItem::Negative, AtcItem::HoldShort("25".into())]);
    assert_eq!(
        render(&m, false),
        "Sierra Bravo Charlie, negative, hold short runway two five"
    );
}

#[test]
fn contact_text() {
    let m = ground(vec![AtcItem::Contact {
        station: "Kraków Tower".into(),
        khz: 118100,
    }]);
    assert_eq!(
        render(&m, false),
        "Sierra Bravo Charlie, contact Kraków Tower one one eight decimal one"
    );
}

#[test]
fn say_again_callsign() {
    let m = msg(
        None,
        "Kraków Ground",
        vec![AtcItem::SayAgainCallsign {
            station: "Kraków Ground".into(),
        }],
    );
    assert_eq!(
        render(&m, true),
        "Station calling Kraków Ground, say again your callsign"
    );
}

#[test]
fn niner_spoken() {
    let m = msg(None, "Kraków Tower", vec![AtcItem::HoldShort("09L".into())]);
    assert_eq!(render(&m, false), "hold short runway zero niner left");
}

#[test]
fn runway_suffixes_and_pair_label() {
    let m = msg(
        None,
        "x",
        vec![
            AtcItem::CrossRunway("25R".into()),
            AtcItem::CrossRunway("25C".into()),
            AtcItem::CrossRunway("07/25".into()),
        ],
    );
    assert_eq!(
        render(&m, false),
        "cross runway two five right, cross runway two five centre, cross runway zero seven, two five"
    );
}

#[test]
fn taxiway_with_digit_and_vacate() {
    let m = msg(
        None,
        "x",
        vec![
            AtcItem::Taxi {
                to: TaxiTo::Stand { name: "5".into() },
                via: vec!["B2".into()],
                crossings: vec!["25".into()],
            },
            AtcItem::Vacate {
                via: Some("A".into()),
            },
        ],
    );
    assert_eq!(
        render(&m, false),
        "taxi to stand five via Bravo two, cross runway two five, vacate via Alpha"
    );
}

#[test]
fn calm_wind_landing_and_frequency_decimals() {
    let calm = Wind {
        dir_deg: 0,
        speed_kt: 0,
        calm: true,
    };
    let m = msg(
        None,
        "x",
        vec![
            AtcItem::ClearedLand {
                runway: "25".into(),
                wind: calm,
            },
            AtcItem::Contact {
                station: "Kraków Tower".into(),
                khz: 118125,
            },
        ],
    );
    assert_eq!(
        render(&m, false),
        "wind calm, runway two five, cleared to land, contact Kraków Tower one one eight decimal one two five"
    );
}

#[test]
fn readback_requests() {
    let m = ground(vec![AtcItem::ReadBack(vec![
        ReadbackItem::HoldShort("25".into()),
        ReadbackItem::Runway("25".into()),
        ReadbackItem::Qnh(1013),
    ])]);
    assert_eq!(
        render(&m, false),
        "Sierra Bravo Charlie, read back hold short instruction, runway, QNH"
    );
}

#[test]
fn misc_items() {
    let m = ground(vec![
        AtcItem::Sequence(2),
        AtcItem::AtisCurrent('B'),
        AtcItem::HoldPosition,
        AtcItem::GoAround,
        AtcItem::Roger,
    ]);
    assert_eq!(
        render(&m, false),
        "Sierra Bravo Charlie, number two, information Bravo is current, hold position, go around, roger"
    );
}

#[test]
fn single_digit_wind_speed_and_low_heading() {
    let w = Wind {
        dir_deg: 90,
        speed_kt: 5,
        calm: false,
    };
    let m = msg(
        None,
        "x",
        vec![AtcItem::ClearedTouchAndGo {
            runway: "09".into(),
            wind: w,
        }],
    );
    assert_eq!(
        render(&m, false),
        "wind zero niner zero degrees, five knots, runway zero niner, cleared touch and go"
    );
}

#[test]
fn line_up_and_wait_word_order() {
    let m = ground(vec![AtcItem::LineUpAndWait("25".into())]);
    assert_eq!(
        render(&m, false),
        "Sierra Bravo Charlie, runway two five, line up and wait"
    );
}

#[test]
fn non_runway_alphanumerics_are_spelled() {
    let m = msg(
        None,
        "x",
        vec![
            AtcItem::Taxi {
                to: TaxiTo::Stand { name: "C5".into() },
                via: vec![],
                crossings: vec![],
            },
            AtcItem::Taxi {
                to: TaxiTo::Stand { name: "5A".into() },
                via: vec![],
                crossings: vec![],
            },
            AtcItem::Taxi {
                to: TaxiTo::HoldingPoint {
                    runway: "25L".into(),
                },
                via: vec![],
                crossings: vec![],
            },
            AtcItem::Vacate {
                via: Some("R2".into()),
            },
        ],
    );
    assert_eq!(
        render(&m, false),
        "taxi to stand Charlie five, taxi to stand five Alpha, taxi to holding point runway two five left, vacate via Romeo two"
    );
}

#[test]
fn squawk_text() {
    let m = ground(vec![AtcItem::Qnh(1013), AtcItem::Squawk(4201)]);
    assert_eq!(
        render(&m, false),
        "Sierra Bravo Charlie, QNH one zero one three, squawk four two zero one"
    );
    let m = ground(vec![AtcItem::CheckSquawk(421)]);
    assert_eq!(
        render(&m, false),
        "Sierra Bravo Charlie, check squawk zero four two one"
    );
    let m = ground(vec![AtcItem::SquawkCharlie]);
    assert_eq!(render(&m, false), "Sierra Bravo Charlie, squawk Charlie");
}

#[test]
fn typed_items_render_as_before() {
    let cases: Vec<(AtcItem, &str)> = vec![
        (AtcItem::SayAgain, "say again"),
        (AtcItem::SayFlightRules, "say flight rules"),
        (AtcItem::SayIntentions, "say intentions"),
        (AtcItem::SayStand, "say stand"),
        (AtcItem::PassYourMessage, "pass your message"),
        (AtcItem::Standby, "standby"),
        (AtcItem::FollowMeOnTheWay, "follow-me car is on the way"),
        (
            AtcItem::FollowTheFollowMe {
                to: TaxiTo::Stand { name: "17".into() },
            },
            "follow the follow-me car to stand one seven",
        ),
        (
            AtcItem::FollowTheFollowMe {
                to: TaxiTo::HoldingPoint {
                    runway: "25".into(),
                },
            },
            "follow the follow-me car to holding point runway two five",
        ),
        (
            AtcItem::StandPosition {
                stand: "20".into(),
                metres: 120,
                side: Side::Left,
            },
            "stand two zero is one hundred twenty metres on your left",
        ),
        (
            AtcItem::StandPosition {
                stand: "5A".into(),
                metres: 1050,
                side: Side::Ahead,
            },
            "stand five Alpha is one thousand fifty metres ahead",
        ),
        (
            AtcItem::StandPosition {
                stand: "C5".into(),
                metres: 40,
                side: Side::Right,
            },
            "stand Charlie five is forty metres on your right",
        ),
        (
            AtcItem::StandPosition {
                stand: "GA".into(),
                metres: 10,
                side: Side::Behind,
            },
            "stand GA is ten metres behind you",
        ),
        (
            AtcItem::TakenOffWithoutClearance,
            "you have taken off without clearance",
        ),
        (
            AtcItem::LandedWithoutClearance,
            "you have landed without clearance",
        ),
        (
            AtcItem::RunwayIncursion {
                runway: "25".into(),
            },
            "you have entered runway two five without clearance",
        ),
        (
            AtcItem::CheckAltitude { circuit_ft: 1500 },
            "check altitude, circuit altitude one thousand five hundred feet",
        ),
        (
            AtcItem::TaxiAndReportReady {
                runway: "25".into(),
            },
            "taxi to holding point runway two five, report ready",
        ),
        (
            AtcItem::NotClearedForTakeoff,
            "negative, not cleared for take-off, report ready for departure",
        ),
        (AtcItem::NotClearedToLand, "negative, not cleared to land"),
        (AtcItem::Courtesy(Courtesy::Hello), "hello"),
        (AtcItem::Courtesy(Courtesy::GoodMorning), "good morning"),
        (AtcItem::Courtesy(Courtesy::GoodAfternoon), "good afternoon"),
        (AtcItem::Courtesy(Courtesy::GoodEvening), "good evening"),
        (AtcItem::Courtesy(Courtesy::GoodDay), "good day"),
        (
            AtcItem::Courtesy(Courtesy::HaveANiceFlight),
            "have a nice flight",
        ),
        (
            AtcItem::Courtesy(Courtesy::ThankYouYouToo),
            "thank you, you too",
        ),
        (
            AtcItem::Unable(UnableReason::NoRoute),
            "unable, no taxi route",
        ),
        (
            AtcItem::Unable(UnableReason::UnknownStand { stand: "5A".into() }),
            "unable, stand five Alpha unknown",
        ),
        (
            AtcItem::Taxi {
                to: TaxiTo::HoldingPoint {
                    runway: "25".into(),
                },
                via: vec!["B5".into(), "B6".into(), "A".into()],
                crossings: vec![],
            },
            "taxi to holding point runway two five via Bravo five, Bravo six, Alpha",
        ),
        (
            AtcItem::Taxi {
                to: TaxiTo::Stand { name: "20".into() },
                via: vec!["A".into()],
                crossings: vec!["07/25".into()],
            },
            "taxi to stand two zero via Alpha, cross runway zero seven, two five",
        ),
    ];
    for (item, text) in cases {
        let m = msg(None, "x", vec![item.clone()]);
        assert_eq!(render(&m, false), text, "{item:?}");
    }
}

fn one(item: AtcItem) -> String {
    render(&msg(None, "Kraków Delivery", vec![item]), false)
}

#[test]
fn ifr_clearance_as_one_message() {
    let m = msg(
        Some("SP-ABC"),
        "Kraków Delivery",
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
        ],
    );
    assert_eq!(
        render(&m, true),
        "Sierra Papa Alpha Bravo Charlie, cleared to Katowice, SUDOL two Juliett departure, \
         climb altitude five thousand feet, squawk four two zero one"
    );
}

#[test]
fn procedures_and_levels() {
    assert_eq!(
        one(AtcItem::Star {
            fix: "MAPIK".into(),
            number: 1,
            letter: 'G'
        }),
        "MAPIK one Golf arrival"
    );
    assert_eq!(
        one(AtcItem::ClimbTo(Level::FlightLevel(80))),
        "climb flight level eight zero"
    );
    assert_eq!(
        one(AtcItem::DescendTo(Level::Altitude(3000))),
        "descend to altitude three thousand feet"
    );
    assert_eq!(
        one(AtcItem::MaintainLevel(Level::Altitude(2500))),
        "maintain altitude two thousand five hundred feet"
    );
}

#[test]
fn descent_with_qnh_is_two_items() {
    let m = ground(vec![
        AtcItem::DescendTo(Level::Altitude(3000)),
        AtcItem::Qnh(1013),
    ]);
    assert_eq!(
        render(&m, false),
        "Sierra Bravo Charlie, descend to altitude three thousand feet, QNH one zero one three"
    );
}

#[test]
fn headings_and_direct() {
    assert_eq!(
        one(AtcItem::Heading {
            deg: 220,
            turn: Some(Turn::Left)
        }),
        "turn left heading two two zero"
    );
    assert_eq!(
        one(AtcItem::Heading {
            deg: 90,
            turn: Some(Turn::Right)
        }),
        "turn right heading zero niner zero"
    );
    assert_eq!(
        one(AtcItem::Heading {
            deg: 250,
            turn: None
        }),
        "fly heading two five zero"
    );
    assert_eq!(
        one(AtcItem::DirectTo {
            fix: "SUDOL".into()
        }),
        "proceed direct SUDOL"
    );
}

#[test]
fn approaches() {
    assert_eq!(
        one(AtcItem::ClearedApproach {
            kind: ApproachKind::Ils,
            runway: "25".into()
        }),
        "cleared ILS approach runway two five"
    );
    assert_eq!(
        one(AtcItem::ExpectApproach {
            kind: ApproachKind::VorDme,
            runway: "07".into()
        }),
        "expect VOR D M E approach runway zero seven"
    );
    assert_eq!(
        one(AtcItem::ExpectApproach {
            kind: ApproachKind::Rnav,
            runway: "07".into()
        }),
        "expect R NAV approach runway zero seven"
    );
    assert_eq!(
        one(AtcItem::VisualApproachNotAvailable),
        "visual approach not available"
    );
}

#[test]
fn radar_phrases() {
    assert_eq!(one(AtcItem::ReportEstablished), "report established");
    assert_eq!(
        one(AtcItem::CheckEstablished {
            kind: ApproachKind::Ils,
            runway: "25".into()
        }),
        "check established, report established localizer runway two five"
    );
    assert_eq!(one(AtcItem::Identified), "identified");
    assert_eq!(
        one(AtcItem::RadarServiceTerminated),
        "radar service terminated"
    );
    assert_eq!(one(AtcItem::DistanceToGo(8)), "eight miles from touchdown");
    assert_eq!(one(AtcItem::DistanceToGo(1)), "one mile from touchdown");
    assert_eq!(
        one(AtcItem::OffTrack { nm: 2 }),
        "you are two miles off track, check position"
    );
    assert_eq!(
        one(AtcItem::CheckHeading(90)),
        "check heading, fly heading zero niner zero"
    );
    assert_eq!(
        one(AtcItem::CheckLevel(Level::Altitude(5000))),
        "check level, maintain altitude five thousand feet"
    );
}

#[test]
fn ground_phrases() {
    assert_eq!(one(AtcItem::StartUpApproved), "start-up approved");
    assert_eq!(
        one(AtcItem::PushAndStartApproved { facing: None }),
        "push-back and start-up approved"
    );
    assert_eq!(
        one(AtcItem::PushAndStartApproved {
            facing: Some("north".into())
        }),
        "push-back and start-up approved, facing north"
    );
    assert_eq!(one(AtcItem::ReadbackCorrect), "readback correct");
}

#[test]
fn vfr_zone_phrases() {
    assert_eq!(
        one(AtcItem::EnterControlZoneVia {
            point: "ZULU".into(),
            alt_ft: 2000
        }),
        "enter control zone via ZULU, altitude two thousand feet"
    );
    assert_eq!(
        one(AtcItem::LeaveControlZoneVia {
            point: "NOVEMBER".into()
        }),
        "leave control zone via NOVEMBER"
    );
    assert_eq!(
        one(AtcItem::HoldOver {
            point: "ZULU".into(),
            max_ft: 2500
        }),
        "hold over ZULU, maximum altitude two thousand five hundred feet"
    );
    assert_eq!(
        one(AtcItem::ReportAt {
            point: "ZULU".into()
        }),
        "report at ZULU"
    );
}

#[test]
fn new_unable_reasons() {
    assert_eq!(
        one(AtcItem::Unable(UnableReason::UnknownDestination)),
        "unable, say again destination"
    );
    assert_eq!(
        one(AtcItem::Unable(UnableReason::NoIfrData)),
        "unable, IFR not available, VFR only"
    );
    assert_eq!(
        one(AtcItem::Unable(UnableReason::Closed)),
        "unable, station closed"
    );
    assert_eq!(
        one(AtcItem::Unable(UnableReason::ApproachNotAvailable)),
        "unable, approach not available"
    );
}

#[test]
fn new_readback_requests() {
    assert_eq!(
        one(AtcItem::ReadBack(vec![
            ReadbackItem::Destination("Katowice".into()),
            ReadbackItem::Sid("SUDOL".into(), 2, 'J'),
            ReadbackItem::Level(Level::Altitude(5000)),
        ])),
        "read back destination, departure procedure, level"
    );
}

#[test]
fn vfr_join_and_frequency_change() {
    assert_eq!(
        one(AtcItem::JoinDownwind {
            runway: "25".into()
        }),
        "join downwind runway two five"
    );
    assert_eq!(
        one(AtcItem::FrequencyChangeApproved),
        "frequency change approved"
    );
    assert_eq!(
        one(AtcItem::ChangeTo {
            station: "Bielsko Radio".into(),
            khz: 118_330
        }),
        "change to Bielsko Radio one one eight decimal three three zero"
    );
    assert_eq!(
        one(AtcItem::Unable(UnableReason::SouthernEntry)),
        "unable, entry from the south only via the southern points"
    );
}

// ICAO Annex 10 Vol II 5.2.1.4.1.4: all six digits, only first four when the
// fifth and sixth are both zero.
#[test]
fn frequency_keeps_a_single_trailing_zero() {
    assert_eq!(frequency_words(118_100), "one one eight decimal one");
    assert_eq!(frequency_words(121_500), "one two one decimal five");
    assert_eq!(
        frequency_words(121_980),
        "one two one decimal niner eight zero"
    );
    assert_eq!(
        frequency_words(126_130),
        "one two six decimal one three zero"
    );
    assert_eq!(
        frequency_words(118_105),
        "one one eight decimal one zero five"
    );
}

#[test]
fn departure_runway_request_answers() {
    assert_eq!(
        one(AtcItem::RunwayApproved("07".into())),
        "runway zero seven approved"
    );
    assert_eq!(
        one(AtcItem::Unable(UnableReason::RunwayInUse {
            runway: "25".into()
        })),
        "unable, runway in use two five"
    );
}

#[test]
fn runway_change_with_new_sid() {
    assert_eq!(
        render(
            &msg(
                None,
                "Kraków Ground",
                vec![
                    AtcItem::RunwayChanged("07".into()),
                    AtcItem::Sid {
                        fix: "SUDOL".into(),
                        number: 1,
                        letter: 'F',
                    },
                ],
            ),
            false
        ),
        "runway changed, runway zero seven, SUDOL one Foxtrot departure"
    );
}

#[test]
fn no_flight_plan_say_intentions() {
    let m = msg(
        None,
        "Kraków Approach",
        vec![AtcItem::NoFlightPlan, AtcItem::SayIntentions],
    );
    assert_eq!(render(&m, false), "no flight plan, say intentions");
}
