#![cfg(feature = "testkit")]
//! Ground + Tower dialogue engine: pilot text + telemetry in, controller messages out.

use atc_core::atis::Weather;
use atc_core::engine::{
    AtcEngine, AtcOutput, EngineConfig, FOLLOW_ME_READY_S, LanguageMode, SNAPSHOT_VERSION,
    SnapshotMismatch, VehicleCommand, VehicleOrder,
};
use atc_core::phase::FlightPhase;
use atc_core::readback::{Strictness, required_readback};
use atc_core::state::{AircraftState, SimTime, TransponderMode};
use atc_core::strip::{PilotProfile, Role};
use atc_core::testkit::circuit_track;
use atc_geo::LatLon;
use atc_navdata::model::{
    ActiveZone, Airport, EdgeKind, Runway, RunwayEnd, Stand, TaxiEdge, TaxiNetwork, TaxiNode,
    ZoneKind,
};
use atc_navdata::parse_apt_dat;
use atc_navdata::procedures::{Procedures, parse_procedures};
use atc_phraseology::{
    AtcItem, Callsign, CircuitLeg, ClearanceKind, Courtesy, Lang, ReadbackItem, Side, TaxiTo, Wind,
};

const APT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/airports/EPKK/apt.dat"
);
const PROCS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/airports/EPKK/procedures.toml"
);

const GROUND: u32 = 118105;
const TOWER: u32 = 123255;
/// Callsign of the testkit track.
const CS: &str = "Sierra Papa Tango Sierra Tango";
const CS_ABBR: &str = "Sierra Sierra Tango";
const WIND: Wind = Wind {
    dir_deg: 250,
    speed_kt: 8,
    calm: false,
};

fn epkk() -> (Airport, Procedures) {
    let ap = parse_apt_dat(&std::fs::read_to_string(APT).unwrap(), "EPKK").unwrap();
    let procs = parse_procedures(&std::fs::read_to_string(PROCS).unwrap()).unwrap();
    (ap, procs)
}

fn weather(qnh: u16) -> Weather {
    Weather {
        wind: WIND,
        visibility_m: 10_000,
        clouds: vec![],
        temp_c: 15,
        dewpoint_c: 10,
        qnh_hpa: qnh,
    }
}

fn engine_for(ap: Airport, procs: Procedures, strictness: Strictness) -> AtcEngine {
    engine_with(ap, procs, EngineConfig::new(strictness))
}

fn engine_with(ap: Airport, procs: Procedures, cfg: EngineConfig) -> AtcEngine {
    let mut e = AtcEngine::new(ap, procs, cfg);
    e.register_pilot(PilotProfile {
        callsign: Callsign::new("SPTST"),
        aircraft_type: "C172".into(),
    });
    e.on_weather(weather(1013));
    e
}

fn engine() -> AtcEngine {
    let (ap, procs) = epkk();
    engine_for(ap, procs, Strictness::Standard)
}

fn track() -> Vec<AircraftState> {
    let (ap, procs) = epkk();
    circuit_track(&ap, &procs, "25", 1.0)
}

fn say(e: &mut AtcEngine, freq: u32, text: &str) -> Vec<AtcOutput> {
    e.on_transmission(freq, text, Lang::En, SimTime(0.0))
}

fn items(out: &[AtcOutput]) -> Vec<AtcItem> {
    out.iter().flat_map(|o| o.message.items.clone()).collect()
}

