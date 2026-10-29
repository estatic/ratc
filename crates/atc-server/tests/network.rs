//! The network dispatcher against the synthetic stations ZZZZ (Zeta) and ZZZY (Testville,
//! 30 NM north): routing by frequency, shared units, hand-offs and the telemetry fan-out.

mod common;

use std::sync::Mutex;

use atc_core::atis::Weather;
use atc_core::engine::AtcOutput;
use atc_core::handover::FlightHandover;
use atc_core::state::{AircraftState, SimTime};
use atc_core::strip::{FlightPlan, PilotProfile};
use atc_geo::LatLon;
use atc_navdata::enroute::{EnrouteKind, EnrouteUnitDef, SectorDef};
use atc_navdata::model::{Destination, FreqKind};
use atc_navdata::procedures::PositionDef;
use atc_phraseology::{AtcItem, Callsign, FlightRules, Lang, Wind};
use atc_server::network::{Network, Owner};
use atc_server::stations::LoadedStation;
use common::fixture_station;

const ZETA_GROUND: u32 = 121_700;
const ZETA_APP: u32 = 119_100;
const TESTVILLE_TWR: u32 = 118_500;
/// Station indices: sorted by id.
const ZZZY: usize = 0;
const ZZZZ: usize = 1;

const ZZZZ_EXTRA: &str = r#"
[[positions]]
role = "Ground"
callsign = "Zeta Ground"
khz = 121700

[[positions]]
role = "Approach"
callsign = "Zeta Approach"
khz = 119100

[[adjacent]]
callsign = "Testville Tower"
khz = 118500
kind = "Tower"
airport = "ZZZY"
"#;

/// Both stations list the same Approach unit.
const SHARED_APP: &str = r#"
[[positions]]
role = "Approach"
callsign = "Shared Approach"
khz = 119100
"#;

fn cs(s: &str) -> Callsign {
    Callsign::new(s)
}

fn network(zzzy_extra: &str, zzzz_extra: &str) -> Network {
    let net = Network::new(vec![
        fixture_station("ZZZY", zzzy_extra),
        fixture_station("ZZZZ", zzzz_extra),
    ]);
    net.on_weather(Weather {
        wind: Wind {
            dir_deg: 90,
            speed_kt: 5,
            calm: false,
        },
        visibility_m: 10_000,
        clouds: vec![],
        temp_c: 15,
        dewpoint_c: 10,
        qnh_hpa: 1013,
    });
    net
}

fn join(net: &Network, callsign: &str) -> Callsign {
    net.register_pilot(PilotProfile {
        callsign: cs(callsign),
        aircraft_type: "C172".into(),
    });
    cs(callsign)
}

fn sample(callsign: &Callsign, t: f64, pos: LatLon, alt: f64, gs: f64, com1: u32) -> AircraftState {
    AircraftState {
        callsign: callsign.clone(),
        t: SimTime(t),
        pos,
        alt_ft_msl: alt,
        heading_deg: 0.0,
        ias_kt: gs,
        gs_kt: gs,
        vs_fpm: 0.0,
        on_ground: gs < 40.0,
        com1_khz: com1,
        xpdr_code: None,
        xpdr_mode: None,
        utc_s: None,
        day_of_year: None,
    }
}

fn at(lat: f64) -> LatLon {
    LatLon { lat, lon: 10.0 }
}

fn stations(out: &[AtcOutput]) -> Vec<&str> {
    out.iter().map(|o| o.station.as_str()).collect()
}

fn registered(net: &Network, i: usize, c: &Callsign) -> bool {
    net.stations()[i].engine.lock().unwrap().strip(c).is_some()
}

#[test]
fn transmission_routed_by_frequency() {
    let net = network("", ZZZZ_EXTRA);
    let c = join(&net, "SPTST");
    // Zeta's stand: within 30 NM of both airports, so both engines know the aircraft.
    net.on_telemetry(sample(&c, 0.0, at(10.0012), 100.0, 0.0, ZETA_GROUND));
    assert!(registered(&net, ZZZZ, &c) && registered(&net, ZZZY, &c));
    assert_eq!(net.owner(&c), None);
    let out = net.on_transmission(
        &c,
        ZETA_GROUND,
        "Zeta Ground, SPTST, request taxi",
        Lang::En,
        SimTime(1.0),
    );
    assert!(!out.is_empty());
    assert!(
        stations(&out).iter().all(|s| *s == "Zeta Ground"),
        "{out:?}"
    );
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZZ)));
    // Testville never heard it.
    let other = net.stations()[ZZZY].engine.lock().unwrap();
    assert!(other.strip(&c).unwrap().issued.is_empty());
}

#[test]
fn unowned_frequency_is_silent() {
    let net = network("", ZZZZ_EXTRA);
    let c = join(&net, "SPTST");
    net.on_telemetry(sample(&c, 0.0, at(10.0012), 100.0, 0.0, 121_500));
    let out = net.on_transmission(&c, 121_500, "SPTST, radio check", Lang::En, SimTime(1.0));
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(net.owner(&c), None);
}

