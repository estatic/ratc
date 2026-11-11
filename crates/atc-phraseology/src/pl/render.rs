//! Polish rendering of ATC messages (ICAO Doc 9432, Polish edition; wording
//! in `docs/phraseology/pl.md`).

use super::numbers::{altitude_ft, cardinal, digit, digits, frequency, plural, qnh, squawk, wind};
use crate::render::station_name;
use crate::types::{
    AtcItem, AtcMessage, Callsign, CircuitLeg, ClearanceKind, Courtesy, Lang, Level, ReadbackItem,
    Side, TaxiTo, UnableReason, Wind,
};

const ALPHABET: [&str; 26] = [
    "Alfa", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliett",
    "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo", "Sierra", "Tango",
    "Uniform", "Victor", "Whiskey", "X-ray", "Yankee", "Zulu",
];

/// Renders `msg` as spoken Polish; same structure as the English renderer.
pub fn render(msg: &AtcMessage, first_contact: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(cs) = &msg.to {
        parts.push(callsign_words(cs, first_contact));
    }
    for item in &msg.items {
        item_parts(item, &mut parts);
    }
    parts.join(", ")
}

pub(crate) fn callsign_words(cs: &Callsign, full: bool) -> String {
    let text = if full { cs.0.clone() } else { cs.abbreviated() };
    spell(&text)
}

/// Letters with the Polish ICAO alphabet, digits one by one, space separated.
fn spell(text: &str) -> String {
    text.chars().map(spell_char).collect::<Vec<_>>().join(" ")
}

/// One character in the Polish ICAO alphabet ("Alfa"), digits as Polish digits.
pub fn spell_char(c: char) -> String {
    if let Some(d) = c.to_digit(10) {
        digit(d).to_string()
    } else if c.is_ascii_alphabetic() {
        ALPHABET[(c.to_ascii_uppercase() as u8 - b'A') as usize].to_string()
    } else {
        c.to_string()
    }
}