/// Digits and letters as the pilot says them (`"25"` -> "two five", `"A1"` -> "Alpha one").
fn spoken(s: &str) -> String {
    const DIGITS: [&str; 10] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "niner",
    ];
    const LETTERS: [&str; 26] = [
        "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
        "Juliett", "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo",
        "Sierra", "Tango", "Uniform", "Victor", "Whiskey", "Xray", "Yankee", "Zulu",
    ];
    s.split_whitespace()
        .map(|w| {
            if w.chars().any(|c| c.is_ascii_digit()) || (w.len() == 1 && w != "a") {
                w.chars()
                    .map(|c| match c.to_digit(10) {
                        Some(d) => DIGITS[d as usize].to_string(),
                        None => LETTERS[(c.to_ascii_uppercase() as u8 - b'A') as usize].into(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                w.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn freq_spoken(khz: u32) -> String {
    format!(
        "{} decimal {}",
        spoken(&(khz / 1000).to_string()),
        spoken(&atc_phraseology::frequency_decimals(khz))
    )
}

/// A correct pilot readback of `items`, ending with the callsign.
fn readback(items: &[AtcItem], cs: &str) -> String {
    let mut parts = Vec::new();
    for it in items {
        match it {
            AtcItem::Taxi { to, via, .. } => {
                let mut s = match to {
                    TaxiTo::HoldingPoint { runway } => {
                        format!("taxi to holding point runway {}", spoken(runway))
                    }
                    TaxiTo::Stand { name } => format!("taxi to stand {}", spoken(name)),
                };
                if !via.is_empty() {
                    s.push_str(" via ");
                    s.push_str(&spoken(&via.join(" ")));
                }
                parts.push(s);
            }
            AtcItem::HoldShort(r) => parts.push(format!("hold short runway {}", spoken(r))),
            AtcItem::Qnh(q) => parts.push(format!("QNH {}", spoken(&q.to_string()))),
            AtcItem::Squawk(c) => parts.push(format!("squawk {}", spoken(&format!("{c:04}")))),
            AtcItem::Contact { khz, .. } => parts.push(format!("contact {}", freq_spoken(*khz))),
            AtcItem::ClearedTakeoff { runway, .. } => {
                parts.push(format!("cleared for takeoff runway {}", spoken(runway)))
            }
            AtcItem::ClearedLand { runway, .. } => {
                parts.push(format!("cleared to land runway {}", spoken(runway)))
            }
            AtcItem::ClearedTouchAndGo { runway, .. } => {
                parts.push(format!("cleared touch and go runway {}", spoken(runway)))
            }
            AtcItem::CrossRunway(r) => parts.push(format!("crossing runway {}", spoken(r))),
            _ => {}
        }
    }
    parts.push(cs.to_string());
    parts.join(", ")
}

fn is_taxi_to_holding_point(it: &AtcItem) -> bool {
    matches!(it, AtcItem::Taxi { to: TaxiTo::HoldingPoint { runway }, .. } if runway == "25")
}

#[test]
fn full_circuit_happy_path() {
    for dt in [0.5, 1.0, 2.0] {
        happy_path(dt);
    }
}

/// How the simulated pilot behaves.
#[derive(Clone, Copy)]
struct Pilot {
    /// Says "full stop" with the downwind report.
    full_stop: bool,
    /// Reads the take-off clearance back wrong and never corrects it.
    wrong_takeoff_readback: bool,
}

const GOOD_PILOT: Pilot = Pilot {
    full_stop: true,
    wrong_takeoff_readback: false,
};

type Log = Vec<(Role, Vec<AtcItem>)>;

fn record(out: &[AtcOutput], log: &mut Log, texts: &mut Vec<String>) {
    for o in out {
        let role = match o.freq_khz {
            GROUND => Role::Ground,
            TOWER => Role::Tower,
            f => panic!("unexpected frequency {f}"),
        };
        log.push((role, o.message.items.clone()));
        texts.push(o.text.clone());
    }
}

/// Flies `samples` with a pilot who calls at each phase change and reads everything back
/// (except as `pilot` says). Returns the controller messages and their rendered texts.
fn fly(e: &mut AtcEngine, samples: Vec<AircraftState>, pilot: Pilot) -> (Log, Vec<String>) {
    let cs = Callsign::new("SPTST");
    let (mut log, mut texts) = (Log::new(), Vec::new());
    let mut last_phase = None;
    for s in samples {
        let out = e.on_telemetry(s);
        record(&out, &mut log, &mut texts);
        // The pilot reads back every controller-initiated clearance at once.
        // A reminder to read back is no clearance; the pilot just stays as it is.
        for o in out.iter().filter(|o| !is_reminder(std::slice::from_ref(o))) {
            let silent = say(e, o.freq_khz, &readback(&o.message.items, CS_ABBR));
            assert!(silent.is_empty(), "readback of {:?}: {silent:?}", o.message);
        }
        if out
            .iter()
            .flat_map(|o| &o.message.items)
            .any(|i| matches!(i, AtcItem::Contact { khz: TOWER, .. }))
        {
            let out = say(
                e,
                TOWER,
                &format!("Krakow Tower, {CS}, holding point runway two five, ready for departure"),
            );
            record(&out, &mut log, &mut texts);
            if pilot.wrong_takeoff_readback {
                let wrong = format!("cleared for takeoff runway zero seven, {CS_ABBR}");
                let neg = say(e, TOWER, &wrong);
                assert_eq!(items(&neg)[0], AtcItem::Negative, "{neg:?}");
            } else {
                let rb = say(e, TOWER, &readback(&items(&out), CS_ABBR));
                assert!(rb.is_empty(), "{rb:?}");
            }
        }
        let phase = e.strip(&cs).unwrap().phase;
        if last_phase != Some(phase) {
            let downwind = if pilot.full_stop {
                format!("{CS_ABBR}, downwind, full stop")
            } else {
                format!("{CS_ABBR}, downwind")
            };
            let call = match phase {
                FlightPhase::Parked if last_phase.is_none() => Some((
                    GROUND,
                    format!("Krakow Ground, {CS}, information Alpha, request taxi for circuits"),
                )),
                FlightPhase::Circuit(CircuitLeg::Downwind) => Some((TOWER, downwind)),
                FlightPhase::Circuit(CircuitLeg::Final) => {
                    Some((TOWER, format!("{CS_ABBR}, final")))
                }
                FlightPhase::TaxiIn => Some((
                    GROUND,
                    format!("Krakow Ground, {CS_ABBR}, request taxi to stand"),
                )),
                _ => None,
            };
            if let Some((freq, text)) = call {
                let out = say(e, freq, &text);
                assert!(!out.is_empty(), "{phase:?}: no reply to {text}");
                record(&out, &mut log, &mut texts);
                let rb = say(e, freq, &readback(&items(&out), CS_ABBR));
                assert!(rb.is_empty(), "{phase:?}: {rb:?}");
            }
            last_phase = Some(phase);
        }
    }
    (log, texts)
}

fn happy_path(dt: f64) {
    let mut e = engine();
    let cs = Callsign::new("SPTST");
    let (ap, procs) = epkk();
    let (log, texts) = fly(&mut e, circuit_track(&ap, &procs, "25", dt), GOOD_PILOT);

    let kinds: Vec<(Role, Vec<&'static str>)> = log
        .iter()
        .map(|(r, items)| {
            let names = items
                .iter()
                .map(|i| match i {
                    AtcItem::Taxi { .. } => "Taxi",
                    AtcItem::HoldShort(_) => "HoldShort",
                    AtcItem::Qnh(_) => "Qnh",
                    AtcItem::Squawk(_) => "Squawk",
                    AtcItem::Contact { .. } => "Contact",
                    AtcItem::ClearedTakeoff { .. } => "ClearedTakeoff",
                    AtcItem::Report(_) => "Report",
                    AtcItem::Sequence(_) => "Sequence",
                    AtcItem::ClearedLand { .. } => "ClearedLand",
                    AtcItem::Vacate { .. } => "Vacate",
                    AtcItem::Courtesy(Courtesy::GoodDay) => "GoodDay",
                    AtcItem::Courtesy(Courtesy::HaveANiceFlight) => "NiceFlight",
                    other => panic!("unexpected item {other:?}"),
                })
                .collect();
            (*r, names)
        })
        .collect();
    use Role::{Ground, Tower};
    assert_eq!(
        kinds,
        vec![
            (Ground, vec!["Taxi", "HoldShort", "Qnh", "Squawk"]),
            (Ground, vec!["Contact", "NiceFlight"]),
            (Tower, vec!["ClearedTakeoff", "Report"]),
            (Tower, vec!["Sequence", "Report"]),
            (Tower, vec!["ClearedLand"]),
            (Tower, vec!["Vacate", "Contact", "GoodDay"]),
            (Ground, vec!["Taxi"]),
        ]
    );
    assert!(is_taxi_to_holding_point(&log[0].1[0]));
    assert_eq!(log[0].1[1], AtcItem::HoldShort("25".into()));
    assert_eq!(log[0].1[2], AtcItem::Qnh(1013));
    assert_eq!(log[0].1[3], AtcItem::Squawk(4201));
    assert_eq!(
        log[1].1[0],
        AtcItem::Contact {
            station: "Kraków Tower".into(),
            khz: TOWER
        }
    );
    assert_eq!(
        log[2].1,
        vec![
            AtcItem::ClearedTakeoff {
                runway: "25".into(),
                wind: WIND
            },
            AtcItem::Report(CircuitLeg::Downwind)
        ]
    );
    assert_eq!(
        log[3].1,
        vec![AtcItem::Sequence(1), AtcItem::Report(CircuitLeg::Final)]
    );
    assert_eq!(
        log[4].1,
        vec![AtcItem::ClearedLand {
            runway: "25".into(),
            wind: WIND
        }]
    );
    assert_eq!(
        log[5].1[1],
        AtcItem::Contact {
            station: "Kraków Ground".into(),
            khz: GROUND
        }
    );
    assert!(matches!(
        &log[6].1[0],
        AtcItem::Taxi {
            to: TaxiTo::Stand { .. },
            ..
        }
    ));

    // First contact per station in full, abbreviated afterwards.
    assert!(texts[0].starts_with(CS), "{}", texts[0]);
    assert!(texts[1].starts_with(CS_ABBR), "{}", texts[1]);
    assert!(texts[2].starts_with(CS), "{}", texts[2]);
    assert!(texts[3].starts_with(CS_ABBR), "{}", texts[3]);
    assert!(texts[6].starts_with(CS_ABBR), "{}", texts[6]);

    let strip = e.strip(&cs).unwrap();
    assert_eq!(strip.owner, Role::Ground);
    assert!(strip.pending_readback.is_none());
}

#[test]
fn unknown_callsign_says_again() {
    let mut e = engine();
    for text in [
        "Krakow Ground, Sierra Papa Kilo Lima Mike, request taxi",
        "Krakow Ground, request taxi",
    ] {
        let out = say(&mut e, GROUND, text);
        assert_eq!(out.len(), 1, "{text}");
        assert_eq!(out[0].freq_khz, GROUND);
        assert_eq!(out[0].station, "Kraków Ground");
        assert_eq!(out[0].message.to, None);
        assert_eq!(
            out[0].message.items,
            vec![AtcItem::SayAgainCallsign {
                station: "Kraków Ground".into()
            }]
        );
    }
}

/// Registers the track's first sample (aircraft parked at its stand) and requests taxi.
fn taxi_cleared(e: &mut AtcEngine, atis: &str) -> Vec<AtcItem> {
    let first = track().into_iter().next().unwrap();
    assert!(e.on_telemetry(first).is_empty());
    let out = say(
        e,
        GROUND,
        &format!("Krakow Ground, {CS}, information {atis}, request taxi"),
    );
    assert_eq!(out.len(), 1);
    items(&out)
}

#[test]
fn repeat_request_while_readback_pending() {
    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    let pending = required_readback(&clearance, Strictness::Standard);
    let out = say(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {CS}, request taxi"),
    );
    assert_eq!(items(&out), vec![AtcItem::ReadBack(pending.clone())]);
    let cs = Callsign::new("SPTST");
    assert_eq!(e.strip(&cs).unwrap().pending_readback, Some(pending));
    // A correct readback then closes it silently.
    assert!(say(&mut e, GROUND, &readback(&clearance, CS_ABBR)).is_empty());
    assert_eq!(e.strip(&cs).unwrap().pending_readback, None);
}

#[test]
fn stale_atis_letter_corrected() {
    let mut e = engine();
    e.on_weather(weather(1009)); // QNH change: information Bravo
    let clearance = taxi_cleared(&mut e, "Alpha");
    assert_eq!(
        clearance[..2],
        [AtcItem::AtisCurrent('B'), AtcItem::Qnh(1009)]
    );
    assert!(is_taxi_to_holding_point(&clearance[2]));
    assert_eq!(
        clearance
            .iter()
            .filter(|i| matches!(i, AtcItem::Qnh(_)))
            .count(),
        1
    );

    assert_eq!(clearance.last(), Some(&AtcItem::Squawk(4201)));

    // Current letter: no ATIS item, QNH after the route, the squawk last.
    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    assert!(is_taxi_to_holding_point(&clearance[0]));
    assert_eq!(
        clearance[clearance.len() - 2..],
        [AtcItem::Qnh(1013), AtcItem::Squawk(4201)]
    );
    assert!(
        !clearance
            .iter()
            .any(|i| matches!(i, AtcItem::AtisCurrent(_)))
    );
}

/// Registers `cs` parked at the testkit stand and requests taxi as `spoken_cs`.
fn taxi_cleared_as(e: &mut AtcEngine, cs: &str, spoken_cs: &str) -> Vec<AtcItem> {
    e.register_pilot(PilotProfile {
        callsign: Callsign::new(cs),
        aircraft_type: "C172".into(),
    });
    let first = AircraftState {
        callsign: Callsign::new(cs),
        ..track().into_iter().next().unwrap()
    };
    assert!(e.on_telemetry(first).is_empty());
    let out = say(
        e,
        GROUND,
        &format!("Krakow Ground, {spoken_cs}, information Alpha, request taxi"),
    );
    assert_eq!(out.len(), 1, "{out:?}");
    items(&out)
}

fn squawk_of(items: &[AtcItem]) -> Option<u16> {
    items.iter().find_map(|i| match i {
        AtcItem::Squawk(c) => Some(*c),
        _ => None,
    })
}

/// Each aircraft gets the next free code of the pool, kept on its strip for good.
#[test]
fn squawk_assigned_per_aircraft() {
    let mut e = engine();
    let first = taxi_cleared(&mut e, "Alpha");
    assert_eq!(squawk_of(&first), Some(4201));
    // The repeated request is answered with the same code.
    say(&mut e, GROUND, &readback(&first, CS_ABBR));
    let again = say(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {CS}, information Alpha, request taxi"),
    );
    assert_eq!(squawk_of(&items(&again)), Some(4201));
    let second = taxi_cleared_as(&mut e, "SPTSU", "Sierra Papa Tango Sierra Uniform");
    assert_eq!(squawk_of(&second), Some(4202));
    assert_eq!(e.strip(&Callsign::new("SPTSU")).unwrap().squawk, Some(4202));
}

/// Codes that are not octal are skipped; an exhausted pool assigns no code.
#[test]
fn squawk_pool_octal_and_exhausted() {
    let (ap, procs) = epkk();
    let cfg = EngineConfig {
        squawk_pool: 4207..=4210,
        ..EngineConfig::new(Strictness::Standard)
    };
    let mut e = engine_with(ap, procs, cfg);
    assert_eq!(squawk_of(&taxi_cleared(&mut e, "Alpha")), Some(4207));
    let second = taxi_cleared_as(&mut e, "SPTSU", "Sierra Papa Tango Sierra Uniform");
    assert_eq!(squawk_of(&second), Some(4210));
    let third = taxi_cleared_as(&mut e, "SPTSV", "Sierra Papa Tango Sierra Victor");
    assert_eq!(squawk_of(&third), None);
    assert!(third.iter().any(is_taxi_to_holding_point), "{third:?}");
}

/// A wrong code in the readback is corrected like a wrong QNH; a missing one is asked for.
#[test]
fn wrong_squawk_readback_corrected() {
    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    let mut wrong = clearance.clone();
    for it in &mut wrong {
        if let AtcItem::Squawk(c) = it {
            *c = 4210;
        }
    }
    let out = say(&mut e, GROUND, &readback(&wrong, CS_ABBR));
    assert_eq!(items(&out), vec![AtcItem::Negative, AtcItem::Squawk(4201)]);
    assert_eq!(
        out[0].text,
        format!("{CS_ABBR}, negative, squawk four two zero one")
    );

    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    let without: Vec<AtcItem> = clearance
        .into_iter()
        .filter(|i| !matches!(i, AtcItem::Squawk(_)))
        .collect();
    let out = say(&mut e, GROUND, &readback(&without, CS_ABBR));
    assert_eq!(
        items(&out),
        vec![AtcItem::ReadBack(vec![ReadbackItem::Squawk(4201)])]
    );
    assert_eq!(out[0].text, format!("{CS_ABBR}, read back squawk"));
}

/// Taxis out (taxi clearance read back) with the transponder as given, then calls Tower
/// ready at the holding point. Returns Tower's reply and the last sample.
fn ready_with_transponder(
    e: &mut AtcEngine,
    code: Option<u16>,
    mode: Option<TransponderMode>,
) -> (Vec<AtcItem>, AircraftState) {
    let last = hold_short_with_transponder(e, code, mode);
    (ready(e), last)
}

/// Taxis out (taxi clearance read back) with the transponder as given to the holding
/// point; the last sample.
fn hold_short_with_transponder(
    e: &mut AtcEngine,
    code: Option<u16>,
    mode: Option<TransponderMode>,
) -> AircraftState {
    let clearance = taxi_cleared(e, "Alpha");
    assert!(say(e, GROUND, &readback(&clearance, CS_ABBR)).is_empty());
    let cs = Callsign::new("SPTST");
    let mut last = None;
    for s in track().into_iter().skip(1) {
        let s = AircraftState {
            xpdr_code: code,
            xpdr_mode: mode,
            utc_s: None,
            day_of_year: None,
            ..s
        };
        e.on_telemetry(s.clone());
        last = Some(s);
        if e.strip(&cs).unwrap().phase == FlightPhase::HoldingShort {
            break;
        }
    }
    last.unwrap()
}

fn ready(e: &mut AtcEngine) -> Vec<AtcItem> {
    let text = format!("Krakow Tower, {CS}, holding point runway two five, ready for departure");
    items(&say(e, TOWER, &text))
}

fn is_takeoff_clearance(items: &[AtcItem]) -> bool {
    items
        .iter()
        .any(|i| matches!(i, AtcItem::ClearedTakeoff { .. }))
}

/// Sends `last` again, `dt` seconds later, with the transponder changed by `set`.
fn set_transponder(
    e: &mut AtcEngine,
    last: &mut AircraftState,
    set: impl FnOnce(&mut AircraftState),
) {
    last.t = SimTime(last.t.0 + 1.0);
    set(last);
    e.on_telemetry(last.clone());
}

/// Another code than the one assigned: "check squawk", read back like a squawk. A correct
/// readback re-runs the departure: still wrong -> checked again, set -> take-off clearance.
#[test]
fn wrong_squawk_checked_before_takeoff() {
    let mut e = engine();
    let (reply, mut last) = ready_with_transponder(&mut e, Some(7000), Some(TransponderMode::On));
    assert_eq!(reply, vec![AtcItem::CheckSquawk(4201)]);
    let cs = Callsign::new("SPTST");
    assert!(!e.strip(&cs).unwrap().permissions.may_take_off);
    assert_eq!(
        e.strip(&cs).unwrap().pending_readback,
        Some(vec![ReadbackItem::Squawk(4201)])
    );

    // A wrong code read back: corrected as usual.
    let out = say(
        &mut e,
        TOWER,
        &format!("squawk four two one zero, {CS_ABBR}"),
    );
    assert_eq!(items(&out), vec![AtcItem::Negative, AtcItem::Squawk(4201)]);
    assert_eq!(
        out[0].text,
        format!("{CS_ABBR}, negative, squawk four two zero one")
    );

    // Read back right, but the transponder still shows the old code.
    let squawking = format!("squawk four two zero one, {CS_ABBR}");
    let out = say(&mut e, TOWER, &squawking);
    assert_eq!(items(&out), vec![AtcItem::CheckSquawk(4201)]);

    set_transponder(&mut e, &mut last, |s| s.xpdr_code = Some(4201));
    let out = say(&mut e, TOWER, &squawking);
    assert!(is_takeoff_clearance(&items(&out)), "{out:?}");
    assert!(e.strip(&cs).unwrap().permissions.may_take_off);
}

/// A transponder off or on standby: "squawk Charlie"; any acknowledgement re-runs the
/// departure.
#[test]
fn silent_transponder_gets_squawk_charlie() {
    for mode in [TransponderMode::Off, TransponderMode::Standby] {
        let mut e = engine();
        let (reply, mut last) = ready_with_transponder(&mut e, Some(4201), Some(mode));
        assert_eq!(reply, vec![AtcItem::SquawkCharlie], "{mode:?}");
        let out = say(&mut e, TOWER, &format!("wilco, {CS_ABBR}"));
        assert_eq!(items(&out), vec![AtcItem::SquawkCharlie], "{mode:?}");
        set_transponder(&mut e, &mut last, |s| {
            s.xpdr_mode = Some(TransponderMode::On)
        });
        let out = say(&mut e, TOWER, &format!("squawk Charlie, {CS_ABBR}"));
        assert!(is_takeoff_clearance(&items(&out)), "{mode:?}: {out:?}");
    }
    let mut e = engine();
    let (reply, _) = ready_with_transponder(&mut e, Some(4201), Some(TransponderMode::On));
    assert!(is_takeoff_clearance(&reply), "{reply:?}");
}

/// "Request departure runway two five" at the holding point is Tower's ready for
/// departure: the take-off clearance as before.
#[test]
fn departure_runway_request_at_holding_point_is_ready() {
    let mut e = engine();
    hold_short_with_transponder(&mut e, Some(4201), Some(TransponderMode::On));
    let text = format!("Krakow Tower, {CS}, request departure runway two five");
    let reply = items(&say(&mut e, TOWER, &text));
    assert!(is_takeoff_clearance(&reply), "{reply:?}");
}

/// Training strictness needs no readback of the code: an acknowledgement re-runs the
/// departure.
#[test]
fn check_squawk_acknowledged_in_training() {
    let (ap, procs) = epkk();
    let mut e = engine_for(ap, procs, Strictness::Training);
    let (reply, mut last) = ready_with_transponder(&mut e, Some(7000), Some(TransponderMode::On));
    assert_eq!(reply, vec![AtcItem::CheckSquawk(4201)]);
    let cs = Callsign::new("SPTST");
    assert!(e.strip(&cs).unwrap().pending_readback.is_none());
    set_transponder(&mut e, &mut last, |s| s.xpdr_code = Some(4201));
    let out = say(&mut e, TOWER, &format!("wilco, {CS_ABBR}"));
    assert!(is_takeoff_clearance(&items(&out)), "{out:?}");
}

/// No transponder data, or no code assigned: no check.
#[test]
fn no_squawk_check_without_data_or_code() {
    let mut e = engine();
    let (reply, _) = ready_with_transponder(&mut e, None, None);
    assert!(is_takeoff_clearance(&reply), "{reply:?}");

    let (ap, procs) = epkk();
    let cfg = EngineConfig {
        squawk_pool: 4208..=4209,
        ..EngineConfig::new(Strictness::Standard)
    };
    let mut e = engine_with(ap, procs, cfg);
    let (reply, _) = ready_with_transponder(&mut e, Some(7000), Some(TransponderMode::Standby));
    assert!(is_takeoff_clearance(&reply), "{reply:?}");
}

#[test]
fn wrong_readback_gets_negative() {
    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    let mut wrong = clearance.clone();
    for it in &mut wrong {
        if let AtcItem::Qnh(q) = it {
            *q = 1012;
        }
    }
    let out = say(&mut e, GROUND, &readback(&wrong, CS_ABBR));
    assert_eq!(items(&out), vec![AtcItem::Negative, AtcItem::Qnh(1013)]);
    let cs = Callsign::new("SPTST");
    assert!(e.strip(&cs).unwrap().pending_readback.is_some());

    // Missing items: asked to read them back.
    let out = say(
        &mut e,
        GROUND,
        &format!("QNH one zero one three, {CS_ABBR}"),
    );
    assert!(
        matches!(&items(&out)[..], [AtcItem::ReadBack(m)] if m.contains(&ReadbackItem::HoldShort("25".into()))),
        "{out:?}"
    );
}

#[test]
fn incursion_triggers_hold_position() {
    // No clearance at all: the aircraft lines up, takes off and lands on its own.
    let mut e = engine();
    let mut out = Vec::new();
    for s in track() {
        out.extend(e.on_telemetry(s));
    }
    let incursion = out
        .iter()
        .position(|o| o.message.items.contains(&AtcItem::HoldPosition))
        .expect("hold position");
    // The aircraft never left Ground (no handover readback): Ground speaks every deviation.
    assert!(out.iter().all(|o| {
        o.freq_khz == GROUND
            || !o.message.items.iter().any(|i| {
                matches!(
                    i,
                    AtcItem::RunwayIncursion { .. }
                        | AtcItem::TakenOffWithoutClearance
                        | AtcItem::LandedWithoutClearance
                )
            })
    }));
    assert_eq!(out[incursion].freq_khz, GROUND);
    assert_eq!(
        out[incursion].message.items,
        vec![
            AtcItem::HoldPosition,
            AtcItem::RunwayIncursion {
                runway: "25".into()
            }
        ]
    );
    assert!(
        out[incursion]
            .text
            .ends_with("hold position, you have entered runway two five without clearance"),
        "{}",
        out[incursion].text
    );
    let free: Vec<&AtcItem> = out
        .iter()
        .flat_map(|o| &o.message.items)
        .filter(|i| {
            matches!(
                i,
                AtcItem::RunwayIncursion { .. }
                    | AtcItem::TakenOffWithoutClearance
                    | AtcItem::LandedWithoutClearance
            )
        })
        .collect();
    // The uncleared landing is also an incursion; only the landing is spoken.
    assert_eq!(
        free,
        [
            &AtcItem::RunwayIncursion {
                runway: "25".into()
            },
            &AtcItem::TakenOffWithoutClearance,
            &AtcItem::LandedWithoutClearance,
        ]
    );
}

#[test]
fn altitude_deviation_on_downwind() {
    let mut e = engine();
    let mut out = Vec::new();
    for mut s in track() {
        if !s.on_ground {
            s.alt_ft_msl = s.alt_ft_msl.min(1300.0);
        }
        out.extend(e.on_telemetry(s));
    }
    assert!(
        out.iter().any(
            |o| o.message.items == [AtcItem::CheckAltitude { circuit_ft: 1800 }]
                && o.text
                    .ends_with("check altitude, circuit altitude one thousand eight hundred feet")
        ),
        "{out:?}"
    );
}

#[test]
fn transmission_on_unused_freq_is_silent() {
    let mut e = engine();
    assert!(
        say(
            &mut e,
            121500,
            &format!("Krakow Ground, {CS}, request taxi")
        )
        .is_empty()
    );
    assert!(say(&mut e, 121500, "Krakow Ground, request taxi").is_empty());
}

/// Runway 07/25 east-west at 50.0 N; taxiway A north of it, D south, stand S north.
fn crossing_airport() -> Airport {
    let (ap, _) = epkk();
    let node = |id, lat, lon| TaxiNode {
        id,
        pos: LatLon { lat, lon },
        name: String::new(),
    };
    let zone = |rwys: &[&str]| {
        vec![ActiveZone {
            kind: ZoneKind::Departure,
            runways: rwys.iter().map(|r| r.to_string()).collect(),
        }]
    };
    let edge = |from, to, kind, name: &str, zones| TaxiEdge {
        from,
        to,
        one_way: false,
        kind,
        name: name.into(),
        active_zones: zones,
    };
    let nodes = vec![
        node(1, 50.0, 19.004),
        node(2, 50.0, 19.010),
        node(3, 50.0, 19.016),
        node(10, 50.001, 19.004),
        node(11, 50.002, 19.004),
        node(12, 49.999, 19.016),
        node(13, 49.998, 19.016),
    ];
    let rz = zone(&["07", "25"]);
    let edges = vec![
        edge(1, 2, EdgeKind::Runway, "07/25", rz.clone()),
        edge(2, 3, EdgeKind::Runway, "07/25", rz),
        edge(10, 1, EdgeKind::Taxiway, "A", zone(&["07"])),
        edge(11, 10, EdgeKind::Taxiway, "A", vec![]),
        edge(12, 3, EdgeKind::Taxiway, "D", zone(&["25"])),
        edge(13, 12, EdgeKind::Taxiway, "D", vec![]),
    ];
    let end = |ident: &str, lon| RunwayEnd {
        ident: ident.into(),
        threshold: LatLon { lat: 50.0, lon },
        displaced_m: 0.0,
    };
    Airport {
        runways: vec![Runway {
            width_m: 45.0,
            ends: [end("07", 19.0), end("25", 19.02)],
        }],
        taxi: TaxiNetwork { nodes, edges },
        stands: vec![Stand {
            name: "S".into(),
            pos: LatLon {
                lat: 50.002,
                lon: 19.004,
            },
            heading_deg: 0.0,
        }],
        ..ap
    }
}

fn ground_sample(t: f64, lat: f64, lon: f64, gs_kt: f64) -> AircraftState {
    AircraftState {
        callsign: Callsign::new("SPTST"),
        t: SimTime(t),
        pos: LatLon { lat, lon },
        alt_ft_msl: 791.0,
        heading_deg: 270.0,
        ias_kt: gs_kt,
        gs_kt,
        vs_fpm: 0.0,
        on_ground: true,
        com1_khz: GROUND,
        xpdr_code: None,
        xpdr_mode: None,
        utc_s: None,
        day_of_year: None,
    }
}

#[test]
fn runway_crossing_cleared_on_approach() {
    let (_, procs) = epkk();
    let mut e = engine_for(crossing_airport(), procs, Strictness::Standard);
    // South of the runway, east end, taxiing.
    assert!(
        e.on_telemetry(ground_sample(0.0, 49.998, 19.016, 5.0))
            .is_empty()
    );
    let out = say(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {CS}, request taxi to stand Sierra"),
    );
    let clearance = items(&out);
    assert_eq!(
        clearance,
        vec![AtcItem::Taxi {
            to: TaxiTo::Stand { name: "S".into() },
            via: vec!["D".into(), "A".into()],
            crossings: vec!["25".into()],
        }]
    );
    assert!(say(&mut e, GROUND, &readback(&clearance, CS_ABBR)).is_empty());
    // Far from the runway: nothing yet.
    assert!(
        e.on_telemetry(ground_sample(1.0, 49.9983, 19.016, 5.0))
            .is_empty()
    );
    // Approaching the crossing entry (D hold line, 49.999 N): cross runway 25.
    let out = e.on_telemetry(ground_sample(2.0, 49.9988, 19.016, 5.0));
    assert_eq!(items(&out), vec![AtcItem::CrossRunway("25".into())]);
    let cs = Callsign::new("SPTST");
    assert!(e.strip(&cs).unwrap().permissions.may_enter_runway);
    assert!(say(&mut e, GROUND, &readback(&items(&out), CS_ABBR)).is_empty());
    // Across: the permission ends.
    let mut t = 3.0;
    for lat in [49.9996, 50.0, 50.0004, 50.0008, 50.001] {
        let lon = if lat < 50.0 { 19.016 } else { 19.004 };
        let out = e.on_telemetry(ground_sample(t, lat, lon, 5.0));
        assert!(!items(&out).contains(&AtcItem::HoldPosition), "{out:?}");
        t += 1.0;
    }
    assert!(!e.strip(&cs).unwrap().permissions.may_enter_runway);
}

/// The track shifted in time to follow `prev`.
fn after(prev: &[AircraftState], samples: Vec<AircraftState>) -> Vec<AircraftState> {
    let t0 = prev.last().map_or(0.0, |s| s.t.0 + 1.0);
    samples
        .into_iter()
        .map(|mut s| {
            s.t = SimTime(s.t.0 + t0);
            s
        })
        .collect()
}

#[test]
fn misheard_takeoff_readback_does_not_lock_session() {
    let mut e = engine();
    let pilot = Pilot {
        wrong_takeoff_readback: true,
        ..GOOD_PILOT
    };
    let (log, _) = fly(&mut e, track(), pilot);
    let tower: Vec<&Vec<AtcItem>> = log
        .iter()
        .filter(|(r, i)| *r == Role::Tower && !matches!(i[..], [AtcItem::ReadBack(_)]))
        .map(|(_, i)| i)
        .collect();
    assert!(matches!(tower[0][0], AtcItem::ClearedTakeoff { .. }));
    assert_eq!(
        *tower[1],
        vec![AtcItem::Sequence(1), AtcItem::Report(CircuitLeg::Final)]
    );
    assert!(matches!(tower[2][..], [AtcItem::ClearedLand { .. }]));

    // A readback asked for twice without success is then accepted.
    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    let wrong = format!("hold short runway zero seven, {CS_ABBR}");
    for _ in 0..2 {
        assert_eq!(items(&say(&mut e, GROUND, &wrong))[0], AtcItem::Negative);
    }
    assert!(say(&mut e, GROUND, &wrong).is_empty());
    let cs = Callsign::new("SPTST");
    assert_eq!(e.strip(&cs).unwrap().pending_readback, None);
    assert!(is_taxi_to_holding_point(&clearance[0]));
}

#[test]
fn full_stop_without_phrase_still_handed_to_ground() {
    let mut e = engine();
    let pilot = Pilot {
        full_stop: false,
        ..GOOD_PILOT
    };
    let first = track();
    let (log, _) = fly(&mut e, first.clone(), pilot);
    let final_clearance = log
        .iter()
        .find(|(_, i)| matches!(i[..], [AtcItem::ClearedTouchAndGo { .. }]));
    assert!(final_clearance.is_some(), "{log:?}");
    // The aircraft stopped on the runway anyway: vacate and contact Ground.
    assert!(log.contains(&(
        Role::Tower,
        vec![
            AtcItem::Vacate { via: None },
            AtcItem::Contact {
                station: "Kraków Ground".into(),
                khz: GROUND
            },
            AtcItem::Courtesy(Courtesy::GoodDay)
        ]
    )));
    let cs = Callsign::new("SPTST");
    assert_eq!(e.strip(&cs).unwrap().owner, Role::Ground);

    // Next departure: Ground hands over to Tower again.
    let (log, _) = fly(&mut e, after(&first, track()), GOOD_PILOT);
    assert!(log.contains(&(
        Role::Ground,
        vec![
            AtcItem::Contact {
                station: "Kraków Tower".into(),
                khz: TOWER
            },
            AtcItem::Courtesy(Courtesy::HaveANiceFlight)
        ]
    )));
}

#[test]
fn wrong_position_request_gets_contact() {
    let mut e = engine();
    let first = track().into_iter().next().unwrap();
    assert!(e.on_telemetry(first).is_empty());
    let out = say(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {CS}, ready for departure"),
    );
    assert_eq!(out[0].freq_khz, GROUND);
    assert_eq!(
        items(&out),
        vec![AtcItem::Contact {
            station: "Kraków Tower".into(),
            khz: TOWER
        }]
    );
    assert!(
        out[0]
            .text
            .ends_with("contact Kraków Tower one two three decimal two five five"),
        "{}",
        out[0].text
    );
    let out = say(&mut e, TOWER, &format!("Krakow Tower, {CS}, request taxi"));
    assert_eq!(out[0].freq_khz, TOWER);
    assert_eq!(
        items(&out),
        vec![AtcItem::Contact {
            station: "Kraków Ground".into(),
            khz: GROUND
        }]
    );
}

/// Telemetry up to and including the first sample on runway 25, after a correct taxi
/// clearance; returns the controller messages of the last sample.
fn roll_onto_runway(e: &mut AtcEngine) -> Vec<AtcOutput> {
    let clearance = taxi_cleared(e, "Alpha");
    assert!(say(e, GROUND, &readback(&clearance, CS_ABBR)).is_empty());
    let cs = Callsign::new("SPTST");
    for s in track().into_iter().skip(1) {
        let out = e.on_telemetry(s);
        if out
            .iter()
            .any(|o| o.message.items.contains(&AtcItem::HoldPosition))
        {
            return out;
        }
        // Ignore the handover: the pilot stays with Ground.
        assert_ne!(e.strip(&cs).unwrap().phase, FlightPhase::TakeoffRoll);
    }
    panic!("no incursion");
}

#[test]
fn hold_position_must_be_read_back() {
    let cs = Callsign::new("SPTST");
    let mut e = engine();
    let out = roll_onto_runway(&mut e);
    assert_eq!(out[0].freq_khz, GROUND);
    let hold = vec![ReadbackItem::Kind(ClearanceKind::HoldPosition)];
    let strip = e.strip(&cs).unwrap();
    assert_eq!(strip.pending_readback, Some(hold.clone()));
    assert_eq!(strip.handoff_to, None);
    assert!(say(&mut e, GROUND, &format!("holding position, {CS_ABBR}")).is_empty());
    assert_eq!(e.strip(&cs).unwrap().pending_readback, None);

    // No readback: asked twice, then accepted.
    let mut e = engine();
    roll_onto_runway(&mut e);
    for _ in 0..2 {
        let out = say(&mut e, GROUND, CS_ABBR);
        assert_eq!(items(&out), vec![AtcItem::ReadBack(hold.clone())]);
    }
    assert!(say(&mut e, GROUND, CS_ABBR).is_empty());
    assert_eq!(e.strip(&cs).unwrap().pending_readback, None);
}

#[test]
fn no_takeoff_clearance_while_taxiing() {
    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    assert!(say(&mut e, GROUND, &readback(&clearance, CS_ABBR)).is_empty());
    let cs = Callsign::new("SPTST");
    for s in track().into_iter().skip(1) {
        e.on_telemetry(s);
        if e.strip(&cs).unwrap().phase == FlightPhase::TaxiOut {
            break;
        }
    }
    let out = say(
        &mut e,
        TOWER,
        &format!("Krakow Tower, {CS}, ready for departure"),
    );
    assert_eq!(
        items(&out),
        vec![AtcItem::TaxiAndReportReady {
            runway: "25".into()
        }]
    );
    assert!(
        out[0]
            .text
            .ends_with("taxi to holding point runway two five, report ready"),
        "{}",
        out[0].text
    );
    let strip = e.strip(&cs).unwrap();
    assert!(!strip.permissions.may_take_off);
}

#[test]
fn runway_change_before_taxi_is_used() {
    let (ap, procs) = epkk();
    let samples = circuit_track(&ap, &procs, "07", 1.0);
    let mut e = engine(); // wind 250/8: runway 25
    let cs = Callsign::new("SPTST");
    assert!(e.on_telemetry(samples[0].clone()).is_empty());
    assert_eq!(e.strip(&cs).unwrap().runway, "25");
    let east = Wind {
        dir_deg: 70,
        speed_kt: 15,
        calm: false,
    };
    e.on_weather(Weather {
        wind: east,
        ..weather(1013)
    }); // information Bravo, runway 07
    let (log, _) = fly(&mut e, samples[1..].to_vec(), GOOD_PILOT);
    let all: Vec<&AtcItem> = log.iter().flat_map(|(_, i)| i).collect();
    assert!(
        all.iter()
            .any(|i| matches!(i, AtcItem::Taxi { to: TaxiTo::HoldingPoint { runway }, .. } if runway == "07")),
        "{log:?}"
    );
    assert!(all.contains(&&AtcItem::HoldShort("07".into())));
    assert!(all.contains(&&AtcItem::ClearedTakeoff {
        runway: "07".into(),
        wind: east
    }));
}

#[test]
fn atis_broadcast_letter_and_text() {
    let (ap, procs) = epkk();
    let mut e = AtcEngine::new(ap, procs, EngineConfig::new(Strictness::Standard));
    assert_eq!(e.atis_broadcast(), None);
    e.on_weather(weather(1013));
    let (letter, text) = e.atis_broadcast().unwrap();
    assert_eq!(letter, 'A');
    assert!(
        text.starts_with("Kraków information Alpha, runway in use two five"),
        "{text}"
    );
    e.on_weather(weather(1009));
    let (letter, text) = e.atis_broadcast().unwrap();
    assert_eq!(letter, 'B');
    assert!(text.contains("information Bravo"), "{text}");
}

/// A known aircraft whose request is not understood is asked to say again; plain
/// acknowledgements stay unanswered.
#[test]
fn unintelligible_request_says_again() {
    let mut e = engine();
    let out = say(
        &mut e,
        TOWER,
        &format!("Krakow Tower, {CS}, request flowers"),
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].message.items, vec![AtcItem::SayAgain]);
    assert!(out[0].text.ends_with("say again"), "{}", out[0].text);
    for ack in ["roger", "wilco", "thank you"] {
        assert!(
            say(&mut e, TOWER, &format!("{CS}, {ack}")).is_empty(),
            "{ack}"
        );
    }
}

/// Greetings in front of the call do not hide the request.
#[test]
fn greeting_before_request() {
    for greeting in ["Hello", "Good morning", "Hi, good afternoon"] {
        let mut e = engine();
        let first = track().into_iter().next().unwrap();
        assert!(e.on_telemetry(first).is_empty());
        let out = say(
            &mut e,
            GROUND,
            &format!("{greeting}, Krakow Ground, {CS}, information Alpha, request taxi"),
        );
        assert!(
            items(&out).iter().any(is_taxi_to_holding_point),
            "{greeting}"
        );
    }
}

/// A call with only the station and the callsign is answered with "pass your message".
#[test]
fn initial_call_gets_pass_your_message() {
    for text in [
        format!("Krakow Tower, {CS}"),
        format!("Good morning, Krakow Tower, {CS}"),
        format!("Hello Krakow Tower, {CS}, good day"),
    ] {
        let mut e = engine();
        let out = items(&say(&mut e, TOWER, &text));
        assert_eq!(out.last(), Some(&AtcItem::PassYourMessage), "{text}");
    }
}

/// Anything from a known aircraft that is neither understood nor an acknowledgement gets
/// "say again"; acknowledgements stay unanswered.
#[test]
fn every_transmission_is_answered() {
    let mut e = engine();
    for text in [
        format!("Krakow Tower, {CS}, we are at the, uh"),
        format!("Krakow Tower, {CS}, report final"),
    ] {
        assert_eq!(
            items(&say(&mut e, TOWER, &text)),
            vec![AtcItem::SayAgain],
            "{text}"
        );
    }
    for ack in ["copy", "thanks", "roger, thank you"] {
        assert!(
            say(&mut e, TOWER, &format!("{CS}, {ack}")).is_empty(),
            "{ack}"
        );
    }
}

/// A transmission broken off and finished in the next one is understood as a whole.
#[test]
fn interrupted_transmission_is_joined() {
    let mut e = engine();
    let first = track().into_iter().next().unwrap();
    assert!(e.on_telemetry(first).is_empty());
    let out = say(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {CS}, information Alpha"),
    );
    assert_eq!(items(&out), vec![AtcItem::SayAgain]);
    let out = e.on_transmission(GROUND, "request taxi", Lang::En, SimTime(5.0));
    assert!(items(&out).iter().any(is_taxi_to_holding_point), "{out:?}");
}

/// A fragment without the callsign is joined with the callsign said next.
#[test]
fn callsign_after_interrupted_request() {
    let mut e = engine();
    let first = track().into_iter().next().unwrap();
    assert!(e.on_telemetry(first).is_empty());
    let out = say(
        &mut e,
        GROUND,
        "Krakow Ground, information Alpha, request taxi",
    );
    assert!(matches!(
        items(&out)[..],
        [AtcItem::SayAgainCallsign { .. }]
    ));
    let out = e.on_transmission(GROUND, CS, Lang::En, SimTime(4.0));
    assert!(items(&out).iter().any(is_taxi_to_holding_point), "{out:?}");
}

/// A readback finished in a second transmission is accepted.
#[test]
fn interrupted_readback_is_joined() {
    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    let text = readback(&clearance, CS);
    let cut = text.find("hold short").expect(&text);
    let (a, b) = text.split_at(cut);
    let out = e.on_transmission(GROUND, a, Lang::En, SimTime(10.0));
    assert!(!out.is_empty());
    let out = e.on_transmission(GROUND, b, Lang::En, SimTime(14.0));
    assert!(out.is_empty(), "{out:?}");
    let cs = Callsign::new("SPTST");
    assert!(e.strip(&cs).unwrap().pending_readback.is_none());
}

/// Fragments too old are not joined.
#[test]
fn old_fragment_is_not_joined() {
    let mut e = engine();
    let first = track().into_iter().next().unwrap();
    assert!(e.on_telemetry(first).is_empty());
    say(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {CS}, information Alpha"),
    );
    let out = e.on_transmission(GROUND, "request taxi", Lang::En, SimTime(120.0));
    assert!(matches!(
        items(&out)[..],
        [AtcItem::SayAgainCallsign { .. }]
    ));
}

/// A transcript too unclear to use gets a station "say again".
#[test]
fn unclear_transmission_says_again() {
    let e = engine();
    let out = e.say_again(TOWER, Lang::En);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].message.to, None);
    assert_eq!(
        out[0].message.items,
        vec![AtcItem::SayAgainStation {
            station: "Kraków Tower".into()
        }]
    );
    assert_eq!(out[0].text, "Station calling Kraków Tower, say again");
    assert!(e.say_again(100_000, Lang::En).is_empty());
}

/// The aircraft is found when another letter group is taken for the callsign, or when a
/// letter of the callsign is lost.
#[test]
fn callsign_found_despite_misheard_letters() {
    for text in [
        format!("Krakow Ground, WNC, information Alpha, request taxi, {CS}"),
        format!("Krakow Ground, via Sierra zero six, information Alpha, request taxi, {CS}"),
        "Krakow Ground, Sierra Papa Sierra Tango, information Alpha, request taxi".to_string(),
    ] {
        let mut e = engine();
        let first = track().into_iter().next().unwrap();
        assert!(e.on_telemetry(first).is_empty());
        let out = say(&mut e, GROUND, &text);
        assert!(
            items(&out).iter().any(is_taxi_to_holding_point),
            "{text}: {out:?}"
        );
    }
}

/// "Zulu six, Alpha" heard as "zero six, Alpha" makes the parser take "06A" for the
/// callsign; the readback still reaches the aircraft (here: a correction, not
/// "say again your callsign").
#[test]
fn readback_with_digit_group_before_callsign() {
    let mut e = engine();
    taxi_cleared(&mut e, "Alpha");
    let out = say(
        &mut e,
        GROUND,
        &format!(
            "taxi to holding point runway two five via Sierra zero six, Alpha, hold short \
             runway two five, QNH one zero one three, {CS}"
        ),
    );
    assert_eq!(out.len(), 1);
    assert!(out[0].message.to.is_some(), "{out:?}");
}

/// A farewell is returned, also at the end of a correct readback: a wish with "thank you,
/// you too", a plain farewell with "good day".
#[test]
fn farewell_is_returned() {
    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    let you_too = vec![AtcItem::Courtesy(Courtesy::ThankYouYouToo)];
    let text = format!("{}, have a nice day", readback(&clearance, CS));
    assert_eq!(items(&say(&mut e, GROUND, &text)), you_too);
    for wish in [
        "have a nice flight",
        "safe flight",
        "thanks, have a good one",
    ] {
        let out = say(&mut e, GROUND, &format!("{CS}, {wish}"));
        assert_eq!(items(&out), you_too, "{wish}");
    }
    let good_day = vec![AtcItem::Courtesy(Courtesy::GoodDay)];
    for bye in ["good day", "bye bye", "thank you, good day", "cheers"] {
        let out = say(&mut e, GROUND, &format!("{CS}, {bye}"));
        assert_eq!(items(&out), good_day, "{bye}");
    }
}

/// A greeting is returned once per station, in front of the reply.
#[test]
fn greeting_is_returned() {
    for (said, back) in [
        ("Hello", Courtesy::Hello),
        ("Good morning", Courtesy::GoodMorning),
        ("Good afternoon", Courtesy::GoodAfternoon),
        ("Good day", Courtesy::GoodDay),
    ] {
        let mut e = engine();
        let first = track().into_iter().next().unwrap();
        assert!(e.on_telemetry(first).is_empty());
        let text = format!("{said}, Krakow Ground, {CS}, information Alpha, request taxi");
        let out = items(&say(&mut e, GROUND, &text));
        assert_eq!(out[0], AtcItem::Courtesy(back), "{said}");
        assert!(is_taxi_to_holding_point(&out[1]), "{said}");
        // Once per station: the repeated request is not greeted again.
        let out = items(&say(&mut e, GROUND, &text));
        assert!(!out.contains(&AtcItem::Courtesy(back)), "{said}: {out:?}");
    }
    // "Good day" after the callsign is a farewell, not a greeting.
    let mut e = engine();
    let out = say(&mut e, TOWER, &format!("Krakow Tower, {CS}, good day"));
    assert_eq!(items(&out), vec![AtcItem::PassYourMessage]);
}

/// A pilot reading back a take-off or landing clearance that was never given is told so.
#[test]
fn clearance_not_given_gets_negative() {
    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    assert!(say(&mut e, GROUND, &readback(&clearance, CS_ABBR)).is_empty());
    let cs = Callsign::new("SPTST");
    for s in track().into_iter().skip(1) {
        e.on_telemetry(s);
        if e.strip(&cs).unwrap().phase == FlightPhase::TaxiOut {
            break;
        }
    }
    let takeoff = AtcItem::NotClearedForTakeoff;
    for text in [
        format!("Hello, Krakow Tower, cleared for take-off, {CS}"),
        format!("Hello, Krakow Tower, {CS}, cleared for departure"),
    ] {
        let out = items(&say(&mut e, TOWER, &text));
        assert_eq!(out.last(), Some(&takeoff), "{text}");
    }
    assert_eq!(
        items(&say(&mut e, TOWER, &format!("{CS}, cleared to land"))),
        vec![AtcItem::NotClearedToLand]
    );
    assert!(!e.strip(&cs).unwrap().permissions.may_take_off);
}

/// [`crossing_airport`] with taxiway A named S, and SP-ABC south of the runway cleared to
/// the stand via D and S.
fn sierra_route() -> (AtcEngine, Vec<AtcItem>) {
    let (_, procs) = epkk();
    let mut ap = crossing_airport();
    for e in &mut ap.taxi.edges {
        if e.name == "A" {
            e.name = "S".into();
        }
    }
    let mut e = engine_for(ap, procs, Strictness::Standard);
    e.register_pilot(PilotProfile {
        callsign: Callsign::new("SPABC"),
        aircraft_type: "C172".into(),
    });
    let sample = AircraftState {
        callsign: Callsign::new("SPABC"),
        ..ground_sample(0.0, 49.998, 19.016, 5.0)
    };
    assert!(e.on_telemetry(sample).is_empty());
    let out = say(
        &mut e,
        GROUND,
        "Krakow Ground, Sierra Papa Alpha Bravo Charlie, request taxi to stand Sierra",
    );
    let clearance = items(&out);
    assert!(
        matches!(&clearance[..], [AtcItem::Taxi { via, .. }] if via == &["D", "S"]),
        "{clearance:?}"
    );
    (e, clearance)
}

/// Real readbacks: the last taxiway "Sierra" runs into the callsign "Sierra Bravo Charlie",
/// with Whisper's commas anywhere, also inside the callsign; a stuttered taxiway is one.
#[test]
fn readback_with_taxiway_before_callsign_is_correct() {
    for cs in [
        "Sierra Sierra Bravo Charlie",
        "Sierra Sierra, Bravo Charlie",
        "Sierra Sierra, Sierra, Bravo Charlie",
    ] {
        let (mut e, _) = sierra_route();
        let text = format!("Taxi to stand Sierra via Delta, {cs}");
        assert!(say(&mut e, GROUND, &text).is_empty(), "{text}");
        let strip = e.strip(&Callsign::new("SPABC")).unwrap();
        assert_eq!(strip.pending_readback, None, "{text}");
    }
}

/// "Say again" gets the last transmission to the aircraft again, word for word; the
/// readback it asked for is still open and not counted as failed.
#[test]
fn say_again_repeats_the_last_transmission() {
    let mut e = engine();
    let first = track().into_iter().next().unwrap();
    assert!(e.on_telemetry(first).is_empty());
    let clearance = say(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {CS}, information Alpha, request taxi"),
    );
    for text in [
        format!("{CS_ABBR}, say again"),
        format!("Say again all after taxi, {CS_ABBR}"),
        // No callsign: the only aircraft on frequency, the last one spoken to.
        "Say again please".to_string(),
        "repeat".to_string(),
    ] {
        let out = say(&mut e, GROUND, &text);
        assert_eq!(out, clearance, "{text}");
    }
    let strip = e.strip(&Callsign::new("SPTST")).unwrap();
    assert!(strip.pending_readback.is_some());
    assert_eq!(strip.readback_requests, 0);
    assert!(say(&mut e, GROUND, &readback(&items(&clearance), CS_ABBR)).is_empty());
}

/// Without a callsign, "say again" is only answered for the one aircraft on frequency.
#[test]
fn say_again_without_callsign_two_aircraft() {
    let mut e = engine();
    taxi_cleared(&mut e, "Alpha");
    taxi_cleared_as(&mut e, "SPTSU", "Sierra Papa Tango Sierra Uniform");
    let out = say(&mut e, GROUND, "Say again please");
    assert_eq!(
        items(&out),
        vec![AtcItem::SayAgainCallsign {
            station: "Kraków Ground".into()
        }]
    );
    // With the callsign it is.
    let out = say(&mut e, GROUND, "Say again, Sierra Sierra Uniform");
    assert_eq!(out[0].message.to, Some(Callsign::new("SPTSU")));
    assert!(items(&out).iter().any(is_taxi_to_holding_point), "{out:?}");
}

/// A pilot unsure of the way gets the route again from where it is, to read back.
#[test]
fn progressive_taxi_gives_the_route_again() {
    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    assert!(say(&mut e, GROUND, &readback(&clearance, CS_ABBR)).is_empty());
    let out = say(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {CS_ABBR}, request progressive taxi"),
    );
    let help = items(&out);
    assert!(help.iter().any(is_taxi_to_holding_point), "{help:?}");
    assert!(help.contains(&AtcItem::HoldShort("25".into())), "{help:?}");
    assert!(
        !help
            .iter()
            .any(|i| matches!(i, AtcItem::Qnh(_) | AtcItem::Squawk(_))),
        "{help:?}"
    );
    let strip = e.strip(&Callsign::new("SPTST")).unwrap();
    assert!(strip.pending_readback.is_some());
    assert!(say(&mut e, GROUND, &readback(&help, CS_ABBR)).is_empty());
}

/// Real: asked while the taxi-in readback is open; the new route replaces it.
#[test]
fn taxi_help_while_readback_pending() {
    let (mut e, clearance) = sierra_route();
    let out = say(
        &mut e,
        GROUND,
        "Kraków Ground, could you get me how to taxi to the Sierra Sierra Bravo Charlie",
    );
    assert_eq!(items(&out), clearance);
}

/// No taxi clearance to help with: "say intentions".
#[test]
fn taxi_help_without_clearance() {
    let mut e = engine();
    let first = track().into_iter().next().unwrap();
    assert!(e.on_telemetry(first).is_empty());
    let out = say(&mut e, GROUND, &format!("{CS}, we are lost"));
    assert_eq!(items(&out), vec![AtcItem::SayIntentions]);
}

/// Real: "requests" and a message that names the aircraft but asks nothing known.
#[test]
fn real_requests_are_answered() {
    let (mut e, _) = sierra_route();
    assert!(
        say(
            &mut e,
            GROUND,
            "Taxi to stand Sierra via Delta, Sierra, Sierra Bravo Charlie"
        )
        .is_empty()
    );
    let out = say(
        &mut e,
        GROUND,
        "Kraków Ground's Sierra Bravo Charlie requests taxi to stand.",
    );
    // Not parked at a stand before: which one.
    assert_eq!(items(&out), vec![AtcItem::SayStand]);
    let out = say(&mut e, GROUND, "taxi to the moon, Sierra Bravo Charlie");
    assert_eq!(items(&out), vec![AtcItem::SayAgain]);
}

const ABC: &str = "Sierra Papa Alpha Bravo Charlie";

/// EPKK with SP-ABC standing `m` metres from stand 20, on the bearing `from_deg` from the
/// stand, heading `heading_deg`, at `gs_kt`.
fn abc_near_stand_20(m: f64, from_deg: f64, heading_deg: f64, gs_kt: f64) -> AtcEngine {
    let mut e = engine();
    e.register_pilot(PilotProfile {
        callsign: Callsign::new("SPABC"),
        aircraft_type: "C172".into(),
    });
    let (ap, _) = epkk();
    let stand = ap.stands.iter().find(|s| s.name == "20").unwrap().pos;
    let pos = atc_geo::destination(stand, from_deg, m);
    let s = AircraftState {
        callsign: Callsign::new("SPABC"),
        heading_deg,
        ..ground_sample(0.0, pos.lat, pos.lon, gs_kt)
    };
    assert!(e.on_telemetry(s).is_empty());
    e
}

/// Moves SP-ABC (heading and speed kept) to `m` metres from stand 20 at sim time `t`.
fn move_abc(e: &mut AtcEngine, t: f64, m: f64, from_deg: f64) -> Vec<AtcOutput> {
    let (ap, _) = epkk();
    let stand = ap.stands.iter().find(|s| s.name == "20").unwrap().pos;
    let pos = atc_geo::destination(stand, from_deg, m);
    e.on_telemetry(AircraftState {
        callsign: Callsign::new("SPABC"),
        heading_deg: (from_deg + 180.0) % 360.0,
        ..ground_sample(t, pos.lat, pos.lon, 5.0)
    })
}

fn stand_20(metres: u32, side: Side) -> AtcItem {
    AtcItem::StandPosition {
        stand: "20".into(),
        metres,
        side,
    }
}

/// Real transcripts: "on stand" got "say again".
#[test]
fn on_stand_gets_good_day() {
    for (said, back) in [
        ("on stand, have a good day.", Courtesy::ThankYouYouToo),
        (", on stand", Courtesy::GoodDay),
        ("on stand, good day", Courtesy::GoodDay),
    ] {
        let mut e = abc_near_stand_20(10.0, 167.7, 347.7, 0.0);
        let out = say(&mut e, GROUND, &format!("{ABC} {said}"));
        assert_eq!(items(&out), [AtcItem::Courtesy(back)], "{said}");
        assert!(out[0].text.starts_with(ABC), "{}", out[0].text);
    }
}

/// Not on the stand yet: where it is, relative to the nose, to 10 m.
#[test]
fn on_stand_report_short_of_the_stand() {
    for (from, heading, side, words) in [
        (167.7, 347.7, Side::Ahead, "ahead"),
        (167.7, 257.7, Side::Right, "on your right"),
        (167.7, 77.7, Side::Left, "on your left"),
        (347.7, 347.7, Side::Behind, "behind you"),
    ] {
        let mut e = abc_near_stand_20(118.0, from, heading, 0.0);
        say(
            &mut e,
            GROUND,
            &format!("{ABC}, request taxi to stand two zero"),
        );
        let out = say(&mut e, GROUND, &format!("{ABC}, on stand"));
        assert_eq!(items(&out), [stand_20(120, side)], "{words}");
        assert!(
            out[0].text.ends_with(&format!(
                "stand two zero is one hundred twenty metres {words}"
            )),
            "{}",
            out[0].text
        );
    }
    // Rolling across the stand is not being on it.
    let mut e = abc_near_stand_20(10.0, 167.7, 347.7, 8.0);
    let out = say(&mut e, GROUND, &format!("{ABC}, on stand"));
    assert_eq!(items(&out), [stand_20(10, Side::Ahead)]);
}

/// Real: "understand" got "say again"; acknowledgements stay unanswered.
#[test]
fn acknowledgements_are_not_answered() {
    let mut e = abc_near_stand_20(300.0, 167.7, 347.7, 0.0);
    for ack in ["understand", "understood", "roger", "wilco", "copy"] {
        let out = say(&mut e, GROUND, &format!("{ABC}, {ack}"));
        assert!(out.is_empty(), "{ack}: {out:?}");
    }
    let out = say(&mut e, GROUND, &format!("{ABC}, thank you"));
    assert!(out.is_empty(), "{out:?}");
}

/// Taxi help close to the stand says where it is; the second help sends the follow-me
/// car, which leads after [`FOLLOW_ME_READY_S`].
#[test]
fn second_taxi_help_sends_the_follow_me_car() {
    let mut e = abc_near_stand_20(300.0, 167.7, 347.7, 5.0);
    let clearance = items(&say(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {ABC}, request taxi to stand two zero"),
    ));
    assert!(
        matches!(&clearance[..], [AtcItem::Taxi { .. }]),
        "{clearance:?}"
    );
    assert!(e.take_vehicle_orders().is_empty());
    let help = items(&say(
        &mut e,
        GROUND,
        "Sierra Papa Alpha Bravo Charlie, I am lost, I need help, taxi",
    ));
    assert_eq!(help[0], clearance[0], "{help:?}");
    assert_eq!(help[1..], [stand_20(300, Side::Ahead)]);

    let out = say(&mut e, GROUND, &format!("{ABC}, I am lost, I need help"));
    assert_eq!(
        items(&out),
        [AtcItem::HoldPosition, AtcItem::FollowMeOnTheWay]
    );
    assert_eq!(
        out[0].text,
        "Sierra Bravo Charlie, hold position, follow-me car is on the way"
    );
    let orders = e.take_vehicle_orders();
    let [
        VehicleOrder {
            callsign,
            command:
                VehicleCommand::FollowMe {
                    id,
                    path,
                    end_heading_deg,
                },
        },
    ] = &orders[..]
    else {
        panic!("{orders:?}")
    };
    assert_eq!(callsign, &Callsign::new("SPABC"));
    let (ap, _) = epkk();
    let stand = ap.stands.iter().find(|s| s.name == "20").unwrap();
    assert_eq!(path.last(), Some(&stand.pos));
    assert!(path.len() >= 2, "{path:?}");
    assert_eq!(*end_heading_deg, stand.heading_deg);
    assert!(say(&mut e, GROUND, &format!("Holding position, {ABC}")).is_empty());

    // In front of the aircraft after a while: "follow the follow-me car".
    assert!(move_abc(&mut e, FOLLOW_ME_READY_S - 1.0, 300.0, 167.7).is_empty());
    let out = move_abc(&mut e, FOLLOW_ME_READY_S, 300.0, 167.7);
    // `follow_me_phrase_names_the_stand`: the instruction says where the car leads.
    assert_eq!(
        items(&out),
        vec![AtcItem::FollowTheFollowMe {
            to: TaxiTo::Stand { name: "20".into() }
        }]
    );
    assert_eq!(
        out[0].text,
        "Sierra Bravo Charlie, follow the follow-me car to stand two zero"
    );
    assert!(move_abc(&mut e, FOLLOW_ME_READY_S + 1.0, 290.0, 167.7).is_empty());
    for ack in [
        "Follow the follow-me, Sierra Bravo Charlie",
        "Wilco, Sierra Bravo Charlie",
    ] {
        assert!(say(&mut e, GROUND, ack).is_empty(), "{ack}");
    }
    // Asked again: the car is already leading.
    let out = say(&mut e, GROUND, &format!("{ABC}, request follow me"));
    assert_eq!(
        out[0].text,
        "Sierra Bravo Charlie, follow the follow-me car to stand two zero"
    );
    assert!(e.take_vehicle_orders().is_empty());
    let strip = e.strip(&Callsign::new("SPABC")).unwrap();
    assert_eq!(
        strip.follow_me.as_ref().map(|f| (f.id, f.leading)),
        Some((*id, true))
    );

    // A new taxi clearance sends the car away.
    say(
        &mut e,
        GROUND,
        &format!("{ABC}, request taxi to stand one niner"),
    );
    assert_eq!(
        e.take_vehicle_orders(),
        vec![VehicleOrder {
            callsign: Callsign::new("SPABC"),
            command: VehicleCommand::FollowMeCancel { id: *id }
        }]
    );
}

/// To the holding point the car stops short of it.
#[test]
fn follow_me_to_the_holding_point() {
    let mut e = engine();
    let clearance = taxi_cleared(&mut e, "Alpha");
    assert!(say(&mut e, GROUND, &readback(&clearance, CS_ABBR)).is_empty());
    let out = say(&mut e, GROUND, &format!("{CS_ABBR}, request follow-me"));
    assert_eq!(items(&out)[0], AtcItem::HoldPosition);
    let orders = e.take_vehicle_orders();
    let [
        VehicleOrder {
            command: VehicleCommand::FollowMe { path, .. },
            ..
        },
    ] = &orders[..]
    else {
        panic!("{orders:?}")
    };
    let (ap, _) = epkk();
    let graph = atc_core::taxi::TaxiGraph::build(&ap.taxi);
    let holds: Vec<LatLon> = graph
        .holding_points(&ap, "25")
        .into_iter()
        .filter_map(|n| graph.node_pos(n))
        .collect();
    let end = *path.last().unwrap();
    let to_hold = holds
        .iter()
        .map(|&h| atc_geo::distance_m(h, end))
        .fold(f64::INFINITY, f64::min);
    assert!((to_hold - 20.0).abs() < 1.0, "{to_hold}");
}

/// Real: "Dunn, wind, full stop" is the downwind report.
#[test]
fn misheard_downwind_report_is_answered() {
    let mut e = engine();
    let cs = Callsign::new("SPTST");
    for s in track() {
        e.on_telemetry(s);
        if e.strip(&cs).unwrap().phase == FlightPhase::Circuit(CircuitLeg::Downwind) {
            break;
        }
    }
    let out = say(&mut e, TOWER, &format!("{CS} Dunn, wind, full stop"));
    assert_eq!(
        items(&out),
        vec![AtcItem::Sequence(1), AtcItem::Report(CircuitLeg::Final)]
    );
    assert!(say(&mut e, TOWER, &format!("#1, report final, {CS_ABBR}")).is_empty());
}

const PL_CS: &str = "Sierra Papa Alfa Bravo Charlie";
const PL_CS_ABBR: &str = "Sierra Bravo Charlie";
const PL_TAXI: &str =
    "Kraków Ground, Sierra Papa Alfa Bravo Charlie, informacja Alfa, proszę o kołowanie do kręgów";

/// An engine in `mode` with SP-ABC on its stand.
fn engine_in(mode: LanguageMode) -> AtcEngine {
    let (ap, procs) = epkk();
    let cfg = EngineConfig {
        language: mode,
        ..EngineConfig::new(Strictness::Standard)
    };
    let mut e = engine_with(ap, procs, cfg);
    e.register_pilot(PilotProfile {
        callsign: Callsign::new("SPABC"),
        aircraft_type: "C172".into(),
    });
    let first = AircraftState {
        callsign: Callsign::new("SPABC"),
        ..track().into_iter().next().unwrap()
    };
    assert!(e.on_telemetry(first).is_empty());
    e
}

fn say_in(e: &mut AtcEngine, freq: u32, text: &str, lang: Lang) -> Vec<AtcOutput> {
    e.on_transmission(freq, text, lang, SimTime(0.0))
}

fn spabc_lang(e: &AtcEngine) -> Lang {
    e.strip(&Callsign::new("SPABC")).unwrap().lang
}

/// Auto: a Polish request gets a Polish clearance from the same station.
#[test]
fn polish_taxi_request_gets_polish_clearance() {
    let mut e = engine_in(LanguageMode::Auto);
    assert_eq!(spabc_lang(&e), Lang::En);
    let out = say_in(&mut e, GROUND, PL_TAXI, Lang::Pl);
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(
        out[0].text.starts_with(&format!(
            "{PL_CS}, kołuj do punktu oczekiwania przed pasem dwa pięć"
        )),
        "{}",
        out[0].text
    );
    assert_eq!(out[0].station, "Kraków Ground");
    assert_eq!(out[0].message.lang, Lang::Pl);
    assert_eq!(spabc_lang(&e), Lang::Pl);
}

/// Auto: an English call understood in English switches the session back.
#[test]
fn language_switches_back_to_english() {
    let mut e = engine_in(LanguageMode::Auto);
    say_in(&mut e, GROUND, PL_TAXI, Lang::Pl);
    let out = say_in(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {PL_CS_ABBR}, request taxi"),
        Lang::En,
    );
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(
        out[0].text.starts_with("Sierra Bravo Charlie, read back"),
        "{}",
        out[0].text
    );
    assert_eq!(spabc_lang(&e), Lang::En);
}

/// Auto: a short phrase tagged with the other language does not flip the session.
#[test]
fn misdetected_short_phrase_keeps_language() {
    let mut e = engine_in(LanguageMode::Auto);
    say_in(&mut e, GROUND, PL_TAXI, Lang::Pl);
    say_in(&mut e, GROUND, "roger", Lang::En);
    assert_eq!(spabc_lang(&e), Lang::Pl);
    let out = say_in(&mut e, GROUND, &format!("{PL_CS_ABBR}, roger"), Lang::En);
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].message.lang, Lang::Pl, "{}", out[0].text);
    assert!(out[0].text.contains("powtórz"), "{}", out[0].text);
    assert_eq!(spabc_lang(&e), Lang::Pl);
    // Not understood in either language: answered in the session language.
    let out = say_in(
        &mut e,
        GROUND,
        &format!("{PL_CS_ABBR}, blah blah"),
        Lang::En,
    );
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].message.lang, Lang::Pl, "{}", out[0].text);
    assert_eq!(spabc_lang(&e), Lang::Pl);
}