#[test]
fn shared_frequency_answered_once() {
    let net = network(SHARED_APP, SHARED_APP);
    // Owned by Testville (called its Tower first), now near Zeta: Testville keeps it.
    let a = join(&net, "SPAAA");
    net.on_telemetry(sample(&a, 0.0, at(10.45), 2000.0, 100.0, TESTVILLE_TWR));
    net.on_transmission(
        &a,
        TESTVILLE_TWR,
        "Testville Tower, SPAAA, request radio check",
        Lang::En,
        SimTime(1.0),
    );
    assert_eq!(net.owner(&a), Some(Owner::Station(ZZZY)));
    net.on_telemetry(sample(&a, 2.0, at(10.05), 2000.0, 100.0, ZETA_APP));
    let out = net.on_transmission(
        &a,
        ZETA_APP,
        "Shared Approach, SPAAA, two thousand feet",
        Lang::En,
        SimTime(3.0),
    );
    assert_eq!(out.len(), 1, "answered once: {out:?}");
    assert_eq!(net.owner(&a), Some(Owner::Station(ZZZY)));
    assert!(
        net.stations()[ZZZZ]
            .engine
            .lock()
            .unwrap()
            .strip(&a)
            .unwrap()
            .issued
            .is_empty()
    );
    // The prompt of the nearest station working the frequency; unowned, the nearest one.
    assert_eq!(
        net.stt_prompt_owner(ZETA_APP, Some(at(10.45))),
        Some(Owner::Station(ZZZY))
    );
    assert_eq!(
        net.stt_prompt_owner(121_500, Some(at(10.05))),
        Some(Owner::Station(ZZZZ))
    );
    // Unowned: the station nearest the aircraft.
    for (callsign, lat, owner) in [("SPBBB", 10.05, ZZZZ), ("SPCCC", 10.45, ZZZY)] {
        let b = join(&net, callsign);
        net.on_telemetry(sample(&b, 0.0, at(lat), 2000.0, 100.0, ZETA_APP));
        net.on_transmission(
            &b,
            ZETA_APP,
            &format!("Shared Approach, {callsign}, two thousand feet"),
            Lang::En,
            SimTime(1.0),
        );
        assert_eq!(net.owner(&b), Some(Owner::Station(owner)), "{callsign}");
    }
}

#[test]
fn telemetry_fans_out_within_30nm() {
    let net = network("", ZZZZ_EXTRA);
    let c = join(&net, "SPTST");
    // 6 NM south of Zeta, 36 NM from Testville.
    net.on_telemetry(sample(&c, 0.0, at(9.9), 2000.0, 100.0, 121_500));
    assert!(registered(&net, ZZZZ, &c));
    assert!(!registered(&net, ZZZY, &c));
    // Between the two: both.
    net.on_telemetry(sample(&c, 2.0, at(10.25), 2000.0, 100.0, 121_500));
    assert!(registered(&net, ZZZY, &c));
}

/// ZZZZ with Testville as a destination (30 NM north, 50 ft).
fn zeta_with_testville() -> LoadedStation {
    let LoadedStation {
        id,
        dir,
        procs,
        airport,
        engine,
    } = fixture_station("ZZZZ", ZZZZ_EXTRA);
    let testville = Destination {
        icao: "ZZZY".into(),
        name: "Testville".into(),
        pos: at(10.5),
        elevation_ft: Some(50),
    };
    let engine = engine
        .into_inner()
        .unwrap()
        .with_destinations(vec![testville]);
    LoadedStation {
        id,
        dir,
        procs,
        airport,
        engine: Mutex::new(engine),
    }
}

fn ifr_to_testville(c: &Callsign) -> FlightHandover {
    FlightHandover {
        callsign: c.clone(),
        aircraft_type: "C172".into(),
        plan: Some(FlightPlan {
            ifr: true,
            destination: "ZZZY".into(),
            dest_name: "Testville".into(),
            cruise_ft: 2000,
            sid: None,
            dep_runway: None,
            star: None,
            approach: None,
            vectors: false,
            cleared_ft: 2000,
            direct: None,
            heading: None,
            approach_cleared: false,
            pattern: None,
            established_check: false,
        }),
        squawk: Some(4601),
        cleared_ft: Some(2000),
        rules: Some(FlightRules::Ifr),
    }
}

#[test]
fn handoff_moves_flight_with_plan() {
    let net = Network::new(vec![fixture_station("ZZZY", ""), zeta_with_testville()]);
    let c = join(&net, "SPTST");
    net.stations()[ZZZZ]
        .engine
        .lock()
        .unwrap()
        .import_flight(ifr_to_testville(&c));
    // Airborne 18 NM south of Testville, northbound.
    let mut lat = 10.2;
    let mut t = 0.0;
    net.on_telemetry(sample(&c, t, at(lat), 2000.0, 150.0, ZETA_APP));
    let out = net.on_transmission(
        &c,
        ZETA_APP,
        "Zeta Approach, Sierra Papa Tango Sierra Tango, two thousand feet",
        Lang::En,
        SimTime(t),
    );
    assert_eq!(
        out.first().map(|o| o.message.items[0].clone()),
        Some(AtcItem::Identified),
        "{out:?}"
    );
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZZ)));
    let contact = loop {
        assert!(lat < 10.5, "no hand-off before Testville");
        t += 2.0;
        lat += 150.0 / 3600.0 * 2.0 / 60.0;
        let out = net.on_telemetry(sample(&c, t, at(lat), 2000.0, 150.0, ZETA_APP));
        if let Some(o) = out.into_iter().find(|o| {
            o.message.items.iter().any(|i| {
                matches!(
                    i,
                    AtcItem::Contact {
                        khz: TESTVILLE_TWR,
                        ..
                    }
                )
            })
        }) {
            break o;
        }
    };
    assert_eq!(contact.station, "Zeta Approach");
    // Not read back yet: still Zeta's.
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZZ)));
    let rb = "contact one one eight decimal five, Sierra Papa Tango Sierra Tango";
    net.on_transmission(&c, ZETA_APP, rb, Lang::En, SimTime(t + 1.0));
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZY)));
    assert!(!registered(&net, ZZZZ, &c));
    // Still within 30 NM of Zeta: its telemetry does not bring back a ghost strip there.
    net.on_telemetry(sample(&c, t + 2.0, at(lat), 2000.0, 150.0, TESTVILLE_TWR));
    net.on_telemetry(sample(&c, t + 4.0, at(lat), 2000.0, 150.0, TESTVILLE_TWR));
    assert!(!registered(&net, ZZZZ, &c));
    let testville = net.stations()[ZZZY].engine.lock().unwrap();
    let strip = testville.strip(&c).expect("handed in");
    assert!(strip.handed_in);
    assert_eq!(strip.plan.as_ref().unwrap().destination, "ZZZY");
    assert_eq!(strip.squawk, Some(4601));
}

