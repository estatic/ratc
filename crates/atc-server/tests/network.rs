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
use atc_navdata::model::Destination;
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