/// Auto, English session: English words tagged `pl` (the Polish parser passes them
/// through) do not switch the aircraft to Polish.
#[test]
fn english_misdetected_as_polish_keeps_english() {
    let mut e = engine_in(LanguageMode::Auto);
    let out = say_in(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {PL_CS}, information Alpha, request taxi"),
        Lang::Pl,
    );
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].message.lang, Lang::En, "{}", out[0].text);
    assert!(
        out[0].text.contains("taxi to holding point"),
        "{}",
        out[0].text
    );
    assert_eq!(spabc_lang(&e), Lang::En);
    assert_eq!(e.atis_lang(), Lang::En);
    let clearance = items(&out);
    let out = say_in(
        &mut e,
        GROUND,
        &format!("{PL_CS_ABBR}, request taxi"),
        Lang::Pl,
    );
    assert_eq!(out[0].message.lang, Lang::En, "{}", out[0].text);
    assert!(out[0].text.contains("read back"), "{}", out[0].text);
    let out = say_in(&mut e, GROUND, &readback(&clearance, PL_CS_ABBR), Lang::Pl);
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(spabc_lang(&e), Lang::En);
}

/// Readbacks are judged on items: a Polish clearance read back in English is accepted.
#[test]
fn polish_clearance_english_readback_accepted() {
    let mut e = engine_in(LanguageMode::Auto);
    let clearance = items(&say_in(&mut e, GROUND, PL_TAXI, Lang::Pl));
    let out = say_in(&mut e, GROUND, &readback(&clearance, PL_CS_ABBR), Lang::En);
    assert!(out.is_empty(), "{out:?}");
    let strip = e.strip(&Callsign::new("SPABC")).unwrap();
    assert!(strip.pending_readback.is_none());
}

/// Fixed English (the default): the language tag is ignored.
#[test]
fn fixed_english_ignores_polish_tag() {
    let mut e = engine();
    let first = track().into_iter().next().unwrap();
    assert!(e.on_telemetry(first).is_empty());
    let out = say_in(
        &mut e,
        GROUND,
        &format!("Krakow Ground, {CS}, information Alpha, request taxi"),
        Lang::Pl,
    );
    assert!(items(&out).iter().any(is_taxi_to_holding_point), "{out:?}");
    assert_eq!(out[0].message.lang, Lang::En);
    assert!(
        out[0].text.contains("taxi to holding point"),
        "{}",
        out[0].text
    );
}

/// Auto: the ATIS is broadcast in the language of the last understood transmission.
#[test]
fn atis_follows_session_language() {
    let mut e = engine_in(LanguageMode::Auto);
    assert_eq!(e.atis_lang(), Lang::En);
    let (_, text) = e.atis_broadcast().unwrap();
    assert!(text.starts_with("Kraków information Alpha"), "{text}");
    say_in(&mut e, GROUND, PL_TAXI, Lang::Pl);
    assert_eq!(e.atis_lang(), Lang::Pl);
    let (_, text) = e.atis_broadcast().unwrap();
    assert!(text.starts_with("Kraków, informacja Alfa"), "{text}");
    let fixed = engine_in(LanguageMode::Fixed(Lang::Pl));
    assert_eq!(fixed.atis_lang(), Lang::Pl);
    assert_eq!(spabc_lang(&fixed), Lang::Pl);
}

/// "Say again" to an unclear transmission and to an unknown callsign go through the renderer:
/// Polish in a Polish session.
#[test]
fn say_again_in_session_language() {
    let mut e = engine_in(LanguageMode::Fixed(Lang::Pl));
    let out = e.say_again(TOWER, Lang::En);
    assert_eq!(out[0].text, "wołający Kraków Wieża, powtórz");
    assert_eq!(out[0].station, "Kraków Tower");
    let out = say_in(
        &mut e,
        GROUND,
        "Kraków Ground, proszę o kołowanie",
        Lang::En,
    );
    assert_eq!(
        out[0].text,
        "wołający Kraków Ground, powtórz znak wywoławczy"
    );
    // Auto: the callsign unknown, the transmission's language.
    let mut e = engine_in(LanguageMode::Auto);
    let out = say_in(
        &mut e,
        GROUND,
        "Kraków Ground, proszę o kołowanie",
        Lang::Pl,
    );
    assert_eq!(
        out[0].text,
        "wołający Kraków Ground, powtórz znak wywoławczy"
    );
    let out = say_in(&mut e, GROUND, "Krakow Ground, request taxi", Lang::En);
    assert_eq!(
        out[0].text,
        "Station calling Kraków Ground, say again your callsign"
    );
}

/// Auto: an unclear transmission is asked again in the speaker's language, not in the
/// language of the last understood transmission (another aircraft's).
#[test]
fn say_again_in_speaker_language() {
    let mut e = engine_in(LanguageMode::Auto);
    say_in(&mut e, GROUND, PL_TAXI, Lang::Pl);
    assert_eq!(e.atis_lang(), Lang::Pl);
    let out = e.say_again(TOWER, Lang::En);
    assert_eq!(out[0].text, "Station calling Kraków Tower, say again");
    let out = e.say_again(TOWER, Lang::Pl);
    assert_eq!(out[0].text, "wołający Kraków Wieża, powtórz");
}

/// Auto: when the parsed callsign is no registered aircraft, the letter groups of a Polish
/// transmission are read in Polish: "Sierra trzy dwa jeden" is S321, SP321 with a letter
/// lost.
#[test]
fn polish_digit_callsign_found_in_letter_groups() {
    let (ap, procs) = epkk();
    let cfg = EngineConfig {
        language: LanguageMode::Auto,
        ..EngineConfig::new(Strictness::Standard)
    };
    let mut e = engine_with(ap, procs, cfg);
    e.register_pilot(PilotProfile {
        callsign: Callsign::new("SP321"),
        aircraft_type: "C172".into(),
    });
    let first = AircraftState {
        callsign: Callsign::new("SP321"),
        ..track().into_iter().next().unwrap()
    };
    assert!(e.on_telemetry(first).is_empty());
    let text =
        "Kraków Ground, Sierra trzy dwa jeden, informacja Alfa, proszę o kołowanie do kręgów";
    let out = say_in(&mut e, GROUND, text, Lang::Pl);
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].message.to, Some(Callsign::new("SP321")), "{out:?}");
    assert!(items(&out).iter().any(is_taxi_to_holding_point), "{out:?}");
    assert_eq!(out[0].message.lang, Lang::Pl);
}

/// Auto, two aircraft on one frequency: each is answered in its own language, and one
/// switching language leaves the other's as it was.
#[test]
fn two_aircraft_each_in_own_language() {
    let mut e = engine_in(LanguageMode::Auto);
    let sptst = |e: &AtcEngine| e.strip(&Callsign::new("SPTST")).unwrap().lang;
    let first = track().into_iter().next().unwrap();
    assert!(e.on_telemetry(first).is_empty());
    assert_eq!((spabc_lang(&e), sptst(&e)), (Lang::En, Lang::En));

    let out = say_in(&mut e, GROUND, PL_TAXI, Lang::Pl);
    assert_eq!(out[0].message.to, Some(Callsign::new("SPABC")));
    assert_eq!(out[0].message.lang, Lang::Pl);
    assert!(
        out[0].text.contains("kołuj do punktu oczekiwania"),
        "{}",
        out[0].text
    );
    assert_eq!((spabc_lang(&e), sptst(&e)), (Lang::Pl, Lang::En));

    let en_taxi = format!("Krakow Ground, {CS}, information Alpha, request taxi");
    let out = say_in(&mut e, GROUND, &en_taxi, Lang::En);
    assert_eq!(out[0].message.to, Some(Callsign::new("SPTST")));
    assert_eq!(out[0].message.lang, Lang::En);
    assert!(
        out[0].text.contains("taxi to holding point"),
        "{}",
        out[0].text
    );
    assert_eq!((spabc_lang(&e), sptst(&e)), (Lang::Pl, Lang::En));

    let out = say_in(&mut e, GROUND, &format!("{PL_CS_ABBR}, powtórz"), Lang::Pl);
    assert_eq!(out[0].message.to, Some(Callsign::new("SPABC")), "{out:?}");
    assert_eq!(out[0].message.lang, Lang::Pl);
    assert!(
        out[0].text.contains("kołuj do punktu oczekiwania"),
        "{}",
        out[0].text
    );

    // SP-ABC switches to English; SP-TST is not touched, and vice versa.
    let en = "Krakow Ground, Sierra Papa Alpha Bravo Charlie, information Alpha, request taxi";
    let out = say_in(&mut e, GROUND, en, Lang::En);
    assert_eq!(out[0].message.lang, Lang::En, "{out:?}");
    assert_eq!((spabc_lang(&e), sptst(&e)), (Lang::En, Lang::En));
    let pl = format!("Kraków Ground, {CS}, informacja Alfa, proszę o kołowanie do kręgów");
    let out = say_in(&mut e, GROUND, &pl, Lang::Pl);
    assert_eq!(out[0].message.to, Some(Callsign::new("SPTST")), "{out:?}");
    assert_eq!(out[0].message.lang, Lang::Pl, "{out:?}");
    assert_eq!((spabc_lang(&e), sptst(&e)), (Lang::En, Lang::Pl));
}

/// Urgent instructions are always English (spec, "Язык"): a Polish aircraft entering the
/// runway without clearance is told in English; its next ordinary reply is Polish again.
#[test]
fn urgent_instructions_in_english_in_polish_session() {
    let mut e = engine_in(LanguageMode::Auto);
    let out = say_in(&mut e, GROUND, PL_TAXI, Lang::Pl);
    assert_eq!(out[0].message.lang, Lang::Pl);
    let mut urgent = None;
    for s in track() {
        let out = e.on_telemetry(AircraftState {
            callsign: Callsign::new("SPABC"),
            ..s
        });
        if let Some(o) = out
            .into_iter()
            .find(|o| o.message.items.contains(&AtcItem::HoldPosition))
        {
            urgent = Some(o);
            break;
        }
    }
    let urgent = urgent.expect("hold position");
    assert_eq!(urgent.message.lang, Lang::En);
    assert_eq!(
        urgent.text,
        "Sierra Bravo Charlie, hold position, you have entered runway two five without clearance"
    );
    assert_eq!(spabc_lang(&e), Lang::Pl);
    // "Say again" repeats it word for word, still in English.
    let out = say_in(&mut e, GROUND, &format!("{PL_CS_ABBR}, powtórz"), Lang::Pl);
    assert_eq!(out[0].text, urgent.text);
    // Read back in Polish; the next ordinary reply is Polish again.
    assert!(say_in(&mut e, GROUND, &format!("stoję, {PL_CS_ABBR}"), Lang::Pl).is_empty());
    let strip = e.strip(&Callsign::new("SPABC")).unwrap();
    assert!(strip.pending_readback.is_none());
    let out = say_in(
        &mut e,
        GROUND,
        &format!("Kraków Ground, {PL_CS_ABBR}, proszę o pomoc"),
        Lang::Pl,
    );
    assert_eq!(out[0].message.lang, Lang::Pl, "{out:?}");
    assert_eq!(out[0].text, "Sierra Bravo Charlie, podaj zamiary");
    assert_eq!(spabc_lang(&e), Lang::Pl);
}

/// A Polish readback repeated after it was accepted acknowledges: inflected words
/// ("kołuję" for "kołuj") are the words it was told.
#[test]
fn repeated_polish_readback_is_acknowledged() {
    let mut e = engine_in(LanguageMode::Auto);
    let out = say_in(&mut e, GROUND, PL_TAXI, Lang::Pl);
    let rb = format!("{} {PL_CS_ABBR}", polish_readback(&out[0].text));
    assert!(say_in(&mut e, GROUND, &rb, Lang::Pl).is_empty(), "{rb}");
    let strip = e.strip(&Callsign::new("SPABC")).unwrap();
    assert!(strip.pending_readback.is_none());
    let out = say_in(&mut e, GROUND, &rb, Lang::Pl);
    assert!(out.is_empty(), "{rb}: {out:?}");
}