/// Calling the next station without reading back the contact moves the flight on that call.
#[test]
fn direct_call_to_other_station_moves_flight() {
    let net = network("", ZZZZ_EXTRA);
    let c = join(&net, "SPTST");
    net.on_telemetry(sample(&c, 0.0, at(10.0012), 100.0, 0.0, ZETA_GROUND));
    net.on_transmission(
        &c,
        ZETA_GROUND,
        "Zeta Ground, SPTST, request taxi",
        Lang::En,
        SimTime(1.0),
    );
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZZ)));
    let out = net.on_transmission(
        &c,
        TESTVILLE_TWR,
        "Testville Tower, SPTST, request radio check",
        Lang::En,
        SimTime(2.0),
    );
    assert!(!out.is_empty());
    assert!(
        stations(&out).iter().all(|s| *s == "Testville Tower"),
        "{out:?}"
    );
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZY)));
    assert!(!registered(&net, ZZZZ, &c));
    assert!(
        net.stations()[ZZZY]
            .engine
            .lock()
            .unwrap()
            .strip(&c)
            .is_some()
    );
}

/// One voice per frequency over all stations: real positions before emulated units.
#[test]
fn station_map_is_the_union() {
    use atc_server::StationMap;
    use atc_voice::VoiceId;
    let net = network("", ZZZZ_EXTRA);
    let m = StationMap::union(net.stations().iter().map(|s| &s.procs));
    // ZZZZ emulates Testville Tower as an adjacent unit; ZZZY's real Tower wins.
    assert_eq!(m.voice_for(TESTVILLE_TWR), Some(VoiceId::Tower));
    assert_eq!(m.voice_for(ZETA_GROUND), Some(VoiceId::Ground));
    assert_eq!(m.station_name(ZETA_APP), Some("Zeta Approach"));
    assert_eq!(net.atis_khzs(), Vec::<u32>::new());
}

/// An ATIS (or any unstaffed) frequency of another station owns nothing: a call there moves
/// no flight and nobody answers.
#[test]
fn call_on_other_stations_atis_moves_nothing() {
    let atis = r#"
[[positions]]
role = "Atis"
callsign = "Testville ATIS"
khz = 120500
"#;
    let net = network(atis, ZZZZ_EXTRA);
    let c = join(&net, "SPTST");
    net.on_telemetry(sample(&c, 0.0, at(10.0012), 100.0, 0.0, ZETA_GROUND));
    net.on_transmission(
        &c,
        ZETA_GROUND,
        "Zeta Ground, SPTST, request taxi",
        Lang::En,
        SimTime(1.0),
    );
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZZ)));
    let out = net.on_transmission(
        &c,
        120_500,
        "Testville ATIS, SPTST, request taxi",
        Lang::En,
        SimTime(2.0),
    );
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZZ)));
    assert!(registered(&net, ZZZZ, &c));
    assert_eq!(net.atis_khzs(), vec![120_500]);
}

// En-route units: Alpha Information (FIS, 120.500) covers 9.0..10.4 N up to 9500 ft, Bravo
// Radar (ACC, 127.660) 10.4..11.5 N; Testville's TMA is 10.35..10.65 N.

const ALPHA: u32 = 120_500;
const BRAVO: u32 = 127_660;
const TESTVILLE_APP: u32 = 119_900;

fn box_sector(name: &str, south: f64, north: f64, upper_ft: i32) -> SectorDef {
    SectorDef {
        name: name.into(),
        lower_ft: 0,
        upper_ft,
        polygon: vec![[south, 9.0], [south, 11.0], [north, 11.0], [north, 9.0]],
    }
}

fn alpha() -> EnrouteUnitDef {
    EnrouteUnitDef {
        callsign: "Alpha Information".into(),
        khz: ALPHA,
        kind: EnrouteKind::Fis,
        sectors: vec![box_sector("FIS ALPHA", 9.0, 10.4, 9500)],
    }
}

fn bravo() -> EnrouteUnitDef {
    EnrouteUnitDef {
        callsign: "Bravo Radar".into(),
        khz: BRAVO,
        kind: EnrouteKind::Acc,
        sectors: vec![box_sector("ACC BRAVO", 10.4, 11.5, 24500)],
    }
}

const TESTVILLE_TMA: &str = r#"
[[airspaces]]
kind = "Tma"
name = "TMA TESTVILLE"
lower_ft = 0
upper_ft = 9500
polygon = [[10.35, 9.5], [10.35, 10.5], [10.65, 10.5], [10.65, 9.5]]
"#;