/// Runway designator: digits one by one, `L`/`R`/`C` as words; a pair label
/// (`"07/25"`) becomes two designators separated by a comma pause.
pub fn runway_words(rwy: &str) -> String {
    rwy.split('/')
        .map(|r| {
            r.chars()
                .map(|c| match c.to_ascii_uppercase() {
                    'L' => "lewy".to_string(),
                    'R' => "prawy".to_string(),
                    'C' => "środkowy".to_string(),
                    _ => spell_char(c),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A letter run of a name this long or longer is said as a word, a shorter one is spelled.
const WORD_MIN_LETTERS: usize = 4;

/// A stand name made speakable: each word split into runs of digits and of letters
/// (punctuation separates runs and is dropped);
/// digits and runs of up to three letters are spelled (`"9R"` -> "dziewięć Romeo", `"GA"`
/// -> "Golf Alfa"), longer runs are said as a word (`"APRON4 GA"` -> "apron cztery Golf
/// Alfa").
fn name_words(name: &str) -> String {
    let mut parts = Vec::new();
    for word in name.split_whitespace() {
        let mut run = String::new();
        let flush = |run: &mut String, parts: &mut Vec<String>| {
            if run.chars().count() >= WORD_MIN_LETTERS
                && !run.starts_with(|c: char| c.is_ascii_digit())
            {
                parts.push(run.to_lowercase());
            } else if !run.is_empty() {
                parts.push(spell(run));
            }
            run.clear();
        };
        for c in word.chars() {
            // Punctuation separates runs and is not said ("APRON-4").
            if !c.is_alphanumeric() {
                flush(&mut run, &mut parts);
                continue;
            }
            let digit = c.is_ascii_digit();
            if run.starts_with(|r: char| r.is_ascii_digit()) != digit {
                flush(&mut run, &mut parts);
            }
            run.push(c);
        }
        flush(&mut run, &mut parts);
    }
    parts.join(" ")
}

fn wind_words(w: &Wind) -> String {
    if w.calm {
        "wiatr cisza".to_string()
    } else {
        format!("wiatr {}", wind(w.dir_deg, w.speed_kt))
    }
}

fn clearance(runway: &str, w: &Wind, what: &str) -> String {
    format!("{}, pas {}, {what}", wind_words(w), runway_words(runway))
}

fn leg_words(leg: CircuitLeg) -> &'static str {
    match leg {
        CircuitLeg::Upwind => "pozycję pod wiatr",
        CircuitLeg::Crosswind => "pozycję boczną",
        CircuitLeg::Downwind => "z wiatrem",
        CircuitLeg::Base => "bejs",
        CircuitLeg::Final => "prostą",
    }
}

fn readback_label(item: &ReadbackItem) -> &'static str {
    match item {
        ReadbackItem::Runway(_) => "pas",
        ReadbackItem::HoldShort(_) => "instrukcję oczekiwania przed pasem",
        ReadbackItem::Route(_) => "drogę kołowania",
        ReadbackItem::Kind(ClearanceKind::HoldPosition) => "instrukcję stój",
        ReadbackItem::Kind(_) => "zezwolenie",
        ReadbackItem::Altitude(_) => "wysokość",
        ReadbackItem::Heading(_) => "kurs",
        ReadbackItem::Frequency(_) => "częstotliwość",
        ReadbackItem::Qnh(_) => "QNH",
        ReadbackItem::Squawk(_) => "skłok",
        ReadbackItem::Sid(..) => "odlot",
        ReadbackItem::Star(..) => "przylot",
        ReadbackItem::Level(_) => "poziom",
        ReadbackItem::Direct(_) => "trasę bezpośrednią",
        ReadbackItem::Approach(..) => "zezwolenie na podejście",
        ReadbackItem::Destination(_) => "miejsce przeznaczenia",
    }
}

fn holding_point_words(runway: &str) -> String {
    format!(
        "kołuj do punktu oczekiwania przed pasem {}",
        runway_words(runway)
    )
}

fn side_words(side: Side) -> &'static str {
    match side {
        Side::Ahead => "przed tobą",
        Side::Right => "po twojej prawej",
        Side::Behind => "za tobą",
        Side::Left => "po twojej lewej",
    }
}

fn courtesy_words(c: Courtesy) -> &'static str {
    match c {
        Courtesy::Hello | Courtesy::GoodMorning | Courtesy::GoodAfternoon => "dzień dobry",
        Courtesy::GoodEvening => "dobry wieczór",
        Courtesy::GoodDay => "do widzenia",
        Courtesy::HaveANiceFlight => "miłego lotu",
        Courtesy::ThankYouYouToo => "dziękuję, wzajemnie",
    }
}

fn item_parts(item: &AtcItem, out: &mut Vec<String>) {
    match item {
        AtcItem::Taxi { to, via, crossings } => {
            let mut s = match to {
                TaxiTo::HoldingPoint { runway } => holding_point_words(runway),
                TaxiTo::Stand { name } => format!("kołuj do stanowiska {}", name_words(name)),
            };
            if !via.is_empty() {
                let names: Vec<String> = via.iter().map(|t| spell(t)).collect();
                s.push_str(&format!(" drogą {}", names.join(", ")));
            }
            out.push(s);
            for r in crossings {
                out.push(format!("przetnij pas {}", runway_words(r)));
            }
        }
        AtcItem::HoldShort(r) => out.push(format!("oczekuj przed pasem {}", runway_words(r))),
        AtcItem::CrossRunway(r) => out.push(format!("przetnij pas {}", runway_words(r))),
        AtcItem::RunwayApproved(r) => out.push(format!("pas {}, zgoda", runway_words(r))),
        AtcItem::RunwayChanged(r) => {
            out.push("zmiana pasa".into());
            out.push(format!("pas {}", runway_words(r)));
        }
        AtcItem::LineUpAndWait(r) => {
            out.push(format!("pas {}", runway_words(r)));
            out.push("zajmij pas i czekaj".into());
        }
        AtcItem::ClearedTakeoff { runway, wind } => {
            out.push(clearance(runway, wind, "zezwalam startować"))
        }
        AtcItem::ClearedLand { runway, wind } => {
            out.push(clearance(runway, wind, "zezwalam lądować"))
        }
        AtcItem::ClearedTouchAndGo { runway, wind } => {
            out.push(clearance(runway, wind, "zezwalam konwojer"))
        }
        AtcItem::Report(leg) => out.push(format!("zgłoś {}", leg_words(*leg))),
        AtcItem::Sequence(n) => out.push(format!("numer {}", cardinal(u32::from(*n)))),
        AtcItem::Qnh(q) => out.push(format!("QNH {}", qnh(*q))),
        AtcItem::Squawk(c) => out.push(format!("skłok {}", squawk(*c))),
        AtcItem::CheckSquawk(c) => out.push(format!("sprawdź skłok {}", squawk(*c))),
        AtcItem::SquawkCharlie => out.push("skłok Charlie".into()),
        AtcItem::Contact { station, khz } => out.push(format!(
            "łączność z {} {}",
            station_name(station, Lang::Pl),
            frequency(*khz)
        )),
        AtcItem::HoldPosition => out.push("stój".into()),
        AtcItem::GiveWay { traffic } => out.push(format!("ustąp pierwszeństwa {traffic}")),
        AtcItem::Follow { traffic } => out.push(format!("kołuj za {traffic}")),
        AtcItem::GoAround => out.push("przejdź na drugi krąg".into()),
        AtcItem::FollowMissedApproach => {
            out.push("wykonuj opublikowaną procedurę nieudanego podejścia".into())
        }
        AtcItem::Negative => out.push("nieprawidłowo".into()),
        AtcItem::ReadBack(items) => {
            let labels: Vec<&str> = items.iter().map(readback_label).collect();
            out.push(format!("powtórz {}", labels.join(" i ")));
        }
        AtcItem::SayAgainCallsign { station } => {
            out.push(format!("wołający {}", station_name(station, Lang::Pl)));
            out.push("powtórz znak wywoławczy".into());
        }
        AtcItem::SayAgainStation { station } => {
            out.push(format!("wołający {}", station_name(station, Lang::Pl)));
            out.push("powtórz".into());
        }
        AtcItem::ThisIs { station } => out.push(format!("tu {}", station_name(station, Lang::Pl))),
        AtcItem::Station { station } => out.push(station_name(station, Lang::Pl)),
        AtcItem::AtisCurrent(c) => out.push(format!("aktualna informacja {}", spell_char(*c))),
        AtcItem::Vacate { via } => match via {
            Some(t) => out.push(format!("zwolnij pas drogą {}", spell(t))),
            None => out.push("zwolnij pas".into()),
        },
        AtcItem::Unable(reason) => {
            out.push("nie mogę".into());
            out.push(match reason {
                UnableReason::NoRoute => "brak drogi kołowania".into(),
                UnableReason::UnknownStand { stand } => {
                    format!("nieznane stanowisko {}", name_words(stand))
                }
                UnableReason::UnknownDestination => "powtórz miejsce przeznaczenia".into(),
                UnableReason::NoIfrData => "brak procedur IFR, tylko VFR".into(),
                UnableReason::Closed => "stanowisko zamknięte".into(),
                UnableReason::ApproachNotAvailable => "podejście niedostępne".into(),
                UnableReason::SouthernEntry => {
                    "wlot od południa tylko przez punkty południowe".into()
                }
                UnableReason::RunwayInUse { runway } => {
                    format!("pas w użyciu {}", runway_words(runway))
                }
            });
        }
        AtcItem::Roger => out.push("przyjąłem".into()),
        AtcItem::UseCallsign => {
            out.push("używaj znaku wywoławczego w każdej korespondencji".into())
        }
        AtcItem::SayAgain => out.push("powtórz".into()),
        AtcItem::SayFlightRules => out.push("podaj przepisy lotu".into()),
        AtcItem::NoFlightPlan => out.push("brak planu lotu".into()),
        AtcItem::SayIntentions => out.push("podaj zamiary".into()),
        AtcItem::SayStand => out.push("podaj stanowisko".into()),
        AtcItem::PassYourMessage => out.push("nadawaj".into()),
        AtcItem::Standby => out.push("czekaj".into()),
        AtcItem::FollowMeOnTheWay => out.push("samochód follow me jest w drodze".into()),
        AtcItem::FollowTheFollowMe { to } => out.push(match to {
            TaxiTo::HoldingPoint { runway } => format!(
                "kołuj za samochodem follow me do punktu oczekiwania drogi startowej {}",
                runway_words(runway)
            ),
            TaxiTo::Stand { name } => format!(
                "kołuj za samochodem follow me do stanowiska {}",
                name_words(name)
            ),
        }),
        AtcItem::StandPosition {
            stand,
            metres,
            side,
        } => out.push(format!(
            "stanowisko {} jest {} {} {}",
            name_words(stand),
            cardinal(*metres),
            plural(*metres, "metr", "metry", "metrów"),
            side_words(*side)
        )),
        AtcItem::TakenOffWithoutClearance => out.push("wystartowałeś bez zezwolenia".into()),
        AtcItem::LandedWithoutClearance => out.push("wylądowałeś bez zezwolenia".into()),
        AtcItem::RunwayIncursion { runway } => out.push(format!(
            "wjechałeś na pas {} bez zezwolenia",
            runway_words(runway)
        )),
        AtcItem::CheckAltitude { circuit_ft } => out.push(format!(
            "sprawdź wysokość, wysokość kręgu {}",
            altitude_ft(*circuit_ft)
        )),
        AtcItem::TaxiAndReportReady { runway } => {
            out.push(format!("{}, zgłoś gotowość", holding_point_words(runway)))
        }
        AtcItem::NotClearedForTakeoff => out.push("nie masz zezwolenia na start".into()),
        AtcItem::NotClearedToLand => out.push("nie masz zezwolenia na lądowanie".into()),
        AtcItem::Courtesy(c) => out.push(courtesy_words(*c).into()),
        AtcItem::ClearedTo { destination, .. } => {
            out.push(format!("zezwolenie do {}", genitive(destination)))
        }
        AtcItem::Sid {
            fix,
            number,
            letter,
        } => out.push(format!("odlot {}", procedure_words(fix, *number, *letter))),
        AtcItem::ClimbTo(l) => out.push(format!(
            "wznoszenie na {}",
            match l {
                Level::Altitude(ft) => format!("wysokość {}", altitude_ft((*ft).max(0) as u32)),
                Level::FlightLevel(fl) => format!("poziom lotu {}", digits(u32::from(*fl), 1)),
            }
        )),
        AtcItem::StartUpApproved => out.push("uruchomienie zezwalam".into()),
        AtcItem::PushAndStartApproved { facing } => {
            out.push("wypychanie i uruchomienie zezwalam".into());
            if let Some(f) = facing {
                out.push(format!("dziobem w kierunku {f}"));
            }
        }
        AtcItem::ReadbackCorrect => out.push("powtórzenie poprawne".into()),
        AtcItem::EnterControlZoneVia { point, alt_ft } => out.push(format!(
            "wejście w strefę przez {point}, wysokość {}",
            altitude_ft(*alt_ft)
        )),
        AtcItem::LeaveControlZoneVia { point } => {
            out.push(format!("wyjście ze strefy przez {point}"))
        }
        AtcItem::HoldOver { point, max_ft } => out.push(format!(
            "oczekuj nad {point}, wysokość maksymalna {}",
            altitude_ft(*max_ft)
        )),
        AtcItem::ReportAt { point } => out.push(format!("zgłoś nad {point}")),
        AtcItem::JoinDownwind { runway } => out.push(format!(
            "wejdź w krąg na pozycję z wiatrem, pas {}",
            runway_words(runway)
        )),
        AtcItem::FrequencyChangeApproved => out.push("zmiana częstotliwości zatwierdzona".into()),
        AtcItem::ChangeTo { station, khz } => out.push(format!(
            "przejdź na {} {}",
            station_name(station, Lang::Pl),
            frequency(*khz)
        )),
        // Tower says these to a handed-in IFR inbound (Tower speaks Polish).
        AtcItem::ExpectApproach { kind, runway } => out.push(format!(
            "spodziewaj się podejścia {}, pas {}",
            crate::render::approach_words(*kind),
            runway_words(runway)
        )),
        AtcItem::ExpectVisualApproach { runway } => out.push(format!(
            "spodziewaj się podejścia z widocznością, pas {}",
            runway_words(runway)
        )),
        AtcItem::ReportEstablished => {
            out.push("zgłoś ustalenie na kursie podejścia końcowego".into())
        }
        // Approach-only items: the engine never asks for Polish, English fallback.
        AtcItem::Star { .. }
        | AtcItem::DescendTo(_)
        | AtcItem::MaintainLevel(_)
        | AtcItem::Heading { .. }
        | AtcItem::DirectTo { .. }
        | AtcItem::ClearedApproach { .. }
        | AtcItem::Identified
        | AtcItem::RadarServiceTerminated
        | AtcItem::OffTrack { .. }
        | AtcItem::CheckHeading(_)
        | AtcItem::CheckLevel(_)
        | AtcItem::CheckEstablished { .. }
        | AtcItem::DistanceToGo(_)
        | AtcItem::VisualApproachNotAvailable => crate::render::item_parts(item, out),
    }
}

/// `("SUDOL", 2, 'J')` -> "SUDOL dwa Juliett" (fix as written).
fn procedure_words(fix: &str, number: u8, letter: char) -> String {
    format!("{fix} {} {}", digit(u32::from(number)), spell_char(letter))
}

/// Genitive of a place name for "do ...": `-ice` -> `-ic` (Katowice ->
/// Katowic), `-a` -> `-y`/`-i` (Warszawa -> Warszawy), a few final
/// consonants get `-a` (Gdańsk -> Gdańska). Multi-word and other names are
/// kept unchanged (no general Polish declension).
fn genitive(name: &str) -> String {
    if name.contains(' ') {
        return name.to_string();
    }
    if let Some(stem) = name.strip_suffix("ice") {
        return format!("{stem}ic");
    }
    if let Some(stem) = name.strip_suffix('a') {
        let soft = matches!(stem.chars().last(), Some('k' | 'g'));
        return format!("{stem}{}", if soft { 'i' } else { 'y' });
    }
    if name.ends_with([
        'k', 'w', 'n', 'ń', 'ł', 'r', 'z', 'd', 't', 'b', 'm', 'p', 's',
    ]) {
        return format!("{name}a");
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ApproachKind;

    #[test]
    fn use_callsign_in_polish() {
        let mut out = Vec::new();
        item_parts(&AtcItem::UseCallsign, &mut out);
        assert_eq!(
            out,
            vec!["używaj znaku wywoławczego w każdej korespondencji"]
        );
    }

    #[test]
    fn station_in_polish() {
        let mut out = Vec::new();
        item_parts(
            &AtcItem::Station {
                station: "Katowice Tower".into(),
            },
            &mut out,
        );
        assert_eq!(out, vec!["Katowice Wieża"]);
    }

    /// The items Tower says to a handed-in IFR inbound, in Polish.
    #[test]
    fn tower_inbound_items_in_polish() {
        let mut out = Vec::new();
        item_parts(
            &AtcItem::ExpectApproach {
                kind: ApproachKind::Ils,
                runway: "27".into(),
            },
            &mut out,
        );
        item_parts(
            &AtcItem::ExpectVisualApproach {
                runway: "09R".into(),
            },
            &mut out,
        );
        item_parts(&AtcItem::ReportEstablished, &mut out);
        assert_eq!(
            out,
            vec![
                "spodziewaj się podejścia ILS, pas dwa siedem",
                "spodziewaj się podejścia z widocznością, pas zero dziewięć prawy",
                "zgłoś ustalenie na kursie podejścia końcowego",
            ]
        );
    }
}