/// The pilot's Polish readback of Ground's Polish taxi clearance `text`: the clearance
/// without the callsign, verbs in the first person ("kołuj" -> "kołuję"), the runway
/// in the nominative once ("pas" for "przed pasem").
fn polish_readback(text: &str) -> String {
    let body = text.split_once(", ").map_or(text, |(_, b)| b);
    let body = body.replacen("oczekiwania przed pasem", "oczekiwania pas", 1);
    body.split(' ')
        .map(|w| match w {
            "kołuj" => "kołuję",
            "oczekuj" => "oczekuję",
            w => w,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// "z pełnym zatrzymaniem" with the Polish downwind report asks for a full stop, as
/// "full stop" does in English: the landing clearance is not a touch-and-go.
#[test]
fn polish_full_stop_is_heard() {
    for mode in [LanguageMode::Fixed(Lang::Pl), LanguageMode::Auto] {
        let mut e = engine_in(mode);
        let text = format!("{PL_CS_ABBR}, z wiatrem, z pełnym zatrzymaniem");
        say_in(&mut e, TOWER, &text, Lang::Pl);
        let strip = e.strip(&Callsign::new("SPABC")).unwrap();
        assert!(strip.full_stop_requested, "{mode:?}");
    }
}

/// Delivery, IFR/VFR clearances, start-up and the sim UTC time, on the fictional ZZZZ.
mod delivery {
    use std::path::PathBuf;

    use atc_navdata::cifp::AltConstraint;
    use atc_navdata::model::Destination;
    use atc_navdata::xplane::{load_destinations, load_procedure_set, resolve_airport};
    use atc_phraseology::{Level, ReadbackItem, UnableReason};

    use super::*;

    const DEL: u32 = 121900;
    const GND: u32 = 121700;
    const STAND: LatLon = LatLon {
        lat: 10.0012,
        lon: 10.0,
    };
    /// Day of year in summer time (day 0 = 1 Jan).
    const SUMMER: u16 = 200;
    const WINTER: u16 = 10;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../atc-navdata/tests/fixtures/xplane")
    }

    fn zzzz() -> (Airport, Procedures) {
        let ap = resolve_airport(&root(), "ZZZZ")
            .unwrap()
            .parse("ZZZZ")
            .unwrap();
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/ZZZZ/procedures.toml"
        ))
        .unwrap();
        (ap, parse_procedures(&text).unwrap())
    }

    fn destinations() -> Vec<Destination> {
        load_destinations(
            &root(),
            LatLon {
                lat: 10.0,
                lon: 10.0,
            },
            100.0,
            &[],
        )
        .unwrap()
    }

    /// ZZZZ with its procedures and destinations (`ifr`) or without IFR data, SPTST registered.
    fn engine_ifr(ifr: bool, language: LanguageMode) -> AtcEngine {
        let (ap, procs) = zzzz();
        let cfg = EngineConfig {
            language,
            ..EngineConfig::new(Strictness::Standard)
        };
        let mut e = AtcEngine::new(ap, procs, cfg).with_destinations(destinations());
        if ifr {
            e = e.with_procedure_set(load_procedure_set(&root(), "ZZZZ").unwrap());
        }
        e.register_pilot(PilotProfile {
            callsign: Callsign::new("SPTST"),
            aircraft_type: "C172".into(),
        });
        e.on_weather(weather(1013));
        e
    }

    fn engine() -> AtcEngine {
        engine_ifr(true, LanguageMode::Fixed(Lang::En))
    }

    /// SPTST parked on the stand at `utc_s` on `day`.
    fn parked(e: &mut AtcEngine, utc_s: Option<f64>, day: Option<u16>) {
        e.on_telemetry(AircraftState {
            callsign: Callsign::new("SPTST"),
            t: SimTime(0.0),
            pos: STAND,
            alt_ft_msl: 100.0,
            heading_deg: 0.0,
            ias_kt: 0.0,
            gs_kt: 0.0,
            vs_fpm: 0.0,
            on_ground: true,
            com1_khz: DEL,
            xpdr_code: None,
            xpdr_mode: None,
            utc_s,
            day_of_year: day,
        });
    }

    const REQUEST: &str =
        "Zeta Delivery, Sierra Papa Tango Sierra Tango, IFR to Testville, request clearance";

    fn clearance(code: u16) -> Vec<AtcItem> {
        vec![
            AtcItem::ClearedTo {
                destination: "Testville".into(),
                icao: "ZZZY".into(),
            },
            AtcItem::Sid {
                fix: "ALFAX".into(),
                number: 1,
                letter: 'A',
            },
            AtcItem::ClimbTo(Level::Altitude(5000)),
            AtcItem::Squawk(code),
        ]
    }

    fn code_of(items: &[AtcItem]) -> u16 {
        squawk_of(items).expect("squawk")
    }

    fn clearance_readback(sid: &str, code: u16) -> String {
        format!(
            "cleared to Testville, {sid} departure, climb five thousand feet, squawk {}, {CS_ABBR}",
            spoken(&format!("{code:04}"))
        )
    }

    #[test]
    fn clearance_read_back_then_ground() {
        let mut e = engine();
        parked(&mut e, Some(12.0 * 3600.0), Some(SUMMER));
        let out = say(&mut e, DEL, REQUEST);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(
            (out[0].freq_khz, out[0].station.as_str()),
            (DEL, "Zeta Delivery")
        );
        let code = code_of(&items(&out));
        assert_eq!(items(&out), clearance(code));
        assert_eq!(
            out[0].text,
            format!(
                "Sierra Papa Tango Sierra Tango, cleared to Testville, ALFAX one Alpha departure, \
                 climb altitude five thousand feet, squawk {}",
                spoken(&format!("{code:04}"))
            )
        );
        let plan = e
            .strip(&Callsign::new("SPTST"))
            .unwrap()
            .plan
            .clone()
            .unwrap();
        assert!(plan.ifr);
        assert_eq!(
            (plan.destination.as_str(), plan.dest_name.as_str()),
            ("ZZZY", "Testville")
        );
        assert_eq!((plan.cruise_ft, plan.cleared_ft), (5000, 5000));
        assert_eq!(plan.sid.as_deref(), Some("ALFA1A"));

        let out = say(&mut e, DEL, &clearance_readback("Alfax one Alpha", code));
        assert_eq!(
            items(&out),
            vec![
                AtcItem::ReadbackCorrect,
                AtcItem::Contact {
                    station: "Zeta Ground".into(),
                    khz: GND,
                },
            ]
        );
        // Ground: start-up, then taxi to the holding point of runway 27.
        let out = say(
            &mut e,
            GND,
            &format!("Zeta Ground, {CS}, information Alpha, request start up"),
        );
        assert_eq!(
            items(&out),
            vec![AtcItem::StartUpApproved, AtcItem::Qnh(1013)]
        );
        let rb = format!("start up approved, QNH one zero one three, {CS_ABBR}");
        assert!(say(&mut e, GND, &rb).is_empty());
        let out = say(
            &mut e,
            GND,
            &format!("Zeta Ground, {CS_ABBR}, information Alpha, request taxi"),
        );
        assert!(
            items(&out).iter().any(|it| matches!(
                it,
                AtcItem::Taxi { to: TaxiTo::HoldingPoint { runway }, .. } if runway == "27"
            )),
            "{out:?}"
        );
    }

    #[test]
    fn wrong_sid_in_readback_is_corrected() {
        let mut e = engine();
        let code = code_of(&items(&say(&mut e, DEL, REQUEST)));
        let out = say(&mut e, DEL, &clearance_readback("Bravx one Alpha", code));
        assert_eq!(
            items(&out),
            vec![
                AtcItem::Negative,
                AtcItem::Sid {
                    fix: "ALFAX".into(),
                    number: 1,
                    letter: 'A',
                },
            ]
        );
    }

    /// Review Focus 1: a destination not among the known airports, or none at all.
    #[test]
    fn unknown_destination_says_again() {
        let mut e = engine();
        for text in [
            "Zeta Delivery, Sierra Papa Tango Sierra Tango, IFR to Echo Papa Kilo Tango, request clearance",
            "Zeta Delivery, Sierra Papa Tango Sierra Tango, IFR to Nowhere, request clearance",
            "Zeta Delivery, Sierra Papa Tango Sierra Tango, IFR, request clearance",
        ] {
            let out = say(&mut e, DEL, text);
            assert_eq!(
                items(&out),
                vec![AtcItem::Unable(UnableReason::UnknownDestination)],
                "{text}"
            );
            assert!(
                out[0].text.ends_with("unable, say again destination"),
                "{}",
                out[0].text
            );
        }
        assert!(e.strip(&Callsign::new("SPTST")).unwrap().plan.is_none());
    }

    #[test]
    fn no_procedure_set_is_unable_ifr() {
        let mut e = engine_ifr(false, LanguageMode::Fixed(Lang::En));
        let out = say(&mut e, DEL, REQUEST);
        assert_eq!(items(&out), vec![AtcItem::Unable(UnableReason::NoIfrData)]);
    }

    /// A VFR clearance request naming the exit point gets that point.
    #[test]
    fn vfr_clearance_via_the_requested_point() {
        let mut e = engine();
        let out = say(
            &mut e,
            DEL,
            "Zeta Delivery, Sierra Papa Tango Sierra Tango, VFR to Testville via Kilo, \
             request clearance",
        );
        assert!(
            items(&out).contains(&AtcItem::LeaveControlZoneVia {
                point: "KILO".into()
            }),
            "{out:?}"
        );
    }

    #[test]
    fn request_without_rules_gets_say_flight_rules() {
        let mut e = engine();
        let out = say(
            &mut e,
            DEL,
            "Zeta Delivery, Sierra Papa Tango Sierra Tango, request clearance to Testville",
        );
        assert_eq!(items(&out), vec![AtcItem::SayFlightRules]);
        let strip = e.strip(&Callsign::new("SPTST")).unwrap();
        assert!(strip.plan.is_none() && strip.pending_readback.is_none());
        let out = say(
            &mut e,
            DEL,
            "Zeta Delivery, Sierra Papa Tango Sierra Tango, VFR to Testville, request clearance",
        );
        assert!(matches!(
            items(&out).as_slice(),
            [
                AtcItem::ClearedTo { .. },
                AtcItem::LeaveControlZoneVia { .. },
                AtcItem::Squawk(_)
            ]
        ));
        assert!(
            !e.strip(&Callsign::new("SPTST"))
                .unwrap()
                .plan
                .clone()
                .unwrap()
                .ifr
        );
    }

    #[test]
    fn vfr_clearance_leaves_towards_destination() {
        let mut e = engine();
        let out = say(
            &mut e,
            DEL,
            "Zeta Delivery, Sierra Papa Tango Sierra Tango, VFR to Testville, request clearance",
        );
        let code = code_of(&items(&out));
        assert_eq!(
            items(&out),
            vec![
                AtcItem::ClearedTo {
                    destination: "Testville".into(),
                    icao: "ZZZY".into(),
                },
                AtcItem::LeaveControlZoneVia {
                    point: "ROMEO".into(),
                },
                AtcItem::Squawk(code),
            ]
        );
        assert!(
            !e.strip(&Callsign::new("SPTST"))
                .unwrap()
                .plan
                .clone()
                .unwrap()
                .ifr
        );
    }

    #[test]
    fn delivery_hours() {
        let e = engine();
        let h = |x: f64| Some(x * 3600.0);
        // Summer 0600-2200, winter 0500-2100 UTC.
        assert!(e.delivery_open(h(6.0), Some(SUMMER)));
        assert!(!e.delivery_open(h(5.5), Some(SUMMER)));
        assert!(!e.delivery_open(h(22.0), Some(SUMMER)));
        assert!(e.delivery_open(h(5.5), Some(WINTER)));
        assert!(!e.delivery_open(h(21.5), Some(WINTER)));
        // Review Focus 4: no time -> open.
        assert!(e.delivery_open(None, None));
        assert!(e.delivery_open(None, Some(WINTER)));
    }

    /// Delivery closed at 23:00 UTC: Ground issues the clearance, Delivery is silent.
    #[test]
    fn delivery_closed_ground_clears() {
        let mut e = engine();
        parked(&mut e, Some(23.0 * 3600.0), Some(SUMMER));
        assert!(say(&mut e, DEL, REQUEST).is_empty());
        let out = say(
            &mut e,
            GND,
            "Zeta Ground, Sierra Papa Tango Sierra Tango, IFR to Testville, request clearance",
        );
        assert_eq!(out[0].freq_khz, GND);
        let code = code_of(&items(&out));
        assert_eq!(items(&out), clearance(code));
        let out = say(&mut e, GND, &clearance_readback("Alfax one Alpha", code));
        assert_eq!(items(&out), vec![AtcItem::ReadbackCorrect]);
        // The ATIS names Ground as the initial contact.
        let (_, text) = e.atis_broadcast().unwrap();
        assert!(
            text.contains("for start-up contact Ground one two one decimal seven"),
            "{text}"
        );
    }

    /// While Delivery is open, a clearance request to Ground goes to Delivery.
    #[test]
    fn ground_sends_clearance_request_to_delivery() {
        let mut e = engine();
        parked(&mut e, Some(12.0 * 3600.0), Some(SUMMER));
        let out = say(
            &mut e,
            GND,
            "Zeta Ground, Sierra Papa Tango Sierra Tango, IFR to Testville, request clearance",
        );
        let delivery = AtcItem::Contact {
            station: "Zeta Delivery".into(),
            khz: DEL,
        };
        assert_eq!(items(&out), vec![delivery.clone()]);
        // An IFR aircraft without a clearance asking for taxi too.
        let mut e = engine();
        parked(&mut e, Some(12.0 * 3600.0), Some(SUMMER));
        let out = say(
            &mut e,
            GND,
            &format!("Zeta Ground, {CS}, IFR to Testville, information Alpha, request taxi"),
        );
        assert_eq!(items(&out), vec![delivery]);
        let (_, text) = e.atis_broadcast().unwrap();
        assert!(
            text.contains("for clearance contact Delivery one two one decimal niner"),
            "{text}"
        );
    }

    /// The initial climb is the cruise level: the SID's "at or above 3000" is a minimum; an
    /// "at or below 4000" on it is a ceiling and stops the climb there.
    #[test]
    fn sid_ceiling_lowers_initial_climb() {
        let (ap, procs) = zzzz();
        let mut set = load_procedure_set(&root(), "ZZZZ").unwrap();
        for p in set
            .procedures
            .iter_mut()
            .filter(|p| p.procedure.ident == "ALFA1A")
        {
            p.legs[0].leg.alt = AltConstraint::AtOrBelow(4000);
        }
        let mut e = AtcEngine::new(ap, procs, EngineConfig::new(Strictness::Standard))
            .with_procedure_set(set)
            .with_destinations(destinations());
        e.register_pilot(PilotProfile {
            callsign: Callsign::new("SPTST"),
            aircraft_type: "C172".into(),
        });
        e.on_weather(weather(1013));
        let out = say(&mut e, DEL, REQUEST);
        assert_eq!(
            items(&out)[2],
            AtcItem::ClimbTo(Level::Altitude(4000)),
            "{out:?}"
        );
    }

    /// A sample without time after one at 23:00 UTC keeps the last time: still closed.
    #[test]
    fn sample_without_time_keeps_the_clock() {
        let mut e = engine();
        parked(&mut e, Some(23.0 * 3600.0), Some(SUMMER));
        parked(&mut e, None, None);
        assert!(say(&mut e, DEL, REQUEST).is_empty());
        let (_, text) = e.atis_broadcast().unwrap();
        assert!(text.contains("for start-up contact Ground"), "{text}");
    }

    /// Review Focus 4: telemetry without UTC time keeps Delivery open.
    #[test]
    fn no_utc_delivery_open() {
        let mut e = engine();
        parked(&mut e, None, None);
        let out = say(&mut e, DEL, REQUEST);
        assert_eq!(out[0].freq_khz, DEL);
        assert!(matches!(out[0].message.items[0], AtcItem::ClearedTo { .. }));
    }

    #[test]
    fn push_and_start_approved() {
        let mut e = engine();
        parked(&mut e, None, None);
        let out = say(
            &mut e,
            GND,
            &format!("Zeta Ground, {CS}, request push back and start up"),
        );
        assert_eq!(
            items(&out),
            vec![
                AtcItem::PushAndStartApproved { facing: None },
                AtcItem::Qnh(1013)
            ]
        );
    }

    /// The taxi clearance after an IFR clearance and the start-up does not repeat the squawk
    /// (in the clearance) or the QNH (with the start-up).
    #[test]
    fn taxi_after_clearance_and_start_up_omits_squawk_and_qnh() {
        let mut e = engine();
        parked(&mut e, Some(12.0 * 3600.0), Some(SUMMER));
        let code = code_of(&items(&say(&mut e, DEL, REQUEST)));
        say(&mut e, DEL, &clearance_readback("Alfax one Alpha", code));
        say(
            &mut e,
            GND,
            &format!("Zeta Ground, {CS}, information Alpha, request start up"),
        );
        say(
            &mut e,
            GND,
            &format!("start up approved, QNH one zero one three, {CS_ABBR}"),
        );
        let out = say(
            &mut e,
            GND,
            &format!("{CS_ABBR}, information Alpha, request taxi"),
        );
        let got = items(&out);
        assert!(
            got.iter().any(|it| matches!(it, AtcItem::Taxi { .. })),
            "{got:?}"
        );
        assert!(
            !got.iter()
                .any(|it| matches!(it, AtcItem::Squawk(_) | AtcItem::Qnh(_))),
            "{got:?}"
        );
    }

    /// The omission is per departure: after the flight lands and parks, a second flight
    /// (VFR circuits, no start-up, no clearance) gets the QNH and the squawk again.
    #[test]
    fn second_flight_gets_qnh_and_squawk_again() {
        let mut e = engine();
        parked(&mut e, Some(12.0 * 3600.0), Some(SUMMER));
        let code = code_of(&items(&say(&mut e, DEL, REQUEST)));
        say(&mut e, DEL, &clearance_readback("Alfax one Alpha", code));
        say(
            &mut e,
            GND,
            &format!("Zeta Ground, {CS}, information Alpha, request start up"),
        );
        say(
            &mut e,
            GND,
            &format!("start up approved, QNH one zero one three, {CS_ABBR}"),
        );
        // The flight: airborne away from the field, then back on the stand and stopped.
        let sample = |t: f64, pos: LatLon, alt: f64, ground: bool| AircraftState {
            callsign: Callsign::new("SPTST"),
            t: SimTime(t),
            pos,
            alt_ft_msl: alt,
            heading_deg: 0.0,
            ias_kt: if ground { 0.0 } else { 90.0 },
            gs_kt: if ground { 0.0 } else { 90.0 },
            vs_fpm: 0.0,
            on_ground: ground,
            com1_khz: GND,
            xpdr_code: None,
            xpdr_mode: None,
            utc_s: Some(12.0 * 3600.0 + t),
            day_of_year: Some(SUMMER),
        };
        let away = LatLon {
            lat: 10.2,
            lon: 10.0,
        };
        for k in 1..=20 {
            e.on_telemetry(sample(f64::from(k) * 2.0, away, 3000.0, false));
        }
        for k in 21..=30 {
            e.on_telemetry(sample(f64::from(k) * 2.0, STAND, 100.0, true));
        }
        let strip = e.strip(&Callsign::new("SPTST")).unwrap();
        assert_eq!(strip.phase, FlightPhase::Parked);
        assert!(strip.plan.is_none(), "the old plan ends with the flight");
        let out = say(
            &mut e,
            GND,
            &format!("Zeta Ground, {CS}, information Alpha, request taxi for circuits"),
        );
        let got = items(&out);
        assert!(got.contains(&AtcItem::Qnh(1013)), "{got:?}");
        assert!(got.contains(&AtcItem::Squawk(code)), "{got:?}");
    }

    #[test]
    fn polish_clearance() {
        let mut e = engine_ifr(true, LanguageMode::Fixed(Lang::Pl));
        let out = e.on_transmission(
            DEL,
            "Zeta Delivery, Sierra Papa Tango Sierra Tango, IFR do Testville, proszę o zezwolenie",
            Lang::Pl,
            SimTime(0.0),
        );
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].message.lang, Lang::Pl);
        let code = code_of(&items(&out));
        assert_eq!(items(&out), clearance(code));
        assert!(out[0].text.contains("zezwolenie do"), "{}", out[0].text);
        assert!(out[0].text.contains("odlot ALFAX"), "{}", out[0].text);
    }

    /// The transmission on the Approach frequency is heard but not yet handled.
    #[test]
    fn approach_frequency_passes_message() {
        let mut e = engine();
        let out = say(
            &mut e,
            119100,
            "Zeta Approach, Sierra Papa Tango Sierra Tango",
        );
        assert_eq!(items(&out), vec![AtcItem::PassYourMessage]);
        assert_eq!(out[0].station, "Zeta Approach");
    }

    /// The ATIS wind `dir_deg`/`speed_kt` (runway 27 stays in use for a westerly wind).
    fn wind(e: &mut AtcEngine, dir_deg: u16, speed_kt: u16) {
        e.on_weather(Weather {
            wind: Wind {
                dir_deg,
                speed_kt,
                calm: false,
            },
            ..weather(1013)
        });
    }

    fn requested_runway(e: &AtcEngine) -> Option<String> {
        e.strip(&Callsign::new("SPTST"))
            .unwrap()
            .requested_runway
            .clone()
    }

    fn request_runway(e: &mut AtcEngine, runway: &str) -> Vec<AtcItem> {
        let text = format!(
            "Zeta Delivery, Sierra Papa Tango Sierra Tango, request departure runway {}",
            spoken(runway)
        );
        items(&say(e, DEL, &text))
    }

    #[test]
    fn runway_request_within_tailwind_is_approved() {
        let mut e = engine();
        wind(&mut e, 270, 3);
        parked(&mut e, None, None);
        assert_eq!(
            request_runway(&mut e, "09"),
            vec![AtcItem::RunwayApproved("09".into())]
        );
        assert_eq!(requested_runway(&e).as_deref(), Some("09"));
    }

    #[test]
    fn clearance_after_runway_request_uses_its_sid() {
        let mut e = engine();
        wind(&mut e, 270, 3);
        parked(&mut e, None, None);
        request_runway(&mut e, "09");
        let out = items(&say(&mut e, DEL, REQUEST));
        assert!(
            out.contains(&AtcItem::Sid {
                fix: "CHARX".into(),
                number: 1,
                letter: 'A',
            }),
            "{out:?}"
        );
        let plan = e
            .strip(&Callsign::new("SPTST"))
            .unwrap()
            .plan
            .clone()
            .unwrap();
        assert_eq!(plan.sid.as_deref(), Some("CHAR1A"));
    }

    #[test]
    fn runway_request_above_tailwind_is_unable() {
        let mut e = engine();
        wind(&mut e, 270, 8);
        parked(&mut e, None, None);
        assert_eq!(
            request_runway(&mut e, "09"),
            vec![AtcItem::Unable(UnableReason::RunwayInUse {
                runway: "27".into()
            })]
        );
        assert_eq!(requested_runway(&e), None);
    }

    #[test]
    fn unknown_runway_request_is_unable() {
        let mut e = engine();
        wind(&mut e, 270, 3);
        parked(&mut e, None, None);
        assert_eq!(
            request_runway(&mut e, "18"),
            vec![AtcItem::Unable(UnableReason::RunwayInUse {
                runway: "27".into()
            })]
        );
        assert_eq!(requested_runway(&e), None);
    }

    #[test]
    fn request_runway_in_use_is_approved_without_change() {
        let mut e = engine();
        wind(&mut e, 270, 3);
        parked(&mut e, None, None);
        assert_eq!(
            request_runway(&mut e, "27"),
            vec![AtcItem::RunwayApproved("27".into())]
        );
        assert_eq!(requested_runway(&e).as_deref(), Some("27"));
    }

    #[test]
    fn ground_taxis_to_requested_runway() {
        let mut e = engine();
        wind(&mut e, 270, 3);
        parked(&mut e, None, None);
        request_runway(&mut e, "09");
        let out = say(
            &mut e,
            GND,
            &format!("Zeta Ground, {CS_ABBR}, information Alpha, request taxi"),
        );
        assert!(
            items(&out).iter().any(|it| matches!(
                it,
                AtcItem::Taxi { to: TaxiTo::HoldingPoint { runway }, .. } if runway == "09"
            )),
            "{out:?}"
        );
    }

    fn char1a() -> AtcItem {
        AtcItem::Sid {
            fix: "CHARX".into(),
            number: 1,
            letter: 'A',
        }
    }

    fn plan(e: &AtcEngine) -> atc_core::strip::FlightPlan {
        e.strip(&Callsign::new("SPTST"))
            .unwrap()
            .plan
            .clone()
            .unwrap()
    }

    /// SPTST parked, cleared IFR on runway 27 (ALFA1A) and read back.
    fn ifr_cleared_on_27(e: &mut AtcEngine) {
        parked(e, None, None);
        let out = say(e, DEL, REQUEST);
        let code = code_of(&items(&out));
        assert_eq!(items(&out), clearance(code));
        let out = items(&say(e, DEL, &clearance_readback("Alfax one Alpha", code)));
        assert_eq!(out[0], AtcItem::ReadbackCorrect);
        say(e, DEL, &readback(&out, CS_ABBR));
        assert!(
            e.strip(&Callsign::new("SPTST"))
                .unwrap()
                .pending_readback
                .is_none()
        );
        assert_eq!(plan(e).dep_runway.as_deref(), Some("27"));
    }

    fn request_taxi(e: &mut AtcEngine) -> Vec<AtcItem> {
        items(&say(
            e,
            GND,
            &format!("Zeta Ground, {CS_ABBR}, information Alpha, request taxi"),
        ))
    }

    fn taxis_to(items: &[AtcItem], runway: &str) -> bool {
        items.iter().any(|it| {
            matches!(
                it,
                AtcItem::Taxi { to: TaxiTo::HoldingPoint { runway: r }, .. } if r == runway
            )
        })
    }

    #[test]
    fn runway_change_before_taxi_is_announced() {
        let mut e = engine();
        ifr_cleared_on_27(&mut e);
        // Tailwind 10 kt on 27: runway 09 in use.
        wind(&mut e, 90, 10);
        let out = request_taxi(&mut e);
        assert_eq!(out[..2], [AtcItem::RunwayChanged("09".into()), char1a()]);
        assert!(taxis_to(&out[2..], "09"), "{out:?}");
        let pending = e
            .strip(&Callsign::new("SPTST"))
            .unwrap()
            .pending_readback
            .clone()
            .unwrap();
        assert!(pending.contains(&ReadbackItem::Runway("09".into())));
        assert!(pending.contains(&ReadbackItem::Sid("CHARX".into(), 1, 'A')));
        let plan = plan(&e);
        assert_eq!(plan.sid.as_deref(), Some("CHAR1A"));
        assert_eq!(plan.dep_runway.as_deref(), Some("09"));
        // The whole transmission is read back.
        let text = format!(
            "runway zero niner, Charx one Alpha departure, {}",
            readback(&out, CS_ABBR)
        );
        say(&mut e, GND, &text);
        assert!(
            e.strip(&Callsign::new("SPTST"))
                .unwrap()
                .pending_readback
                .is_none()
        );
    }

    #[test]
    fn runway_request_after_clearance_amends_it() {
        let mut e = engine();
        wind(&mut e, 270, 3);
        ifr_cleared_on_27(&mut e);
        let out = request_runway(&mut e, "09");
        assert_eq!(
            out,
            vec![
                AtcItem::RunwayApproved("09".into()),
                AtcItem::RunwayChanged("09".into()),
                char1a(),
            ]
        );
        let strip = e.strip(&Callsign::new("SPTST")).unwrap();
        assert_eq!(
            strip.pending_readback,
            Some(vec![
                ReadbackItem::Runway("09".into()),
                ReadbackItem::Sid("CHARX".into(), 1, 'A'),
            ])
        );
        let plan = plan(&e);
        assert_eq!(
            (plan.sid.as_deref(), plan.dep_runway.as_deref()),
            (Some("CHAR1A"), Some("09"))
        );
    }

    #[test]
    fn vfr_runway_change_has_no_sid() {
        let mut e = engine();
        parked(&mut e, None, None);
        let out = items(&say(
            &mut e,
            DEL,
            "Zeta Delivery, Sierra Papa Tango Sierra Tango, VFR to Testville, request clearance",
        ));
        let text = format!("cleared to Testville, {}", readback(&out, CS_ABBR));
        let out = say(&mut e, DEL, &text);
        assert_eq!(items(&out)[0], AtcItem::ReadbackCorrect);
        assert_eq!(plan(&e).dep_runway.as_deref(), Some("27"));
        wind(&mut e, 90, 10);
        let out = request_taxi(&mut e);
        assert_eq!(out[0], AtcItem::RunwayChanged("09".into()));
        assert!(taxis_to(&out, "09"), "{out:?}");
        assert!(
            !out.iter().any(|it| matches!(it, AtcItem::Sid { .. })),
            "{out:?}"
        );
        assert_eq!(plan(&e).dep_runway.as_deref(), Some("09"));
    }

    #[test]
    fn no_change_no_announcement() {
        let mut e = engine();
        ifr_cleared_on_27(&mut e);
        let out = request_taxi(&mut e);
        assert!(taxis_to(&out, "27"), "{out:?}");
        assert!(
            !out.iter()
                .any(|it| matches!(it, AtcItem::RunwayChanged(_) | AtcItem::Sid { .. })),
            "{out:?}"
        );
    }

    fn strip_runway(e: &AtcEngine) -> String {
        e.strip(&Callsign::new("SPTST")).unwrap().runway.clone()
    }

    #[test]
    fn runway_request_while_taxiing_retaxis_to_new_runway() {
        let mut e = engine();
        wind(&mut e, 270, 3);
        ifr_cleared_on_27(&mut e);
        let taxi = request_taxi(&mut e);
        assert!(taxis_to(&taxi, "27"), "{taxi:?}");
        say(&mut e, GND, &readback(&taxi, CS_ABBR));
        e.on_telemetry(AircraftState {
            callsign: Callsign::new("SPTST"),
            t: SimTime(5.0),
            pos: LatLon {
                lat: 10.0011,
                lon: 10.0,
            },
            alt_ft_msl: 100.0,
            heading_deg: 180.0,
            ias_kt: 10.0,
            gs_kt: 10.0,
            vs_fpm: 0.0,
            on_ground: true,
            com1_khz: GND,
            xpdr_code: None,
            xpdr_mode: None,
            utc_s: None,
            day_of_year: None,
        });
        assert_eq!(
            e.strip(&Callsign::new("SPTST")).unwrap().phase,
            FlightPhase::TaxiOut
        );
        let out = items(&say(
            &mut e,
            GND,
            &format!("Zeta Ground, {CS_ABBR}, request departure runway zero niner"),
        ));
        assert_eq!(
            out[..3],
            [
                AtcItem::RunwayApproved("09".into()),
                AtcItem::RunwayChanged("09".into()),
                char1a(),
            ]
        );
        assert!(taxis_to(&out[3..], "09"), "{out:?}");
        assert_eq!(strip_runway(&e), "09");
        let pending = e
            .strip(&Callsign::new("SPTST"))
            .unwrap()
            .pending_readback
            .clone()
            .unwrap();
        assert!(pending.contains(&ReadbackItem::Sid("CHARX".into(), 1, 'A')));
        assert!(pending.contains(&ReadbackItem::Kind(ClearanceKind::Taxi)));
        assert_eq!(plan(&e).dep_runway.as_deref(), Some("09"));
    }

    /// SPTST cleared on 27 to a destination the engine no longer knows: no SID can be chosen
    /// for another runway.
    fn cleared_to_unknown_destination(e: &mut AtcEngine) {
        ifr_cleared_on_27(e);
        let mut json = serde_json::to_value(e.snapshot()).unwrap();
        json["strips"][0]["plan"]["destination"] = "XXXX".into();
        e.restore(serde_json::from_value(json).unwrap()).unwrap();
    }

    #[test]
    fn runway_change_without_sid_keeps_cleared_runway() {
        let mut e = engine();
        cleared_to_unknown_destination(&mut e);
        wind(&mut e, 90, 10);
        let out = request_taxi(&mut e);
        assert!(taxis_to(&out, "27"), "{out:?}");
        assert!(
            !out.iter()
                .any(|it| matches!(it, AtcItem::RunwayChanged(_) | AtcItem::Sid { .. })),
            "{out:?}"
        );
        assert_eq!(strip_runway(&e), "27");
        assert_eq!(plan(&e).dep_runway.as_deref(), Some("27"));
    }

    #[test]
    fn runway_request_without_sid_keeps_cleared_runway() {
        let mut e = engine();
        wind(&mut e, 270, 3);
        cleared_to_unknown_destination(&mut e);
        assert_eq!(
            request_runway(&mut e, "09"),
            vec![AtcItem::Unable(UnableReason::RunwayInUse {
                runway: "27".into()
            })]
        );
        assert_eq!(requested_runway(&e), None);
        assert_eq!(strip_runway(&e), "27");
        assert_eq!(plan(&e).dep_runway.as_deref(), Some("27"));
    }

    const TWR: u32 = 118300;

    /// SPTST cleared on 27, its strip put in `phase` (as the snapshot spells it).
    fn cleared_in_phase(e: &mut AtcEngine, phase: &str) {
        ifr_cleared_on_27(e);
        let mut json = serde_json::to_value(e.snapshot()).unwrap();
        json["strips"][0]["phase"] = phase.into();
        e.restore(serde_json::from_value(json).unwrap()).unwrap();
    }

    fn contacts_tower(out: &[AtcItem]) -> bool {
        out.iter()
            .any(|it| matches!(it, AtcItem::Contact { khz: TWR, .. }))
    }

    /// At the holding point or on the runway a runway request to Ground or Delivery can only
    /// mean the departure: "contact Tower", as for ready for departure.
    #[test]
    fn runway_request_at_holding_point_to_ground_or_delivery_goes_to_tower() {
        for phase in ["HoldingShort", "OnRunway"] {
            for (khz, station) in [(GND, "Zeta Ground"), (DEL, "Zeta Delivery")] {
                for runway in ["two seven", "zero niner"] {
                    let mut e = engine();
                    wind(&mut e, 270, 3);
                    cleared_in_phase(&mut e, phase);
                    let text = format!("{station}, {CS_ABBR}, request departure runway {runway}");
                    let out = items(&say(&mut e, khz, &text));
                    assert!(contacts_tower(&out), "{phase} {station} {runway}: {out:?}");
                    assert_eq!(strip_runway(&e), "27");
                }
            }
        }
    }

    /// Taxiing, a runway request to Tower is answered like ready for departure: taxi to
    /// the holding point and report ready.
    #[test]
    fn runway_request_to_tower_while_taxiing_continues_taxi() {
        let mut e = engine();
        wind(&mut e, 270, 3);
        cleared_in_phase(&mut e, "TaxiOut");
        let text = format!("Zeta Tower, {CS_ABBR}, request departure runway two seven");
        assert_eq!(
            items(&say(&mut e, TWR, &text)),
            vec![AtcItem::TaxiAndReportReady {
                runway: "27".into()
            }]
        );
    }

    /// Taxiing to the runway it asks for, the request to Ground is ready for departure.
    #[test]
    fn runway_request_for_taxi_runway_goes_to_tower() {
        let mut e = engine();
        wind(&mut e, 270, 3);
        cleared_in_phase(&mut e, "TaxiOut");
        let text = format!("Zeta Ground, {CS_ABBR}, request departure runway two seven");
        let out = items(&say(&mut e, GND, &text));
        assert!(contacts_tower(&out), "{out:?}");
    }

    /// A snapshot written before `dep_runway` existed restores the plan without it.
    #[test]
    fn old_snapshot_without_dep_runway_restores() {
        let mut e = engine();
        ifr_cleared_on_27(&mut e);
        let mut json = serde_json::to_value(e.snapshot()).unwrap();
        let plan_json = json["strips"][0]["plan"].as_object_mut().unwrap();
        assert!(plan_json.remove("dep_runway").is_some());
        let mut fresh = engine();
        fresh
            .restore(serde_json::from_value(json).unwrap())
            .unwrap();
        assert_eq!(plan(&fresh).dep_runway, None);
    }

    /// A snapshot written before `requested_runway` existed restores without it.
    #[test]
    fn old_snapshot_without_requested_runway_restores() {
        let mut e = engine();
        parked(&mut e, None, None);
        let mut json = serde_json::to_value(e.snapshot()).unwrap();
        let strip = json["strips"][0].as_object_mut().unwrap();
        assert!(strip.remove("requested_runway").is_some());
        let mut fresh = engine();
        fresh
            .restore(serde_json::from_value(json).unwrap())
            .unwrap();
        assert_eq!(requested_runway(&fresh), None);
    }
}

/// Taxis out and holds short; Ground hands the aircraft to Tower (readback given on Ground).
/// Returns the samples still to come, with COM1 on `com1` from the next one on.
fn handed_to_tower(e: &mut AtcEngine, com1: u32) -> std::vec::IntoIter<AircraftState> {
    let clearance = taxi_cleared(e, "Alpha");
    assert!(say(e, GROUND, &readback(&clearance, CS_ABBR)).is_empty());
    let mut rest = track().into_iter().skip(1);
    for s in rest.by_ref() {
        let out = e.on_telemetry(AircraftState {
            com1_khz: GROUND,
            ..s
        });
        if let Some(contact) = out.first() {
            assert!(
                items(&out)
                    .iter()
                    .any(|i| matches!(i, AtcItem::Contact { khz: TOWER, .. })),
                "{out:?}"
            );
            assert!(say(e, GROUND, &readback(&items(&out), CS_ABBR)).is_empty());
            assert_eq!(contact.freq_khz, GROUND);
            break;
        }
    }
    rest.map(|s| AircraftState {
        com1_khz: com1,
        ..s
    })
    .collect::<Vec<_>>()
    .into_iter()
}

#[test]
fn tower_calls_aircraft_at_holding_point_after_transfer() {
    let mut e = engine();
    let mut rest = handed_to_tower(&mut e, TOWER);
    let cs = Callsign::new("SPTST");
    assert_eq!(e.strip(&cs).unwrap().owner, Role::Tower);
    let out = e.on_telemetry(rest.next().unwrap());
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].freq_khz, TOWER);
    assert!(is_takeoff_clearance(&items(&out)), "{out:?}");
    assert!(
        out[0].text.contains("cleared for take-off") && out[0].text.contains("wind"),
        "{}",
        out[0].text
    );
    assert!(e.strip(&cs).unwrap().permissions.may_take_off);
    // Read back: nothing more; the departure is never transmitted twice.
    assert!(say(&mut e, TOWER, &readback(&items(&out), CS_ABBR)).is_empty());
    for s in rest.take(3) {
        assert!(e.on_telemetry(s).is_empty());
    }
}

#[test]
fn tower_waits_until_com1_is_on_its_frequency() {
    let mut e = engine();
    let mut rest = handed_to_tower(&mut e, GROUND);
    assert!(e.on_telemetry(rest.next().unwrap()).is_empty());
    // The same 8.33 kHz channel under its 25 kHz name counts as the Tower frequency.
    let s = rest.next().unwrap();
    let out = e.on_telemetry(AircraftState {
        com1_khz: 123_250,
        ..s
    });
    assert!(is_takeoff_clearance(&items(&out)), "{out:?}");
}

#[test]
fn pilot_call_after_tower_called_is_not_answered_twice() {
    let ready = format!("Krakow Tower, {CS}, holding point runway two five, ready for departure");
    // Cleared and read back: the call gets no second clearance.
    let mut e = engine();
    let mut rest = handed_to_tower(&mut e, TOWER);
    let out = e.on_telemetry(rest.next().unwrap());
    assert!(is_takeoff_clearance(&items(&out)));
    let cs = Callsign::new("SPTST");
    // Readback still open: it is asked for again.
    let pending = e.strip(&cs).unwrap().pending_readback.clone().unwrap();
    assert_eq!(
        items(&say(&mut e, TOWER, &ready)),
        vec![AtcItem::ReadBack(pending)]
    );
    assert!(say(&mut e, TOWER, &readback(&items(&out), CS_ABBR)).is_empty());
    let late = say(&mut e, TOWER, &ready);
    assert!(late.is_empty(), "{late:?}");
}