const TESTVILLE_APPROACH: &str = r#"
[[positions]]
role = "Approach"
callsign = "Testville Approach"
khz = 119900
"#;

fn items_of(out: &[AtcOutput]) -> Vec<AtcItem> {
    out.iter().flat_map(|o| o.message.items.clone()).collect()
}

/// Flies `c` north (`step` > 0) or south from `lat` at `alt` ft, 10 s and `step` degrees
/// per sample, until an output satisfies `until`: that output, the latitude and the time.
#[allow(clippy::too_many_arguments)]
fn fly_until(
    net: &Network,
    c: &Callsign,
    mut lat: f64,
    step: f64,
    alt: f64,
    com1: u32,
    mut t: f64,
    until: impl Fn(&AtcOutput) -> bool,
) -> (AtcOutput, f64, f64) {
    for _ in 0..200 {
        lat += step;
        t += 10.0;
        let out = net.on_telemetry(sample(c, t, at(lat), alt, 150.0, com1));
        if let Some(o) = out.into_iter().find(&until) {
            return (o, lat, t);
        }
    }
    panic!("no such output flying from {lat}");
}

fn has(item: impl Fn(&AtcItem) -> bool) -> impl Fn(&AtcOutput) -> bool {
    move |o| o.message.items.iter().any(&item)
}

/// Aircraft `c` at `lat`, 4000 ft, calls Alpha Information and is identified.
fn identified_by_alpha(net: &Network, c: &Callsign, lat: f64) {
    net.on_telemetry(sample(c, 0.0, at(lat), 4000.0, 150.0, ALPHA));
    let out = net.on_transmission(
        c,
        ALPHA,
        "Alpha Information, SPTST, four thousand feet",
        Lang::En,
        SimTime(1.0),
    );
    assert_eq!(stations(&out), vec!["Alpha Information"], "{out:?}");
    assert_eq!(out[0].freq_khz, ALPHA);
    assert_eq!(
        out[0].message.items,
        vec![AtcItem::Identified, AtcItem::Qnh(1013)]
    );
    assert!(
        out[0].text.contains("identified, QNH one zero one three"),
        "{}",
        out[0].text
    );
    assert_eq!(net.owner(c), Some(Owner::Unit(0)));
}

#[test]
fn left_sector_to_next_unit() {
    let net = network("", ZZZZ_EXTRA).with_units(vec![alpha(), bravo()]);
    let c = join(&net, "SPTST");
    identified_by_alpha(&net, &c, 10.2);
    let (contact, lat, t) = fly_until(&net, &c, 10.2, 0.02, 4000.0, ALPHA, 1.0, |o| {
        !o.message.items.is_empty()
    });
    assert_eq!(contact.station, "Alpha Information");
    assert_eq!(contact.freq_khz, ALPHA);
    assert_eq!(
        contact.message.items,
        vec![AtcItem::Contact {
            station: "Bravo Radar".into(),
            khz: BRAVO
        }]
    );
    assert!(lat > 10.4);
    // Not read back yet: still Alpha's.
    assert_eq!(net.owner(&c), Some(Owner::Unit(0)));
    let rb = net.on_transmission(
        &c,
        ALPHA,
        "one two seven decimal six six, SPTST",
        Lang::En,
        SimTime(t + 1.0),
    );
    assert!(rb.is_empty(), "{rb:?}");
    assert_eq!(net.owner(&c), Some(Owner::Unit(1)));
    let out = net.on_transmission(
        &c,
        BRAVO,
        "Bravo Radar, SPTST, four thousand feet",
        Lang::En,
        SimTime(t + 5.0),
    );
    assert_eq!(stations(&out), vec!["Bravo Radar"]);
    assert_eq!(
        items_of(&out),
        vec![AtcItem::Identified, AtcItem::Qnh(1013)]
    );
}

/// Without a readback the flight moves 30 s after the instruction anyway.
#[test]
fn unit_handoff_taken_after_timeout() {
    let net = network("", ZZZZ_EXTRA).with_units(vec![alpha(), bravo()]);
    let c = join(&net, "SPTST");
    identified_by_alpha(&net, &c, 10.2);
    let (_, lat, t) = fly_until(
        &net,
        &c,
        10.2,
        0.02,
        4000.0,
        ALPHA,
        1.0,
        has(|i| matches!(i, AtcItem::Contact { .. })),
    );
    net.on_telemetry(sample(&c, t + 20.0, at(lat), 4000.0, 150.0, ALPHA));
    assert_eq!(net.owner(&c), Some(Owner::Unit(0)));
    let out = net.on_telemetry(sample(&c, t + 30.0, at(lat), 4000.0, 150.0, ALPHA));
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(net.owner(&c), Some(Owner::Unit(1)));
}

#[test]
fn left_sector_into_station_tma_contacts_approach() {
    let extra = format!("{TESTVILLE_TMA}{TESTVILLE_APPROACH}");
    let net = network(&extra, ZZZZ_EXTRA).with_units(vec![alpha()]);
    let c = join(&net, "SPTST");
    identified_by_alpha(&net, &c, 10.2);
    let (contact, _, t) = fly_until(&net, &c, 10.2, 0.02, 4000.0, ALPHA, 1.0, |o| {
        !o.message.items.is_empty()
    });
    assert_eq!(contact.station, "Alpha Information");
    assert_eq!(
        contact.message.items,
        vec![AtcItem::Contact {
            station: "Testville Approach".into(),
            khz: TESTVILLE_APP
        }]
    );
    net.on_transmission(
        &c,
        ALPHA,
        "one one niner decimal niner, SPTST",
        Lang::En,
        SimTime(t + 1.0),
    );
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZY)));
    let testville = net.stations()[ZZZY].engine.lock().unwrap();
    assert!(testville.strip(&c).expect("handed in").handed_in);
}

