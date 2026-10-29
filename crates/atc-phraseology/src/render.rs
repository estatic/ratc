//! ICAO rendering of ATC messages into the words a controller says
//! (the output is fed to a TTS engine).

use crate::types::{
    ApproachKind, AtcItem, AtcMessage, Callsign, CircuitLeg, Courtesy, Lang, Level, ReadbackItem,
    Side, TaxiTo, Turn, UnableReason, Wind,
};

const DIGITS: [&str; 10] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "niner",
];

const ALPHABET: [&str; 26] = [
    "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliett",
    "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo", "Sierra", "Tango",
    "Uniform", "Victor", "Whiskey", "X-ray", "Yankee", "Zulu",
];

/// Renders `msg` as speech in `msg.lang`. The addressee callsign opens the
/// transmission: in full when `first_contact`, abbreviated otherwise.
/// Items are separated by commas.
pub fn render(msg: &AtcMessage, first_contact: bool) -> String {
    match msg.lang {
        Lang::En => render_en(msg, first_contact),
        Lang::Pl => crate::pl::render::render(msg, first_contact),
    }
}

/// A station name as spoken in `lang`: Polish says "Wieża" for "Tower"
/// ("Ground" stays).
pub fn station_name(station: &str, lang: Lang) -> String {
    match lang {
        Lang::En => station.to_string(),
        Lang::Pl => station
            .split(' ')
            .map(|w| if w == "Tower" { "Wieża" } else { w })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn render_en(msg: &AtcMessage, first_contact: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(cs) = &msg.to {
        parts.push(callsign_words(cs, first_contact));
    }
    for item in &msg.items {
        item_parts(item, &mut parts);
    }
    parts.join(", ")
}

fn callsign_words(cs: &Callsign, full: bool) -> String {
    let text = if full { cs.0.clone() } else { cs.abbreviated() };
    spell(&text)
}

/// Spells letters with the ICAO alphabet and digits one by one, separated
/// by spaces. Other characters are kept as they are.
fn spell(text: &str) -> String {
    text.chars().map(spell_char).collect::<Vec<_>>().join(" ")
}

/// One character as an ICAO word: letter -> alphabet word, digit -> digit word.
pub fn spell_char(c: char) -> String {
    if let Some(d) = c.to_digit(10) {
        DIGITS[d as usize].to_string()
    } else if c.is_ascii_alphabetic() {
        ALPHABET[(c.to_ascii_uppercase() as u8 - b'A') as usize].to_string()
    } else {
        c.to_string()
    }
}

/// Digits one by one, zero-padded to `width` (`1013, 4` -> "one zero one three").
pub fn digits_words(n: u32, width: usize) -> String {
    format!("{n:0width$}")
        .chars()
        .map(spell_char)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runway designator: digits one by one, `L`/`R`/`C` as words. A pair label
/// (`"07/25"`) becomes two designators separated by a comma pause.
pub fn runway_words(rwy: &str) -> String {
    rwy.split('/')
        .map(|r| {
            r.chars()
                .map(|c| match c.to_ascii_uppercase() {
                    'L' => "left".to_string(),
                    'R' => "right".to_string(),
                    'C' => "centre".to_string(),
                    _ => spell_char(c),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A name (stand) made speakable: a word containing a digit is spelled
/// character by character (`"C5"` -> "Charlie five", `"5A"` -> "five Alpha"),
/// other words are kept.
fn name_words(name: &str) -> String {
    name.split_whitespace()
        .map(|w| {
            if w.chars().any(|c| c.is_ascii_digit()) {
                spell(w)
            } else {
                w.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Altitude, height or short distance: thousands digit by digit, then hundreds
/// (`2500` -> "two thousand five hundred", `12000` -> "one two thousand").
/// Rounded down to whole hundreds; below 100 it reads "zero".
pub fn altitude_words(ft: u32) -> String {
    let (thousands, hundreds) = (ft / 1000, ft % 1000 / 100);
    let mut parts = Vec::new();
    if thousands > 0 {
        parts.push(format!("{} thousand", digits_words(thousands, 1)));
    }
    if hundreds > 0 {
        parts.push(format!("{} hundred", digits_words(hundreds, 1)));
    }
    if parts.is_empty() {
        return "zero".to_string();
    }
    parts.join(" ")
}

/// Signed number digit by digit (`15` -> "one five", `-3` -> "minus three").
pub fn number_words(n: i32) -> String {
    let digits = digits_words(n.unsigned_abs(), 1);
    if n < 0 {
        format!("minus {digits}")
    } else {
        digits
    }
}

fn wind_words(w: &Wind) -> String {
    if w.calm {
        "wind calm".to_string()
    } else {
        format!(
            "wind {} degrees, {} knots",
            digits_words(u32::from(w.dir_deg), 3),
            digits_words(u32::from(w.speed_kt), 1)
        )
    }
}

/// Decimals of a frequency as spoken: all three, or only the first when the
/// last two are zero (ICAO Annex 10 Vol II 5.2.1.4.1.4).
pub fn frequency_decimals(khz: u32) -> String {
    let frac = format!("{:03}", khz % 1000);
    if khz.is_multiple_of(100) {
        frac[..1].to_string()
    } else {
        frac
    }
}

/// `118100` kHz -> "one one eight decimal one", `121980` -> "one two one
/// decimal niner eight zero"; see [`frequency_decimals`].
pub fn frequency_words(khz: u32) -> String {
    let mhz = khz / 1000;
    format!(
        "{} decimal {}",
        digits_words(mhz, 1),
        spell(&frequency_decimals(khz))
    )
}

fn leg_word(leg: CircuitLeg) -> &'static str {
    match leg {
        CircuitLeg::Upwind => "upwind",
        CircuitLeg::Crosswind => "crosswind",
        CircuitLeg::Downwind => "downwind",
        CircuitLeg::Base => "base",
        CircuitLeg::Final => "final",
    }
}

fn readback_label(item: &ReadbackItem) -> &'static str {
    match item {
        ReadbackItem::Runway(_) => "runway",
        ReadbackItem::HoldShort(_) => "hold short instruction",
        ReadbackItem::Route(_) => "taxi route",
        ReadbackItem::Kind(_) => "clearance",
        ReadbackItem::Altitude(_) => "altitude",
        ReadbackItem::Heading(_) => "heading",
        ReadbackItem::Frequency(_) => "frequency",
        ReadbackItem::Qnh(_) => "QNH",
        ReadbackItem::Squawk(_) => "squawk",
        ReadbackItem::Sid(..) => "departure procedure",
        ReadbackItem::Star(..) => "arrival procedure",
        ReadbackItem::Level(_) => "level",
        ReadbackItem::Direct(_) => "direct routing",
        ReadbackItem::Approach(..) => "approach clearance",
        ReadbackItem::Destination(_) => "destination",
    }
}

fn clearance(runway: &str, wind: &Wind, what: &str) -> String {
    format!(
        "{}, runway {}, {what}",
        wind_words(wind),
        runway_words(runway)
    )
}

pub(crate) fn item_parts(item: &AtcItem, out: &mut Vec<String>) {
    match item {
        AtcItem::Taxi { to, via, crossings } => {
            let mut s = format!("taxi to {}", taxi_to_words(to));
            if !via.is_empty() {
                let names: Vec<String> = via.iter().map(|t| spell(t)).collect();
                s.push_str(&format!(" via {}", names.join(", ")));
            }
            out.push(s);
            for r in crossings {
                out.push(format!("cross runway {}", runway_words(r)));
            }
        }
        AtcItem::HoldShort(r) => out.push(format!("hold short runway {}", runway_words(r))),
        AtcItem::CrossRunway(r) => out.push(format!("cross runway {}", runway_words(r))),
        AtcItem::RunwayApproved(r) => out.push(format!("runway {} approved", runway_words(r))),
        AtcItem::RunwayChanged(r) => {
            out.push("runway changed".into());
            out.push(format!("runway {}", runway_words(r)));
        }
        AtcItem::LineUpAndWait(r) => {
            out.push(format!("runway {}", runway_words(r)));
            out.push("line up and wait".into());
        }
        AtcItem::ClearedTakeoff { runway, wind } => {
            out.push(clearance(runway, wind, "cleared for take-off"))
        }
        AtcItem::ClearedLand { runway, wind } => {
            out.push(clearance(runway, wind, "cleared to land"))
        }
        AtcItem::ClearedTouchAndGo { runway, wind } => {
            out.push(clearance(runway, wind, "cleared touch and go"))
        }
        AtcItem::Report(leg) => out.push(format!("report {}", leg_word(*leg))),
        AtcItem::Sequence(n) => out.push(format!("number {}", digits_words(u32::from(*n), 1))),
        AtcItem::Qnh(q) => out.push(format!("QNH {}", digits_words(u32::from(*q), 4))),
        AtcItem::Squawk(c) => out.push(format!("squawk {}", digits_words(u32::from(*c), 4))),
        AtcItem::CheckSquawk(c) => {
            out.push(format!("check squawk {}", digits_words(u32::from(*c), 4)))
        }
        AtcItem::SquawkCharlie => out.push("squawk Charlie".into()),
        AtcItem::Contact { station, khz } => {
            out.push(format!("contact {station} {}", frequency_words(*khz)))
        }
        AtcItem::HoldPosition => out.push("hold position".into()),
        AtcItem::GiveWay { traffic } => out.push(format!("give way to {traffic}")),
        AtcItem::Follow { traffic } => out.push(format!("follow {traffic}")),
        AtcItem::GoAround => out.push("go around".into()),
        AtcItem::Negative => out.push("negative".into()),
        AtcItem::ReadBack(items) => {
            let labels: Vec<&str> = items.iter().map(readback_label).collect();
            out.push(format!("read back {}", labels.join(", ")));
        }
        AtcItem::SayAgainCallsign { station } => {
            out.push(format!("Station calling {station}"));
            out.push("say again your callsign".into());
        }
        AtcItem::SayAgainStation { station } => {
            out.push(format!("Station calling {station}"));
            out.push("say again".into());
        }
        AtcItem::ThisIs { station } => out.push(format!("this is {station}")),
        AtcItem::AtisCurrent(c) => out.push(format!("information {} is current", spell_char(*c))),
        AtcItem::Vacate { via } => match via {
            Some(t) => out.push(format!("vacate via {}", spell(t))),
            None => out.push("vacate".into()),
        },
        AtcItem::Unable(reason) => {
            out.push("unable".into());
            out.push(match reason {
                UnableReason::NoRoute => "no taxi route".into(),
                UnableReason::UnknownStand { stand } => {
                    format!("stand {} unknown", name_words(stand))
                }
                UnableReason::UnknownDestination => "say again destination".into(),
                UnableReason::NoIfrData => "IFR not available, VFR only".into(),
                UnableReason::Closed => "station closed".into(),
                UnableReason::ApproachNotAvailable => "approach not available".into(),
                UnableReason::SouthernEntry => {
                    "entry from the south only via the southern points".into()
                }
                UnableReason::RunwayInUse { runway } => {
                    format!("runway in use {}", runway_words(runway))
                }
            });
        }
        AtcItem::Roger => out.push("roger".into()),
        AtcItem::SayAgain => out.push("say again".into()),
        AtcItem::SayFlightRules => out.push("say flight rules".into()),
        AtcItem::NoFlightPlan => out.push("no flight plan".into()),
        AtcItem::SayIntentions => out.push("say intentions".into()),
        AtcItem::SayStand => out.push("say stand".into()),
        AtcItem::PassYourMessage => out.push("pass your message".into()),
        AtcItem::Standby => out.push("standby".into()),
        AtcItem::FollowMeOnTheWay => out.push("follow-me car is on the way".into()),
        AtcItem::FollowTheFollowMe => out.push("follow the follow-me car".into()),
        AtcItem::StandPosition {
            stand,
            metres,
            side,
        } => out.push(format!(
            "stand {} is {} metres {}",
            name_words(stand),
            metres_words(*metres),
            side_words(*side)
        )),
        AtcItem::TakenOffWithoutClearance => {
            out.push("you have taken off without clearance".into())
        }
        AtcItem::LandedWithoutClearance => out.push("you have landed without clearance".into()),
        AtcItem::RunwayIncursion { runway } => out.push(format!(
            "you have entered runway {} without clearance",
            runway_words(runway)
        )),
        AtcItem::CheckAltitude { circuit_ft } => out.push(format!(
            "check altitude, circuit altitude {} feet",
            altitude_words(*circuit_ft)
        )),
        AtcItem::TaxiAndReportReady { runway } => out.push(format!(
            "taxi to holding point runway {}, report ready",
            runway_words(runway)
        )),
        AtcItem::NotClearedForTakeoff => {
            out.push("negative, not cleared for take-off, report ready for departure".into())
        }
        AtcItem::NotClearedToLand => out.push("negative, not cleared to land".into()),
        AtcItem::Courtesy(c) => out.push(courtesy_words(*c).into()),
        AtcItem::ClearedTo { destination, .. } => out.push(format!("cleared to {destination}")),
        AtcItem::Sid {
            fix,
            number,
            letter,
        } => out.push(procedure_words(fix, *number, *letter, "departure")),
        AtcItem::Star {
            fix,
            number,
            letter,
        } => out.push(procedure_words(fix, *number, *letter, "arrival")),
        AtcItem::ClimbTo(l) => out.push(format!("climb {}", level_words(*l))),
        AtcItem::DescendTo(l) => out.push(format!("descend to {}", level_words(*l))),
        AtcItem::MaintainLevel(l) => out.push(format!("maintain {}", level_words(*l))),
        AtcItem::Heading { deg, turn } => {
            let turn = match turn {
                Some(Turn::Left) => "turn left ",
                Some(Turn::Right) => "turn right ",
                None => "fly ",
            };
            out.push(format!(
                "{turn}heading {}",
                digits_words(u32::from(*deg), 3)
            ));
        }
        AtcItem::DirectTo { fix } => out.push(format!("proceed direct {fix}")),
        AtcItem::ExpectApproach { kind, runway } => out.push(format!(
            "expect {} approach runway {}",
            approach_words(*kind),
            runway_words(runway)
        )),
        AtcItem::ClearedApproach { kind, runway } => out.push(format!(
            "cleared {} approach runway {}",
            approach_words(*kind),
            runway_words(runway)
        )),
        AtcItem::ReportEstablished => out.push("report established".into()),
        AtcItem::Identified => out.push("identified".into()),
        AtcItem::RadarServiceTerminated => out.push("radar service terminated".into()),
        AtcItem::OffTrack { nm } => out.push(format!(
            "you are {} {} off track, check position",
            digits_words(u32::from(*nm), 1),
            if *nm == 1 { "mile" } else { "miles" }
        )),
        AtcItem::CheckHeading(deg) => out.push(format!(
            "check heading, fly heading {}",
            digits_words(u32::from(*deg), 3)
        )),
        AtcItem::CheckLevel(l) => out.push(format!("check level, maintain {}", level_words(*l))),
        AtcItem::DistanceToGo(nm) => out.push(format!(
            "{} {} from touchdown",
            digits_words(u32::from(*nm), 1),
            if *nm == 1 { "mile" } else { "miles" }
        )),
        AtcItem::CheckEstablished { kind, runway } => out.push(format!(
            "check established, report established {} runway {}",
            match kind {
                ApproachKind::Ils => "localizer",
                ApproachKind::VorDme | ApproachKind::Rnav => "final approach track",
            },
            runway_words(runway)
        )),
        AtcItem::StartUpApproved => out.push("start-up approved".into()),
        AtcItem::PushAndStartApproved { facing } => {
            out.push("push-back and start-up approved".into());
            if let Some(f) = facing {
                out.push(format!("facing {f}"));
            }
        }
        AtcItem::ReadbackCorrect => out.push("readback correct".into()),
        AtcItem::EnterControlZoneVia { point, alt_ft } => out.push(format!(
            "enter control zone via {point}, altitude {} feet",
            altitude_words(*alt_ft)
        )),
        AtcItem::LeaveControlZoneVia { point } => {
            out.push(format!("leave control zone via {point}"))
        }
        AtcItem::HoldOver { point, max_ft } => out.push(format!(
            "hold over {point}, maximum altitude {} feet",
            altitude_words(*max_ft)
        )),
        AtcItem::ReportAt { point } => out.push(format!("report at {point}")),
        AtcItem::VisualApproachNotAvailable => out.push("visual approach not available".into()),
        AtcItem::JoinDownwind { runway } => {
            out.push(format!("join downwind runway {}", runway_words(runway)))
        }
        AtcItem::FrequencyChangeApproved => out.push("frequency change approved".into()),
    }
}

/// `("SUDOL", 2, 'J', "departure")` -> "SUDOL two Juliett departure": the fix
/// is spoken as written (the TTS reads it as a word).
fn procedure_words(fix: &str, number: u8, letter: char, what: &str) -> String {
    format!(
        "{fix} {} {} {what}",
        spell_char(char::from_digit(u32::from(number % 10), 10).unwrap_or('0')),
        spell_char(letter)
    )
}

/// "altitude five thousand feet" / "flight level eight zero".
fn level_words(level: Level) -> String {
    match level {
        Level::Altitude(ft) => format!("altitude {} feet", altitude_words(ft.max(0) as u32)),
        Level::FlightLevel(fl) => format!("flight level {}", digits_words(u32::from(fl), 1)),
    }
}

/// "ILS", "VOR D M E", "R NAV": spelled so Piper does not read a word.
fn approach_words(kind: ApproachKind) -> &'static str {
    match kind {
        ApproachKind::Ils => "ILS",
        ApproachKind::VorDme => "VOR D M E",
        ApproachKind::Rnav => "R NAV",
    }
}

fn taxi_to_words(to: &TaxiTo) -> String {
    match to {
        TaxiTo::HoldingPoint { runway } => format!("holding point runway {}", runway_words(runway)),
        TaxiTo::Stand { name } => format!("stand {}", name_words(name)),
    }
}

fn side_words(side: Side) -> &'static str {
    match side {
        Side::Ahead => "ahead",
        Side::Right => "on your right",
        Side::Behind => "behind you",
        Side::Left => "on your left",
    }
}

fn courtesy_words(c: Courtesy) -> &'static str {
    match c {
        Courtesy::Hello => "hello",
        Courtesy::GoodMorning => "good morning",
        Courtesy::GoodAfternoon => "good afternoon",
        Courtesy::GoodEvening => "good evening",
        Courtesy::GoodDay => "good day",
        Courtesy::HaveANiceFlight => "have a nice flight",
        Courtesy::ThankYouYouToo => "thank you, you too",
    }
}

/// A distance in words as a controller says it: `120` -> "one hundred twenty",
/// `1050` -> "one thousand fifty".
fn metres_words(n: u32) -> String {
    const ONES: [&str; 10] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    ];
    const TEENS: [&str; 10] = [
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 10] = [
        "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    let mut parts = Vec::new();
    let (thousands, hundreds, rest) = (n / 1000, n % 1000 / 100, n % 100);
    if thousands > 0 {
        parts.push(format!("{} thousand", metres_words(thousands)));
    }
    if hundreds > 0 {
        parts.push(format!("{} hundred", ONES[hundreds as usize]));
    }
    match rest {
        0 if !parts.is_empty() => {}
        0..=9 => parts.push(ONES[rest as usize].to_string()),
        10..=19 => parts.push(TEENS[rest as usize - 10].to_string()),
        _ => {
            let tens = TENS[(rest / 10) as usize];
            parts.push(match rest % 10 {
                0 => tens.to_string(),
                d => format!("{tens} {}", ONES[d as usize]),
            });
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_in_words() {
        assert_eq!(metres_words(10), "ten");
        assert_eq!(metres_words(40), "forty");
        assert_eq!(metres_words(120), "one hundred twenty");
        assert_eq!(metres_words(300), "three hundred");
        assert_eq!(metres_words(1050), "one thousand fifty");
        assert_eq!(metres_words(2310), "two thousand three hundred ten");
    }
}