/// Approach control of IFR departures on the fixture airport ZZZZ (RWY 27, SID ALFA1A north
/// over ZZ001 and ZZ002 to ALFAX).
mod approach_departure {
    use std::path::PathBuf;

    use atc_geo::{M_PER_NM, bearing_deg, destination, distance_m};
    use atc_navdata::cifp::AltConstraint;
    use atc_navdata::fixes::ProcedureSet;
    use atc_navdata::model::Destination;
    use atc_navdata::procedures::{AdjacentKind, AdjacentUnit, Tma};
    use atc_navdata::xplane::{load_destinations, load_procedure_set, resolve_airport};
    use atc_phraseology::Level;

    use super::*;

    pub(super) const DEL: u32 = 121900;
    pub(super) const GND: u32 = 121700;
    pub(super) const TWR: u32 = 118300;
    pub(super) const APP: u32 = 119100;
    /// An alternate frequency of the same Approach unit.
    pub(super) const APP_ALT: u32 = 126530;
    pub(super) const DEST_TWR: u32 = 118500;
    pub(super) const FIS: u32 = 119275;
    pub(super) const HOLD_27: LatLon = LatLon {
        lat: 10.0004,
        lon: 10.016,
    };
    pub(super) const THR_27: LatLon = LatLon {
        lat: 10.0,
        lon: 10.01667,
    };
    pub(super) const END_27: LatLon = LatLon {
        lat: 10.0,
        lon: 9.98333,
    };
    pub(super) const ZZ002: LatLon = LatLon {
        lat: 10.0,
        lon: 9.9,
    };
    pub(super) const ALFAX: LatLon = LatLon {
        lat: 10.1,
        lon: 9.8,
    };
    const TESTVILLE: LatLon = LatLon {
        lat: 10.5,
        lon: 10.0,
    };
    /// 80 NM north of ZZZZ.
    pub(super) const FARLAND: LatLon = LatLon {
        lat: 11.3333,
        lon: 10.0,
    };

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../atc-navdata/tests/fixtures/xplane")
    }

    /// ZZZZ with a 40 NM TMA, an ACC from 9500 ft, a FIS unit and Testville Tower as the
    /// tower of ZZZY; destinations Testville (30 NM north) and Farland (80 NM north).
    pub(super) fn engine() -> AtcEngine {
        engine_with(|_| {})
    }

    /// [`engine`] with the procedure set changed by `edit`.
    pub(super) fn engine_with(edit: impl FnOnce(&mut ProcedureSet)) -> AtcEngine {
        engine_with_procs(|_| {}, edit)
    }

    /// [`engine`] with `procedures.toml` changed by `edit_procs` and the procedure set by
    /// `edit`.
    pub(super) fn engine_with_procs(
        edit_procs: impl FnOnce(&mut Procedures),
        edit: impl FnOnce(&mut ProcedureSet),
    ) -> AtcEngine {
        engine_full(Strictness::Standard, edit_procs, edit)
    }

    /// [`engine_with_procs`] judging readbacks with `strictness`.
    pub(super) fn engine_full(
        strictness: Strictness,
        edit_procs: impl FnOnce(&mut Procedures),
        edit: impl FnOnce(&mut ProcedureSet),
    ) -> AtcEngine {
        let ap = resolve_airport(&root(), "ZZZZ")
            .unwrap()
            .parse("ZZZZ")
            .unwrap();
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/ZZZZ/procedures.toml"
        ))
        .unwrap();
        let mut procs = parse_procedures(&text).unwrap();
        procs.approach_alt_khz = vec![APP_ALT];
        procs.tma = Some(Tma {
            radius_nm: 40.0,
            ceiling_ft: 9500,
        });
        let unit = |callsign: &str, khz, kind, airport: Option<&str>, min| AdjacentUnit {
            callsign: callsign.into(),
            khz,
            kind,
            airport: airport.map(Into::into),
            min_level_ft: min,
        };
        procs.adjacent = vec![
            unit("Zeta Radar", 127660, AdjacentKind::Acc, None, Some(9500)),
            unit("Zeta Information", FIS, AdjacentKind::Fis, None, None),
            unit(
                "Testville Tower",
                DEST_TWR,
                AdjacentKind::Tower,
                Some("ZZZY"),
                None,
            ),
        ];
        edit_procs(&mut procs);
        let mut dests = load_destinations(
            &root(),
            LatLon {
                lat: 10.0,
                lon: 10.0,
            },
            100.0,
            &[],
        )
        .unwrap();
        dests.push(Destination {
            icao: "ZZZF".into(),
            name: "Farland".into(),
            pos: FARLAND,
            elevation_ft: None,
        });
        let mut set = load_procedure_set(&root(), "ZZZZ").unwrap();
        edit(&mut set);
        let mut e = AtcEngine::new(ap, procs, EngineConfig::new(strictness))
            .with_procedure_set(set)
            .with_destinations(dests);
        e.register_pilot(PilotProfile {
            callsign: Callsign::new("SPTST"),
            aircraft_type: "C172".into(),
        });
        e.on_weather(weather(1013));
        e
    }

    /// The simulated aircraft: one sample every 2 s.
    pub(super) struct Flight {
        pub(super) t: f64,
        pub(super) pos: LatLon,
        pub(super) alt: f64,
        pub(super) com1: u32,
        /// Rolling on the runway rather than flying.
        pub(super) rolling: bool,
        /// Transponder code set.
        pub(super) xpdr: Option<u16>,
        /// Everything the controllers said.
        pub(super) heard: Vec<AtcOutput>,
    }

    pub(super) const DT: f64 = 2.0;

    impl Flight {
        pub(super) fn sample(&self, hdg: f64, gs: f64, vs: f64, on_ground: bool) -> AircraftState {
            AircraftState {
                callsign: Callsign::new("SPTST"),
                t: SimTime(self.t),
                pos: self.pos,
                alt_ft_msl: self.alt,
                heading_deg: hdg,
                ias_kt: gs,
                gs_kt: gs,
                vs_fpm: vs,
                on_ground,
                com1_khz: self.com1,
                xpdr_code: self.xpdr,
                xpdr_mode: None,
                utc_s: None,
                day_of_year: None,
            }
        }

        /// One sample at the current position.
        pub(super) fn step(
            &mut self,
            e: &mut AtcEngine,
            hdg: f64,
            gs: f64,
            vs: f64,
            gnd: bool,
        ) -> usize {
            let out = e.on_telemetry(self.sample(hdg, gs, vs, gnd));
            let n = out.len();
            self.heard.extend(out);
            self.t += DT;
            n
        }

        /// Flies (or rolls) straight to `to` at `gs` kt, climbing at `vs` fpm up to `top` ft,
        /// until it is there or `stop` holds for what the controllers said so far.
        pub(super) fn fly_to(
            &mut self,
            e: &mut AtcEngine,
            to: LatLon,
            (gs, vs, top): (f64, f64, f64),
            stop: &dyn Fn(&[AtcOutput]) -> bool,
        ) {
            let gnd = self.rolling;
            let leg = gs * 1852.0 / 3600.0 * DT;
            while distance_m(self.pos, to) > leg && !stop(&self.heard) {
                let hdg = bearing_deg(self.pos, to);
                let climbing = !gnd && self.alt < top;
                self.step(e, hdg, gs, if climbing { vs } else { 0.0 }, gnd);
                self.pos = destination(self.pos, hdg, leg);
                if climbing {
                    self.alt = (self.alt + vs * DT / 60.0).min(top);
                }
            }
        }

        pub(super) fn said(&self, f: impl Fn(&AtcItem) -> bool) -> Vec<&AtcOutput> {
            self.heard
                .iter()
                .filter(|o| o.message.items.iter().any(&f))
                .collect()
        }
    }

    /// 90 kt, 700 fpm up to 5000 ft.
    pub(super) const CLIMB: (f64, f64, f64) = (90.0, 700.0, 5000.0);

    pub(super) fn never(_: &[AtcOutput]) -> bool {
        false
    }

    /// Cleared IFR (`ifr`) or VFR to `dest_spoken`, read back; holding short of RWY 27 owned
    /// by Tower; cleared for take-off and read back; airborne at the end of RWY 27.
    pub(super) fn departed(e: &mut AtcEngine, dest_spoken: &str, ifr: bool) -> Flight {
        departed_climbing(e, dest_spoken, ifr, "five thousand")
    }

    /// [`departed`] with an initial climb of `climb` feet (spoken).
    fn departed_climbing(e: &mut AtcEngine, dest_spoken: &str, ifr: bool, climb: &str) -> Flight {
        departed_at(e, dest_spoken, ifr, climb, 0.0)
    }

    /// [`departed_climbing`] with the first sample at sim time `t0` (a later flight of the
    /// same aircraft).
    pub(super) fn departed_at(
        e: &mut AtcEngine,
        dest_spoken: &str,
        ifr: bool,
        climb: &str,
        t0: f64,
    ) -> Flight {
        let rules = if ifr { "IFR" } else { "VFR" };
        let out = say(
            e,
            DEL,
            &format!("Zeta Delivery, {CS}, {rules} to {dest_spoken}, request clearance"),
        );
        let clearance = items(&out);
        let code = squawk_of(&clearance).expect("squawk");
        let cleared = if ifr {
            format!(
                "cleared to {dest_spoken}, Alfax one Alpha departure, climb {climb} feet, \
                 squawk {}, {CS_ABBR}",
                spoken(&format!("{code:04}"))
            )
        } else {
            format!(
                "cleared to {dest_spoken}, leave control zone via Romeo, squawk {}, {CS_ABBR}",
                spoken(&format!("{code:04}"))
            )
        };
        let out = say(e, DEL, &cleared);
        assert_eq!(items(&out)[0], AtcItem::ReadbackCorrect, "{out:?}");
        assert!(say(e, DEL, &readback(&items(&out), CS_ABBR)).is_empty());
        let mut f = Flight {
            t: t0,
            pos: HOLD_27,
            alt: 100.0,
            com1: TWR,
            rolling: true,
            xpdr: None,
            heard: Vec::new(),
        };
        // Ground hands the aircraft to Tower; read back on Ground.
        assert_eq!(f.step(e, 270.0, 0.0, 0.0, true), 1, "{:?}", f.heard);
        let contact = f.heard.pop().unwrap();
        assert_eq!(contact.freq_khz, GND, "{contact:?}");
        assert!(say(e, GND, &readback(&contact.message.items, CS_ABBR)).is_empty());
        // Tower calls with the take-off clearance.
        assert_eq!(f.step(e, 270.0, 0.0, 0.0, true), 1);
        let takeoff = f.heard.pop().unwrap();
        assert!(is_takeoff_clearance(&takeoff.message.items), "{takeoff:?}");
        assert!(
            !takeoff
                .message
                .items
                .contains(&AtcItem::Report(CircuitLeg::Downwind)),
            "a departure does not join the circuit: {takeoff:?}"
        );
        assert!(say(e, TWR, &readback(&takeoff.message.items, CS_ABBR)).is_empty());
        // Line up, roll and lift off half-way down the runway.
        f.pos = THR_27;
        let mid = LatLon {
            lat: 10.0,
            lon: 10.0,
        };
        f.fly_to(e, mid, (60.0, 0.0, 100.0), &never);
        f.rolling = false;
        f.fly_to(e, END_27, (80.0, 700.0, 5000.0), &never);
        f
    }

    pub(super) fn contact_of(o: &AtcOutput) -> Option<(String, u32)> {
        o.message.items.iter().find_map(|it| match it {
            AtcItem::Contact { station, khz } => Some((station.clone(), *khz)),
            _ => None,
        })
    }

    pub(super) const AIRBORNE: &str = "Zeta Approach, Sierra Papa Tango Sierra Tango, airborne, passing \
                            one thousand two hundred feet, climbing five thousand feet";

    #[test]
    fn departure_to_airport_with_adjacent_tower() {
        let mut e = engine();
        let mut f = departed(&mut e, "Testville", true);
        // Tower sends the IFR departure to Approach after lift-off (and, as this pilot
        // reads back later, reminds of the missing readback).
        let mut tower = std::mem::take(&mut f.heard);
        tower.retain(|o| !is_reminder(std::slice::from_ref(o)));
        assert_eq!(tower.len(), 1, "{tower:?}");
        assert_eq!(tower[0].freq_khz, TWR);
        assert_eq!(
            contact_of(&tower[0]),
            Some(("Zeta Approach".to_string(), APP))
        );
        let cs = Callsign::new("SPTST");
        assert_eq!(e.strip(&cs).unwrap().phase, FlightPhase::Airborne);
        assert!(say(&mut e, TWR, &readback(&tower[0].message.items, CS_ABBR)).is_empty());
        assert_eq!(e.strip(&cs).unwrap().owner, Role::Approach);
        // Approach identifies, in English.
        let out = say(&mut e, APP, AIRBORNE);
        assert_eq!(items(&out), vec![AtcItem::Identified], "{out:?}");
        assert_eq!(
            (out[0].freq_khz, out[0].station.as_str()),
            (APP, "Zeta Approach")
        );
        assert_eq!(out[0].message.lang, Lang::En);
        // Along the SID: no direct before the turn at ZZ002, then direct to Testville.
        let direct = |h: &[AtcOutput]| {
            h.iter().any(|o| {
                o.message
                    .items
                    .iter()
                    .any(|i| matches!(i, AtcItem::DirectTo { .. }))
            })
        };
        f.fly_to(&mut e, ZZ002, CLIMB, &direct);
        let d = f.said(|i| matches!(i, AtcItem::DirectTo { .. }));
        assert_eq!(d.len(), 1, "{:?}", f.heard);
        assert_eq!(
            d[0].message.items,
            vec![AtcItem::DirectTo {
                fix: "Testville".into()
            }]
        );
        assert_eq!(d[0].freq_khz, APP);
        assert!(
            distance_m(f.pos, ZZ002) <= M_PER_NM + 100.0,
            "direct only once at the second fix"
        );
        assert!(say(&mut e, APP, &format!("proceed direct Testville, {CS_ABBR}")).is_empty());
        // 20 NM from Testville (elevation 50 ft): descent to 2500 ft with the QNH.
        f.heard.clear();
        let any = |h: &[AtcOutput]| !h.is_empty();
        f.fly_to(&mut e, TESTVILLE, CLIMB, &any);
        assert_eq!(
            items(&f.heard),
            vec![
                AtcItem::DescendTo(Level::Altitude(2500)),
                AtcItem::Qnh(1013)
            ]
        );
        assert_eq!(
            (f.heard[0].freq_khz, f.heard[0].message.lang),
            (APP, Lang::En)
        );
        let left = distance_m(f.pos, TESTVILLE) / M_PER_NM;
        assert!((19.0..=20.0).contains(&left), "{left} NM");
        let plan = e.strip(&cs).unwrap().plan.clone().unwrap();
        assert_eq!(plan.cleared_ft, 2500);
        let descent =
            format!("descend to two thousand five hundred feet, QNH one zero one three, {CS_ABBR}");
        assert!(say(&mut e, APP, &descent).is_empty());
        // Hand-off to Testville Tower at 10 NM from Testville; still above 2500 ft while
        // descending is no level bust.
        f.heard.clear();
        let handed = |h: &[AtcOutput]| h.iter().any(|o| contact_of(o).is_some());
        f.fly_to(&mut e, TESTVILLE, CLIMB, &handed);
        assert_eq!(f.heard.len(), 1, "{:?}", f.heard);
        assert_eq!(
            contact_of(&f.heard[0]),
            Some(("Testville Tower".to_string(), DEST_TWR))
        );
        assert_eq!(f.heard[0].message.items.len(), 1, "{:?}", f.heard[0]);
        let left = distance_m(f.pos, TESTVILLE) / M_PER_NM;
        assert!((9.0..=10.0).contains(&left), "{left} NM");
        assert_eq!(
            e.strip(&cs).unwrap().handed_off.as_deref(),
            Some("Testville Tower")
        );
        assert!(say(&mut e, APP, &readback(&f.heard[0].message.items, CS_ABBR)).is_empty());
        // The adjacent tower answers once.
        let call = format!("Testville Tower, {CS}, five thousand feet");
        let out = say(&mut e, DEST_TWR, &call);
        assert_eq!(items(&out), vec![AtcItem::Identified], "{out:?}");
        assert_eq!(
            (out[0].freq_khz, out[0].station.as_str()),
            (DEST_TWR, "Testville Tower")
        );
        assert!(say(&mut e, DEST_TWR, &call).is_empty());
        // Approach says nothing more.
        f.heard.clear();
        f.fly_to(&mut e, TESTVILLE, CLIMB, &never);
        assert!(f.heard.is_empty(), "{:?}", f.heard);
    }

    #[test]
    fn departure_beyond_tma_below_acc_goes_to_fis() {
        let mut e = engine();
        let mut f = departed(&mut e, "Farland", true);
        let tower = f.heard.drain(..).next().unwrap();
        assert!(say(&mut e, TWR, &readback(&tower.message.items, CS_ABBR)).is_empty());
        assert_eq!(
            items(&say(&mut e, APP, AIRBORNE)),
            vec![AtcItem::Identified]
        );
        // Farland is beyond the TMA: direct to the SID's last fix.
        let any = |h: &[AtcOutput]| !h.is_empty();
        f.fly_to(&mut e, ZZ002, CLIMB, &any);
        assert_eq!(
            items(&f.heard),
            vec![AtcItem::DirectTo {
                fix: "ALFAX".into()
            }]
        );
        assert!(say(&mut e, APP, &format!("direct Alfax, {CS_ABBR}")).is_empty());
        f.heard.clear();
        f.fly_to(&mut e, ALFAX, CLIMB, &any);
        assert!(f.heard.is_empty(), "{:?}", f.heard);
        f.fly_to(&mut e, FARLAND, CLIMB, &any);
        let home = LatLon {
            lat: 10.0,
            lon: 10.0,
        };
        let out = distance_m(f.pos, home) / M_PER_NM;
        assert!((39.5..=41.0).contains(&out), "{out} NM");
        assert_eq!(
            items(&f.heard),
            vec![
                AtcItem::RadarServiceTerminated,
                AtcItem::Contact {
                    station: "Zeta Information".into(),
                    khz: FIS,
                },
            ]
        );
        assert_eq!(f.heard[0].message.lang, Lang::En);
        assert_eq!(
            e.strip(&Callsign::new("SPTST"))
                .unwrap()
                .handed_off
                .as_deref(),
            Some("Zeta Information")
        );
    }

    #[test]
    fn off_the_sid_is_called_once() {
        let mut e = engine();
        let mut f = departed(&mut e, "Testville", true);
        let tower = f.heard.drain(..).next().unwrap();
        assert!(say(&mut e, TWR, &readback(&tower.message.items, CS_ABBR)).is_empty());
        assert_eq!(
            items(&say(&mut e, APP, AIRBORNE)),
            vec![AtcItem::Identified]
        );
        // 1.5 NM north of the SID between ZZ001 and ZZ002, flown for well over 25 s.
        let north = |lon: f64| LatLon { lat: 10.025, lon };
        f.fly_to(&mut e, north(9.97), CLIMB, &never);
        f.fly_to(&mut e, north(9.92), CLIMB, &never);
        let calls = f.said(|i| matches!(i, AtcItem::OffTrack { .. }));
        assert_eq!(calls.len(), 1, "{:?}", f.heard);
        assert_eq!(calls[0].freq_khz, APP);
        assert_eq!(calls[0].message.lang, Lang::En);
        assert!(calls[0].text.contains("off track"), "{}", calls[0].text);
        assert_eq!(f.heard.len(), 1, "{:?}", f.heard);
    }

    #[test]
    fn vfr_departure_stays_with_tower() {
        let mut e = engine();
        let mut f = departed(&mut e, "Testville", false);
        f.fly_to(&mut e, ALFAX, (90.0, 500.0, 2500.0), &never);
        // Only Tower speaks: it asks for the report at the exit point (AIP AD 2.22 §3).
        assert_eq!(f.heard.len(), 1, "{:?}", f.heard);
        assert_eq!(f.heard[0].freq_khz, TWR);
        assert_eq!(
            f.heard[0].message.items,
            vec![AtcItem::ReportAt {
                point: "ROMEO".into()
            }]
        );
        let strip = e.strip(&Callsign::new("SPTST")).unwrap();
        assert_eq!(strip.owner, Role::Tower);
        assert_eq!(strip.phase, FlightPhase::Airborne);
    }

    /// A SID ceiling below the cruise level: Approach clears the climb to cruise with the
    /// identification.
    #[test]
    fn identification_clears_the_climb_to_cruise() {
        let mut e = engine_with(|set| {
            for p in set
                .procedures
                .iter_mut()
                .filter(|p| p.procedure.ident == "ALFA1A")
            {
                p.legs[0].leg.alt = AltConstraint::AtOrBelow(4000);
            }
        });
        let mut f = departed_climbing(&mut e, "Testville", true, "four thousand");
        let tower = f.heard.drain(..).next().unwrap();
        assert!(say(&mut e, TWR, &readback(&tower.message.items, CS_ABBR)).is_empty());
        let out = say(&mut e, APP, AIRBORNE);
        assert_eq!(
            items(&out),
            vec![AtcItem::Identified, AtcItem::ClimbTo(Level::Altitude(5000))]
        );
        assert_eq!(
            e.strip(&Callsign::new("SPTST"))
                .unwrap()
                .plan
                .as_ref()
                .unwrap()
                .cleared_ft,
            5000
        );
    }

    /// Stand 1 of ZZZZ.
    pub(super) const STAND_1: LatLon = LatLon {
        lat: 10.0012,
        lon: 10.0,
    };

    /// The IFR departure `f` to Testville, read back throughout: transferred to Approach,
    /// identified, direct to Testville after ZZ002, the descent 20 NM out and the hand-off to
    /// Testville Tower 10 NM out, which identifies it.
    pub(super) fn fly_to_testville_tower(e: &mut AtcEngine, f: &mut Flight) {
        let tower = std::mem::take(&mut f.heard);
        assert_eq!(
            tower.iter().filter_map(contact_of).collect::<Vec<_>>(),
            vec![("Zeta Approach".to_string(), APP)],
            "{tower:?}"
        );
        assert!(say(e, TWR, &readback(&tower[0].message.items, CS_ABBR)).is_empty());
        let out = say(e, APP, AIRBORNE);
        assert_eq!(items(&out), vec![AtcItem::Identified], "{out:?}");
        let any = |h: &[AtcOutput]| !h.is_empty();
        f.fly_to(e, ZZ002, CLIMB, &any);
        assert_eq!(
            items(&f.heard),
            vec![AtcItem::DirectTo {
                fix: "Testville".into()
            }]
        );
        assert!(say(e, APP, &format!("proceed direct Testville, {CS_ABBR}")).is_empty());
        f.heard.clear();
        f.fly_to(e, TESTVILLE, CLIMB, &any);
        assert_eq!(
            items(&f.heard),
            vec![
                AtcItem::DescendTo(Level::Altitude(2500)),
                AtcItem::Qnh(1013)
            ]
        );
        let descent =
            format!("descend to two thousand five hundred feet, QNH one zero one three, {CS_ABBR}");
        assert!(say(e, APP, &descent).is_empty());
        f.heard.clear();
        f.fly_to(e, TESTVILLE, CLIMB, &any);
        assert_eq!(
            f.heard.iter().filter_map(contact_of).collect::<Vec<_>>(),
            vec![("Testville Tower".to_string(), DEST_TWR)],
            "{:?}",
            f.heard
        );
        assert!(say(e, APP, &readback(&f.heard[0].message.items, CS_ABBR)).is_empty());
        let out = say(
            e,
            DEST_TWR,
            &format!("Testville Tower, {CS}, five thousand feet"),
        );
        assert_eq!(items(&out), vec![AtcItem::Identified], "{out:?}");
        f.heard.clear();
    }

    /// Spec 3.6: a second IFR departure of the same aircraft in the same session, after the
    /// first was handed off to the adjacent tower and the aircraft parked at home again, gets
    /// the full Approach service again.
    #[test]
    fn second_departure_after_a_hand_off() {
        let mut e = engine();
        let mut f = departed(&mut e, "Testville", true);
        fly_to_testville_tower(&mut e, &mut f);
        // Repositioned to Stand 1 and stopped there: parked.
        f.pos = STAND_1;
        f.alt = 800.0;
        f.rolling = true;
        for _ in 0..3 {
            f.step(&mut e, 0.0, 0.0, 0.0, true);
        }
        let cs = Callsign::new("SPTST");
        assert_eq!(e.strip(&cs).unwrap().phase, FlightPhase::Parked);
        let strip = e.strip(&cs).unwrap();
        assert_eq!(strip.handed_off, None);
        assert_eq!(strip.plan, None);
        assert!(!strip.identified && !strip.adjacent_identified);
        // The second flight: Approach identifies, clears the direct and hands off again.
        let mut f = departed_at(&mut e, "Testville", true, "five thousand", f.t);
        fly_to_testville_tower(&mut e, &mut f);
        assert_eq!(
            e.strip(&cs).unwrap().handed_off.as_deref(),
            Some("Testville Tower")
        );
    }

    /// The first call to Approach made on the Tower frequency (COM1 not switched): Tower
    /// says who it is and gives Approach's frequency, rather than "say again".
    #[test]
    fn call_to_approach_on_the_tower_frequency() {
        let mut e = engine();
        departed(&mut e, "Testville", true);
        let out = say(&mut e, TWR, AIRBORNE);
        assert_eq!(
            items(&out),
            vec![
                AtcItem::ThisIs {
                    station: "Zeta Tower".into()
                },
                AtcItem::Contact {
                    station: "Zeta Approach".into(),
                    khz: APP
                }
            ],
            "{out:?}"
        );
        assert!(
            out[0]
                .text
                .contains("this is Zeta Tower, contact Zeta Approach"),
            "{out:?}"
        );
        // On Approach's frequency the same call is answered.
        let out = say(&mut e, APP, AIRBORNE);
        assert_eq!(items(&out), vec![AtcItem::Identified], "{out:?}");
    }

    /// A server restart in the middle of an IFR departure: the snapshot taken after the
    /// take-off, through JSON into a fresh engine, carries the flight on (Approach identifies,
    /// clears the direct, descends and hands off) as if nothing happened.
    #[test]
    fn snapshot_restores_a_departure_in_progress() {
        let mut e = engine();
        let mut f = departed(&mut e, "Testville", true);
        let json = serde_json::to_string(&e.snapshot()).unwrap();
        let mut e = engine();
        e.restore(serde_json::from_str(&json).unwrap()).unwrap();
        fly_to_testville_tower(&mut e, &mut f);
        assert_eq!(
            e.strip(&Callsign::new("SPTST"))
                .unwrap()
                .handed_off
                .as_deref(),
            Some("Testville Tower")
        );
    }

    /// A snapshot of another format version or another airport is refused; the engine keeps
    /// its own state.
    #[test]
    fn snapshot_of_another_version_or_airport_is_refused() {
        let mut e = engine();
        departed(&mut e, "Testville", true);
        let mut snap = e.snapshot();
        snap.version += 1;
        let mut fresh = engine();
        assert!(matches!(
            fresh.restore(snap.clone()),
            Err(SnapshotMismatch::Version { .. })
        ));
        snap.version = SNAPSHOT_VERSION;
        snap.airport = "EPKT".into();
        assert!(matches!(
            fresh.restore(snap),
            Err(SnapshotMismatch::Airport { .. })
        ));
        assert_eq!(fresh.strip(&Callsign::new("SPTST")).unwrap().plan, None);
    }

    /// An aircraft that stopped after a flight on the apron, away from every stand, and
    /// asks for start-up begins its next flight there: start-up approved, not "say again".
    #[test]
    fn start_up_after_a_flight_away_from_a_stand() {
        let mut e = engine();
        let mut f = departed(&mut e, "Testville", true);
        fly_to_testville_tower(&mut e, &mut f);
        // Repositioned about 55 m north of Stand 1 and stopped there.
        f.pos = LatLon {
            lat: STAND_1.lat + 0.0005,
            lon: STAND_1.lon,
        };
        f.alt = 800.0;
        f.rolling = true;
        for _ in 0..3 {
            f.step(&mut e, 0.0, 0.0, 0.0, true);
        }
        let cs = Callsign::new("SPTST");
        assert_eq!(e.strip(&cs).unwrap().phase, FlightPhase::TaxiIn);
        let out = say(
            &mut e,
            GND,
            &format!("Zeta Ground, {CS}, information Alpha, request start up"),
        );
        assert_eq!(
            items(&out),
            vec![AtcItem::StartUpApproved, AtcItem::Qnh(1013)],
            "{out:?}"
        );
        let strip = e.strip(&cs).unwrap();
        assert_eq!(strip.phase, FlightPhase::Parked);
        assert_eq!(strip.handed_off, None);
        assert_eq!(strip.plan, None);
    }

    /// Spec 3.6: a departure handed to the information unit beyond the TMA turns back and
    /// calls the unit inbound: it is sent to Approach, which serves it as an arrival.
    #[test]
    fn inbound_to_information_after_a_hand_off() {
        let mut e = engine();
        let mut f = departed(&mut e, "Farland", true);
        let tower = f.heard.drain(..).next().unwrap();
        assert!(say(&mut e, TWR, &readback(&tower.message.items, CS_ABBR)).is_empty());
        assert_eq!(
            items(&say(&mut e, APP, AIRBORNE)),
            vec![AtcItem::Identified]
        );
        let handed = |h: &[AtcOutput]| h.iter().any(|o| contact_of(o).is_some());
        f.fly_to(&mut e, FARLAND, CLIMB, &handed);
        let contact = f.heard.last().unwrap().clone();
        assert_eq!(
            contact_of(&contact),
            Some(("Zeta Information".to_string(), FIS))
        );
        assert!(say(&mut e, APP, &readback(&contact.message.items, CS_ABBR)).is_empty());
        let out = say(
            &mut e,
            FIS,
            &format!("Zeta Information, {CS}, five thousand feet"),
        );
        assert_eq!(items(&out), vec![AtcItem::Identified], "{out:?}");
        let out = say(
            &mut e,
            FIS,
            &format!("Zeta Information, {CS}, IFR, five thousand feet, inbound Zeta"),
        );
        assert_eq!(
            items(&out),
            vec![AtcItem::Contact {
                station: "Zeta Approach".into(),
                khz: APP,
            }],
            "{out:?}"
        );
        let cs = Callsign::new("SPTST");
        assert_eq!(e.strip(&cs).unwrap().handed_off, None);
        assert!(
            say(
                &mut e,
                FIS,
                &format!("Approach one one niner decimal one, {CS_ABBR}")
            )
            .is_empty()
        );
        f.step(&mut e, 180.0, 90.0, 0.0, false);
        let out = say(&mut e, APP, &format!("Zeta Approach, {CS}, IFR, inbound"));
        assert_eq!(items(&out).first(), Some(&AtcItem::Identified), "{out:?}");
        let strip = e.strip(&cs).unwrap();
        assert!(strip.identified);
        assert_eq!(
            strip.plan.as_ref().map(|p| p.destination.as_str()),
            Some("ZZZZ")
        );
    }

    /// 12 NM west of the field, outside the circuit.
    const WEST: LatLon = LatLon {
        lat: 10.0,
        lon: 9.8,
    };

    /// SPTST airborne at 3000 ft west of the field on the Approach frequency, without a
    /// flight plan (it took off VFR without a clearance and left the circuit).
    fn airborne_without_plan(e: &mut AtcEngine) -> Flight {
        let mut f = Flight {
            t: 0.0,
            pos: WEST,
            alt: 3000.0,
            com1: APP,
            rolling: false,
            xpdr: None,
            heard: Vec::new(),
        };
        f.step(e, 0.0, 90.0, 0.0, false);
        assert!(f.heard.is_empty(), "{:?}", f.heard);
        assert!(e.strip(&Callsign::new("SPTST")).unwrap().plan.is_none());
        f
    }

    const IFR_REQUEST: &str = "Zeta Approach, Sierra Papa Tango Sierra Tango, request IFR \
                               clearance to Testville, flight level eight zero";

    #[test]
    fn departure_report_without_plan_gets_say_intentions() {
        let mut e = engine();
        airborne_without_plan(&mut e);
        let out = say(&mut e, APP, AIRBORNE);
        assert_eq!(
            items(&out),
            vec![AtcItem::NoFlightPlan, AtcItem::SayIntentions],
            "{out:?}"
        );
        assert!(
            out[0].text.ends_with("no flight plan, say intentions"),
            "{}",
            out[0].text
        );
    }

    /// Plan 5 ruling: no SID or route in the air, cleared to the destination, the cruise
    /// level and a squawk; then handed off like a SID departure.
    #[test]
    fn airborne_ifr_clearance() {
        let mut e = engine();
        let mut f = airborne_without_plan(&mut e);
        assert_eq!(
            items(&say(&mut e, APP, AIRBORNE)),
            vec![AtcItem::NoFlightPlan, AtcItem::SayIntentions]
        );
        let out = say(&mut e, APP, IFR_REQUEST);
        let got = items(&out);
        let code = squawk_of(&got).expect("squawk");
        assert_eq!(
            got,
            vec![
                AtcItem::Identified,
                AtcItem::ClearedTo {
                    destination: "Testville".into(),
                    icao: "ZZZY".into(),
                },
                AtcItem::ClimbTo(Level::FlightLevel(80)),
                AtcItem::Squawk(code),
            ],
            "{out:?}"
        );
        let rb = format!(
            "cleared to Testville, climb flight level eight zero, squawk {}, {CS_ABBR}",
            spoken(&format!("{code:04}"))
        );
        assert_eq!(
            items(&say(&mut e, APP, &rb)),
            vec![AtcItem::ReadbackCorrect]
        );
        let cs = Callsign::new("SPTST");
        let strip = e.strip(&cs).unwrap();
        assert!(strip.identified);
        assert!(strip.pending_readback.is_none());
        let plan = strip.plan.clone().expect("plan");
        assert!(plan.ifr);
        assert_eq!(plan.destination, "ZZZY");
        assert_eq!(plan.sid, None);
        assert_eq!(plan.dep_runway, None);
        assert_eq!((plan.cruise_ft, plan.cleared_ft), (8000, 8000));
        // Flying on: no direct (no SID), the descent 20 NM out, the hand-off 10 NM out.
        let any = |h: &[AtcOutput]| !h.is_empty();
        f.xpdr = Some(code);
        f.fly_to(&mut e, TESTVILLE, (90.0, 700.0, 8000.0), &any);
        assert_eq!(
            items(&f.heard),
            vec![
                AtcItem::DescendTo(Level::Altitude(2500)),
                AtcItem::Qnh(1013)
            ]
        );
        let descent =
            format!("descend to two thousand five hundred feet, QNH one zero one three, {CS_ABBR}");
        assert!(say(&mut e, APP, &descent).is_empty());
        f.heard.clear();
        f.fly_to(&mut e, TESTVILLE, (90.0, -700.0, 8000.0), &any);
        assert_eq!(f.heard.len(), 1, "{:?}", f.heard);
        assert_eq!(
            contact_of(&f.heard[0]),
            Some(("Testville Tower".to_string(), DEST_TWR))
        );
        assert_eq!(
            e.strip(&cs).unwrap().handed_off.as_deref(),
            Some("Testville Tower")
        );
    }

    #[test]
    fn airborne_request_without_rules_gets_say_flight_rules() {
        let mut e = engine();
        airborne_without_plan(&mut e);
        let out = say(
            &mut e,
            APP,
            "Zeta Approach, Sierra Papa Tango Sierra Tango, request clearance to Testville",
        );
        assert_eq!(items(&out), vec![AtcItem::SayFlightRules], "{out:?}");
        assert!(e.strip(&Callsign::new("SPTST")).unwrap().plan.is_none());
    }

    /// A VFR request in the air is not Approach's: handled as before this plan.
    #[test]
    fn airborne_vfr_request_goes_to_tower_rules() {
        let mut e = engine();
        airborne_without_plan(&mut e);
        let out = say(
            &mut e,
            APP,
            "Zeta Approach, Sierra Papa Tango Sierra Tango, request VFR clearance to Testville",
        );
        assert_eq!(items(&out), vec![AtcItem::SayAgain], "{out:?}");
        assert!(e.strip(&Callsign::new("SPTST")).unwrap().plan.is_none());
    }

    #[test]
    fn airborne_clearance_with_plan_is_ignored() {
        let mut e = engine();
        let mut f = departed(&mut e, "Testville", true);
        let tower = f.heard.drain(..).next().unwrap();
        assert!(say(&mut e, TWR, &readback(&tower.message.items, CS_ABBR)).is_empty());
        assert_eq!(
            items(&say(&mut e, APP, AIRBORNE)),
            vec![AtcItem::Identified]
        );
        let cs = Callsign::new("SPTST");
        let before = e.strip(&cs).unwrap().plan.clone();
        let out = say(
            &mut e,
            APP,
            "Zeta Approach, Sierra Papa Tango Sierra Tango, request IFR clearance to Farland, \
             flight level eight zero",
        );
        assert!(
            !items(&out)
                .iter()
                .any(|i| matches!(i, AtcItem::ClearedTo { .. })),
            "{out:?}"
        );
        assert_eq!(e.strip(&cs).unwrap().plan, before);
    }
}