/// A station without Approach: its Tower.
#[test]
fn left_sector_into_station_tma_contacts_tower() {
    let net = network(TESTVILLE_TMA, ZZZZ_EXTRA).with_units(vec![alpha()]);
    let c = join(&net, "SPTST");
    identified_by_alpha(&net, &c, 10.2);
    let (contact, _, _) = fly_until(&net, &c, 10.2, 0.02, 4000.0, ALPHA, 1.0, |o| {
        !o.message.items.is_empty()
    });
    assert_eq!(
        contact.message.items,
        vec![AtcItem::Contact {
            station: "Testville Tower".into(),
            khz: TESTVILLE_TWR
        }]
    );
}

#[test]
fn last_sector_releases_once() {
    let net = network("", ZZZZ_EXTRA).with_units(vec![alpha()]);
    let c = join(&net, "SPTST");
    identified_by_alpha(&net, &c, 9.2);
    let mut heard = Vec::new();
    let mut t = 1.0;
    let mut lat = 9.2;
    for _ in 0..30 {
        t += 10.0;
        lat -= 0.02;
        heard.extend(net.on_telemetry(sample(&c, t, at(lat), 4000.0, 150.0, ALPHA)));
    }
    assert!(lat < 9.0);
    assert_eq!(
        items_of(&heard),
        vec![AtcItem::FrequencyChangeApproved],
        "{heard:?}"
    );
    assert_eq!(stations(&heard), vec!["Alpha Information"]);
    assert!(heard[0].text.ends_with("frequency change approved"));
    assert_eq!(net.owner(&c), None);
    // The pilot's acknowledgement is not answered; nobody owns the flight.
    let out = net.on_transmission(
        &c,
        ALPHA,
        "frequency change approved, SPTST",
        Lang::En,
        SimTime(t + 1.0),
    );
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(net.owner(&c), None);
    for k in 0..5 {
        let out = net.on_telemetry(sample(
            &c,
            t + 10.0 * f64::from(k),
            at(lat),
            4000.0,
            150.0,
            ALPHA,
        ));
        assert!(out.is_empty(), "{out:?}");
    }
    // A station called next answers.
    let out = net.on_transmission(
        &c,
        ZETA_APP,
        "Zeta Approach, SPTST, request radio check",
        Lang::En,
        SimTime(t + 60.0),
    );
    assert!(!out.is_empty());
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZZ)));
}

/// Testville has no controller: "change to" its published frequency, or with none a plain
/// release; the flight is nobody's.
#[test]
fn silent_station_change_to_or_release() {
    for (positions, expected) in [
        (
            vec![PositionDef {
                role: FreqKind::Unicom,
                callsign: "Testville Radio".into(),
                khz: 122_800,
            }],
            AtcItem::ChangeTo {
                station: "Testville Radio".into(),
                khz: 122_800,
            },
        ),
        (vec![], AtcItem::FrequencyChangeApproved),
    ] {
        let mut testville = fixture_station("ZZZY", TESTVILLE_TMA);
        testville.procs.positions = positions;
        let net = Network::new(vec![testville, fixture_station("ZZZZ", ZZZZ_EXTRA)])
            .with_units(vec![alpha()]);
        let c = join(&net, "SPTST");
        net.on_weather(Weather {
            wind: Wind {
                dir_deg: 90,
                speed_kt: 5,
                calm: false,
            },
            visibility_m: 10_000,
            clouds: vec![],
            temp_c: 15,
            dewpoint_c: 10,
            qnh_hpa: 1013,
        });
        identified_by_alpha(&net, &c, 10.2);
        let mut heard = Vec::new();
        let (mut lat, mut t) = (10.2, 1.0);
        for _ in 0..20 {
            lat += 0.02;
            t += 10.0;
            heard.extend(net.on_telemetry(sample(&c, t, at(lat), 4000.0, 150.0, ALPHA)));
        }
        assert_eq!(items_of(&heard), vec![expected.clone()], "{heard:?}");
        assert_eq!(net.owner(&c), None);
        if let AtcItem::ChangeTo { .. } = expected {
            assert!(
                heard[0]
                    .text
                    .ends_with("change to Testville Radio one two two decimal eight"),
                "{}",
                heard[0].text
            );
        }
    }
}

