use atc_phraseology::*;
use std::sync::Arc;

fn en(text: &str) -> PilotMessage {
    parse_pilot(text, Lang::En)
}

#[test]
fn request_taxi_with_atis() {
    let m = en(
        "Kraków Ground, Sierra Papa Alpha Bravo Charlie, Cessna one seven two at stand five, information Alpha, request taxi for VFR circuits",
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
    assert!(m.items.kinds.is_empty());
}

#[test]
fn request_taxi_without_atis_or_circuits() {
    let m = en("Ground, Sierra Papa Alpha Bravo Charlie, request taxi");
    assert_eq!(
        m.intent,
        PilotIntent::RequestTaxi {
            atis: None,
            vfr_circuits: false
        }
    );
}

#[test]
fn request_taxi_to_stand() {
    let m = en("Ground, Sierra Papa Alpha Bravo Charlie, request taxi to stand five");
    assert_eq!(
        m.intent,
        PilotIntent::RequestTaxiToStand {
            stand: Some("5".into())
        }
    );
}

#[test]
fn readback_taxi() {
    let m = en(
        "taxi holding point runway two five via alpha bravo hold short runway two five QNH one zero one three Sierra Bravo Charlie",
    );
    assert_eq!(m.intent, PilotIntent::Readback);
    assert_eq!(m.items.runways, vec!["25"]);
    assert_eq!(m.items.hold_short, vec!["25"]);
    assert_eq!(m.items.taxiways, vec!["A", "B"]);
    assert_eq!(m.items.qnh, vec![1013]);
    assert_eq!(m.items.kinds, vec![ClearanceKind::Taxi]);
    assert_eq!(m.callsign.as_deref(), Some("SBC"));
}

#[test]
fn hold_short_does_not_double_count_runway() {
    let m = en("hold short runway two five");
    assert!(m.items.runways.is_empty());
    assert_eq!(m.items.hold_short, vec!["25"]);
}

#[test]
fn readback_takeoff() {
    let m = en("cleared for takeoff runway two five Sierra Bravo Charlie");
    assert!(m.items.kinds.contains(&ClearanceKind::Takeoff));
    assert_eq!(m.items.runways, vec!["25"]);
    assert_eq!(m.intent, PilotIntent::Readback);
}

#[test]
fn clearance_kinds() {
    assert_eq!(
        en("cleared to land runway 07").items.kinds,
        vec![ClearanceKind::Land]
    );
    assert_eq!(en("cleared to land runway 07").items.runways, vec!["07"]);
    assert_eq!(
        en("cleared touch and go").items.kinds,
        vec![ClearanceKind::TouchAndGo]
    );
    assert_eq!(
        en("line up and wait runway two five").items.kinds,
        vec![ClearanceKind::LineUp]
    );
    assert_eq!(
        en("cross runway two five").items.kinds,
        vec![ClearanceKind::Cross]
    );
    assert_eq!(
        en("holding position").items.kinds,
        vec![ClearanceKind::HoldPosition]
    );
    assert_eq!(
        en("going around").items.kinds,
        vec![ClearanceKind::GoAround]
    );
    assert_eq!(
        en("contact tower").items.kinds,
        vec![ClearanceKind::Contact]
    );
    assert_eq!(
        en("cleared for take off").items.kinds,
        vec![ClearanceKind::Takeoff]
    );
}

#[test]
fn report_downwind() {
    let m = en("Kraków Tower, Sierra Papa Alpha Bravo Charlie, downwind runway two five");
    assert_eq!(m.addressee.as_deref(), Some("tower"));
    assert_eq!(m.callsign.as_deref(), Some("SPABC"));
    assert_eq!(m.intent, PilotIntent::ReportLeg(CircuitLeg::Downwind));
    assert_eq!(m.items.runways, vec!["25"]);
}

#[test]
fn ready_for_departure_and_full_stop() {
    let m = en(
        "Tower, Sierra Papa Alpha Bravo Charlie, holding short runway two five, ready for departure",
    );
    assert_eq!(
        m.intent,
        PilotIntent::ReadyForDeparture {
            runway: Some("25".into())
        }
    );
    let m = en("Tower, Sierra Papa Alpha Bravo Charlie, request full stop");
    assert_eq!(m.intent, PilotIntent::RequestFullStop);
}

#[test]
fn frequency_decimal() {
    assert_eq!(
        en("one one eight decimal one").items.freqs_khz,
        vec![118100]
    );
    assert_eq!(en("contact tower 118.125").items.freqs_khz, vec![118125]);
    assert_eq!(
        en("contact one two one decimal five").items.freqs_khz,
        vec![121500]
    );
    assert_eq!(en("frequency 118100").items.freqs_khz, vec![118100]);
}

#[test]
fn numeric_items() {
    let m = en(
        "climb two thousand five hundred feet heading zero niner zero squawk four two one seven",
    );
    assert_eq!(m.items.altitudes_ft, vec![2500]);
    assert_eq!(m.items.headings, vec![90]);
    assert_eq!(m.items.squawk, vec![4217]);
    assert_eq!(en("altitude one thousand").items.altitudes_ft, vec![1000]);
    assert_eq!(en("QNH 1013").items.qnh, vec![1013]);
}

#[test]
fn parse_variants_equivalent() {
    let a = en("Runway 25").items;
    for v in ["runway two five", "runway two-five", "RUNWAY TWO FIVE."] {
        assert_eq!(en(v).items, a, "{v}");
    }
    assert_eq!(a.runways, vec!["25"]);
    assert_eq!(en("runway seven").items.runways, vec!["07"]);
    assert_eq!(en("runway 7").items.runways, vec!["07"]);
    assert_eq!(en("runway two five left").items.runways, vec!["25L"]);
    assert_eq!(en("runway 25R").items.runways, vec!["25R"]);
    assert_eq!(en("squawk niner niner niner zero").items.squawk, vec![9990]);
    assert_eq!(en("QNH one zero one tree").items.qnh, vec![1013]);
}

#[test]
fn taxiway_with_digit() {
    assert_eq!(
        en("taxi via bravo two alpha").items.taxiways,
        vec!["B2", "A"]
    );
}

#[test]
fn callsign_matching() {
    let c = Callsign("SPABC".into());
    assert_eq!(c.abbreviated(), "SBC");
    assert!(c.matches("SPABC"));
    assert!(c.matches("SBC"));
    assert!(c.matches("SP-ABC"));
    assert!(c.matches("s b c"));
    assert!(c.matches("Sierra Bravo Charlie"));
    assert!(c.matches("Sierra Papa Alpha Bravo Charlie"));
    assert!(!c.matches("SPABD"));
    assert!(!c.matches(""));
    let heard = en("Sierra Bravo Charlie").callsign.unwrap();
    assert!(c.matches(&heard));
    assert_eq!(Callsign::new("sp-abc"), c);
}

#[test]
fn garbage_is_unknown() {
    let m = en("");
    assert_eq!(m.intent, PilotIntent::Unknown);
    assert_eq!(m.callsign, None);
    assert_eq!(m.items, HeardItems::default());
    assert_eq!(en("hello there").intent, PilotIntent::Unknown);
}

#[test]
fn break_stops_taxiways_and_callsign() {
    let m = en("taxi via alpha bravo, Sierra Bravo Charlie");
    assert_eq!(m.items.taxiways, vec!["A", "B"]);
    assert_eq!(m.callsign.as_deref(), Some("SBC"));
}

#[test]
fn literal_callsigns() {
    for text in [
        "Kraków Ground, SP-ABC, request taxi",
        "Kraków Ground, sp-abc, request taxi",
        "Kraków Ground, SPABC, request taxi",
    ] {
        let m = en(text);
        assert_eq!(m.callsign.as_deref(), Some("SPABC"), "{text}");
        assert!(
            matches!(m.intent, PilotIntent::RequestTaxi { .. }),
            "{text}"
        );
    }
    let m = en(
        "taxi holding point runway two five via alpha bravo hold short runway two five QNH one zero one three, SP-ABC",
    );
    assert_eq!(m.callsign.as_deref(), Some("SPABC"));
    assert_eq!(m.items.qnh, vec![1013]);
    assert_eq!(m.items.taxiways, vec!["A", "B"]);
    assert_eq!(en("runway 25, Cessna 172").callsign, None);
}

#[test]
fn thousands_separators_are_not_breaks() {
    assert_eq!(en("climb 2,500 feet").items.altitudes_ft, vec![2500]);
    assert_eq!(en("QNH 1,013").items.qnh, vec![1013]);
    // A lone designator after a comma continues the list (Whisper punctuation).
    let m = en("via alpha, bravo");
    assert_eq!(m.items.taxiways, vec!["A", "B"]);
}

#[test]
fn commas_inside_taxiway_list() {
    let m = en(
        "taxi holding point runway two five via Bravo five, Bravo six, Alpha, hold short runway two five, QNH one zero one three, Sierra Bravo Charlie",
    );
    assert_eq!(m.items.taxiways, vec!["B5", "B6", "A"]);
    assert_eq!(m.items.hold_short, vec!["25"]);
    assert_eq!(m.items.qnh, vec![1013]);
    assert_eq!(m.callsign.as_deref(), Some("SBC"));
    let m = en("taxi via Bravo five, Alpha, Sierra Bravo Charlie");
    assert_eq!(m.items.taxiways, vec!["B5", "A"]);
    assert_eq!(m.callsign.as_deref(), Some("SBC"));
}

/// Whisper puts commas at every pause; values after a keyword continue across them.
#[test]
fn readback_with_whisper_pause_commas() {
    let m = en(
        "Sierra Papa Alpha Bravo Charlie, taxi to holding point, runway two five, via Sierra, Zulu six, Alpha, hold short, runway two five, QNH, one zero, one three, Sierra Papa Alpha Bravo Charlie",
    );
    assert_eq!(m.callsign.as_deref(), Some("SPABC"));
    assert_eq!(m.items.runways, vec!["25".to_string()]);
    assert_eq!(m.items.hold_short, vec!["25".to_string()]);
    assert_eq!(m.items.taxiways, vec!["S", "Z6", "A"]);
    assert_eq!(m.items.qnh, vec![1013]);
}

#[test]
fn qnh_and_squawk_digits_continue_across_commas() {
    let m = en(
        "Sierra Papa Alpha Bravo Charlie, QNH one zero, one three, squawk seven, zero zero zero",
    );
    assert_eq!(m.items.qnh, vec![1013]);
    assert_eq!(m.items.squawk, vec![7000]);
}

/// Common Whisper mishearings of "via" and "Zulu" (heard in X-Plane acceptance).
#[test]
fn misheard_via_and_zulu() {
    let m = en(
        "Sierra Papa Alpha Bravo Charlie, taxi to holding point runway two five, FIW Sierra Zolo six, Alpha",
    );
    assert_eq!(m.items.taxiways, vec!["S", "Z6", "A"]);
}

/// Whisper splits a spoken callsign at a pause: "Sierra Papa Alpha, Bravo Charlie".
#[test]
fn callsign_continues_across_pause_comma() {
    let m = en(
        "Kraków Ground, taxi to holding point, runway two five, via Sierra, Zulu 6 Alpha, hold short, runway two five, QNH, one zero, one three, Sierra Papa Alpha, Bravo Charlie",
    );
    assert_eq!(m.callsign.as_deref(), Some("SPABC"));
    assert_eq!(m.items.taxiways, vec!["S", "Z6", "A"]);
    assert_eq!(m.items.hold_short, vec!["25".to_string()]);
    assert_eq!(m.items.qnh, vec![1013]);
}

/// Non-standard but clear take-off requests heard in an X-Plane run.
#[test]
fn request_takeoff_is_ready_for_departure() {
    for t in [
        "request permission to take-off Sierra Papa Alpha Bravo Charlie",
        "Hello, request permission to take off Sierra Papa Alpha Bravo Charlie",
        "Sierra Papa Alpha Bravo Charlie, request departure",
    ] {
        assert!(
            matches!(en(t).intent, PilotIntent::ReadyForDeparture { .. }),
            "{t}"
        );
    }
}

/// With the known aircraft, the letters ending the message are its callsign when they end
/// in its full or abbreviated callsign; the letters before go back to the route.
fn known(text: &str) -> PilotMessage {
    parse_pilot_known(text, Lang::En, &[Callsign::new("SP-ABC")])
}

/// Real readbacks of "via Delta, Bravo three, Bravo four, Bravo five, Whiskey one, Sierra":
/// the last taxiway runs into the callsign, Whisper puts commas anywhere, also inside it.
#[test]
fn trailing_callsign_after_a_letter_taxiway() {
    const ROUTE: &str =
        "taxi to stand two zero via Delta, Bravo three, Bravo four, Bravo five, Whiskey one";
    for cs in [
        "Sierra Sierra Bravo Charlie",
        "Sierra Sierra, Bravo Charlie",
        // A duplicated last taxiway: a stutter, never a route.
        "Sierra Sierra, Sierra, Bravo Charlie",
    ] {
        let m = known(&format!("{ROUTE}, {cs}"));
        assert_eq!(m.callsign.as_deref(), Some("SBC"), "{cs}");
        assert_eq!(
            m.items.taxiways,
            vec!["D", "B3", "B4", "B5", "W1", "S"],
            "{cs}"
        );
        assert_eq!(m.intent, PilotIntent::Readback, "{cs}");
    }
}

#[test]
fn known_callsign_in_full_or_in_front() {
    let m = known("taxi to stand two zero via Sierra, Sierra Papa Alpha Bravo Charlie.");
    assert_eq!(m.callsign.as_deref(), Some("SPABC"));
    assert_eq!(m.items.taxiways, vec!["S"]);
    let m = known("Sierra Bravo Charlie, taxi to stand two zero via Alpha, Sierra");
    assert_eq!(m.callsign.as_deref(), Some("SBC"));
    assert_eq!(m.items.taxiways, vec!["A", "S"]);
    // An unknown callsign is found as before.
    let m = known("Kraków Ground, Sierra Papa Tango Sierra Tango, request taxi");
    assert_eq!(m.callsign.as_deref(), Some("SPTST"));
}

/// "requests", "requesting", "request" all make a request (real: "Kraków Ground's ...").
#[test]
fn request_verb_forms() {
    let m = known("Kraków Ground's Sierra Bravo Charlie requests taxi to stand.");
    assert_eq!(m.addressee.as_deref(), Some("ground"));
    assert_eq!(m.callsign.as_deref(), Some("SBC"));
    assert_eq!(m.intent, PilotIntent::RequestTaxiToStand { stand: None });
    for verb in ["request", "requests", "requesting", "requested"] {
        let m = en(&format!("Ground, Sierra Bravo Charlie, {verb} taxi"));
        assert!(
            matches!(m.intent, PilotIntent::RequestTaxi { .. }),
            "{verb}"
        );
        let m = en(&format!("Tower, Sierra Bravo Charlie, {verb} departure"));
        assert!(
            matches!(m.intent, PilotIntent::ReadyForDeparture { .. }),
            "{verb}"
        );
        let m = en(&format!("Tower, Sierra Bravo Charlie, {verb} full stop"));
        assert_eq!(m.intent, PilotIntent::RequestFullStop, "{verb}");
    }
    for leg in ["reporting downwind", "Sierra Bravo Charlie, downwind"] {
        assert_eq!(
            en(leg).intent,
            PilotIntent::ReportLeg(CircuitLeg::Downwind),
            "{leg}"
        );
    }
}

#[test]
fn say_again_requests() {
    for t in [
        "Say again please",
        "say again",
        "Sierra Bravo Charlie, say again",
        "repeat",
        "Could you repeat, Sierra Bravo Charlie",
        "say again all after taxi, Sierra Bravo Charlie",
    ] {
        assert_eq!(en(t).intent, PilotIntent::SayAgain, "{t}");
    }
}

#[test]
fn taxi_help_requests() {
    for t in [
        "Kraków Ground, could you get me how to taxi to the Sierra Sierra Bravo Charlie",
        "Ground, Sierra Bravo Charlie, request progressive taxi",
        "Ground, Sierra Bravo Charlie, request progressive taxi instructions",
        "Sierra Bravo Charlie, request taxi instructions",
        "Sierra Bravo Charlie, unable to find the holding point",
        "Sierra Bravo Charlie, we are lost",
        "Sierra Bravo Charlie, say route",
        "Sierra Bravo Charlie, request help",
        "Sierra Bravo Charlie, how do I get to the holding point",
    ] {
        assert_eq!(known(t).intent, PilotIntent::RequestTaxiHelp, "{t}");
    }
    // A taxi request is still one.
    assert!(matches!(
        en("Ground, Sierra Bravo Charlie, request taxi").intent,
        PilotIntent::RequestTaxi { .. }
    ));
}

/// Real transcripts of the pilot SP-ABC.
#[test]
fn on_stand_reports() {
    for t in [
        "Sierra Papa Alpha Bravo Charlie on stand, have a good day.",
        "Sierra Papa Alpha Bravo Charlie, on stand",
        "Sierra Papa Alpha Bravo Charlie on stand, good day",
        "Ground, Sierra Bravo Charlie, on the stand",
    ] {
        let m = known(t);
        assert_eq!(m.intent, PilotIntent::ReportOnStand, "{t}");
        assert!(m.callsign.is_some(), "{t}");
    }
    // A taxi request to a stand is still one.
    assert!(matches!(
        known("Ground, Sierra Bravo Charlie, request taxi to stand two zero").intent,
        PilotIntent::RequestTaxiToStand { .. }
    ));
}

#[test]
fn follow_me_requests() {
    for t in [
        "Ground, Sierra Bravo Charlie, request follow me",
        "Sierra Bravo Charlie, request follow-me",
        "Sierra Bravo Charlie, I am lost, I need a follow me car",
        "Sierra Bravo Charlie, request marshaller",
    ] {
        assert_eq!(known(t).intent, PilotIntent::RequestFollowMe, "{t}");
    }
    // The readback of "follow the follow-me car" asks for nothing.
    for t in [
        "Follow the follow-me, Sierra Bravo Charlie",
        "Following the follow me car, Sierra Bravo Charlie",
    ] {
        assert_eq!(known(t).intent, PilotIntent::Unknown, "{t}");
    }
}

#[test]
fn on_the_way_readback_is_not_a_request() {
    for t in [
        "Hold position, follow-me car is on the way, Sierra Bravo Charlie",
        "Roger, hold position, follow me car on the way, SBC",
        "Hold position, follow me car will be on the way, Sierra Bravo Charlie",
        "Hold position, follow-me car on its way, Sierra Bravo Charlie",
    ] {
        assert_ne!(known(t).intent, PilotIntent::RequestFollowMe, "{t}");
    }
}

#[test]
fn request_follow_me_still_parsed() {
    for t in [
        "Sierra Bravo Charlie, request follow me car",
        "Sierra Bravo Charlie, request taxi to stand with follow me car",
        "Sierra Bravo Charlie, request follow me on the way to stand five",
    ] {
        assert_eq!(known(t).intent, PilotIntent::RequestFollowMe, "{t}");
    }
}

/// Whisper's "Dunn, wind" and "#1".
#[test]
fn misheard_downwind_and_number() {
    let m = known("Sierra Papa Alpha Bravo Charlie Dunn, wind, full stop");
    assert_eq!(m.intent, PilotIntent::ReportLeg(CircuitLeg::Downwind));
    assert_eq!(m.callsign.as_deref(), Some("SPABC"));
    let m = known("#1, report final, Sierra Bravo Charlie");
    assert_eq!(m.raw, "#1, report final, Sierra Bravo Charlie");
    assert_eq!(m.callsign.as_deref(), Some("SBC"));
}

#[test]
fn courtesy_fields() {
    let m = en("Kraków Ground, good morning, Sierra Bravo Charlie, request taxi");
    assert_eq!(m.greeting, Some(Courtesy::GoodMorning));
    assert_eq!(m.farewell, None);
    let m = en("Kraków Ground, Sierra Bravo Charlie, have a nice day");
    assert_eq!(m.farewell, Some(Courtesy::ThankYouYouToo));
    let m = en("Kraków Ground, Sierra Bravo Charlie, good day");
    assert_eq!(m.farewell, Some(Courtesy::GoodDay));
    assert_eq!(m.greeting, None);
    let m = en("Sierra Papa Alpha Bravo Charlie, understand");
    assert!(m.ack);
    assert!(!en("Sierra Papa Alpha Bravo Charlie, request taxi").ack);
}

#[test]
fn courtesy_words_are_sorted() {
    let m = en("Kraków Ground, good morning, Sierra Bravo Charlie, roger, thanks");
    assert_eq!(m.greeting_words, ["good", "morning"]);
    assert_eq!(m.ack_words, ["roger", "thanks"]);
    assert_eq!(m.residual, ["kraków", "ground"]);
    assert!(en("Sierra Bravo Charlie").residual.is_empty());
}

fn vocab() -> Vocabulary {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    Vocabulary {
        callsigns: vec![],
        points: s(&["ZULU", "KILO"]),
        fixes: Arc::new(s(&["SUDOL", "NUMBA", "MAPIK"]).into_iter().collect()),
        places: vec![
            Place::new("katowice", "EPKT"),
            Place::new("warszawa", "EPWA"),
        ]
        .into(),
        place_fixes: Default::default(),
        origin: None,
    }
}

fn with(text: &str) -> PilotMessage {
    parse_pilot_with(text, Lang::En, &vocab())
}

#[test]
fn ifr_clearance_request_with_place_name() {
    let m = with(
        "Krakow Delivery, Sierra Papa Alpha Bravo Charlie, stand five, information Alpha, IFR to Katowice, request clearance",
    );
    assert_eq!(m.callsign.as_deref(), Some("SPABC"));
    assert_eq!(
        m.intent,
        PilotIntent::RequestClearance {
            destination: Some("EPKT".into()),
            rules: Some(FlightRules::Ifr),
            level_ft: None
        }
    );
}

#[test]
fn clearance_request_with_spelled_icao_and_level() {
    let m = with(
        "Delivery, Sierra Bravo Charlie, request IFR clearance to Echo Papa Whiskey Alpha, flight level one zero zero",
    );
    assert_eq!(
        m.intent,
        PilotIntent::RequestClearance {
            destination: Some("EPWA".into()),
            rules: Some(FlightRules::Ifr),
            level_ft: Some(10_000)
        }
    );
    let none = with("Delivery, Sierra Bravo Charlie, request clearance");
    assert_eq!(
        none.intent,
        PilotIntent::RequestClearance {
            destination: None,
            rules: None,
            level_ft: None
        }
    );
}

#[test]
fn clearance_readback_items() {
    let m = with(
        "cleared to Katowice, Sudol two J departure, climb five thousand, squawk four two zero one, Sierra Bravo Charlie",
    );
    assert_eq!(m.intent, PilotIntent::Readback);
    assert_eq!(m.items.destination.as_deref(), Some("EPKT"));
    assert_eq!(m.items.sids, vec![("SUDOL".to_string(), 2, 'J')]);
    assert_eq!(m.items.altitudes_ft, vec![5000]);
    assert_eq!(m.items.squawk, vec![4201]);
    assert_eq!(m.callsign.as_deref(), Some("SBC"));
}

#[test]
fn designator_spellings() {
    let sid = vec![("SUDOL".to_string(), 2, 'J')];
    for text in [
        "SUDOL 2 Juliett",
        "Sudol two Juliett",
        "sudol 2 J",
        "Sudol 2J departure",
        "Su dol two Juliett",
        "Sudol, two Juliett",
    ] {
        assert_eq!(with(text).items.sids, sid, "{text}");
    }
    let arr = with("MAPIK one Golf arrival, Sierra Bravo Charlie");
    assert_eq!(arr.items.stars, vec![("MAPIK".to_string(), 1, 'G')]);
    assert!(arr.items.sids.is_empty());
    assert_eq!(arr.callsign.as_deref(), Some("SBC"));
    // A fix without number and letter is no designator; an unknown fix is none either.
    assert!(with("Sudol").items.sids.is_empty());
    assert!(with("Tarnow two Juliett").items.sids.is_empty());
}

#[test]
fn directs_levels_and_approaches() {
    let m = with(
        "proceed direct Sudol, descend flight level eight zero, cleared ILS approach runway two five left",
    );
    assert_eq!(m.items.directs, vec!["SUDOL"]);
    assert_eq!(m.items.flight_levels, vec![80]);
    assert_eq!(
        m.items.approaches,
        vec![(ApproachKind::Ils, "25L".to_string())]
    );
    assert!(m.items.runways.is_empty());
    let v =
        with("cleared VOR DME approach runway zero seven, maintain altitude three thousand feet");
    assert_eq!(
        v.items.approaches,
        vec![(ApproachKind::VorDme, "07".to_string())]
    );
    assert_eq!(v.items.altitudes_ft, vec![3000]);
    let r = with("descend two thousand five hundred, direct to Numba");
    assert_eq!(r.items.altitudes_ft, vec![2500]);
    assert_eq!(r.items.directs, vec!["NUMBA"]);
}

#[test]
fn vfr_points_and_joining() {
    let m = with("Krakow Tower, Sierra Bravo Charlie, at Zulu, two thousand feet, request joining");
    assert_eq!(
        m.intent,
        PilotIntent::RequestJoin {
            point: Some("ZULU".into())
        }
    );
    assert_eq!(m.items.points, vec!["ZULU"]);
    assert_eq!(m.items.altitudes_ft, vec![2000]);
    assert_eq!(m.callsign.as_deref(), Some("SBC"));
    let l = with("Tower, Sierra Bravo Charlie, request leaving the zone via Kilo");
    assert_eq!(
        l.intent,
        PilotIntent::RequestLeave {
            point: Some("KILO".into())
        }
    );
    let i = with(
        "Approach, Sierra Bravo Charlie, inbound from Katowice, at Zulu, one thousand five hundred feet",
    );
    assert_eq!(
        i.intent,
        PilotIntent::ReportInbound {
            point: Some("ZULU".into())
        }
    );
    assert_eq!(i.items.destination, None);
    // A flight from a neighbouring airport calls airborne and inbound: inbound, not a
    // departure report.
    let d = with(
        "Krakow Approach, Sierra Papa Alpha Bravo Charlie, airborne Katowice, passing four \
         thousand feet climbing five thousand feet, inbound Krakow",
    );
    assert_eq!(d.intent, PilotIntent::ReportInbound { point: None });
}

#[test]
fn point_is_a_letter_elsewhere() {
    // Not in the vocabulary: a taxiway.
    let m = parse_pilot_with("taxi via Zulu", Lang::En, &Vocabulary::default());
    assert_eq!(m.items.taxiways, vec!["Z"]);
    assert!(m.items.points.is_empty());
    // In the vocabulary, but the message is about taxiing.
    let m = with("taxi via Zulu five, hold short runway two five");
    assert_eq!(m.items.taxiways, vec!["Z5"]);
    assert!(m.items.points.is_empty());
    // Not after a preposition: part of the callsign.
    let m = with("Ground, Zulu Kilo Alpha, request taxi");
    assert!(m.items.points.is_empty());
    assert_eq!(m.callsign.as_deref(), Some("ZKA"));
}

#[test]
fn approach_requests_and_reports() {
    assert_eq!(
        with("Approach, Sierra Bravo Charlie, request vectors").intent,
        PilotIntent::RequestApproach {
            kind: None,
            full_procedure: false
        }
    );
    assert_eq!(
        with("Approach, Sierra Bravo Charlie, request full procedure").intent,
        PilotIntent::RequestApproach {
            kind: None,
            full_procedure: true
        }
    );
    assert_eq!(
        with("Approach, Sierra Bravo Charlie, request ILS approach runway two five").intent,
        PilotIntent::RequestApproach {
            kind: Some(ApproachKind::Ils),
            full_procedure: false
        }
    );
    assert_eq!(
        with("Approach, Sierra Bravo Charlie, requesting R NAV approach").intent,
        PilotIntent::RequestApproach {
            kind: Some(ApproachKind::Rnav),
            full_procedure: false
        }
    );
    let e = with("Sierra Bravo Charlie, established ILS runway two five");
    assert_eq!(e.intent, PilotIntent::ReportEstablished);
    assert_eq!(
        e.items.approaches,
        vec![(ApproachKind::Ils, "25".to_string())]
    );
    // The controller's "report established" read back is not a report.
    assert_eq!(
        with("report established, Sierra Bravo Charlie").intent,
        PilotIntent::Unknown
    );
}

#[test]
fn start_up_and_departure_reports() {
    assert_eq!(
        with("Delivery, Sierra Bravo Charlie, request start-up").intent,
        PilotIntent::RequestStartUp { push: false }
    );
    assert_eq!(
        with("Delivery, Sierra Bravo Charlie, request push back and start up").intent,
        PilotIntent::RequestStartUp { push: true }
    );
    assert_eq!(
        with("Approach, Sierra Bravo Charlie, passing two thousand five hundred climbing five thousand").intent,
        PilotIntent::ReportDeparture
    );
}

#[test]
fn known_wrapper_matches_with() {
    let known = vec![Callsign::new("SPABC")];
    let text = "Ground, Sierra Bravo Charlie, request taxi";
    let v = Vocabulary {
        callsigns: known.clone(),
        ..Vocabulary::default()
    };
    assert_eq!(
        parse_pilot_known(text, Lang::En, &known),
        parse_pilot_with(text, Lang::En, &v)
    );
}

/// A call opening with the place and the station names that station; a readback or a
/// request naming a station is not a call.
#[test]
fn station_called_only_by_a_call_up() {
    let called = |t: &str| {
        let m = en(t);
        (m.addressee, m.station_called)
    };
    assert_eq!(
        called("Krakow Approach, Sierra Papa Alpha Bravo Charlie, Sudol two Juliett departure"),
        (Some("approach".into()), true)
    );
    assert_eq!(
        called("Zeta Delivery, Sierra Papa Alpha Bravo Charlie, request clearance"),
        (Some("delivery".into()), true)
    );
    assert_eq!(
        called("Krakow Tower, Sierra Papa Alpha Bravo Charlie, ready for departure"),
        (Some("tower".into()), true)
    );
    assert!(!called("Approach one two one decimal zero seven five, Sierra Bravo Charlie").1);
    assert!(
        !called("Contact Krakow Tower one two three decimal two five five, Sierra Bravo Charlie").1
    );
    assert!(!called("Request approach, Sierra Bravo Charlie").1);
}

#[test]
fn clearance_request_without_rules() {
    let m = with("Krakow Delivery, Sierra Papa Tango Sierra Tango, request clearance to Katowice");
    assert_eq!(
        m.intent,
        PilotIntent::RequestClearance {
            destination: Some("EPKT".into()),
            rules: None,
            level_ft: None
        }
    );
    let vfr = with("Delivery, Sierra Bravo Charlie, VFR to Katowice, request clearance");
    assert!(matches!(
        vfr.intent,
        PilotIntent::RequestClearance {
            rules: Some(FlightRules::Vfr),
            ..
        }
    ));
}

#[test]
fn request_departure_runway() {
    for t in [
        "Zeta Delivery, Sierra Papa Alpha Bravo Charlie, request departure runway zero seven",
        "Sierra Papa Alpha Bravo Charlie, request runway zero seven for departure",
    ] {
        assert_eq!(
            en(t).intent,
            PilotIntent::RequestDepartureRunway {
                runway: "07".into()
            },
            "{t}"
        );
    }
    // A full stop or a circuit report naming the runway keeps its meaning.
    assert_eq!(
        en("Sierra Papa Alpha Bravo Charlie, request full stop runway two seven").intent,
        PilotIntent::RequestFullStop
    );
    assert_eq!(
        en("Sierra Papa Alpha Bravo Charlie, downwind runway two seven, request runway two seven")
            .intent,
        PilotIntent::ReportLeg(CircuitLeg::Downwind)
    );
    // Requests to cross, backtrack, touch and go, land on or line up on a runway are not
    // departure runway requests.
    for t in [
        "Sierra Papa Alpha Bravo Charlie, request cross runway two five",
        "Sierra Papa Alpha Bravo Charlie, request crossing runway two five",
        "Sierra Papa Alpha Bravo Charlie, request backtrack runway two five",
        "Sierra Papa Alpha Bravo Charlie, request touch and go runway two five",
        "Sierra Papa Alpha Bravo Charlie, request landing runway two five",
        "Sierra Papa Alpha Bravo Charlie, request line up runway two five",
        "Sierra Papa Alpha Bravo Charlie, request backtrack runway two five for departure",
    ] {
        assert!(
            !matches!(en(t).intent, PilotIntent::RequestDepartureRunway { .. }),
            "{t}: {:?}",
            en(t).intent
        );
    }
    // As before the runway request: heard as a crossing (readback).
    assert_eq!(
        en("Sierra Papa Alpha Bravo Charlie, request cross runway two five").intent,
        PilotIntent::Readback
    );
    // A runway request after a circuit leg word that is no position report is still one.
    assert_eq!(
        en("Sierra Papa Alpha Bravo Charlie, request runway zero seven for departure, final check complete").intent,
        PilotIntent::RequestDepartureRunway {
            runway: "07".into()
        }
    );
    // A take-off request naming the runway stays a departure request.
    assert!(matches!(
        en("Sierra Papa Alpha Bravo Charlie, request takeoff runway two seven").intent,
        PilotIntent::ReadyForDeparture { .. }
    ));
}

fn world() -> Vocabulary {
    Vocabulary {
        places: vec![
            Place {
                pos: Some((51.0, 20.0)),
                ..Place::new("springfield", "ZZFAR")
            },
            Place {
                pos: Some((50.1, 19.9)),
                ..Place::new("springfield", "ZZNEAR")
            },
            Place::new("land", "ZZLA"),
            Place::new("final", "ZZFI"),
        ]
        .into(),
        origin: Some((50.0, 20.0)),
        ..Vocabulary::default()
    }
}

#[test]
fn equal_destination_names_resolve_to_the_nearer_airport() {
    let m = parse_pilot_with(
        "Delivery, Sierra Bravo Charlie, IFR to Springfield, request clearance",
        Lang::En,
        &world(),
    );
    assert_eq!(m.items.destination.as_deref(), Some("ZZNEAR"));
}

#[test]
fn landing_words_are_never_destinations() {
    for said in [
        "Tower, Sierra Bravo Charlie, request to land",
        "Tower, Sierra Bravo Charlie, cleared to land, runway two five",
        "Tower, Sierra Bravo Charlie, to final",
    ] {
        let m = parse_pilot_with(said, Lang::En, &world());
        assert_eq!(m.items.destination, None, "{said}");
    }
}