/// Approach, arrivals (spec 3.3): identification and the route to the approach, radar vectors,
/// approach clearances, transfer to Tower and the missed approach, on the fictional ZZZZ
/// (RWY 27 threshold at 10.0 N 10.01667 E, final course 270, magnetic variation 0).
mod approach_arrival {
    use std::collections::HashMap;

    use atc_geo::{M_PER_NM, RunwayFrame, bearing_deg, destination, distance_m};
    use atc_navdata::fixes::ProcedureSet;
    use atc_phraseology::{ApproachKind, Level, UnableReason};

    use super::approach_departure::{
        APP, APP_ALT, DEST_TWR, DT, END_27, Flight, THR_27, TWR, contact_of, engine, engine_with,
    };
    use super::*;

    /// The 8 NM point on the final of RWY 27.
    fn intercept_point() -> LatLon {
        destination(THR_27, 90.0, 8.0 * M_PER_NM)
    }

    /// Inside every STAR start fix (ALFAX is 13 NM out), north-east of the field.
    const INSIDE: LatLon = LatLon {
        lat: 10.1,
        lon: 10.1,
    };
    /// 32 NM north-west, beyond ALFAX.
    const BEYOND: LatLon = LatLon {
        lat: 10.3,
        lon: 9.5,
    };
    /// The ILS 27 IAF (fixture ZZ011, renamed so that it can be spoken).
    const IAF: LatLon = LatLon {
        lat: 10.05,
        lon: 9.88,
    };

    const INBOUND: &str = "Zeta Approach, Sierra Papa Tango Sierra Tango, IFR, inbound";
    /// West of the field, inside the STAR start fixes and 21 NM from the intercept point.
    const WEST: LatLon = LatLon {
        lat: 10.0,
        lon: 9.8,
    };

    /// [`engine`] with the ILS 27 IAF ZZ011 renamed IAFOX (a name the parser can hear).
    fn engine_iafox() -> AtcEngine {
        engine_with(|set: &mut ProcedureSet| {
            for p in &mut set.procedures {
                for l in &mut p.legs {
                    if let Some(fix) = l.leg.fix.as_mut().filter(|f| f.ident == "ZZ011") {
                        fix.ident = "IAFOX".into();
                    }
                }
            }
            if let Some(v) = set.fixes.get("ZZ011").cloned() {
                set.fixes.insert("IAFOX".into(), v);
            }
        })
    }

    /// Feet as the pilot says them: 2500 -> "two thousand five hundred feet".
    fn feet(ft: i32) -> String {
        let (th, hu) = (ft / 1000, ft % 1000 / 100);
        let mut s = format!("{} thousand", spoken(&th.to_string()));
        if hu > 0 {
            s.push_str(&format!(" {} hundred", spoken(&hu.to_string())));
        }
        s + " feet"
    }

    fn letter(c: char) -> String {
        spoken(&c.to_string())
    }

    /// A correct readback of an Approach or Tower instruction, `None` when none is needed.
    fn ifr_readback(items: &[AtcItem]) -> Option<String> {
        if required_readback(items, Strictness::Standard).is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        for it in items {
            match it {
                AtcItem::Star {
                    fix,
                    number,
                    letter: l,
                } => parts.push(format!(
                    "{fix} {} {} arrival",
                    spoken(&number.to_string()),
                    letter(*l)
                )),
                AtcItem::DescendTo(Level::Altitude(a)) => {
                    parts.push(format!("descend to {}", feet(*a)))
                }
                AtcItem::ClimbTo(Level::Altitude(a)) => parts.push(format!("climb {}", feet(*a))),
                AtcItem::MaintainLevel(Level::Altitude(a)) => {
                    parts.push(format!("maintain {}", feet(*a)))
                }
                AtcItem::Qnh(q) => parts.push(format!("QNH {}", spoken(&q.to_string()))),
                AtcItem::Heading { deg, .. } => {
                    parts.push(format!("heading {}", spoken(&format!("{deg:03}"))))
                }
                AtcItem::DirectTo { fix } => parts.push(format!("direct {fix}")),
                AtcItem::ClearedApproach { kind, runway } => parts.push(format!(
                    "cleared {} approach runway {}",
                    match kind {
                        ApproachKind::Ils => "ILS",
                        ApproachKind::VorDme => "VOR DME",
                        ApproachKind::Rnav => "RNAV",
                    },
                    spoken(runway)
                )),
                AtcItem::Contact { khz, .. } => {
                    parts.push(format!("contact {}", freq_spoken(*khz)))
                }
                AtcItem::ClearedLand { runway, .. } => {
                    parts.push(format!("cleared to land runway {}", spoken(runway)))
                }
                _ => {}
            }
        }
        parts.push(CS_ABBR.into());
        Some(parts.join(", "))
    }

    /// A simulated IFR pilot at 100 kt: flies the last heading (or direct to a fix), climbs
    /// or descends at 500 fpm to the last level, and reads back every instruction on the
    /// frequency it came on.
    struct Pilot {
        f: Flight,
        /// True heading flown (variation 0: also the magnetic heading).
        hdg: f64,
        /// Flying direct to this point instead of a heading.
        to: Option<LatLon>,
        /// Level the pilot climbs or descends to.
        target: f64,
        /// Turns onto assigned headings.
        obeys_headings: bool,
        /// Positions of fixes the pilot may be sent direct to.
        fixes: HashMap<String, LatLon>,
    }

    impl Pilot {
        fn new(pos: LatLon, alt: f64, hdg: f64) -> Self {
            Pilot {
                f: Flight {
                    t: 0.0,
                    pos,
                    alt,
                    com1: APP,
                    rolling: false,
                    xpdr: None,
                    heard: Vec::new(),
                },
                hdg,
                to: None,
                target: alt,
                obeys_headings: true,
                fixes: HashMap::from([("IAFOX".to_string(), IAF)]),
            }
        }

        /// One 2 s step; new instructions are followed and read back.
        fn tick(&mut self, e: &mut AtcEngine) {
            if let Some(p) = self.to {
                self.hdg = bearing_deg(self.f.pos, p);
            }
            let vs = ((self.target - self.f.alt) * 30.0).clamp(-500.0, 500.0);
            let before = self.f.heard.len();
            self.f.step(e, self.hdg, 100.0, vs, false);
            self.f.pos = destination(self.f.pos, self.hdg, 100.0 * 1852.0 / 3600.0 * DT);
            self.f.alt += vs * DT / 60.0;
            let new: Vec<AtcOutput> = self.f.heard[before..].to_vec();
            for o in new {
                self.hear(e, &o);
            }
        }

        /// Follows `o` and reads it back.
        fn hear(&mut self, e: &mut AtcEngine, o: &AtcOutput) {
            for it in &o.message.items {
                match it {
                    AtcItem::Heading { deg, .. } if self.obeys_headings => {
                        self.hdg = f64::from(*deg);
                        self.to = None;
                    }
                    AtcItem::DescendTo(Level::Altitude(a))
                    | AtcItem::ClimbTo(Level::Altitude(a))
                    | AtcItem::MaintainLevel(Level::Altitude(a)) => self.target = f64::from(*a),
                    AtcItem::DirectTo { fix } => self.to = self.fixes.get(fix).copied(),
                    AtcItem::Contact { khz, .. } => self.f.com1 = *khz,
                    _ => {}
                }
            }
            if let Some(rb) = ifr_readback(&o.message.items) {
                let out = say(e, o.freq_khz, &rb);
                assert!(out.is_empty(), "readback {rb:?} answered: {out:?}");
            }
        }

        /// Ticks until `stop` holds (at most `max` steps).
        fn until(&mut self, e: &mut AtcEngine, max: usize, stop: impl Fn(&Pilot) -> bool) {
            for _ in 0..max {
                if stop(self) {
                    return;
                }
                self.tick(e);
            }
            panic!("stop condition not reached: {:?}", self.f.heard);
        }

        fn heard_item(&self, f: impl Fn(&AtcItem) -> bool) -> bool {
            self.f.heard.iter().any(|o| o.message.items.iter().any(&f))
        }

        /// Says `text` on `freq`; the answer is heard, followed and read back.
        fn call(&mut self, e: &mut AtcEngine, freq: u32, text: &str) -> Vec<AtcOutput> {
            let out = say(e, freq, text);
            self.f.heard.extend(out.clone());
            for o in &out {
                self.hear(e, o);
            }
            out
        }
    }

    fn cs() -> Callsign {
        Callsign::new("SPTST")
    }

    /// Calls Approach inbound at `pos` with `text` after one sample there.
    fn inbound(
        e: &mut AtcEngine,
        pos: LatLon,
        alt: f64,
        hdg: f64,
        text: &str,
    ) -> (Pilot, Vec<AtcOutput>) {
        let mut p = Pilot::new(pos, alt, hdg);
        p.tick(e);
        assert!(p.f.heard.is_empty(), "{:?}", p.f.heard);
        let out = p.call(e, APP, text);
        (p, out)
    }

    /// "Passing four thousand climbing five thousand": the level it climbs to is the one it
    /// reports; Approach clears the climb to it rather than stopping it at the passing level.
    #[test]
    fn inbound_while_climbing_keeps_the_level_it_climbs_to() {
        let mut e = engine();
        let text = "Zeta Approach, Sierra Papa Tango Sierra Tango, IFR, passing four thousand \
                    feet climbing five thousand feet, inbound";
        let (_, out) = inbound(&mut e, WEST, 4000.0, 90.0, text);
        let got = items(&out);
        assert!(
            got.contains(&AtcItem::ClimbTo(Level::Altitude(5000))),
            "{got:?}"
        );
        assert_eq!(
            e.strip(&cs()).unwrap().plan.as_ref().unwrap().cleared_ft,
            5000
        );
    }

    #[test]
    fn star_from_beyond_the_start_fix() {
        let mut e = engine();
        let (_, out) = inbound(&mut e, BEYOND, 7000.0, 120.0, INBOUND);
        assert_eq!(
            items(&out),
            vec![
                AtcItem::Identified,
                AtcItem::Star {
                    fix: "ALFAX".into(),
                    number: 1,
                    letter: 'B'
                },
                AtcItem::DescendTo(Level::Altitude(5000)),
                AtcItem::Qnh(1013),
                AtcItem::ExpectApproach {
                    kind: ApproachKind::Ils,
                    runway: "27".into()
                },
            ]
        );
        assert_eq!((out[0].freq_khz, out[0].message.lang), (APP, Lang::En));
        let strip = e.strip(&cs()).unwrap();
        let plan = strip.plan.as_ref().unwrap();
        assert!(plan.ifr);
        assert_eq!(plan.destination, "ZZZZ");
        assert_eq!(plan.star.as_deref(), Some("ALFA1B"));
        assert_eq!(plan.approach, Some((ApproachKind::Ils, "27".into())));
        assert!(plan.vectors);
        assert_eq!(plan.cleared_ft, 5000);
        assert!(strip.identified);
        assert_eq!(strip.owner, Role::Approach);
    }

    /// Flying to the STAR's start fix from far out is not off the STAR's track.
    #[test]
    fn flying_to_the_star_start_fix_is_on_track() {
        let mut e = engine();
        let (mut p, _) = inbound(&mut e, BEYOND, 5000.0, 120.0, INBOUND);
        p.to = Some(LatLon {
            lat: 10.1,
            lon: 9.8,
        });
        for _ in 0..150 {
            p.tick(&mut e);
        }
        assert!(
            !p.heard_item(|i| matches!(i, AtcItem::OffTrack { .. })),
            "{:?}",
            p.f.heard
        );
    }

    /// Vectored from the side (at right angles to the final), the aircraft is turned onto
    /// the 30-degree intercept early enough to join the final at about the 8 NM point, not
    /// deep inside it.
    #[test]
    fn side_arrival_joins_the_final_near_the_intercept_point() {
        let mut e = engine();
        let start = destination(intercept_point(), 0.0, 9.0 * M_PER_NM);
        let (mut p, _) = inbound(&mut e, start, 3000.0, 180.0, INBOUND);
        p.until(&mut e, 600, |p| {
            p.heard_item(|i| matches!(i, AtcItem::ClearedApproach { .. }))
        });
        let frame = RunwayFrame {
            origin: THR_27,
            course_deg: 270.0,
        };
        p.until(&mut e, 600, |p| {
            frame.to_local(p.f.pos).1.abs() < 0.1 * M_PER_NM
        });
        let out_nm = -frame.to_local(p.f.pos).0 / M_PER_NM;
        assert!(out_nm >= 7.0, "joined the final {out_nm:.1} NM out");
        // One heading to the lead point, one onto the intercept: no re-vector as the
        // bearing to the lead point swings near it.
        let headings =
            p.f.heard
                .iter()
                .filter(|o| {
                    o.message
                        .items
                        .iter()
                        .any(|i| matches!(i, AtcItem::Heading { .. }))
                })
                .count();
        assert_eq!(headings, 2, "{:?}", p.f.heard);
    }

    /// The intercept altitude is the final approach fix's, not a higher one published at
    /// the start of the final segment (EPKK VOR/DME 07: IF +5000, FAF +3000).
    #[test]
    fn intercept_altitude_is_the_faf_altitude() {
        let mut e = engine_with(|set: &mut ProcedureSet| {
            for p in &mut set.procedures {
                if p.procedure.ident == "I27" && p.procedure.transition.is_empty() {
                    let first = p.legs.first_mut().unwrap();
                    first.leg.alt = atc_navdata::cifp::AltConstraint::AtOrAbove(5000);
                }
            }
        });
        let (_, out) = inbound(&mut e, INSIDE, 5000.0, 180.0, INBOUND);
        assert!(
            items(&out).contains(&AtcItem::DescendTo(Level::Altitude(3000))),
            "{out:?}"
        );
    }

    /// Vectored from inside the start fixes to the 8 NM point, cleared for the ILS on the
    /// intercept heading, transferred to Tower on "established", cleared to land by Tower.
    #[test]
    fn vectors_to_final_and_transfer_on_established() {
        let mut e = engine();
        let (mut p, out) = inbound(&mut e, INSIDE, 5000.0, 180.0, INBOUND);
        let first = items(&out);
        // Towards the lead point 3 NM back along the 30-degree intercept from the north.
        let lead = destination(intercept_point(), 60.0, 3.0 * M_PER_NM);
        let want = (bearing_deg(INSIDE, lead) / 5.0).round() as u16 * 5;
        assert!(
            matches!(first.as_slice(),
                [AtcItem::Identified, AtcItem::Heading { deg, .. }, AtcItem::DescendTo(Level::Altitude(3000)),
                 AtcItem::Qnh(1013), AtcItem::ExpectApproach { kind: ApproachKind::Ils, .. }]
                if *deg == want),
            "{first:?}, want heading {want}"
        );
        // On the heading until the approach clearance with the intercept heading.
        p.until(&mut e, 400, |p| {
            p.heard_item(|i| matches!(i, AtcItem::ClearedApproach { .. }))
        });
        let clearance = p.f.heard.last().unwrap().message.items.clone();
        assert!(
            matches!(clearance.as_slice(),
                [AtcItem::Heading { deg: 240, .. }, AtcItem::DistanceToGo(_),
                 AtcItem::ClearedApproach { kind: ApproachKind::Ils, runway }, AtcItem::ReportEstablished]
                if runway == "27"),
            "{clearance:?}"
        );
        let to_point = distance_m(p.f.pos, intercept_point()) / M_PER_NM;
        assert!(to_point <= 3.0, "{to_point} NM from the intercept point");
        let dtg = distance_m(p.f.pos, THR_27) / M_PER_NM;
        assert!(
            clearance.contains(&AtcItem::DistanceToGo(dtg.round() as u16)),
            "{clearance:?} at {dtg} NM"
        );
        // Every Approach transmission is English, on its frequency.
        assert!(
            p.f.heard
                .iter()
                .all(|o| o.freq_khz == APP && o.message.lang == Lang::En)
        );
        // Intercept the localizer, then report established.
        let centreline = destination(THR_27, 90.0, 6.0 * M_PER_NM);
        p.to = Some(centreline);
        p.until(&mut e, 200, |p| distance_m(p.f.pos, centreline) < 300.0);
        p.to = Some(THR_27);
        p.target = 2000.0;
        p.tick(&mut e);
        p.tick(&mut e);
        let n = p.f.heard.len();
        let out = p.call(
            &mut e,
            APP,
            "Zeta Approach, Sierra Sierra Tango, established ILS runway two seven",
        );
        assert_eq!(
            items(&out),
            vec![AtcItem::Contact {
                station: "Zeta Tower".into(),
                khz: TWR
            }]
        );
        assert_eq!(e.strip(&cs()).unwrap().owner, Role::Tower);
        // Tower answers the first call with the landing clearance.
        let out = p.call(
            &mut e,
            TWR,
            "Zeta Tower, Sierra Sierra Tango, established ILS runway two seven",
        );
        assert!(
            matches!(items(&out).as_slice(), [AtcItem::ClearedLand { runway, .. }] if runway == "27"),
            "{out:?} (phase {:?})",
            e.strip(&cs()).unwrap().phase
        );
        assert_eq!(p.f.heard.len(), n + 2);
    }

    /// "Request full procedure": direct to the IAF, the approach clearance there.
    #[test]
    fn full_procedure_from_the_iaf() {
        let mut e = engine_iafox();
        let text = "Zeta Approach, Sierra Papa Tango Sierra Tango, inbound, six thousand feet, \
                    request full procedure";
        let (mut p, out) = inbound(&mut e, INSIDE, 6000.0, 180.0, text);
        let first = items(&out);
        assert!(
            matches!(first.as_slice(),
                [AtcItem::Identified, AtcItem::DirectTo { fix }, AtcItem::DescendTo(Level::Altitude(5000)),
                 AtcItem::Qnh(1013), AtcItem::ExpectApproach { kind: ApproachKind::Ils, .. }]
                if fix == "IAFOX"),
            "{first:?}"
        );
        assert!(!e.strip(&cs()).unwrap().plan.as_ref().unwrap().vectors);
        p.until(&mut e, 600, |p| {
            p.heard_item(|i| matches!(i, AtcItem::ClearedApproach { .. }))
        });
        assert_eq!(
            p.f.heard.last().unwrap().message.items,
            vec![
                AtcItem::ClearedApproach {
                    kind: ApproachKind::Ils,
                    runway: "27".into()
                },
                AtcItem::ReportEstablished
            ]
        );
        assert!(distance_m(p.f.pos, IAF) <= M_PER_NM + 100.0);
        assert!(!p.heard_item(|i| matches!(i, AtcItem::Heading { .. })));
    }

    /// Vectored, cleared, flown down the final without a report: transferred to Tower
    /// inside the FAF (5 NM); cleared to land, overflies the runway and climbs: Tower hands
    /// it back to Approach, which vectors it again.
    #[test]
    fn missed_approach_goes_back_to_approach() {
        let mut e = engine();
        let (mut p, _) = inbound(&mut e, INSIDE, 5000.0, 180.0, INBOUND);
        p.until(&mut e, 400, |p| {
            p.heard_item(|i| matches!(i, AtcItem::ClearedApproach { .. }))
        });
        let centreline = destination(THR_27, 90.0, 7.0 * M_PER_NM);
        p.to = Some(centreline);
        p.until(&mut e, 200, |p| distance_m(p.f.pos, centreline) < 300.0);
        p.to = Some(THR_27);
        p.target = 1500.0;
        let n = p.f.heard.len();
        p.until(&mut e, 200, |p| p.f.heard.len() > n);
        assert_eq!(
            contact_of(&p.f.heard[n]),
            Some(("Zeta Tower".to_string(), TWR))
        );
        let out_nm = distance_m(p.f.pos, THR_27) / M_PER_NM;
        assert!((4.5..=5.3).contains(&out_nm), "{out_nm} NM");
        let out = p.call(
            &mut e,
            TWR,
            "Zeta Tower, Sierra Sierra Tango, ILS runway two seven, final",
        );
        assert!(
            matches!(items(&out).as_slice(), [AtcItem::ClearedLand { .. }]),
            "{out:?}"
        );
        // Over the runway at 500 ft and away: missed approach.
        p.target = 500.0;
        p.until(&mut e, 200, |p| distance_m(p.f.pos, THR_27) < 200.0);
        p.to = Some(END_27);
        p.target = 3000.0;
        let n = p.f.heard.len();
        p.until(&mut e, 100, |p| p.f.heard.len() > n);
        let back = &p.f.heard[n];
        assert_eq!(back.freq_khz, TWR, "{back:?}");
        assert_eq!(contact_of(back), Some(("Zeta Approach".to_string(), APP)));
        assert_eq!(e.strip(&cs()).unwrap().owner, Role::Approach);
        let out = p.call(
            &mut e,
            APP,
            "Zeta Approach, Sierra Sierra Tango, missed approach, climbing three thousand feet",
        );
        assert!(
            matches!(
                items(&out).as_slice(),
                [
                    AtcItem::Heading { .. },
                    AtcItem::ClimbTo(Level::Altitude(3000))
                ]
            ),
            "{out:?}"
        );
        assert_eq!(out[0].freq_khz, APP);
        assert!(e.strip(&cs()).unwrap().identified);
    }

    /// Spec 3.6: an IFR departure handed off to the adjacent tower turns back and calls
    /// Approach inbound (same session, still airborne): a new flight, served as an arrival
    /// (identified, vectors, approach clearance, transfer to Tower).
    #[test]
    fn inbound_after_a_hand_off_is_a_new_arrival() {
        let mut e = engine();
        let mut f = super::approach_departure::departed(&mut e, "Testville", true);
        super::approach_departure::fly_to_testville_tower(&mut e, &mut f);
        // Repositioned (still airborne) inside the STAR start fixes, north-east of the field.
        let mut p = Pilot::new(INSIDE, 5000.0, 180.0);
        p.f.t = f.t;
        p.tick(&mut e);
        assert!(p.f.heard.is_empty(), "{:?}", p.f.heard);
        let out = p.call(&mut e, APP, INBOUND);
        let first = items(&out);
        assert!(
            matches!(
                first.as_slice(),
                [
                    AtcItem::Identified,
                    AtcItem::Heading { .. },
                    ..,
                    AtcItem::ExpectApproach {
                        kind: ApproachKind::Ils,
                        ..
                    }
                ]
            ),
            "{out:?}"
        );
        assert_eq!((out[0].freq_khz, out[0].message.lang), (APP, Lang::En));
        let strip = e.strip(&cs()).unwrap();
        assert_eq!(strip.handed_off, None);
        assert!(strip.identified);
        assert_eq!(strip.owner, Role::Approach);
        assert_eq!(
            strip.plan.as_ref().map(|p| p.destination.as_str()),
            Some("ZZZZ")
        );
        p.until(&mut e, 800, |p| {
            p.heard_item(|i| matches!(i, AtcItem::ClearedApproach { .. }))
        });
        let centreline = destination(THR_27, 90.0, 6.0 * M_PER_NM);
        p.to = Some(centreline);
        p.until(&mut e, 300, |p| distance_m(p.f.pos, centreline) < 300.0);
        p.to = Some(THR_27);
        p.target = 2000.0;
        p.tick(&mut e);
        p.tick(&mut e);
        let out = p.call(
            &mut e,
            APP,
            "Zeta Approach, Sierra Sierra Tango, established ILS runway two seven",
        );
        assert_eq!(
            items(&out),
            vec![AtcItem::Contact {
                station: "Zeta Tower".into(),
                khz: TWR
            }]
        );
        assert_eq!(e.strip(&cs()).unwrap().owner, Role::Tower);
    }

    /// Review Focus 5: a heading ignored for 35 s and more is called once, in English.
    #[test]
    fn ignored_heading_is_called_once() {
        let mut e = engine();
        let (mut p, out) = inbound(&mut e, WEST, 5000.0, 0.0, INBOUND);
        let assigned = items(&out)
            .iter()
            .find_map(|i| match i {
                AtcItem::Heading { deg, .. } => Some(*deg),
                _ => None,
            })
            .expect("a heading");
        p.obeys_headings = false;
        p.hdg = f64::from(assigned) + 60.0;
        let n = p.f.heard.len();
        for _ in 0..40 {
            p.tick(&mut e);
        }
        let calls: Vec<_> = p.f.heard[n..]
            .iter()
            .filter(|o| {
                o.message
                    .items
                    .iter()
                    .any(|i| matches!(i, AtcItem::CheckHeading(_)))
            })
            .collect();
        assert_eq!(calls.len(), 1, "{:?}", &p.f.heard[n..]);
        assert_eq!((calls[0].freq_khz, calls[0].message.lang), (APP, Lang::En));
        assert_eq!(
            calls[0].message.items,
            vec![AtcItem::CheckHeading(assigned)]
        );
    }

    /// Review Focus 5: an aircraft on vectors that descends below its cleared level is
    /// called once by Approach (the owner), in English: "check level".
    #[test]
    fn descent_below_the_cleared_level_is_called_once() {
        let mut e = engine();
        let (mut p, out) = inbound(&mut e, WEST, 5000.0, 0.0, INBOUND);
        assert!(
            items(&out).contains(&AtcItem::MaintainLevel(Level::Altitude(5000))),
            "{out:?}"
        );
        assert_eq!(e.strip(&cs()).unwrap().owner, Role::Approach);
        // Settled at 5000 ft on the heading, then down to 4000 ft on its own.
        for _ in 0..3 {
            p.tick(&mut e);
        }
        p.target = 4000.0;
        let n = p.f.heard.len();
        for _ in 0..40 {
            p.tick(&mut e);
        }
        let calls: Vec<_> = p.f.heard[n..]
            .iter()
            .filter(|o| {
                o.message
                    .items
                    .iter()
                    .any(|i| matches!(i, AtcItem::CheckLevel(_)))
            })
            .collect();
        assert_eq!(calls.len(), 1, "{:?}", &p.f.heard[n..]);
        assert_eq!(
            (
                calls[0].freq_khz,
                calls[0].station.as_str(),
                calls[0].message.lang
            ),
            (APP, "Zeta Approach", Lang::En)
        );
        assert_eq!(
            calls[0].message.items,
            vec![AtcItem::CheckLevel(Level::Altitude(5000))]
        );
    }