/// Calling a unit directly takes the flight there with its plan; the unit hands it on to a
/// station with the level last assigned.
#[test]
fn direct_call_to_unit_moves_flight_with_plan() {
    let extra = format!("{TESTVILLE_TMA}{TESTVILLE_APPROACH}");
    let net = Network::new(vec![fixture_station("ZZZY", &extra), zeta_with_testville()])
        .with_units(vec![alpha()]);
    let c = join(&net, "SPTST");
    net.stations()[ZZZZ]
        .engine
        .lock()
        .unwrap()
        .import_flight(ifr_to_testville(&c));
    net.on_telemetry(sample(&c, 0.0, at(10.2), 2000.0, 150.0, ZETA_APP));
    net.on_transmission(
        &c,
        ZETA_APP,
        "Zeta Approach, Sierra Papa Tango Sierra Tango, two thousand feet",
        Lang::En,
        SimTime(1.0),
    );
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZZ)));
    let out = net.on_transmission(
        &c,
        ALPHA,
        "Alpha Information, SPTST, two thousand feet",
        Lang::En,
        SimTime(2.0),
    );
    assert_eq!(stations(&out), vec!["Alpha Information"]);
    assert_eq!(net.owner(&c), Some(Owner::Unit(0)));
    assert!(!registered(&net, ZZZZ, &c));
    let (_, _, t) = fly_until(&net, &c, 10.2, 0.02, 2000.0, ALPHA, 2.0, |o| {
        !o.message.items.is_empty()
    });
    net.on_transmission(
        &c,
        ALPHA,
        "one one niner decimal niner, SPTST",
        Lang::En,
        SimTime(t + 1.0),
    );
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZY)));
    let testville = net.stations()[ZZZY].engine.lock().unwrap();
    let strip = testville.strip(&c).expect("handed in");
    assert!(strip.handed_in);
    let plan = strip.plan.as_ref().unwrap();
    assert_eq!(plan.destination, "ZZZY");
    assert_eq!(plan.cleared_ft, 2000);
    assert_eq!(strip.squawk, Some(4601));
}

/// A real unit wins over a station's emulation of the same frequency.
#[test]
fn real_unit_wins_over_emulated_adjacent() {
    let emulated = r#"
[[adjacent]]
callsign = "Alpha Information"
khz = 120500
kind = "Fis"
"#;
    let net = network("", &format!("{ZZZZ_EXTRA}{emulated}")).with_units(vec![alpha()]);
    let c = join(&net, "SPTST");
    identified_by_alpha(&net, &c, 10.2);
    assert_eq!(net.owner(&c), Some(Owner::Unit(0)));
}

/// Units speak with the Radar voice, under their own callsigns.
#[test]
fn station_map_has_the_units() {
    use atc_server::StationMap;
    use atc_voice::VoiceId;
    let net = network("", ZZZZ_EXTRA);
    let m =
        StationMap::union_with_units(net.stations().iter().map(|s| &s.procs), &[alpha(), bravo()]);
    assert_eq!(m.voice_for(ALPHA), Some(VoiceId::Radar));
    assert_eq!(m.station_name(BRAVO), Some("Bravo Radar"));
    assert_eq!(m.voice_for(ZETA_GROUND), Some(VoiceId::Ground));
}

/// Zeta Approach hands a departure to the real Bravo Radar at its TMA boundary: read back,
/// the unit works the flight with its plan and level.
#[test]
fn station_hands_flight_to_unit() {
    let tma = r#"
[tma]
radius_nm = 10.0
ceiling_ft = 9500

[[adjacent]]
callsign = "Bravo Radar"
khz = 127660
kind = "Acc"
"#;
    let LoadedStation {
        id,
        dir,
        procs,
        airport,
        engine,
    } = fixture_station("ZZZZ", &format!("{ZZZZ_EXTRA}{tma}"));
    let farland = Destination {
        icao: "ZZZF".into(),
        name: "Farland".into(),
        pos: at(11.3333),
        elevation_ft: None,
    };
    let engine = engine
        .into_inner()
        .unwrap()
        .with_destinations(vec![farland]);
    let zeta = LoadedStation {
        id,
        dir,
        procs,
        airport,
        engine: Mutex::new(engine),
    };
    let net = Network::new(vec![fixture_station("ZZZY", ""), zeta]).with_units(vec![
        alpha(),
        EnrouteUnitDef {
            sectors: vec![box_sector("ACC BRAVO", 9.0, 11.5, 24500)],
            ..bravo()
        },
    ]);
    net.on_weather(Weather {
        wind: Wind {
            dir_deg: 90,
            speed_kt: 5,
            calm: false,
        },
        visibility_m: 10_000,
        clouds: vec![],
        temp_c: 15,
        dewpoint_c: 10,
        qnh_hpa: 1013,
    });
    let c = join(&net, "SPTST");
    let mut h = ifr_to_testville(&c);
    let plan = h.plan.as_mut().unwrap();
    plan.destination = "ZZZF".into();
    plan.dest_name = "Farland".into();
    plan.cruise_ft = 6000;
    plan.cleared_ft = 6000;
    h.cleared_ft = Some(6000);
    net.stations()[ZZZZ].engine.lock().unwrap().import_flight(h);
    net.on_telemetry(sample(&c, 0.0, at(10.05), 6000.0, 150.0, ZETA_APP));
    let out = net.on_transmission(
        &c,
        ZETA_APP,
        "Zeta Approach, Sierra Papa Tango Sierra Tango, six thousand feet",
        Lang::En,
        SimTime(1.0),
    );
    assert_eq!(
        items_of(&out).first(),
        Some(&AtcItem::Identified),
        "{out:?}"
    );
    let (contact, _, t) = fly_until(
        &net,
        &c,
        10.05,
        0.01,
        6000.0,
        ZETA_APP,
        1.0,
        has(|i| matches!(i, AtcItem::Contact { khz: BRAVO, .. })),
    );
    assert_eq!(contact.station, "Zeta Approach");
    let rb = "contact one two seven decimal six six, Sierra Papa Tango Sierra Tango";
    net.on_transmission(&c, ZETA_APP, rb, Lang::En, SimTime(t + 1.0));
    assert_eq!(net.owner(&c), Some(Owner::Unit(1)));
    assert!(!registered(&net, ZZZZ, &c));
    let out = net.on_transmission(
        &c,
        BRAVO,
        "Bravo Radar, SPTST, six thousand feet",
        Lang::En,
        SimTime(t + 5.0),
    );
    assert_eq!(
        items_of(&out),
        vec![AtcItem::Identified, AtcItem::Qnh(1013)]
    );
}

