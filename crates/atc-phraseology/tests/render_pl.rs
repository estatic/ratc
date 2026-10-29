use atc_phraseology::*;

const WIND: Wind = Wind {
    dir_deg: 250,
    speed_kt: 8,
    calm: false,
};

fn pl(items: Vec<AtcItem>, first: bool) -> String {
    render(
        &AtcMessage {
            to: Some(Callsign::new("SP-ABC")),
            from: "Kraków Tower".into(),
            items,
            lang: Lang::Pl,
        },
        first,
    )
}

/// Items only: no addressee.
fn bare(items: Vec<AtcItem>) -> String {
    render(
        &AtcMessage {
            to: None,
            from: "Kraków Tower".into(),
            items,
            lang: Lang::Pl,
        },
        false,
    )
}

fn one(item: AtcItem) -> String {
    bare(vec![item])
}

#[test]
fn takeoff_clearance_pl() {
    assert_eq!(
        pl(
            vec![
                AtcItem::ClearedTakeoff {
                    runway: "25".into(),
                    wind: WIND
                },
                AtcItem::Report(CircuitLeg::Downwind)
            ],
            true
        ),
        "Sierra Papa Alfa Bravo Charlie, wiatr dwieście pięćdziesiąt stopni osiem węzłów, pas dwa pięć, zezwalam startować, zgłoś z wiatrem"
    );
}

#[test]
fn taxi_with_qnh_and_squawk_pl() {
    assert_eq!(
        pl(
            vec![
                AtcItem::Taxi {
                    to: TaxiTo::HoldingPoint {
                        runway: "25".into()
                    },
                    via: vec!["T".into(), "Z5".into(), "A".into()],
                    crossings: vec![]
                },
                AtcItem::HoldShort("25".into()),
                AtcItem::Qnh(1013),
                AtcItem::Squawk(4201)
            ],
            false
        ),
        "Sierra Bravo Charlie, kołuj do punktu oczekiwania przed pasem dwa pięć drogą Tango, Zulu pięć, Alfa, oczekuj przed pasem dwa pięć, QNH dziesięć trzynaście, skłok cztery dwa zero jeden"
    );
}

#[test]
fn station_names_pl() {
    assert_eq!(station_name("Kraków Tower", Lang::Pl), "Kraków Wieża");
    assert_eq!(station_name("Kraków Ground", Lang::Pl), "Kraków Ground");
    assert_eq!(station_name("Kraków Tower", Lang::En), "Kraków Tower");
}

#[test]
fn taxi_to_stand_pl() {
    assert_eq!(
        one(AtcItem::Taxi {
            to: TaxiTo::Stand { name: "35".into() },
            via: vec!["C".into(), "B4".into()],
            crossings: vec![]
        }),
        "kołuj do stanowiska trzy pięć drogą Charlie, Bravo cztery"
    );
}

#[test]
fn taxi_crossings_pl() {
    assert_eq!(
        one(AtcItem::Taxi {
            to: TaxiTo::HoldingPoint {
                runway: "25".into()
            },
            via: vec!["A".into()],
            crossings: vec!["07".into()]
        }),
        "kołuj do punktu oczekiwania przed pasem dwa pięć drogą Alfa, przetnij pas zero siedem"
    );
}

#[test]
fn hold_short_cross_line_up_pl() {
    assert_eq!(
        one(AtcItem::HoldShort("25".into())),
        "oczekuj przed pasem dwa pięć"
    );
    assert_eq!(
        one(AtcItem::CrossRunway("25".into())),
        "przetnij pas dwa pięć"
    );
    assert_eq!(
        one(AtcItem::LineUpAndWait("25".into())),
        "pas dwa pięć, zajmij pas i czekaj"
    );
}

#[test]
fn runway_suffixes_pl() {
    assert_eq!(
        one(AtcItem::CrossRunway("25R".into())),
        "przetnij pas dwa pięć prawy"
    );
    assert_eq!(
        one(AtcItem::CrossRunway("07L".into())),
        "przetnij pas zero siedem lewy"
    );
    assert_eq!(
        one(AtcItem::CrossRunway("07C".into())),
        "przetnij pas zero siedem środkowy"
    );
}