    /// AIP 2.22 §2.1: squawk 7600 on vectors: Approach stops talking to the aircraft.
    #[test]
    fn squawk_7600_silences_approach() {
        let mut e = engine();
        let (mut p, out) = inbound(&mut e, WEST, 5000.0, 0.0, INBOUND);
        assert!(!out.is_empty());
        p.obeys_headings = false;
        p.hdg = 300.0;
        p.f.xpdr = Some(7600);
        let n = p.f.heard.len();
        for _ in 0..60 {
            p.tick(&mut e);
        }
        assert!(e.strip(&cs()).unwrap().radio_failure);
        let out = say(
            &mut e,
            APP,
            "Zeta Approach, Sierra Sierra Tango, request vectors",
        );
        assert!(out.is_empty(), "{out:?}");
        assert!(p.f.heard[n..].is_empty(), "{:?}", &p.f.heard[n..]);
    }

    /// Controller ruling 1: the alternate frequencies are the same Approach unit, which
    /// answers on the frequency called.
    #[test]
    fn alternate_frequency_is_approach() {
        let mut e = engine();
        let mut p = Pilot::new(INSIDE, 5000.0, 180.0);
        p.f.com1 = APP_ALT;
        p.tick(&mut e);
        let out = p.call(&mut e, APP_ALT, INBOUND);
        assert_eq!(items(&out)[0], AtcItem::Identified, "{out:?}");
        assert_eq!(
            (out[0].freq_khz, out[0].station.as_str()),
            (APP_ALT, "Zeta Approach")
        );
        let n = p.f.heard.len();
        p.until(&mut e, 400, |p| p.f.heard.len() > n);
        assert!(p.f.heard[n..].iter().all(|o| o.freq_khz == APP_ALT));
        let again = e.say_again(APP_ALT, Lang::En);
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].freq_khz, APP_ALT);
    }

    /// A VFR flight calling Approach inbound is sent to Tower.
    #[test]
    fn vfr_inbound_contacts_tower() {
        let mut e = engine();
        let (_, out) = inbound(
            &mut e,
            INSIDE,
            2000.0,
            180.0,
            "Zeta Approach, Sierra Papa Tango Sierra Tango, VFR, inbound, at Romeo",
        );
        assert_eq!(
            items(&out),
            vec![AtcItem::Contact {
                station: "Zeta Tower".into(),
                khz: TWR
            }]
        );
        assert!(e.strip(&cs()).unwrap().plan.is_none());
    }

    /// Controller ruling 4: an inbound call on an adjacent unit's frequency before any
    /// hand-off is not answered (adjacent units are stubs).
    #[test]
    fn adjacent_unit_inbound_is_silent() {
        let mut e = engine();
        let mut p = Pilot::new(INSIDE, 5000.0, 180.0);
        p.tick(&mut e);
        let out = say(
            &mut e,
            DEST_TWR,
            "Testville Tower, Sierra Papa Tango Sierra Tango, IFR, inbound",
        );
        assert!(out.is_empty(), "{out:?}");
    }

    /// Spec 3.1/3.3: the area and information units hand an inbound IFR flight to Approach
    /// (in English, from the unit's own frequency); Approach then takes the first call.
    #[test]
    fn information_unit_hands_inbound_to_approach() {
        let mut e = engine();
        let mut p = Pilot::new(BEYOND, 5000.0, 120.0);
        p.tick(&mut e);
        let out = say(
            &mut e,
            119275,
            "Zeta Information, Sierra Papa Tango Sierra Tango, IFR, five thousand feet, \
             inbound Zeta",
        );
        assert_eq!(
            items(&out),
            vec![AtcItem::Contact {
                station: "Zeta Approach".into(),
                khz: APP,
            }],
            "{out:?}"
        );
        assert_eq!(
            (
                out[0].freq_khz,
                out[0].station.as_str(),
                out[0].message.lang
            ),
            (119275, "Zeta Information", Lang::En)
        );
        assert!(e.strip(&cs()).unwrap().plan.is_none());
        // The readback is not answered; Approach takes the first call.
        let rb = format!("Approach one one niner decimal one, {CS_ABBR}");
        assert!(say(&mut e, 119275, &rb).is_empty());
        let out = p.call(&mut e, APP, INBOUND);
        assert_eq!(items(&out)[0], AtcItem::Identified, "{out:?}");
    }

    /// The approach type changes only to one published for the runway.
    #[test]
    fn approach_type_on_request() {
        let mut e = engine();
        let (mut p, _) = inbound(&mut e, BEYOND, 5000.0, 120.0, INBOUND);
        let out = p.call(
            &mut e,
            APP,
            "Zeta Approach, Sierra Sierra Tango, request RNAV approach",
        );
        assert_eq!(
            items(&out),
            vec![AtcItem::Unable(UnableReason::ApproachNotAvailable)]
        );
        let out = p.call(
            &mut e,
            APP,
            "Zeta Approach, Sierra Sierra Tango, request ILS approach",
        );
        assert_eq!(
            items(&out),
            vec![AtcItem::ExpectApproach {
                kind: ApproachKind::Ils,
                runway: "27".into()
            }]
        );
    }

    /// No visual approach at night (summer 21:00-05:00 UTC).
    #[test]
    fn no_visual_approach_at_night() {
        let mut e = engine();
        let (p, _) = inbound(&mut e, BEYOND, 5000.0, 120.0, INBOUND);
        let mut night = p.f.sample(120.0, 100.0, 0.0, false);
        night.t = SimTime(p.f.t);
        night.utc_s = Some(22.0 * 3600.0);
        night.day_of_year = Some(180);
        e.on_telemetry(night);
        let out = say(
            &mut e,
            APP,
            "Zeta Approach, Sierra Sierra Tango, request visual approach",
        );
        assert_eq!(items(&out), vec![AtcItem::VisualApproachNotAvailable]);
    }

    /// [`engine`] with a VOR/DME approach to RWY 27 (a copy of the ILS 27 named D27).
    fn engine_with_vor() -> AtcEngine {
        engine_with(|set: &mut ProcedureSet| {
            let vor: Vec<_> = set
                .procedures
                .iter()
                .filter(|p| p.procedure.ident == "I27")
                .cloned()
                .map(|mut p| {
                    p.procedure.ident = "D27".into();
                    p.procedure.approach = Some(atc_navdata::cifp::ApproachKind::VorDme);
                    p
                })
                .collect();
            set.procedures.extend(vor);
        })
    }

    /// Spec §4: VOR approaches are not linked to STARs — none is given from beyond the start
    /// fix with a VOR request, and switching to VOR leaves the STAR.
    #[test]
    fn vor_approach_has_no_star() {
        let mut e = engine_with_vor();
        let text = "Zeta Approach, Sierra Papa Tango Sierra Tango, IFR, inbound, request VOR \
                    DME approach";
        let (_, out) = inbound(&mut e, BEYOND, 5000.0, 120.0, text);
        let first = items(&out);
        assert!(
            matches!(
                first.as_slice(),
                [
                    AtcItem::Identified,
                    AtcItem::Heading { .. },
                    AtcItem::MaintainLevel(_),
                    AtcItem::Qnh(_),
                    AtcItem::ExpectApproach {
                        kind: ApproachKind::VorDme,
                        ..
                    }
                ]
            ),
            "{first:?}"
        );
        assert_eq!(e.strip(&cs()).unwrap().plan.as_ref().unwrap().star, None);

        let mut e = engine_with_vor();
        let (mut p, out) = inbound(&mut e, BEYOND, 5000.0, 120.0, INBOUND);
        assert!(
            items(&out)
                .iter()
                .any(|i| matches!(i, AtcItem::Star { .. }))
        );
        let out = p.call(
            &mut e,
            APP,
            "Zeta Approach, Sierra Sierra Tango, request VOR DME approach",
        );
        assert_eq!(
            items(&out),
            vec![AtcItem::ExpectApproach {
                kind: ApproachKind::VorDme,
                runway: "27".into()
            }]
        );
        let plan = e.strip(&cs()).unwrap().plan.clone().unwrap();
        assert_eq!(plan.star, None);
        assert_eq!(plan.approach, Some((ApproachKind::VorDme, "27".into())));
        // Vectored from the next sample.
        let n = p.f.heard.len();
        p.tick(&mut e);
        assert!(
            p.f.heard[n..].iter().any(|o| o
                .message
                .items
                .iter()
                .any(|i| matches!(i, AtcItem::Heading { .. }))),
            "{:?}",
            &p.f.heard[n..]
        );
    }

    /// A first call asking for an approach the runway does not have: unable, and the default.
    #[test]
    fn unpublished_approach_on_first_call() {
        let mut e = engine();
        let text = "Zeta Approach, Sierra Papa Tango Sierra Tango, IFR, inbound, request RNAV \
                    approach";
        let (_, out) = inbound(&mut e, BEYOND, 5000.0, 120.0, text);
        let got = items(&out);
        assert_eq!(
            got[got.len() - 2..],
            [
                AtcItem::Unable(UnableReason::ApproachNotAvailable),
                AtcItem::ExpectApproach {
                    kind: ApproachKind::Ils,
                    runway: "27".into()
                }
            ],
            "{got:?}"
        );
        assert_eq!(got[0], AtcItem::Identified);
    }

    /// Magnetic heading difference.
    fn turn(a: u16, b: u16) -> u16 {
        let d = (i32::from(a) - i32::from(b)).rem_euclid(360) as u16;
        d.min(360 - d)
    }

    /// From the far side of the field (west of RWY 27, whose final is from the east): around
    /// a pattern, never a turn over 120 degrees, the intercept within 45 degrees of the
    /// final course, the approach clearance outside the FAF, then established and
    /// transferred to Tower with no "check established".
    #[test]
    fn far_side_arrival_is_vectored_around_a_pattern() {
        let mut e = engine();
        let (mut p, _) = inbound(&mut e, WEST, 5000.0, 90.0, INBOUND);
        p.until(&mut e, 1500, |p| {
            p.heard_item(|i| matches!(i, AtcItem::ClearedApproach { .. }))
        });
        let headings: Vec<u16> =
            p.f.heard
                .iter()
                .flat_map(|o| o.message.items.iter())
                .filter_map(|i| match i {
                    AtcItem::Heading { deg, .. } => Some(*deg),
                    _ => None,
                })
                .collect();
        let mut prev = 90;
        for &h in &headings {
            assert!(turn(prev, h) <= 120, "turn {prev} -> {h} in {headings:?}");
            prev = h;
        }
        // Downwind, base, intercept: no swing of the downwind heading near the base turn.
        assert!(headings.len() <= 3, "{headings:?}");
        let last = *headings.last().unwrap();
        assert!(turn(last, 270) <= 45, "intercept {last} in {headings:?}");
        let faf_nm = distance_m(
            LatLon {
                lat: 10.0,
                lon: 9.93,
            },
            THR_27,
        ) / M_PER_NM;
        let cleared_at = distance_m(p.f.pos, THR_27) / M_PER_NM;
        assert!(
            cleared_at > faf_nm + 0.5,
            "cleared {cleared_at} NM out, FAF {faf_nm}"
        );
        // On the intercept heading to the centreline, then down the final.
        // Within 0.1 NM of the centreline latitude.
        p.until(&mut e, 600, |p| (p.f.pos.lat - 10.0).abs() < 0.1 / 60.0);
        p.to = Some(THR_27);
        p.target = 1500.0;
        p.until(&mut e, 600, |p| {
            p.f.heard.iter().any(|o| contact_of(o).is_some())
        });
        assert!(!p.heard_item(|i| matches!(i, AtcItem::CheckEstablished { .. })));
        assert_eq!(e.strip(&cs()).unwrap().handoff_to, None, "readback done");
        assert_eq!(e.strip(&cs()).unwrap().owner, Role::Tower);
    }

    /// Spec 3.5: cleared for the ILS but never established: "check established" once, in
    /// English, from Approach (still owning it), between the FAF and the threshold.
    #[test]
    fn not_established_by_the_faf_is_called_once() {
        let mut e = engine();
        let (mut p, _) = inbound(&mut e, INSIDE, 5000.0, 180.0, INBOUND);
        p.until(&mut e, 400, |p| {
            p.heard_item(|i| matches!(i, AtcItem::ClearedApproach { .. }))
        });
        // Parallel to the final 1 NM north, down to 2 NM from the threshold.
        let abeam = |nm: f64| destination(destination(THR_27, 90.0, nm * M_PER_NM), 0.0, M_PER_NM);
        p.to = Some(abeam(7.0));
        p.until(&mut e, 300, |p| distance_m(p.f.pos, abeam(7.0)) < 300.0);
        p.to = Some(abeam(2.0));
        let n = p.f.heard.len();
        p.until(&mut e, 300, |p| distance_m(p.f.pos, abeam(2.0)) < 300.0);
        let calls: Vec<_> = p.f.heard[n..].iter().collect();
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(
            calls[0].message.items,
            vec![AtcItem::CheckEstablished {
                kind: ApproachKind::Ils,
                runway: "27".into()
            }]
        );
        assert_eq!((calls[0].freq_khz, calls[0].message.lang), (APP, Lang::En));
        assert!(
            calls[0].text.contains("check established"),
            "{}",
            calls[0].text
        );
    }

    /// Tower sends the arrival around because the runway is occupied: it goes back to
    /// Approach, which vectors it again.
    #[test]
    fn tower_go_around_goes_back_to_approach() {
        let mut e = engine();
        let (mut p, _) = inbound(&mut e, INSIDE, 5000.0, 180.0, INBOUND);
        p.until(&mut e, 400, |p| {
            p.heard_item(|i| matches!(i, AtcItem::ClearedApproach { .. }))
        });
        let centreline = destination(THR_27, 90.0, 7.0 * M_PER_NM);
        p.to = Some(centreline);
        p.until(&mut e, 200, |p| distance_m(p.f.pos, centreline) < 300.0);
        p.to = Some(THR_27);
        p.target = 1500.0;
        p.until(&mut e, 300, |p| {
            p.f.heard.iter().any(|o| contact_of(o).is_some())
        });
        // Another aircraft is cleared for take-off from the holding point.
        let other = Callsign::new("SPABC");
        e.register_pilot(PilotProfile {
            callsign: other.clone(),
            aircraft_type: "C172".into(),
        });
        let mut ground = p.f.sample(270.0, 0.0, 0.0, true);
        ground.callsign = other.clone();
        ground.pos = super::approach_departure::HOLD_27;
        ground.alt_ft_msl = 100.0;
        ground.com1_khz = TWR;
        let contact = e.on_telemetry(ground.clone());
        assert_eq!(contact.len(), 1, "{contact:?}");
        let rb = readback(&contact[0].message.items, "Sierra Papa Alpha Bravo Charlie");
        assert!(say(&mut e, super::approach_departure::GND, &rb).is_empty());
        ground.t = SimTime(ground.t.0 + 2.0);
        let takeoff = e.on_telemetry(ground);
        assert!(is_takeoff_clearance(&items(&takeoff)), "{takeoff:?}");
        // The arrival reports final: go around.
        let out = p.call(
            &mut e,
            TWR,
            "Zeta Tower, Sierra Sierra Tango, ILS runway two seven, final",
        );
        assert_eq!(items(&out), vec![AtcItem::GoAround]);
        assert!(e.strip(&cs()).unwrap().missed_approach);
        p.target = 3000.0;
        let n = p.f.heard.len();
        p.until(&mut e, 300, |p| p.f.heard.len() > n);
        assert_eq!(
            contact_of(&p.f.heard[n]),
            Some(("Zeta Approach".to_string(), APP))
        );
        assert_eq!(p.f.heard[n].freq_khz, TWR);
        let out = p.call(
            &mut e,
            APP,
            "Zeta Approach, Sierra Sierra Tango, going around",
        );
        assert!(
            matches!(
                items(&out).as_slice(),
                [
                    AtcItem::Heading { .. },
                    AtcItem::ClimbTo(_) | AtcItem::MaintainLevel(_)
                ]
            ),
            "{out:?}"
        );
    }

    /// The pilot goes around on his own and says so: Tower sends the IFR arrival back to
    /// Approach at once, which vectors it again.
    #[test]
    fn pilot_going_around_goes_back_to_approach() {
        let mut e = engine();
        let (mut p, _) = inbound(&mut e, INSIDE, 5000.0, 180.0, INBOUND);
        p.until(&mut e, 400, |p| {
            p.heard_item(|i| matches!(i, AtcItem::ClearedApproach { .. }))
        });
        let centreline = destination(THR_27, 90.0, 7.0 * M_PER_NM);
        p.to = Some(centreline);
        p.until(&mut e, 200, |p| distance_m(p.f.pos, centreline) < 300.0);
        p.to = Some(THR_27);
        p.target = 1500.0;
        p.until(&mut e, 300, |p| {
            p.f.heard.iter().any(|o| contact_of(o).is_some())
        });
        let out = p.call(
            &mut e,
            TWR,
            "Zeta Tower, Sierra Papa Tango Sierra Tango, ILS runway two seven, established",
        );
        assert!(
            items(&out)
                .iter()
                .any(|i| matches!(i, AtcItem::ClearedLand { .. })),
            "{out:?}"
        );
        let out = say(&mut e, TWR, "Sierra Sierra Tango, going around");
        assert_eq!(
            out.iter().filter_map(contact_of).collect::<Vec<_>>(),
            vec![("Zeta Approach".to_string(), APP)],
            "{out:?}"
        );
        assert_eq!(out[0].freq_khz, TWR);
        assert!(e.strip(&cs()).unwrap().missed_approach);
        assert!(!e.strip(&cs()).unwrap().permissions.may_land);
    }
}

/// VFR entry to and exit from the zone via the published points, on the fictional ZZZZ
/// (ROMEO 12 NM north; KILO and SIERRA 12 NM south, the southern points; holds over KILO
/// and ROMEO up to 2000 ft; RWY 27, left circuit).
mod vfr_points {
    use atc_core::strip::VfrState;
    use atc_geo::{M_PER_NM, destination};
    use atc_phraseology::UnableReason;

    use super::approach_departure::{DEL, DT, Flight, THR_27, TWR, departed, engine, never};
    use super::*;

    const ROMEO: LatLon = LatLon {
        lat: 10.2,
        lon: 10.0,
    };
    const KILO: LatLon = LatLon {
        lat: 9.8,
        lon: 10.0,
    };
    const REQUEST: &str =
        "Zeta Tower, Sierra Papa Tango Sierra Tango, at Romeo, two thousand feet, request joining";
    const FIS: u32 = 119275;

    fn cs() -> Callsign {
        Callsign::new("SPTST")
    }

    /// An airborne VFR aircraft (no flight plan) at `pos`, 2000 ft, tuned to Tower.
    fn airborne(e: &mut AtcEngine, pos: LatLon) -> Flight {
        let mut f = Flight {
            t: 0.0,
            pos,
            alt: 2000.0,
            com1: TWR,
            rolling: false,
            xpdr: None,
            heard: Vec::new(),
        };
        f.step(e, 180.0, 90.0, 0.0, false);
        f
    }

    fn enter(point: &str) -> Vec<AtcItem> {
        vec![
            AtcItem::EnterControlZoneVia {
                point: point.into(),
                alt_ft: 2000,
            },
            AtcItem::ReportAt {
                point: point.into(),
            },
        ]
    }

    /// A VFR flight cleared to another airport turns back and joins via a point: from the
    /// join on it flies the circuit (legs reported and answered) and lands here.
    #[test]
    fn a_returning_vfr_flight_joins_the_circuit() {
        let mut e = engine();
        let out = say(
            &mut e,
            DEL,
            "Zeta Delivery, Sierra Papa Tango Sierra Tango, VFR to Testville, request clearance",
        );
        let code = squawk_of(&items(&out)).expect("squawk");
        let rb = format!(
            "cleared to Testville, leave control zone via Romeo, squawk {}, {CS_ABBR}",
            spoken(&format!("{code:04}"))
        );
        let out = say(&mut e, DEL, &rb);
        assert_eq!(items(&out)[0], AtcItem::ReadbackCorrect);
        assert!(say(&mut e, DEL, &readback(&items(&out), CS_ABBR)).is_empty());
        let mut f = airborne(&mut e, ROMEO);
        assert_eq!(items(&say(&mut e, TWR, REQUEST)), enter("ROMEO"));
        let out = say(&mut e, TWR, "Zeta Tower, Sierra Sierra Tango, at Romeo");
        assert!(
            matches!(items(&out).first(), Some(AtcItem::JoinDownwind { .. })),
            "{out:?}"
        );
        // Downwind of RWY 27 (left circuit: south of the runway), heading east.
        f.pos = destination(THR_27, 180.0, 0.8 * M_PER_NM);
        for _ in 0..4 {
            f.step(&mut e, 90.0, 90.0, 0.0, false);
        }
        assert_eq!(
            e.strip(&cs()).unwrap().phase,
            FlightPhase::Circuit(CircuitLeg::Downwind)
        );
        let out = say(
            &mut e,
            TWR,
            "Zeta Tower, Sierra Sierra Tango, downwind, full stop",
        );
        assert_eq!(
            items(&out),
            vec![AtcItem::Sequence(1), AtcItem::Report(CircuitLeg::Final)]
        );
    }

    /// Another aircraft on the final of RWY 27, 3 NM out, registered as SPABC.
    fn traffic_on_final(e: &mut AtcEngine) -> AircraftState {
        e.register_pilot(PilotProfile {
            callsign: Callsign::new("SPABC"),
            aircraft_type: "C172".into(),
        });
        let f = Flight {
            t: 0.0,
            pos: THR_27,
            alt: 900.0,
            com1: TWR,
            rolling: false,
            xpdr: None,
            heard: Vec::new(),
        };
        let mut s = f.sample(270.0, 80.0, -500.0, false);
        s.callsign = Callsign::new("SPABC");
        s.pos = destination(THR_27, 90.0, 3.0 * M_PER_NM);
        s.alt_ft_msl = 900.0;
        e.on_telemetry(s.clone());
        s
    }

    #[test]
    fn entry_via_a_point_then_the_circuit() {
        let mut e = engine();
        let mut f = airborne(&mut e, destination(ROMEO, 0.0, 3.0 * M_PER_NM));
        let out = say(&mut e, TWR, REQUEST);
        assert_eq!(items(&out), enter("ROMEO"), "{out:?}");
        assert!(
            out[0].text.contains("enter control zone via"),
            "{}",
            out[0].text
        );
        // The readback is not answered.
        let rb = say(
            &mut e,
            TWR,
            "enter control zone via Romeo, altitude two thousand feet, report at Romeo, Sierra Sierra Tango",
        );
        assert!(rb.is_empty(), "{rb:?}");
        // Off the circuit until it joins.
        f.fly_to(&mut e, ROMEO, (90.0, 0.0, 2000.0), &never);
        assert_eq!(e.strip(&cs()).unwrap().phase, FlightPhase::Airborne);
        assert!(f.heard.is_empty(), "{:?}", f.heard);
        let out = say(&mut e, TWR, "Zeta Tower, Sierra Sierra Tango, at Romeo");
        assert_eq!(
            items(&out),
            vec![
                AtcItem::JoinDownwind {
                    runway: "27".into()
                },
                AtcItem::Report(CircuitLeg::Downwind)
            ]
        );
        assert!(
            out[0].text.contains("join downwind runway two seven"),
            "{}",
            out[0].text
        );
        assert_eq!(e.strip(&cs()).unwrap().vfr, None);
        // The readback of the join instruction is not a downwind report.
        let rb = say(
            &mut e,
            TWR,
            "Join downwind runway two seven, wilco, Sierra Sierra Tango",
        );
        assert!(rb.is_empty(), "{rb:?}");
        // The circuit logic takes over: downwind and final reports are answered as usual.
        f.step(&mut e, 90.0, 90.0, 0.0, false);
        assert!(matches!(
            e.strip(&cs()).unwrap().phase,
            FlightPhase::Circuit(_)
        ));
        let out = say(&mut e, TWR, "Zeta Tower, Sierra Sierra Tango, downwind");
        assert_eq!(
            items(&out),
            vec![AtcItem::Sequence(1), AtcItem::Report(CircuitLeg::Final)]
        );
    }

    #[test]
    fn southern_entry_via_a_northern_point_is_refused() {
        let mut e = engine();
        // 6 NM south-south-west of the field, nearer KILO than SIERRA.
        let _f = airborne(&mut e, destination(KILO, 90.0, -2.0 * M_PER_NM));
        let out = say(&mut e, TWR, REQUEST);
        assert_eq!(
            items(&out),
            vec![
                AtcItem::Unable(UnableReason::SouthernEntry),
                AtcItem::EnterControlZoneVia {
                    point: "KILO".into(),
                    alt_ft: 2000
                }
            ],
            "{out:?}"
        );
        // Via the southern point itself it is accepted.
        let out = say(
            &mut e,
            TWR,
            "Zeta Tower, Sierra Sierra Tango, at Kilo, two thousand feet, request joining",
        );
        assert_eq!(items(&out), enter("KILO"));
    }

    #[test]
    fn busy_runway_holds_over_the_nearest_hold_point() {
        let mut e = engine();
        let mut f = airborne(&mut e, destination(ROMEO, 0.0, 3.0 * M_PER_NM));
        let mut other = traffic_on_final(&mut e);
        assert_eq!(
            e.strip(&Callsign::new("SPABC")).unwrap().phase,
            FlightPhase::Circuit(CircuitLeg::Final)
        );
        let out = say(&mut e, TWR, REQUEST);
        assert_eq!(
            items(&out),
            vec![AtcItem::HoldOver {
                point: "ROMEO".into(),
                max_ft: 2000
            }],
            "{out:?}"
        );
        assert!(out[0].text.contains("hold over"), "{}", out[0].text);
        // Still busy when it reports over the point: told to hold again.
        f.fly_to(&mut e, ROMEO, (90.0, 0.0, 2000.0), &never);
        let out = say(&mut e, TWR, "Zeta Tower, Sierra Sierra Tango, over Romeo");
        assert_eq!(items(&out).len(), 1);
        assert!(
            matches!(items(&out)[0], AtcItem::HoldOver { .. }),
            "{out:?}"
        );
        // The traffic leaves the final (downwind of the circuit): the entry is given.
        for _ in 0..5 {
            other.t = SimTime(other.t.0 + DT);
            other.pos = LatLon {
                lat: 9.98,
                lon: 10.0,
            };
            other.heading_deg = 90.0;
            e.on_telemetry(other.clone());
        }
        assert_ne!(
            e.strip(&Callsign::new("SPABC")).unwrap().phase,
            FlightPhase::Circuit(CircuitLeg::Final)
        );
        let out = say(&mut e, TWR, "Zeta Tower, Sierra Sierra Tango, over Romeo");
        assert_eq!(items(&out), enter("ROMEO"), "{out:?}");
    }

    #[test]
    fn calling_again_when_free_gives_the_entry() {
        let mut e = engine();
        let _f = airborne(&mut e, destination(ROMEO, 0.0, 3.0 * M_PER_NM));
        let mut other = traffic_on_final(&mut e);
        let out = say(&mut e, TWR, REQUEST);
        assert!(matches!(items(&out)[0], AtcItem::HoldOver { .. }));
        for _ in 0..5 {
            other.t = SimTime(other.t.0 + DT);
            other.pos = LatLon {
                lat: 9.98,
                lon: 10.0,
            };
            other.heading_deg = 90.0;
            e.on_telemetry(other.clone());
        }
        let out = say(&mut e, TWR, REQUEST);
        assert_eq!(items(&out), enter("ROMEO"));
    }

    #[test]
    fn vfr_departure_reports_at_the_point_and_leaves_to_information() {
        let mut e = engine();
        let mut f = departed(&mut e, "Testville", false);
        let route = LatLon {
            lat: 10.1,
            lon: 10.0,
        };
        f.fly_to(&mut e, route, (90.0, 500.0, 2500.0), &never);
        assert_eq!(f.heard.len(), 1, "{:?}", f.heard);
        assert_eq!(
            f.heard[0].message.items,
            vec![AtcItem::ReportAt {
                point: "ROMEO".into()
            }]
        );
        f.fly_to(&mut e, ROMEO, (90.0, 0.0, 2500.0), &never);
        let out = say(&mut e, TWR, "Zeta Tower, Sierra Sierra Tango, at Romeo");
        assert_eq!(
            items(&out),
            vec![
                AtcItem::LeaveControlZoneVia {
                    point: "ROMEO".into()
                },
                AtcItem::FrequencyChangeApproved,
                AtcItem::Contact {
                    station: "Zeta Information".into(),
                    khz: FIS
                }
            ],
            "{out:?}"
        );
        assert_eq!(
            e.strip(&cs()).unwrap().handed_off.as_deref(),
            Some("Zeta Information")
        );
        // Read back, then on the information frequency: identified.
        let rb = say(
            &mut e,
            TWR,
            &format!(
                "leave control zone via Romeo, contact {}, Sierra Sierra Tango",
                freq_spoken(FIS)
            ),
        );
        assert!(rb.is_empty(), "{rb:?}");
        let out = say(
            &mut e,
            FIS,
            "Zeta Information, Sierra Papa Tango Sierra Tango, leaving the zone",
        );
        assert_eq!(items(&out), vec![AtcItem::Identified]);
    }

    /// Spec 3.6: a VFR flight that left the zone via a point (handed to information) and comes
    /// back is a new flight: it joins via a point, and its landing without clearance is
    /// called by Tower.
    #[test]
    fn re_entry_after_leaving_the_zone_is_watched_again() {
        let mut e = engine();
        let mut f = departed(&mut e, "Testville", false);
        f.fly_to(
            &mut e,
            LatLon {
                lat: 10.1,
                lon: 10.0,
            },
            (90.0, 500.0, 2000.0),
            &never,
        );
        f.fly_to(&mut e, ROMEO, (90.0, 0.0, 2000.0), &never);
        let out = say(&mut e, TWR, "Zeta Tower, Sierra Sierra Tango, at Romeo");
        assert!(
            items(&out).contains(&AtcItem::FrequencyChangeApproved),
            "{out:?}"
        );
        let rb = format!(
            "leave control zone via Romeo, contact {}, Sierra Sierra Tango",
            freq_spoken(FIS)
        );
        assert!(say(&mut e, TWR, &rb).is_empty());
        let out = say(
            &mut e,
            FIS,
            "Zeta Information, Sierra Papa Tango Sierra Tango, leaving the zone",
        );
        assert_eq!(items(&out), vec![AtcItem::Identified]);
        // Out of the zone and back to Romeo: asks Tower to join.
        f.fly_to(
            &mut e,
            destination(ROMEO, 0.0, 3.0 * M_PER_NM),
            (90.0, 0.0, 2000.0),
            &never,
        );
        f.fly_to(&mut e, ROMEO, (90.0, 0.0, 2000.0), &never);
        f.com1 = TWR;
        f.heard.clear();
        let out = say(&mut e, TWR, REQUEST);
        assert_eq!(items(&out), enter("ROMEO"), "{out:?}");
        assert_eq!(e.strip(&cs()).unwrap().handed_off, None);
        let out = say(&mut e, TWR, "Zeta Tower, Sierra Sierra Tango, at Romeo");
        assert!(
            matches!(items(&out).first(), Some(AtcItem::JoinDownwind { .. })),
            "{out:?}"
        );
        // On final and down on the runway without a landing clearance.
        f.pos = destination(THR_27, 90.0, 2.0 * M_PER_NM);
        f.alt = 800.0;
        let mid = LatLon {
            lat: 10.0,
            lon: 10.0,
        };
        f.fly_to(&mut e, THR_27, (70.0, 0.0, 800.0), &never);
        f.rolling = true;
        f.fly_to(&mut e, mid, (50.0, 0.0, 800.0), &never);
        let calls = f.said(|i| *i == AtcItem::LandedWithoutClearance);
        assert_eq!(calls.len(), 1, "{:?}", f.heard);
        assert_eq!(calls[0].freq_khz, TWR);
    }

