use atc_phraseology::*;

fn pl(text: &str) -> PilotMessage {
    parse_pilot(text, Lang::Pl)
}

/// Parses the Polish transmission and asserts the same intent and items as its English twin.
fn twin(polish: &str, english: &str) -> PilotMessage {
    let p = pl(polish);
    let e = parse_pilot(english, Lang::En);
    assert_eq!(p.intent, e.intent, "intent of {polish:?}");
    assert_eq!(p.items, e.items, "items of {polish:?}");
    assert_eq!(p.callsign, e.callsign, "callsign of {polish:?}");
    p
}

#[test]
fn polish_request_taxi() {
    let m = twin(
        "Kraków Ground, Sierra Papa Alfa Bravo Charlie, dzień dobry, informacja Alfa, proszę o kołowanie do kręgów",
        "Kraków Ground, Sierra Papa Alpha Bravo Charlie, good morning, information Alpha, request taxi for circuits",
    );
    assert_eq!(m.addressee.as_deref(), Some("ground"));
    assert_eq!(m.callsign.as_deref(), Some("SPABC"));
    assert_eq!(
        m.intent,
        PilotIntent::RequestTaxi {
            atis: Some('A'),
            vfr_circuits: true
        }
    );
    // "dzień dobry" maps to the same reply as good morning.
    assert_eq!(m.greeting, Some(Courtesy::GoodMorning));
}