#[test]
fn clearances_pl() {
    let calm = Wind {
        dir_deg: 0,
        speed_kt: 0,
        calm: true,
    };
    assert_eq!(
        one(AtcItem::ClearedTakeoff {
            runway: "25".into(),
            wind: calm
        }),
        "wiatr cisza, pas dwa pięć, zezwalam startować"
    );
    assert_eq!(
        one(AtcItem::ClearedLand {
            runway: "25".into(),
            wind: WIND
        }),
        "wiatr dwieście pięćdziesiąt stopni osiem węzłów, pas dwa pięć, zezwalam lądować"
    );
    assert_eq!(
        one(AtcItem::ClearedTouchAndGo {
            runway: "25".into(),
            wind: WIND
        }),
        "wiatr dwieście pięćdziesiąt stopni osiem węzłów, pas dwa pięć, zezwalam konwojer"
    );
}

#[test]
fn report_legs_pl() {
    let cases = [
        (CircuitLeg::Upwind, "zgłoś pozycję pod wiatr"),
        (CircuitLeg::Crosswind, "zgłoś pozycję boczną"),
        (CircuitLeg::Downwind, "zgłoś z wiatrem"),
        (CircuitLeg::Base, "zgłoś bejs"),
        (CircuitLeg::Final, "zgłoś prostą"),
    ];
    for (leg, want) in cases {
        assert_eq!(one(AtcItem::Report(leg)), want);
    }
}

#[test]
fn sequence_qnh_squawk_pl() {
    assert_eq!(one(AtcItem::Sequence(2)), "numer dwa");
    assert_eq!(one(AtcItem::Qnh(1008)), "QNH dziesięć zero osiem");
    assert_eq!(one(AtcItem::Squawk(4201)), "skłok cztery dwa zero jeden");
    assert_eq!(
        one(AtcItem::CheckSquawk(4201)),
        "sprawdź skłok cztery dwa zero jeden"
    );
    assert_eq!(one(AtcItem::SquawkCharlie), "skłok Charlie");
}

#[test]
fn contact_pl() {
    assert_eq!(
        one(AtcItem::Contact {
            station: "Kraków Tower".into(),
            khz: 123_255
        }),
        "łączność z Kraków Wieża sto dwadzieścia trzy kropka dwa pięć pięć"
    );
}

#[test]
fn short_phrases_pl() {
    assert_eq!(one(AtcItem::HoldPosition), "stój");
    assert_eq!(one(AtcItem::GoAround), "przejdź na drugi krąg");
    assert_eq!(one(AtcItem::Negative), "nieprawidłowo");
    assert_eq!(one(AtcItem::SayAgain), "powtórz");
    assert_eq!(one(AtcItem::SayFlightRules), "podaj przepisy lotu");
    assert_eq!(one(AtcItem::Roger), "przyjąłem");
    assert_eq!(one(AtcItem::Standby), "czekaj");
    assert_eq!(one(AtcItem::SayIntentions), "podaj zamiary");
    assert_eq!(one(AtcItem::SayStand), "podaj stanowisko");
    assert_eq!(one(AtcItem::PassYourMessage), "nadawaj");
}

#[test]
fn read_back_pl() {
    assert_eq!(
        one(AtcItem::ReadBack(vec![
            ReadbackItem::Runway("25".into()),
            ReadbackItem::HoldShort("25".into()),
            ReadbackItem::Route(vec!["A".into()]),
            ReadbackItem::Kind(ClearanceKind::Takeoff),
            ReadbackItem::Altitude(1800),
            ReadbackItem::Heading(90),
            ReadbackItem::Frequency(123_255),
            ReadbackItem::Qnh(1013),
            ReadbackItem::Squawk(4201),
        ])),
        "powtórz pas i instrukcję oczekiwania przed pasem i drogę kołowania i zezwolenie i wysokość i kurs i częstotliwość i QNH i skłok"
    );
}

#[test]
fn say_again_callsign_pl() {
    assert_eq!(
        one(AtcItem::SayAgainCallsign {
            station: "Kraków Ground".into()
        }),
        "wołający Kraków Ground, powtórz znak wywoławczy"
    );
    assert_eq!(
        one(AtcItem::SayAgainCallsign {
            station: "Kraków Tower".into()
        }),
        "wołający Kraków Wieża, powtórz znak wywoławczy"
    );
}

#[test]
fn atis_current_pl() {
    assert_eq!(one(AtcItem::AtisCurrent('B')), "aktualna informacja Bravo");
    assert_eq!(one(AtcItem::AtisCurrent('A')), "aktualna informacja Alfa");
}

#[test]
fn vacate_pl() {
    assert_eq!(one(AtcItem::Vacate { via: None }), "zwolnij pas");
    assert_eq!(
        one(AtcItem::Vacate {
            via: Some("B2".into())
        }),
        "zwolnij pas drogą Bravo dwa"
    );
}