    #[test]
    fn a_circuit_aircraft_asks_to_leave_via_a_point() {
        let mut e = engine();
        let mut f = airborne(&mut e, destination(KILO, 0.0, 3.0 * M_PER_NM));
        let out = say(
            &mut e,
            TWR,
            "Zeta Tower, Sierra Sierra Tango, request leaving via Kilo",
        );
        assert_eq!(
            items(&out),
            vec![
                AtcItem::LeaveControlZoneVia {
                    point: "KILO".into()
                },
                AtcItem::ReportAt {
                    point: "KILO".into()
                }
            ]
        );
        f.fly_to(&mut e, KILO, (90.0, 0.0, 2000.0), &never);
        assert_eq!(e.strip(&cs()).unwrap().phase, FlightPhase::Airborne);
        let out = say(&mut e, TWR, "Zeta Tower, Sierra Sierra Tango, over Kilo");
        assert!(
            items(&out).contains(&AtcItem::FrequencyChangeApproved),
            "{out:?}"
        );
    }

    #[test]
    fn a_report_without_an_instruction_open_is_ignored() {
        let mut e = engine();
        let _f = airborne(&mut e, destination(ROMEO, 0.0, 3.0 * M_PER_NM));
        // No entry or exit was given: a bare position report starts nothing.
        let out = say(
            &mut e,
            TWR,
            "Zeta Tower, Sierra Sierra Tango, holding at Romeo",
        );
        assert!(items(&out).iter().all(|i| !matches!(
            i,
            AtcItem::JoinDownwind { .. }
                | AtcItem::EnterControlZoneVia { .. }
                | AtcItem::FrequencyChangeApproved
        )));
    }

    /// A plain readback of the entry (no instruction words, only the point and the level)
    /// is not a report: it does not join the circuit; the report at the point does.
    #[test]
    fn readback_is_not_a_report() {
        let mut e = engine();
        let mut f = airborne(&mut e, destination(ROMEO, 0.0, 3.0 * M_PER_NM));
        say(&mut e, TWR, REQUEST);
        let out = say(&mut e, TWR, "Romeo, two thousand feet, Sierra Sierra Tango");
        assert!(out.is_empty(), "{out:?}");
        assert!(matches!(
            e.strip(&cs()).unwrap().vfr,
            Some(VfrState::Entering { .. })
        ));
        f.fly_to(&mut e, ROMEO, (90.0, 0.0, 2000.0), &never);
        let out = say(&mut e, TWR, "at Romeo, Sierra Sierra Tango");
        assert!(
            matches!(items(&out)[0], AtcItem::JoinDownwind { .. }),
            "{out:?}"
        );
    }

    /// Reading back "report at Romeo" for the exit does not hand the flight off.
    #[test]
    fn readback_of_the_exit_does_not_hand_off() {
        let mut e = engine();
        let mut f = departed(&mut e, "Testville", false);
        f.fly_to(
            &mut e,
            LatLon {
                lat: 10.1,
                lon: 10.0,
            },
            (90.0, 500.0, 2500.0),
            &never,
        );
        let out = say(&mut e, TWR, "report at Romeo, Sierra Sierra Tango");
        assert!(out.is_empty(), "{out:?}");
        assert_eq!(e.strip(&cs()).unwrap().handed_off, None);
        f.fly_to(&mut e, ROMEO, (90.0, 0.0, 2500.0), &never);
        let out = say(&mut e, TWR, "at Romeo, Sierra Sierra Tango");
        assert!(
            items(&out).contains(&AtcItem::FrequencyChangeApproved),
            "{out:?}"
        );
    }

    /// A join request from an aircraft already in the circuit leaves it there.
    #[test]
    fn join_from_the_circuit_is_ignored() {
        let mut e = engine();
        let mut f = airborne(&mut e, destination(THR_27, 0.0, 1.5 * M_PER_NM));
        f.step(&mut e, 90.0, 90.0, 0.0, false);
        assert!(matches!(
            e.strip(&cs()).unwrap().phase,
            FlightPhase::Circuit(_)
        ));
        let out = say(&mut e, TWR, REQUEST);
        assert_eq!(items(&out), vec![AtcItem::Roger], "{out:?}");
        assert_eq!(e.strip(&cs()).unwrap().vfr, None);
        f.step(&mut e, 90.0, 90.0, 0.0, false);
        assert!(matches!(
            e.strip(&cs()).unwrap().phase,
            FlightPhase::Circuit(_)
        ));
    }

    /// The exit point belongs to the clearance last issued: a later clearance replaces it.
    #[test]
    fn exit_point_follows_the_latest_clearance() {
        let mut e = engine();
        let clear = |e: &mut AtcEngine, rules: &str, cleared: &str| {
            let out = say(
                e,
                DEL,
                &format!("Zeta Delivery, {CS}, {rules} to Testville, request clearance"),
            );
            let code = squawk_of(&items(&out)).expect("squawk");
            let out = say(
                e,
                DEL,
                &format!(
                    "{cleared}, squawk {}, {CS_ABBR}",
                    spoken(&format!("{code:04}"))
                ),
            );
            assert!(say(e, DEL, &readback(&items(&out), CS_ABBR)).is_empty());
        };
        clear(
            &mut e,
            "VFR",
            "cleared to Testville, leave control zone via Romeo",
        );
        assert_eq!(e.strip(&cs()).unwrap().vfr_exit.as_deref(), Some("ROMEO"));
        clear(
            &mut e,
            "IFR",
            "cleared to Testville, Alfax one Alpha departure, climb five thousand feet",
        );
        assert_eq!(e.strip(&cs()).unwrap().vfr_exit, None);
    }
}

/// Plan 9: a flight handed out of one engine and into another (the multi-station server).
mod handover {
    use atc_core::handover::FlightHandover;
    use atc_core::strip::FlightPlan;
    use atc_geo::{M_PER_NM, destination, distance_m};
    use atc_navdata::procedures::AdjacentKind;
    use atc_phraseology::{ApproachKind, FlightRules};

    use super::approach_departure::{
        AIRBORNE, ALFAX, APP, CLIMB, FARLAND, Flight, TWR, ZZ002, contact_of, departed, engine,
        engine_full, engine_with_procs, never,
    };
    use super::*;

    fn cs() -> Callsign {
        Callsign::new("SPTST")
    }

    /// ZZZZ's reference point.
    const HOME: LatLon = LatLon {
        lat: 10.0,
        lon: 10.0,
    };

    fn ifr_plan(dest: &str, name: &str, level_ft: i32) -> FlightPlan {
        FlightPlan {
            ifr: true,
            destination: dest.into(),
            dest_name: name.into(),
            cruise_ft: level_ft,
            sid: None,
            dep_runway: None,
            star: None,
            approach: None,
            vectors: false,
            cleared_ft: level_ft,
            direct: None,
            heading: None,
            approach_cleared: false,
            pattern: None,
            established_check: false,
        }
    }

    fn handover(plan: FlightPlan) -> FlightHandover {
        FlightHandover {
            callsign: cs(),
            aircraft_type: "C172".into(),
            cleared_ft: Some(plan.cleared_ft),
            plan: Some(plan),
            squawk: Some(4601),
            rules: Some(FlightRules::Ifr),
        }
    }

    /// SPTST airborne at `alt` ft at `pos` on the Approach frequency, one sample taken.
    fn airborne(e: &mut AtcEngine, pos: LatLon, alt: f64, hdg: f64) -> Flight {
        let mut f = Flight {
            t: 0.0,
            pos,
            alt,
            com1: APP,
            rolling: false,
            xpdr: Some(4601),
            heard: Vec::new(),
        };
        f.step(e, hdg, 250.0, 0.0, false);
        assert!(f.heard.is_empty(), "{:?}", f.heard);
        f
    }

    #[test]
    fn export_then_import_keeps_plan_and_squawk() {
        let mut e = engine();
        departed(&mut e, "Testville", true);
        let strip = e.strip(&cs()).unwrap().clone();
        let h = e.export_flight(&cs()).expect("exported");
        assert_eq!(h.callsign, cs());
        assert_eq!(h.aircraft_type, "C172");
        assert_eq!(h.plan, strip.plan);
        assert_eq!(h.plan.as_ref().unwrap().destination, "ZZZY");
        assert_eq!(h.squawk, strip.squawk);
        assert!(h.squawk.is_some());
        assert_eq!(h.cleared_ft, Some(5000));
        assert_eq!(h.rules, Some(FlightRules::Ifr));
        // Through JSON, as between stations.
        let h: FlightHandover = serde_json::from_str(&serde_json::to_string(&h).unwrap()).unwrap();
        let mut other = engine();
        other.import_flight(h.clone());
        let got = other.strip(&cs()).unwrap();
        // The departure airport's SID, runway and direct do not travel with it.
        let mut expected = h.plan.clone().unwrap();
        (expected.sid, expected.dep_runway, expected.direct) = (None, None, None);
        assert_eq!(got.plan.as_ref(), Some(&expected));
        assert_eq!(got.owner, Role::Approach);
        assert_eq!(got.squawk, h.squawk);
        assert!(got.handed_in);
    }

    #[test]
    fn export_removes_flight_and_snapshot_entry() {
        let mut e = engine();
        departed(&mut e, "Testville", true);
        assert!(e.export_flight(&cs()).is_some());
        assert!(e.strip(&cs()).is_none());
        let json = serde_json::to_string(&e.snapshot()).unwrap();
        assert!(!json.contains("SPTST"), "{json}");
        assert!(e.export_flight(&cs()).is_none());
        assert_eq!(e.handoff_target(&cs()), None);
        assert!(!e.remove_pilot(&cs()));
        // Registered again: a fresh strip.
        e.register_pilot(PilotProfile {
            callsign: cs(),
            aircraft_type: "C172".into(),
        });
        let s = e.strip(&cs()).unwrap();
        assert_eq!(s.phase, FlightPhase::Parked);
        assert!(s.plan.is_none());
        assert!(s.issued.is_empty());
        assert!(!s.handed_in);
    }

    #[test]
    fn remove_pilot_then_register_starts_fresh() {
        let mut e = engine();
        departed(&mut e, "Testville", true);
        assert!(!e.strip(&cs()).unwrap().issued.is_empty());
        assert!(e.remove_pilot(&cs()));
        e.register_pilot(PilotProfile {
            callsign: cs(),
            aircraft_type: "B738".into(),
        });
        let s = e.strip(&cs()).unwrap();
        assert_eq!(s.phase, FlightPhase::Parked);
        assert!(s.plan.is_none());
        assert!(s.issued.is_empty());
        assert!(s.squawk.is_none());
        assert_eq!(e.export_flight(&cs()).unwrap().aircraft_type, "B738");
    }

    #[test]
    fn remove_pilot_drops_snapshot_entry() {
        let mut e = engine();
        departed(&mut e, "Testville", true);
        assert!(
            serde_json::to_string(&e.snapshot())
                .unwrap()
                .contains("SPTST")
        );
        assert!(e.remove_pilot(&cs()));
        let json = serde_json::to_string(&e.snapshot()).unwrap();
        assert!(!json.contains("SPTST"), "{json}");
    }

    /// An IFR flight bound for ZZZZ handed in from the station it departed: its first call
    /// on Approach is answered as a flight handed over, with the arrival continuation.
    #[test]
    fn imported_flight_first_call_is_identified() {
        let mut e = engine();
        let mut plan = ifr_plan("ZZZZ", "Zeta", 5000);
        plan.sid = Some("OTHER1A".into());
        e.import_flight(handover(plan));
        let west = LatLon {
            lat: 10.0,
            lon: 9.8,
        };
        airborne(&mut e, west, 5000.0, 90.0);
        let out = say(
            &mut e,
            APP,
            "Zeta Approach, Sierra Papa Tango Sierra Tango, five thousand feet",
        );
        let got = items(&out);
        assert_eq!(got.first(), Some(&AtcItem::Identified), "{out:?}");
        assert!(
            got.iter()
                .any(|i| matches!(i, AtcItem::ExpectApproach { .. })),
            "{got:?}"
        );
        assert!(!got.contains(&AtcItem::SayIntentions), "{got:?}");
        assert!(!got.contains(&AtcItem::NoFlightPlan), "{got:?}");
        assert!(
            out[0]
                .text
                .starts_with("Sierra Papa Tango Sierra Tango, identified"),
            "{}",
            out[0].text
        );
        let s = e.strip(&cs()).unwrap();
        assert!(!s.handed_in);
        assert!(s.identified);
        assert!(s.plan.as_ref().unwrap().approach.is_some());
    }

    /// A flight handed in at 10000 ft, passing through to Farland, is identified and handed
    /// off to the area control unit at the TMA boundary; read back, the hand-off target is
    /// the unit's frequency.
    /// A transit flight to Farland at 10000 ft handed in to `e`, identified, flown out to
    /// the TMA boundary until Approach hands it to Zeta Radar: that transmission.
    fn transit_to_the_boundary(e: &mut AtcEngine) -> AtcOutput {
        e.import_flight(handover(ifr_plan("ZZZF", "Farland", 10000)));
        let start = destination(HOME, 0.0, 30.0 * M_PER_NM);
        let mut f = airborne(e, start, 10000.0, 0.0);
        let out = say(
            e,
            APP,
            "Zeta Approach, Sierra Papa Tango Sierra Tango, flight level one zero zero",
        );
        assert_eq!(items(&out), vec![AtcItem::Identified], "{out:?}");
        let handed = |h: &[AtcOutput]| h.iter().any(|o| contact_of(o).is_some());
        f.fly_to(e, FARLAND, (250.0, 0.0, 10000.0), &handed);
        let contact = f.heard.iter().find(|o| contact_of(o).is_some()).unwrap();
        assert_eq!(
            contact_of(contact),
            Some(("Zeta Radar".to_string(), 127660))
        );
        contact.clone()
    }

    /// Training strictness: the contact needs no readback, yet the flight is the old
    /// engine's until the pilot answers it.
    #[test]
    fn handoff_target_waits_for_the_acknowledgement_without_readback() {
        let mut e = engine_full(Strictness::Training, |_| {}, |_| {});
        let contact = transit_to_the_boundary(&mut e);
        assert!(e.strip(&cs()).unwrap().pending_readback.is_none());
        assert_eq!(e.handoff_target(&cs()), None);
        let rb = readback(&contact.message.items, CS_ABBR);
        assert!(say(&mut e, APP, &rb).is_empty());
        assert_eq!(e.handoff_target(&cs()), Some(127660));
    }

    /// First calls that would begin a new flight of an aircraft that went away ("inbound",
    /// an approach request) keep a handed-in flight's plan.
    #[test]
    fn handed_in_inbound_call_keeps_the_plan() {
        let mut e = engine();
        e.import_flight(handover(ifr_plan("ZZZF", "Farland", 10000)));
        airborne(
            &mut e,
            destination(HOME, 0.0, 30.0 * M_PER_NM),
            10000.0,
            0.0,
        );
        let out = say(
            &mut e,
            APP,
            "Zeta Approach, Sierra Papa Tango Sierra Tango, IFR, flight level one zero zero, \
             inbound",
        );
        assert_eq!(items(&out), vec![AtcItem::Identified], "{out:?}");
        let s = e.strip(&cs()).unwrap();
        assert_eq!(s.plan.as_ref().unwrap().destination, "ZZZF");
        assert_eq!(s.plan.as_ref().unwrap().cleared_ft, 10000);
    }

    #[test]
    fn handed_in_approach_request_is_identified_with_it() {
        let mut e = engine();
        e.import_flight(handover(ifr_plan("ZZZZ", "Zeta", 5000)));
        let west = LatLon {
            lat: 10.0,
            lon: 9.8,
        };
        airborne(&mut e, west, 5000.0, 90.0);
        let out = say(
            &mut e,
            APP,
            "Zeta Approach, Sierra Papa Tango Sierra Tango, five thousand feet, request ILS \
             approach",
        );
        let got = items(&out);
        assert_eq!(got.first(), Some(&AtcItem::Identified), "{out:?}");
        assert!(
            got.contains(&AtcItem::ExpectApproach {
                kind: ApproachKind::Ils,
                runway: "27".into()
            }),
            "{got:?}"
        );
        let s = e.strip(&cs()).unwrap();
        assert!(s.identified);
        let plan = s.plan.as_ref().unwrap();
        assert_eq!(plan.destination, "ZZZZ");
        assert_eq!(plan.approach, Some((ApproachKind::Ils, "27".into())));
    }

    #[test]
    fn handoff_target_after_readback() {
        let mut e = engine();
        e.import_flight(handover(ifr_plan("ZZZF", "Farland", 10000)));
        let start = destination(HOME, 0.0, 30.0 * M_PER_NM);
        let mut f = airborne(&mut e, start, 10000.0, 0.0);
        let out = say(
            &mut e,
            APP,
            "Zeta Approach, Sierra Papa Tango Sierra Tango, flight level one zero zero",
        );
        assert_eq!(items(&out), vec![AtcItem::Identified], "{out:?}");
        assert_eq!(e.handoff_target(&cs()), None);
        let handed = |h: &[AtcOutput]| h.iter().any(|o| contact_of(o).is_some());
        f.fly_to(&mut e, FARLAND, (250.0, 0.0, 10000.0), &handed);
        let contact = f.heard.iter().find(|o| contact_of(o).is_some()).unwrap();
        assert_eq!(
            contact_of(contact),
            Some(("Zeta Radar".to_string(), 127660))
        );
        // Not read back yet.
        assert_eq!(e.handoff_target(&cs()), None);
        let rb = readback(&contact.message.items, CS_ABBR);
        assert!(say(&mut e, APP, &rb).is_empty());
        assert_eq!(e.handoff_target(&cs()), Some(127660));
        assert!(e.export_flight(&cs()).is_some());
        assert_eq!(e.handoff_target(&cs()), None);
    }

    /// An internal handover (Tower -> Approach) is no hand-off out of the engine.
    #[test]
    fn internal_handover_is_no_target() {
        let mut e = engine();
        let f = departed(&mut e, "Testville", true);
        let tower = &f.heard[0];
        assert!(say(&mut e, TWR, &readback(&tower.message.items, CS_ABBR)).is_empty());
        assert_eq!(e.strip(&cs()).unwrap().owner, Role::Approach);
        assert_eq!(e.handoff_target(&cs()), None);
    }

    /// Neither an area control unit nor an information unit: at the TMA boundary the
    /// departure is released.
    #[test]
    fn no_adjacent_unit_releases_at_boundary() {
        let mut e = engine_with_procs(
            |p| p.adjacent.retain(|u| u.kind == AdjacentKind::Tower),
            |_| {},
        );
        let mut f = departed(&mut e, "Farland", true);
        let tower = f.heard.drain(..).next().unwrap();
        assert!(say(&mut e, TWR, &readback(&tower.message.items, CS_ABBR)).is_empty());
        assert_eq!(
            items(&say(&mut e, APP, AIRBORNE)),
            vec![AtcItem::Identified]
        );
        let any = |h: &[AtcOutput]| !h.is_empty();
        f.fly_to(&mut e, ZZ002, CLIMB, &any);
        assert!(say(&mut e, APP, &format!("direct Alfax, {CS_ABBR}")).is_empty());
        f.heard.clear();
        f.fly_to(&mut e, ALFAX, CLIMB, &any);
        f.fly_to(&mut e, FARLAND, CLIMB, &any);
        let out = distance_m(f.pos, HOME) / M_PER_NM;
        assert!((39.5..=41.0).contains(&out), "{out} NM");
        assert_eq!(
            items(&f.heard),
            vec![
                AtcItem::RadarServiceTerminated,
                AtcItem::FrequencyChangeApproved
            ]
        );
        assert!(
            f.heard[0]
                .text
                .ends_with("radar service terminated, frequency change approved"),
            "{}",
            f.heard[0].text
        );
        assert!(e.strip(&cs()).unwrap().handed_off.is_some());
        assert_eq!(e.handoff_target(&cs()), None);
        // Approach says nothing more.
        f.heard.clear();
        f.fly_to(&mut e, FARLAND, CLIMB, &never);
        assert!(f.heard.is_empty(), "{:?}", f.heard);
    }
}

// Readback reminders: a missing readback is reminded of once, after 10 s of sim time or
// at once when the aircraft moves with a taxi readback pending.

/// SP-ABC, 300 m from stand 20 and creeping (2 kt) at sim time 0, asks for taxi to the stand
/// (the clearance is given at sim time `t`); its readback is pending.
fn abc_taxi_clearance_at(t: f64) -> (AtcEngine, Vec<AtcItem>, f64) {
    let mut e = abc_near_stand_20(300.0, 167.7, 347.7, 2.0);
    let out = e.on_transmission(
        GROUND,
        &format!("Krakow Ground, {ABC}, request taxi to stand two zero"),
        Lang::En,
        SimTime(t),
    );
    let clearance = items(&out);
    assert!(
        matches!(&clearance[..], [AtcItem::Taxi { .. }]),
        "{clearance:?}"
    );
    // The readback is due once the clearance has been spoken (2.5 words a second).
    let due = t + (out[0].text.split_whitespace().count() as f64 / 2.5).ceil();
    (e, clearance, due)
}

/// SP-ABC's telemetry at sim time `t` and speed `gs_kt`, where `abc_near_stand_20` put it.
fn abc_at(e: &mut AtcEngine, t: f64, gs_kt: f64) -> Vec<AtcOutput> {
    let (ap, _) = epkk();
    let stand = ap.stands.iter().find(|s| s.name == "20").unwrap().pos;
    let pos = atc_geo::destination(stand, 167.7, 300.0);
    e.on_telemetry(AircraftState {
        callsign: Callsign::new("SPABC"),
        heading_deg: 347.7,
        ..ground_sample(t, pos.lat, pos.lon, gs_kt)
    })
}

fn is_reminder(out: &[AtcOutput]) -> bool {
    matches!(&items(out)[..], [AtcItem::ReadBack(_)]) && out[0].text.contains("read back")
}

#[test]
fn silent_pilot_is_reminded_after_10s() {
    let (mut e, _, due) = abc_taxi_clearance_at(100.0);
    assert!(abc_at(&mut e, due + 1.0, 2.0).is_empty());
    assert!(abc_at(&mut e, due + 9.9, 2.0).is_empty());
    let out = abc_at(&mut e, due + 10.0, 2.0);
    assert!(is_reminder(&out), "{out:?}");
    assert_eq!(out[0].message.from, "Kraków Ground");
    assert!(
        out[0].text.starts_with("Sierra Bravo Charlie, read back"),
        "{out:?}"
    );
}

#[test]
fn moving_without_taxi_readback_is_reminded_at_once() {
    let (mut e, _, due) = abc_taxi_clearance_at(100.0);
    assert!(abc_at(&mut e, due - 1.0, 2.0).is_empty());
    let out = abc_at(&mut e, due + 1.0, 6.0);
    assert!(is_reminder(&out), "{out:?}");
}

#[test]
fn reminder_only_once() {
    let (mut e, _, due) = abc_taxi_clearance_at(100.0);
    assert!(is_reminder(&abc_at(&mut e, due + 10.0, 2.0)));
    assert!(abc_at(&mut e, due + 11.0, 6.0).is_empty());
    assert!(abc_at(&mut e, due + 30.0, 6.0).is_empty());
}

#[test]
fn readback_before_10s_cancels_reminder() {
    let (mut e, clearance, due) = abc_taxi_clearance_at(100.0);
    assert!(abc_at(&mut e, due + 1.0, 2.0).is_empty());
    let out = e.on_transmission(
        GROUND,
        &readback(&clearance, "Sierra Bravo Charlie"),
        Lang::En,
        SimTime(due + 9.9),
    );
    assert!(out.is_empty(), "{out:?}");
    assert!(abc_at(&mut e, due + 15.0, 2.0).is_empty());
    assert!(abc_at(&mut e, due + 16.0, 6.0).is_empty());
}

#[test]
fn new_clearance_rearms_reminder() {
    let (mut e, clearance, due) = abc_taxi_clearance_at(100.0);
    assert!(is_reminder(&abc_at(&mut e, due + 10.0, 2.0)));
    let back = readback(&clearance, "Sierra Bravo Charlie");
    assert!(
        e.on_transmission(GROUND, &back, Lang::En, SimTime(due + 12.0))
            .is_empty()
    );
    // A repeated request is a new clearance with a new readback to wait for.
    let out = e.on_transmission(
        GROUND,
        &format!("Krakow Ground, {ABC}, request taxi to stand two zero"),
        Lang::En,
        SimTime(due + 20.0),
    );
    assert!(!out.is_empty());
    assert!(abc_at(&mut e, due + 25.0, 2.0).is_empty());
    assert!(is_reminder(&abc_at(&mut e, due + 40.0, 2.0)));
}

#[test]
fn old_snapshot_without_reminder_fields_restores() {
    let (mut e, _, _) = abc_taxi_clearance_at(100.0);
    let mut json = serde_json::to_value(e.snapshot()).unwrap();
    for strip in json["strips"].as_array_mut().unwrap() {
        let strip = strip.as_object_mut().unwrap();
        assert!(strip.remove("readback_due_s").is_some());
        assert!(strip.remove("reminded").is_some());
    }
    e.restore(serde_json::from_value(json).unwrap()).unwrap();
    let strip = e.strip(&Callsign::new("SPABC")).unwrap();
    assert_eq!((strip.readback_due_s, strip.reminded), (None, false));
}

#[test]
fn engine_without_pending_readback_does_not_remind() {
    let mut e = abc_near_stand_20(300.0, 167.7, 347.7, 2.0);
    assert!(abc_at(&mut e, 50.0, 2.0).is_empty());
    assert!(abc_at(&mut e, 60.0, 6.0).is_empty());
}

#[test]
fn correction_counts_as_the_reminder() {
    let (mut e, _, due) = abc_taxi_clearance_at(100.0);
    let out = e.on_transmission(
        GROUND,
        "hold short runway zero seven, Sierra Bravo Charlie",
        Lang::En,
        SimTime(due + 3.0),
    );
    assert!(!out.is_empty(), "a correction is expected");
    assert!(abc_at(&mut e, due + 10.0, 2.0).is_empty());
    assert!(abc_at(&mut e, due + 30.0, 6.0).is_empty());
}

#[test]
fn request_while_pending_counts_as_the_reminder() {
    let (mut e, _, due) = abc_taxi_clearance_at(100.0);
    let out = e.on_transmission(
        GROUND,
        &format!("Krakow Ground, {ABC}, request taxi to stand two zero"),
        Lang::En,
        SimTime(due + 3.0),
    );
    assert!(
        matches!(&items(&out)[..], [AtcItem::ReadBack(_)]),
        "{out:?}"
    );
    assert!(abc_at(&mut e, due + 10.0, 2.0).is_empty());
}

#[test]
fn moving_trigger_needs_a_standing_aircraft_after_the_clearance_is_spoken() {
    // Already rolling when the clearance is given: only the timed reminder applies.
    let mut e = abc_near_stand_20(300.0, 167.7, 347.7, 8.0);
    let out = e.on_transmission(
        GROUND,
        &format!("Krakow Ground, {ABC}, request taxi to stand two zero"),
        Lang::En,
        SimTime(100.0),
    );
    let due = 100.0 + (out[0].text.split_whitespace().count() as f64 / 2.5).ceil();
    assert!(abc_at(&mut e, due + 1.0, 8.0).is_empty());
    assert!(abc_at(&mut e, due + 9.9, 8.0).is_empty());
    assert!(is_reminder(&abc_at(&mut e, due + 10.0, 8.0)));
    // A standing aircraft that moves before the clearance has been spoken is not reminded.
    let (mut e, _, due) = abc_taxi_clearance_at(100.0);
    assert!(abc_at(&mut e, 100.5, 6.0).is_empty());
    assert!(is_reminder(&abc_at(&mut e, due + 1.0, 6.0)));
}

#[test]
fn reminder_waits_for_the_clearance_to_be_spoken() {
    let (mut e, _, due) = abc_taxi_clearance_at(100.0);
    assert!(due > 100.0 + 1.0);
    // 10 s after the request, but not yet 10 s after the clearance was spoken.
    assert!(abc_at(&mut e, 110.0, 2.0).is_empty());
    assert!(abc_at(&mut e, due + 9.9, 2.0).is_empty());
    assert!(is_reminder(&abc_at(&mut e, due + 10.0, 2.0)));
}

#[test]
fn sim_clock_reset_rearms_the_reminder() {
    let (mut e, _, due) = abc_taxi_clearance_at(100.0);
    assert!(abc_at(&mut e, 20.0, 2.0).is_empty());
    let speech = due - 100.0;
    assert!(abc_at(&mut e, 20.0 + speech + 9.9, 2.0).is_empty());
    assert!(is_reminder(&abc_at(&mut e, 20.0 + speech + 10.0, 2.0)));
}

#[test]
fn restored_snapshot_does_not_remind_old_clearances() {
    let (mut e, _, due) = abc_taxi_clearance_at(100.0);
    let json = serde_json::to_value(e.snapshot()).unwrap();
    e.restore(serde_json::from_value(json).unwrap()).unwrap();
    assert!(
        e.strip(&Callsign::new("SPABC"))
            .unwrap()
            .pending_readback
            .is_some()
    );
    assert!(abc_at(&mut e, due + 30.0, 6.0).is_empty());
}

#[test]
fn contact_ground_without_readback_never_reminds_after_the_call() {
    let mut e = engine();
    let first = track().into_iter().next().unwrap();
    assert!(e.on_telemetry(first.clone()).is_empty());
    let out = say(&mut e, TOWER, &format!("Krakow Tower, {CS}, request taxi"));
    assert!(
        matches!(&items(&out)[..], [AtcItem::Contact { khz: GROUND, .. }]),
        "{out:?}"
    );
    let at = |t: f64| AircraftState {
        t: SimTime(t),
        ..first.clone()
    };
    // The pilot calls Ground without reading the instruction back: Tower is silent after.
    let out = e.on_transmission(
        GROUND,
        &format!("Krakow Ground, {CS}, information Alpha, request taxi"),
        Lang::En,
        SimTime(5.0),
    );
    assert!(out.iter().all(|o| o.freq_khz == GROUND), "{out:?}");
    for t in [8.0, 20.0, 40.0] {
        let out = e.on_telemetry(at(t));
        assert!(out.iter().all(|o| o.freq_khz != TOWER), "{out:?}");
    }
}