/// Alpha Information reaching from 9.0 to 11.0 N: under (and around) Testville's TMA.
fn wide_alpha() -> EnrouteUnitDef {
    EnrouteUnitDef {
        sectors: vec![box_sector("FIS ALPHA", 9.0, 11.0, 9500)],
        ..alpha()
    }
}

/// Still inside the FIS sector, an inbound entering Testville's TMA is handed to Approach.
#[test]
fn inbound_under_fis_enters_tma_and_is_handed_to_approach() {
    let extra = format!("{TESTVILLE_TMA}{TESTVILLE_APPROACH}");
    let net = network(&extra, ZZZZ_EXTRA).with_units(vec![wide_alpha()]);
    let c = join(&net, "SPTST");
    identified_by_alpha(&net, &c, 10.2);
    let (contact, lat, t) = fly_until(&net, &c, 10.2, 0.02, 4000.0, ALPHA, 1.0, |o| {
        !o.message.items.is_empty()
    });
    assert!(lat > 10.35 && lat < 10.4, "{lat}");
    assert_eq!(contact.station, "Alpha Information");
    assert_eq!(
        contact.message.items,
        vec![AtcItem::Contact {
            station: "Testville Approach".into(),
            khz: TESTVILLE_APP
        }]
    );
    net.on_transmission(
        &c,
        ALPHA,
        "one one niner decimal niner, SPTST",
        Lang::En,
        SimTime(t + 1.0),
    );
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZY)));
}

/// A flight taken by the unit while inside the TMA is not handed straight back; leaving it
/// and flying on under the FIS, nothing is said.
#[test]
fn departure_imported_inside_tma_is_not_handed_back() {
    let extra = format!("{TESTVILLE_TMA}{TESTVILLE_APPROACH}");
    let net = network(&extra, ZZZZ_EXTRA).with_units(vec![wide_alpha()]);
    let c = join(&net, "SPTST");
    net.on_telemetry(sample(&c, 0.0, at(10.5), 4000.0, 150.0, TESTVILLE_APP));
    net.on_transmission(
        &c,
        TESTVILLE_APP,
        "Testville Approach, SPTST, four thousand feet",
        Lang::En,
        SimTime(1.0),
    );
    assert_eq!(net.owner(&c), Some(Owner::Station(ZZZY)));
    let out = net.on_transmission(
        &c,
        ALPHA,
        "Alpha Information, SPTST, four thousand feet",
        Lang::En,
        SimTime(2.0),
    );
    assert_eq!(stations(&out), vec!["Alpha Information"]);
    assert_eq!(net.owner(&c), Some(Owner::Unit(0)));
    let mut heard = Vec::new();
    let (mut lat, mut t) = (10.5, 2.0);
    for _ in 0..15 {
        lat += 0.02;
        t += 10.0;
        heard.extend(net.on_telemetry(sample(&c, t, at(lat), 4000.0, 150.0, ALPHA)));
    }
    assert!(lat > 10.65 && lat < 11.0);
    assert!(heard.is_empty(), "{heard:?}");
    assert_eq!(net.owner(&c), Some(Owner::Unit(0)));
}

/// Testville with an ATZ and no controller: a VFR flight under the wide FIS crossing it stays
/// with the FIS and hears nothing.
#[test]
fn crossing_unstaffed_atz_keeps_flight_with_unit() {
    let atz = TESTVILLE_TMA.replace("\"Tma\"", "\"Atz\"");
    let mut testville = fixture_station("ZZZY", &atz);
    testville.procs.positions = vec![PositionDef {
        role: FreqKind::Unicom,
        callsign: "Testville Radio".into(),
        khz: 122_800,
    }];
    let net = Network::new(vec![testville, fixture_station("ZZZZ", ZZZZ_EXTRA)])
        .with_units(vec![wide_alpha()]);
    net.on_weather(Weather {
        wind: Wind {
            dir_deg: 90,
            speed_kt: 5,
            calm: false,
        },
        visibility_m: 10_000,
        clouds: vec![],
        temp_c: 15,
        dewpoint_c: 10,
        qnh_hpa: 1013,
    });
    let c = join(&net, "SPTST");
    identified_by_alpha(&net, &c, 10.2);
    let mut heard = Vec::new();
    let (mut lat, mut t) = (10.2, 1.0);
    for _ in 0..30 {
        lat += 0.02;
        t += 10.0;
        heard.extend(net.on_telemetry(sample(&c, t, at(lat), 2000.0, 100.0, ALPHA)));
    }
    assert!(lat > 10.65 && lat < 11.0);
    assert!(heard.is_empty(), "{heard:?}");
    assert_eq!(net.owner(&c), Some(Owner::Unit(0)));
}