#[test]
fn unable_pl() {
    assert_eq!(
        one(AtcItem::Unable(UnableReason::NoRoute)),
        "nie mogę, brak drogi kołowania"
    );
    assert_eq!(
        one(AtcItem::Unable(UnableReason::UnknownStand {
            stand: "35".into()
        })),
        "nie mogę, nieznane stanowisko trzy pięć"
    );
}

#[test]
fn follow_me_pl() {
    assert_eq!(
        one(AtcItem::FollowMeOnTheWay),
        "samochód follow me jest w drodze"
    );
    assert_eq!(
        one(AtcItem::FollowTheFollowMe),
        "kołuj za samochodem follow me"
    );
}

#[test]
fn stand_position_pl() {
    let cases = [
        (Side::Ahead, "przed tobą"),
        (Side::Right, "po twojej prawej"),
        (Side::Behind, "za tobą"),
        (Side::Left, "po twojej lewej"),
    ];
    for (side, want) in cases {
        assert_eq!(
            one(AtcItem::StandPosition {
                stand: "20".into(),
                metres: 120,
                side
            }),
            format!("stanowisko dwa zero jest sto dwadzieścia metrów {want}")
        );
    }
    assert_eq!(
        one(AtcItem::StandPosition {
            stand: "C5".into(),
            metres: 30,
            side: Side::Ahead
        }),
        "stanowisko Charlie pięć jest trzydzieści metrów przed tobą"
    );
}

#[test]
fn without_clearance_pl() {
    assert_eq!(
        one(AtcItem::TakenOffWithoutClearance),
        "wystartowałeś bez zezwolenia"
    );
    assert_eq!(
        one(AtcItem::LandedWithoutClearance),
        "wylądowałeś bez zezwolenia"
    );
}

#[test]
fn courtesy_pl() {
    let cases = [
        (Courtesy::Hello, "dzień dobry"),
        (Courtesy::GoodMorning, "dzień dobry"),
        (Courtesy::GoodAfternoon, "dzień dobry"),
        (Courtesy::GoodEvening, "dobry wieczór"),
        (Courtesy::GoodDay, "do widzenia"),
        (Courtesy::HaveANiceFlight, "miłego lotu"),
        (Courtesy::ThankYouYouToo, "dziękuję, wzajemnie"),
    ];
    for (c, want) in cases {
        assert_eq!(one(AtcItem::Courtesy(c)), want);
    }
}

#[test]
fn altitude_check_pl() {
    assert_eq!(
        one(AtcItem::CheckAltitude { circuit_ft: 1800 }),
        "sprawdź wysokość, wysokość kręgu tysiąc osiemset stóp"
    );
}

#[test]
fn ruling_rows_pl() {
    assert_eq!(
        one(AtcItem::TaxiAndReportReady {
            runway: "25".into()
        }),
        "kołuj do punktu oczekiwania przed pasem dwa pięć, zgłoś gotowość"
    );
    assert_eq!(
        one(AtcItem::NotClearedForTakeoff),
        "nie masz zezwolenia na start"
    );
    assert_eq!(
        one(AtcItem::NotClearedToLand),
        "nie masz zezwolenia na lądowanie"
    );
    assert_eq!(
        one(AtcItem::RunwayIncursion {
            runway: "25".into()
        }),
        "wjechałeś na pas dwa pięć bez zezwolenia"
    );
    assert_eq!(
        one(AtcItem::GiveWay {
            traffic: "Cessna".into()
        }),
        "ustąp pierwszeństwa Cessna"
    );
    assert_eq!(
        one(AtcItem::Follow {
            traffic: "Cessna".into()
        }),
        "kołuj za Cessna"
    );
}

#[test]
fn full_callsign_on_first_contact_pl() {
    assert_eq!(
        pl(vec![AtcItem::Roger], true),
        "Sierra Papa Alfa Bravo Charlie, przyjąłem"
    );
    assert_eq!(
        pl(vec![AtcItem::Roger], false),
        "Sierra Bravo Charlie, przyjąłem"
    );
}

#[test]
fn pl_ifr_clearance() {
    let text = bare(vec![
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
    ]);
    assert_eq!(
        text,
        "zezwolenie do Katowic, odlot SUDOL dwa Juliett, wznoszenie na wysokość pięć tysięcy stóp, skłok cztery dwa zero jeden"
    );
}

#[test]
fn pl_climb_to_flight_level() {
    assert_eq!(
        bare(vec![AtcItem::ClimbTo(Level::FlightLevel(80))]),
        "wznoszenie na poziom lotu osiem zero"
    );
}