#[test]
fn polish_taxi_readback() {
    let m = twin(
        "Kołuję do punktu oczekiwania przed pasem dwa pięć drogą Tango, Zulu pięć, Alfa, oczekuję przed pasem dwa pięć, QNH dziesięć trzynaście, skłok cztery dwa zero jeden, Sierra Bravo Charlie",
        "Taxi to holding point, hold short runway two five, via Tango, Zulu five, Alpha, hold short runway two five, QNH one zero one three, squawk four two zero one, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.taxiways, vec!["T", "Z5", "A"]);
    assert_eq!(m.items.hold_short, vec!["25"]);
    assert_eq!(m.items.qnh, vec![1013]);
    assert_eq!(m.items.squawk, vec![4201]);
    assert_eq!(m.items.kinds, vec![ClearanceKind::Taxi]);
    assert_eq!(m.callsign.as_deref(), Some("SBC"));
}

#[test]
fn polish_without_diacritics() {
    let m = twin(
        "Koluje do punktu oczekiwania przed pasem dwa piec droga Tango, oczekuje przed pasem dwa piec, QNH dziesiec trzynascie, sklok cztery dwa zero jeden, Sierra Bravo Charlie",
        "Taxi to holding point, hold short runway two five, via Tango, hold short runway two five, QNH 1013, squawk 4201, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.taxiways, vec!["T"]);
    assert_eq!(m.items.qnh, vec![1013]);
}

#[test]
fn polish_ready_for_departure() {
    let m = twin(
        "Kraków Wieża, Sierra Papa Alfa Bravo Charlie, punkt oczekiwania pas dwa pięć, gotowy do startu",
        "Kraków Tower, Sierra Papa Alpha Bravo Charlie, holding point runway two five, ready for departure",
    );
    assert_eq!(m.addressee.as_deref(), Some("tower"));
    assert_eq!(
        m.intent,
        PilotIntent::ReadyForDeparture {
            runway: Some("25".into())
        }
    );
}

#[test]
fn polish_takeoff_readback() {
    let m = twin(
        "Pas dwa pięć, zezwalasz startować, zgłoszę z wiatrem, Sierra Bravo Charlie",
        "Runway two five, cleared for take-off, report downwind, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.kinds, vec![ClearanceKind::Takeoff]);
    assert_eq!(m.items.runways, vec!["25"]);
    assert_eq!(m.intent, PilotIntent::Readback);
}

#[test]
fn polish_landing_readback() {
    let m = twin(
        "Pas dwa pięć, zezwalam lądować, Sierra Bravo Charlie",
        "Runway two five, cleared to land, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.kinds, vec![ClearanceKind::Land]);
}

#[test]
fn polish_downwind_full_stop() {
    let m = twin(
        "Sierra Bravo Charlie, z wiatrem, z pełnym zatrzymaniem",
        "Sierra Bravo Charlie, downwind, full stop",
    );
    assert_eq!(m.intent, PilotIntent::ReportLeg(CircuitLeg::Downwind));
    let m = twin(
        "Sierra Bravo Charlie, do pełnego zatrzymania",
        "Sierra Bravo Charlie, full stop",
    );
    assert_eq!(m.intent, PilotIntent::RequestFullStop);
}

#[test]
fn polish_final() {
    let m = twin(
        "Sierra Bravo Charlie, prosta",
        "Sierra Bravo Charlie, final",
    );
    assert_eq!(m.intent, PilotIntent::ReportLeg(CircuitLeg::Final));
    let m = twin("Sierra Bravo Charlie, baza", "Sierra Bravo Charlie, base");
    assert_eq!(m.intent, PilotIntent::ReportLeg(CircuitLeg::Base));
}

#[test]
fn wind_is_not_downwind() {
    let m = pl("Sierra Bravo Charlie, wiatr dwieście stopni dwadzieścia pięć węzłów");
    assert_eq!(m.intent, PilotIntent::Unknown);
}

#[test]
fn polish_contact_readback() {
    let m = twin(
        "Łączność z Kraków Ground sto osiemnaście kropka jeden zero pięć, Sierra Bravo Charlie",
        "Contact Kraków Ground one one eight decimal one zero five, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.freqs_khz, vec![118105]);
    assert_eq!(m.items.kinds, vec![ClearanceKind::Contact]);
}

#[test]
fn polish_taxi_to_stand() {
    let m = twin(
        "Kraków Ground, Sierra Bravo Charlie, proszę o kołowanie do stanowiska pięć",
        "Kraków Ground, Sierra Bravo Charlie, request taxi to stand five",
    );
    assert_eq!(
        m.intent,
        PilotIntent::RequestTaxiToStand {
            stand: Some("5".into())
        }
    );
}

#[test]
fn polish_say_again() {
    let m = twin(
        "Kraków Ground, Sierra Bravo Charlie, proszę powtórzyć",
        "Kraków Ground, Sierra Bravo Charlie, say again",
    );
    assert_eq!(m.intent, PilotIntent::SayAgain);
    assert_eq!(
        pl("Powtórz, Sierra Bravo Charlie").intent,
        PilotIntent::SayAgain
    );
}

#[test]
fn polish_on_stand() {
    let m = twin(
        "Kraków Ground, Sierra Bravo Charlie, na stanowisku",
        "Kraków Ground, Sierra Bravo Charlie, on stand",
    );
    assert_eq!(m.intent, PilotIntent::ReportOnStand);
}

#[test]
fn polish_lost() {
    for text in [
        "Kraków Ground, Sierra Bravo Charlie, zgubiłem się",
        "Kraków Ground, Sierra Bravo Charlie, zgubilam sie",
        "Kraków Ground, Sierra Bravo Charlie, proszę o pomoc",
        "Kraków Ground, Sierra Bravo Charlie, nie mogę znaleźć drogi",
    ] {
        let m = twin(
            text,
            "Kraków Ground, Sierra Bravo Charlie, request progressive taxi",
        );
        assert_eq!(m.intent, PilotIntent::RequestTaxiHelp, "{text}");
    }
}

#[test]
fn polish_follow_me() {
    let m = twin(
        "Kraków Ground, Sierra Bravo Charlie, proszę o follow me",
        "Kraków Ground, Sierra Bravo Charlie, request follow me",
    );
    assert_eq!(m.intent, PilotIntent::RequestFollowMe);
}

#[test]
fn polish_hold_position() {
    let m = twin(
        "Stoję, Sierra Bravo Charlie",
        "Holding position, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.kinds, vec![ClearanceKind::HoldPosition]);
}

#[test]
fn polish_digits_and_decimal_comma() {
    let m = twin(
        "Łączność z Kraków Wieża 123,255, Sierra Bravo Charlie",
        "Contact Kraków Tower 123.255, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.freqs_khz, vec![123255]);
    assert_eq!(pl("QNH 1013, Sierra Bravo Charlie").items.qnh, vec![1013]);
    assert_eq!(
        pl("Skłok 4201, Sierra Bravo Charlie").items.squawk,
        vec![4201]
    );
    assert_eq!(
        pl("Pas 2 5, Sierra Bravo Charlie").items.runways,
        vec!["25"]
    );
}

#[test]
fn polish_cardinals() {
    assert_eq!(pl("QNH dziesięć zero osiem").items.qnh, vec![1008]);
    assert_eq!(pl("QNH tysiąc").items.qnh, vec![1000]);
    assert_eq!(pl("skłok siedem tysięcy").items.squawk, vec![7000]);
    assert_eq!(pl("skłok trzy tysiące czterysta").items.squawk, vec![3400]);
    assert_eq!(
        pl("łączność z Kraków Wieża sto dwadzieścia trzy kropka dwa pięć pięć")
            .items
            .freqs_khz,
        vec![123255]
    );
    assert_eq!(
        pl("łączność z Kraków Ground sto osiemnaście kropka jeden")
            .items
            .freqs_khz,
        vec![118100]
    );
}

#[test]
fn mixed_polish_english() {
    let m = twin(
        "Kraków Ground, SP-ABC, holding point pas dwa pięć, ready for departure",
        "Kraków Ground, SP-ABC, holding point runway two five, ready for departure",
    );
    assert_eq!(
        m.intent,
        PilotIntent::ReadyForDeparture {
            runway: Some("25".into())
        }
    );
}

#[test]
fn polish_courtesy_words() {
    let m = pl("Przyjąłem, Sierra Bravo Charlie");
    assert!(m.ack);
    assert_eq!(m.ack_words, vec!["przyjąłem"]);
    assert!(m.greeting_words.is_empty());
    assert!(m.residual.is_empty());

    let m = pl("Kraków Wieża, Sierra Bravo Charlie, dzień dobry");
    assert_eq!(m.greeting, Some(Courtesy::GoodMorning));
    assert_eq!(m.greeting_words, vec!["dzień", "dobry"]);
    assert!(m.ack_words.is_empty());
    // The station name as the engine knows it ("Kraków Tower").
    assert_eq!(m.residual, vec!["kraków", "tower"]);

    let m = pl("Sierra Bravo Charlie, dziękuję, do widzenia");
    assert_eq!(m.farewell, Some(Courtesy::GoodDay));
    assert_eq!(m.ack_words, vec!["dziękuję", "do", "widzenia"]);
    assert!(m.residual.is_empty());

    let m = pl("Sierra Bravo Charlie, miłego dnia");
    assert_eq!(m.farewell, Some(Courtesy::ThankYouYouToo));
    assert_eq!(m.ack_words, vec!["miłego", "dnia"]);

    assert_eq!(
        pl("Cześć, Sierra Bravo Charlie").greeting,
        Some(Courtesy::Hello)
    );
}

#[test]
fn unknown_polish_word_is_residual() {
    let m = pl("Sierra Bravo Charlie, przyjąłem, mamy problem z radiem");
    assert_eq!(m.ack_words, vec!["przyjąłem"]);
    assert_eq!(m.residual, vec!["mamy", "problem", "z", "radiem"]);
    assert_eq!(m.intent, PilotIntent::Unknown);

    // Words given a meaning stay residual as said, number words are not words.
    let m = pl("Pas dwa pięć, zezwalasz startować, Sierra Bravo Charlie");
    assert_eq!(m.residual, vec!["pas", "zezwalasz", "startować"]);
}

#[test]
fn polish_cross_readback() {
    let m = twin(
        "Przecinam pas dwa pięć, Sierra Bravo Charlie",
        "Crossing runway two five, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.kinds, vec![ClearanceKind::Cross]);
    assert_eq!(m.items.runways, vec!["25"]);
    let m = twin(
        "Przetnij pas dwa pięć, Sierra Bravo Charlie",
        "Cross runway two five, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.kinds, vec![ClearanceKind::Cross]);
}

#[test]
fn polish_line_up_readback() {
    let m = twin(
        "Pas dwa pięć, zajmuję pas i czekam, Sierra Bravo Charlie",
        "Runway two five, line up and wait, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.kinds, vec![ClearanceKind::LineUp]);
    assert_eq!(m.items.runways, vec!["25"]);
    let m = twin(
        "Zajmij pas dwa pięć i czekaj, Sierra Bravo Charlie",
        "Line up runway two five and wait, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.kinds, vec![ClearanceKind::LineUp]);
    assert_eq!(m.items.runways, vec!["25"]);
}

#[test]
fn polish_go_around() {
    for text in [
        "Przechodzę na drugi krąg, Sierra Bravo Charlie",
        "Przejdź na drugi krąg, Sierra Bravo Charlie",
        "Sierra Bravo Charlie, na drugi krag",
    ] {
        let m = twin(text, "Going around, Sierra Bravo Charlie");
        assert_eq!(m.items.kinds, vec![ClearanceKind::GoAround], "{text}");
    }
}

#[test]
fn polish_touch_and_go_readback() {
    let m = twin(
        "Pas dwa pięć, zezwalam konwojer, Sierra Bravo Charlie",
        "Runway two five, cleared touch and go, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.kinds, vec![ClearanceKind::TouchAndGo]);
}

/// Every instruction the Polish controller gives that is read back as a clearance kind is
/// understood when the pilot repeats it.
#[test]
fn polish_render_kinds_read_back() {
    let r = "25".to_string();
    let wind = Wind {
        dir_deg: 250,
        speed_kt: 8,
        calm: false,
    };
    let cases = [
        (AtcItem::CrossRunway(r.clone()), ClearanceKind::Cross),
        (AtcItem::LineUpAndWait(r.clone()), ClearanceKind::LineUp),
        (
            AtcItem::ClearedTakeoff {
                runway: r.clone(),
                wind,
            },
            ClearanceKind::Takeoff,
        ),
        (
            AtcItem::ClearedLand {
                runway: r.clone(),
                wind,
            },
            ClearanceKind::Land,
        ),
        (
            AtcItem::ClearedTouchAndGo {
                runway: r.clone(),
                wind,
            },
            ClearanceKind::TouchAndGo,
        ),
        (AtcItem::HoldPosition, ClearanceKind::HoldPosition),
        (AtcItem::GoAround, ClearanceKind::GoAround),
    ];
    for (item, kind) in cases {
        let said = render(
            &AtcMessage {
                to: None,
                from: "Kraków Tower".into(),
                items: vec![item],
                lang: Lang::Pl,
            },
            false,
        );
        let m = pl(&format!("{said}, Sierra Bravo Charlie"));
        assert!(m.items.kinds.contains(&kind), "{said:?} -> {:?}", m.items);
    }
}

fn vocab() -> Vocabulary {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    Vocabulary {
        callsigns: vec![],
        points: s(&["ZULU", "KILO"]),
        fixes: s(&["SUDOL"]),
        places: vec![
            ("katowice".into(), "EPKT".into()),
            ("krakow".into(), "EPKK".into()),
        ],
    }
}

fn pl_with(text: &str) -> PilotMessage {
    parse_pilot_with(text, Lang::Pl, &vocab())
}

#[test]
fn polish_destination_inflection_and_clearance() {
    let m = pl_with("Delivery, Sierra Bravo Charlie, IFR do Katowic, proszę o zezwolenie");
    assert_eq!(
        m.intent,
        PilotIntent::RequestClearance {
            destination: Some("EPKT".into()),
            rules: Some(FlightRules::Ifr),
            level_ft: None
        }
    );
    for form in ["Katowice", "Katowic", "Katowicach"] {
        let m = pl_with(&format!("do {form}"));
        assert_eq!(m.items.destination.as_deref(), Some("EPKT"), "{form}");
    }
    // A different word with the same start is no match.
    assert_eq!(pl_with("do Katowa").items.destination, None);
}

#[test]
fn polish_procedures_and_levels() {
    let m = pl_with(
        "zezwolenie do Katowic, Sudol dwa Juliett odlot, wznoszenie na wysokość pięć tysięcy stóp, kod cztery dwa zero jeden",
    );
    assert_eq!(m.items.destination.as_deref(), Some("EPKT"));
    assert_eq!(m.items.sids, vec![("SUDOL".to_string(), 2, 'J')]);
    assert_eq!(m.items.altitudes_ft, vec![5000]);
    assert_eq!(m.items.squawk, vec![4201]);
    assert_eq!(
        pl_with("Sudol dwa Juliett").items.sids,
        vec![("SUDOL".to_string(), 2, 'J')]
    );
    let fl = pl_with("poziom lotu osiem zero, bezpośrednio Sudol");
    assert_eq!(fl.items.flight_levels, vec![80]);
    assert_eq!(fl.items.directs, vec!["SUDOL"]);
}

#[test]
fn polish_requests_and_points() {
    let j = pl_with("Wieża, Sierra Bravo Charlie, nad Zulu, dwa tysiące stóp, proszę o wejście");
    assert_eq!(
        j.intent,
        PilotIntent::RequestJoin {
            point: Some("ZULU".into())
        }
    );
    assert_eq!(
        pl_with("Sierra Bravo Charlie, proszę o wektory").intent,
        PilotIntent::RequestApproach {
            kind: None,
            full_procedure: false
        }
    );
    assert_eq!(
        pl_with("Sierra Bravo Charlie, proszę o uruchomienie").intent,
        PilotIntent::RequestStartUp { push: false }
    );
    assert_eq!(
        pl_with("Sierra Bravo Charlie, ustalony ILS pas dwa pięć").intent,
        PilotIntent::ReportEstablished
    );
}

#[test]
fn polish_clearance_request_without_rules() {
    let m = pl_with("Delivery, Sierra Bravo Charlie, do Katowic, proszę o zezwolenie");
    assert!(matches!(
        m.intent,
        PilotIntent::RequestClearance { rules: None, .. }
    ));
}

#[test]
fn polish_request_departure_runway() {
    let m = twin(
        "Sierra Papa Alfa Bravo Charlie, prośba o pas zero siedem do startu",
        "Sierra Papa Alpha Bravo Charlie, request runway zero seven for departure",
    );
    assert_eq!(
        m.intent,
        PilotIntent::RequestDepartureRunway {
            runway: "07".into()
        }
    );
}