/// On the shared Alpha/Bravo boundary, in and out every sample: no hand-off either way.
#[test]
fn boundary_hover_does_not_ping_pong() {
    let net = network("", ZZZZ_EXTRA).with_units(vec![alpha(), bravo()]);
    let c = join(&net, "SPTST");
    identified_by_alpha(&net, &c, 10.39);
    let mut heard = Vec::new();
    for k in 0..20 {
        let lat = if k % 2 == 0 { 10.401 } else { 10.399 };
        let t = 2.0 + 10.0 * f64::from(k);
        heard.extend(net.on_telemetry(sample(&c, t, at(lat), 4000.0, 150.0, ALPHA)));
    }
    assert!(heard.is_empty(), "{heard:?}");
    assert_eq!(net.owner(&c), Some(Owner::Unit(0)));
}

// Persistence: every station's snapshot, the owners and the units' flights.

mod restart {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use atc_server::network::OwnerRef;
    use atc_server::persist;

    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("atc-restart-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn net() -> Network {
        let net = Network::new(vec![fixture_station("ZZZY", ""), zeta_with_testville()])
            .with_units(vec![alpha()]);
        net.on_weather(Weather {
            wind: Wind {
                dir_deg: 90,
                speed_kt: 5,
                calm: false,
            },
            visibility_m: 10_000,
            clouds: vec![],
            temp_c: 15,
            dewpoint_c: 10,
            qnh_hpa: 1013,
        });
        net
    }

    /// SPONE works with Testville Tower; SPTST (IFR to Testville, 2000 ft) with Alpha.
    fn in_flight(net: &Network) -> (Callsign, Callsign) {
        let one = join(net, "SPONE");
        net.on_telemetry(sample(&one, 0.0, at(10.5), 0.0, 0.0, TESTVILLE_TWR));
        net.on_transmission(
            &one,
            TESTVILLE_TWR,
            "Testville Tower, SPONE, request radio check",
            Lang::En,
            SimTime(1.0),
        );
        assert_eq!(net.owner(&one), Some(Owner::Station(ZZZY)));
        let c = join(net, "SPTST");
        net.stations()[ZZZZ]
            .engine
            .lock()
            .unwrap()
            .import_flight(ifr_to_testville(&c));
        net.on_telemetry(sample(&c, 0.0, at(10.2), 2000.0, 150.0, ZETA_APP));
        net.on_transmission(
            &c,
            ZETA_APP,
            "Zeta Approach, SPTST, two thousand feet",
            Lang::En,
            SimTime(1.0),
        );
        net.on_transmission(
            &c,
            ALPHA,
            "Alpha Information, SPTST, two thousand feet",
            Lang::En,
            SimTime(2.0),
        );
        assert_eq!(net.owner(&c), Some(Owner::Unit(0)));
        (one, c)
    }

    #[test]
    fn restart_restores_every_station_and_owner() {
        let d = dir("all");
        let before = net();
        let (one, c) = in_flight(&before);
        persist::save_all_in(&d, &before, &mut HashMap::new());
        assert!(d.join("state-ZZZY.json").is_file());
        assert!(d.join("state-ZZZZ.json").is_file());
        assert!(d.join("network.json").is_file());
        assert!(!d.join("network.json.tmp").exists());

        let after = net();
        persist::restore_all_in(&d, &after, false, SystemTime::now());
        assert_eq!(after.owner(&one), Some(Owner::Station(ZZZY)));
        assert_eq!(after.owner(&c), Some(Owner::Unit(0)));
        assert!(registered(&after, ZZZY, &one));
        // The unit keeps the flight with its plan and level, and has identified it already.
        let state = after.persisted().1;
        let flight = &state.units[0].flights[0];
        assert_eq!(flight.data.cleared_ft, Some(2000));
        assert_eq!(flight.data.plan.as_ref().unwrap().destination, "ZZZY");
        assert!(flight.identified);
        let out = after.on_transmission(
            &c,
            ALPHA,
            "Alpha Information, SPTST, request radio check",
            Lang::En,
            SimTime(3.0),
        );
        assert!(
            !items_of(&out).contains(&AtcItem::Identified),
            "identified again: {out:?}"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn restore_ignores_vanished_stations_units_and_stale_files() {
        let d = dir("gone");
        let before = net();
        let (one, c) = in_flight(&before);
        persist::save_all_in(&d, &before, &mut HashMap::new());
        let mut state = before.persisted().1;
        state
            .owners
            .insert("SPGONE".into(), OwnerRef::Station("ZZZX".into()));
        state.owners.insert(
            "SPGON2".into(),
            OwnerRef::Unit("Nobody Radar@123450".into()),
        );
        std::fs::write(
            d.join("network.json"),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();

        let after = net();
        persist::restore_all_in(&d, &after, false, SystemTime::now());
        assert_eq!(after.owner(&cs("SPGONE")), None);
        assert_eq!(after.owner(&cs("SPGON2")), None);
        assert_eq!(after.owner(&c), Some(Owner::Unit(0)));

        // Older than the maximum age, or --fresh: nothing comes back.
        let late = SystemTime::now() + persist::MAX_AGE + Duration::from_secs(60);
        for (fresh, now) in [(false, late), (true, SystemTime::now())] {
            let again = net();
            persist::restore_all_in(&d, &again, fresh, now);
            assert_eq!(again.owner(&one), None);
            assert_eq!(again.owner(&c), None);
            assert!(!registered(&again, ZZZY, &one));
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn unchanged_state_is_not_rewritten() {
        let d = dir("same");
        let n = net();
        in_flight(&n);
        let mut last = HashMap::new();
        persist::save_all_in(&d, &n, &mut last);
        let json = d.join("network.json");
        std::fs::remove_file(&json).unwrap();
        persist::save_all_in(&d, &n, &mut last);
        assert!(!json.exists(), "rewritten although nothing changed");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