#[test]
fn pl_ground_and_vfr_phrases() {
    assert_eq!(
        bare(vec![AtcItem::StartUpApproved]),
        "uruchomienie zezwalam"
    );
    assert_eq!(
        bare(vec![AtcItem::PushAndStartApproved { facing: None }]),
        "wypychanie i uruchomienie zezwalam"
    );
    assert_eq!(bare(vec![AtcItem::ReadbackCorrect]), "powtórzenie poprawne");
    assert_eq!(
        bare(vec![AtcItem::EnterControlZoneVia {
            point: "ZULU".into(),
            alt_ft: 2000
        }]),
        "wejście w strefę przez ZULU, wysokość dwa tysiące stóp"
    );
    assert_eq!(
        bare(vec![AtcItem::LeaveControlZoneVia {
            point: "NOVEMBER".into()
        }]),
        "wyjście ze strefy przez NOVEMBER"
    );
    assert_eq!(
        bare(vec![AtcItem::ReportAt {
            point: "ZULU".into()
        }]),
        "zgłoś nad ZULU"
    );
    assert_eq!(
        bare(vec![AtcItem::HoldOver {
            point: "ZULU".into(),
            max_ft: 2000
        }]),
        "oczekuj nad ZULU, wysokość maksymalna dwa tysiące stóp"
    );
}

#[test]
fn pl_new_unable_and_readback() {
    assert_eq!(
        bare(vec![AtcItem::Unable(UnableReason::UnknownDestination)]),
        "nie mogę, powtórz miejsce przeznaczenia"
    );
    assert_eq!(
        bare(vec![AtcItem::ReadBack(vec![
            ReadbackItem::Sid("SUDOL".into(), 2, 'J'),
            ReadbackItem::Level(Level::Altitude(5000)),
        ])]),
        "powtórz odlot i poziom"
    );
}

#[test]
fn pl_destination_genitive() {
    let c = |d: &str| {
        bare(vec![AtcItem::ClearedTo {
            destination: d.into(),
            icao: "EPXX".into(),
        }])
    };
    assert_eq!(c("Warszawa"), "zezwolenie do Warszawy");
    assert_eq!(c("Gdańsk"), "zezwolenie do Gdańska");
}

#[test]
fn pl_approach_items_fall_back_to_english() {
    assert_eq!(
        bare(vec![AtcItem::ClearedApproach {
            kind: ApproachKind::Ils,
            runway: "25".into()
        }]),
        "cleared ILS approach runway two five"
    );
    assert_eq!(bare(vec![AtcItem::Identified]), "identified");
    assert_eq!(
        bare(vec![AtcItem::Star {
            fix: "MAPIK".into(),
            number: 1,
            letter: 'G'
        }]),
        "MAPIK one Golf arrival"
    );
}

#[test]
fn pl_vfr_join_and_frequency_change() {
    assert_eq!(
        bare(vec![AtcItem::JoinDownwind {
            runway: "25".into()
        }]),
        "wejdź w krąg na pozycję z wiatrem, pas dwa pięć"
    );
    assert_eq!(
        bare(vec![AtcItem::FrequencyChangeApproved]),
        "zmiana częstotliwości zatwierdzona"
    );
    assert_eq!(
        bare(vec![AtcItem::ChangeTo {
            station: "Bielsko Radio".into(),
            khz: 118_330
        }]),
        "przejdź na Bielsko Radio sto osiemnaście kropka trzy trzy zero"
    );
    assert_eq!(
        bare(vec![AtcItem::Unable(UnableReason::SouthernEntry)]),
        "nie mogę, wlot od południa tylko przez punkty południowe"
    );
}

#[test]
fn departure_runway_request_answers_pl() {
    assert_eq!(
        one(AtcItem::RunwayApproved("07".into())),
        "pas zero siedem, zgoda"
    );
    assert_eq!(
        one(AtcItem::Unable(UnableReason::RunwayInUse {
            runway: "25".into()
        })),
        "nie mogę, pas w użyciu dwa pięć"
    );
}

#[test]
fn runway_change_pl() {
    assert_eq!(
        one(AtcItem::RunwayChanged("07".into())),
        "zmiana pasa, pas zero siedem"
    );
}

#[test]
fn pl_no_flight_plan_say_intentions() {
    assert_eq!(
        bare(vec![AtcItem::NoFlightPlan, AtcItem::SayIntentions]),
        "brak planu lotu, podaj zamiary"
    );
}
