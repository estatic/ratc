//! Mapping tables and national defaults used when generating a station from OpenAIP data.
//!
//! OpenAIP frequency type to ATC role:
//!
//! | OpenAIP type            | Role       |
//! |-------------------------|------------|
//! | 15 ATIS                 | `Atis`     |
//! | 5, 26                   | `Delivery` |
//! | 9, 1 (Apron)            | `Ground`   |
//! | 14 Tower                | `Tower`    |
//! | 0, 2, 6, 13             | `Approach` (first/primary is `khz`, the rest `approach_alt_khz`) |
//! | 22, 10                  | `Afis`     |
//! | 12, 16, 4, 11           | `Unicom`   |
//!
//! OpenAIP airspace type to kind: 4 CTR, 13 ATZ, 7 TMA, 26 CTA, 33 FIS sector, 27 ACC sector,
//! 10 FIR; every other type is not copied into the station.

use atc_navdata::model::FreqKind;
use atc_navdata::procedures::AirspaceKind;

/// Role of an OpenAIP frequency type, if it has one.
pub fn role_of_frequency(kind: u8) -> Option<FreqKind> {
    Some(match kind {
        15 => FreqKind::Atis,
        5 | 26 => FreqKind::Delivery,
        9 | 1 => FreqKind::Ground,
        14 => FreqKind::Tower,
        0 | 2 | 6 | 13 => FreqKind::Approach,
        22 | 10 => FreqKind::Afis,
        12 | 16 | 4 | 11 => FreqKind::Unicom,
        _ => return None,
    })
}

/// Station airspace kind of an OpenAIP airspace type, if it is copied.
pub fn airspace_kind(kind: u8) -> Option<AirspaceKind> {
    Some(match kind {
        4 => AirspaceKind::Ctr,
        13 => AirspaceKind::Atz,
        7 => AirspaceKind::Tma,
        26 => AirspaceKind::Cta,
        33 => AirspaceKind::Fis,
        27 => AirspaceKind::Acc,
        10 => AirspaceKind::Fir,
        _ => return None,
    })
}

/// Position order in the wizard and the station file.
pub fn role_order(role: FreqKind) -> u8 {
    match role {
        FreqKind::Atis => 0,
        FreqKind::Delivery => 1,
        FreqKind::Ground => 2,
        FreqKind::Tower => 3,
        FreqKind::Approach | FreqKind::Departure => 4,
        FreqKind::Afis => 5,
        FreqKind::Unicom => 6,
    }
}

/// Radio word for a role, used when OpenAIP gives no frequency name.
pub fn role_word(role: FreqKind) -> &'static str {
    match role {
        FreqKind::Atis => "ATIS",
        FreqKind::Delivery => "Delivery",
        FreqKind::Ground => "Ground",
        FreqKind::Tower => "Tower",
        FreqKind::Approach => "Approach",
        FreqKind::Departure => "Departure",
        FreqKind::Afis => "AFIS",
        FreqKind::Unicom => "Radio",
    }
}

/// National transition altitude (ft) with its source, by ISO country code.
const TRANSITION_ALTITUDES: &[(&str, i32, &str)] = &[
    // AIP Polska ENR 1.7: transition altitude 6500 ft in the Polish FIR.
    ("PL", 6500, "default: AIP Polska ENR 1.7"),
];

/// Fallback when no national value is known.
pub const DEFAULT_TRANSITION_ALTITUDE_FT: i32 = 5000;

/// Transition altitude and its provenance for a country.
pub fn transition_altitude(country: &str) -> (i32, String) {
    TRANSITION_ALTITUDES
        .iter()
        .find(|(c, _, _)| c.eq_ignore_ascii_case(country))
        .map(|&(_, ft, src)| (ft, src.to_string()))
        .unwrap_or((
            DEFAULT_TRANSITION_ALTITUDE_FT,
            "default: no national value known".to_string(),
        ))
}

/// Title-cases a name, each hyphen-separated part capitalised (`KRAKÓW-BALICE` to `Kraków-Balice`).
pub fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|w| w.split('-').map(capitalise).collect::<Vec<_>>().join("-"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalise(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f
            .to_uppercase()
            .chain(c.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

/// Words that name a service rather than a place: a frequency name made only of these
/// (`ATIS`, `TOWER`, ...) gets the airport's place name in front.
const ROLE_WORDS: [&str; 16] = [
    "ATIS",
    "ARR-ATIS",
    "DEP-ATIS",
    "TWR",
    "TOWER",
    "GND",
    "GROUND",
    "APP",
    "APPROACH",
    "DEL",
    "DELIVERY",
    "RADIO",
    "INFO",
    "INFORMATION",
    "AFIS",
    "RADAR",
];

/// The place an airport is named after: the first part of its OpenAIP name before `/`, `-`
/// or a space, title-cased (`ZETA/NORTH` to `Zeta`, `ZETA-WEST FIELD` to `Zeta`).
pub fn place_name(airport_name: &str) -> String {
    airport_name
        .split(|c: char| c == '/' || c == '-' || c.is_whitespace())
        .find(|w| !w.is_empty())
        .map(title_case)
        .unwrap_or_default()
}

/// Radio callsign from an OpenAIP frequency name: title-cased, abbreviations expanded
/// (`APP`, `GND`, `TWR`, `DEL`), acronyms kept (`ATIS`, `AFIS`). A name of role words only
/// (`ATIS`, `TOWER`) is prefixed with the place name of `airport_name`.
pub fn callsign_from_name(name: &str, airport_name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let body = words
        .iter()
        .map(|w| match w.to_uppercase().as_str() {
            "APP" => "Approach".to_string(),
            "GND" => "Ground".to_string(),
            "TWR" => "Tower".to_string(),
            "DEL" => "Delivery".to_string(),
            "ATIS" => "ATIS".to_string(),
            "ARR-ATIS" => "Arrival ATIS".to_string(),
            "DEP-ATIS" => "Departure ATIS".to_string(),
            "AFIS" => "AFIS".to_string(),
            _ => title_case(w),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let roles_only = !words.is_empty()
        && words
            .iter()
            .all(|w| ROLE_WORDS.contains(&w.to_uppercase().as_str()));
    let place = place_name(airport_name);
    if roles_only && !place.is_empty() {
        format!("{place} {body}")
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_types_map_to_roles() {
        let table: &[(u8, Option<FreqKind>)] = &[
            (15, Some(FreqKind::Atis)),
            (5, Some(FreqKind::Delivery)),
            (26, Some(FreqKind::Delivery)),
            (9, Some(FreqKind::Ground)),
            (1, Some(FreqKind::Ground)),
            (14, Some(FreqKind::Tower)),
            (0, Some(FreqKind::Approach)),
            (2, Some(FreqKind::Approach)),
            (6, Some(FreqKind::Approach)),
            (13, Some(FreqKind::Approach)),
            (22, Some(FreqKind::Afis)),
            (10, Some(FreqKind::Afis)),
            (12, Some(FreqKind::Unicom)),
            (16, Some(FreqKind::Unicom)),
            (4, Some(FreqKind::Unicom)),
            (11, Some(FreqKind::Unicom)),
            (3, None),
            (99, None),
        ];
        for &(code, role) in table {
            assert_eq!(role_of_frequency(code), role, "frequency type {code}");
        }
    }

    #[test]
    fn airspace_types_map_to_kinds() {
        let table: &[(u8, Option<AirspaceKind>)] = &[
            (4, Some(AirspaceKind::Ctr)),
            (13, Some(AirspaceKind::Atz)),
            (7, Some(AirspaceKind::Tma)),
            (26, Some(AirspaceKind::Cta)),
            (33, Some(AirspaceKind::Fis)),
            (27, Some(AirspaceKind::Acc)),
            (10, Some(AirspaceKind::Fir)),
            (1, None),
            (36, None),
        ];
        for &(code, kind) in table {
            assert_eq!(airspace_kind(code), kind, "airspace type {code}");
        }
    }

    #[test]
    fn title_case_handles_utf8_and_hyphens() {
        assert_eq!(title_case("KRAKÓW-ZACHOD  north"), "Kraków-Zachod North");
        assert_eq!(
            callsign_from_name("KRAKÓW APP", "KRAKÓW BALICE"),
            "Kraków Approach"
        );
    }

    #[test]
    fn place_name_is_the_first_part() {
        assert_eq!(place_name("ZETA/NORTH"), "Zeta");
        assert_eq!(place_name("ZETA-WEST FIELD"), "Zeta");
        assert_eq!(place_name("ZETA INTERNATIONAL"), "Zeta");
        assert_eq!(place_name(" ÉTOILE "), "Étoile");
        assert_eq!(place_name(""), "");
    }

    #[test]
    fn bare_role_names_get_the_place() {
        let ap = "ZETA/NORTH";
        assert_eq!(callsign_from_name("ATIS", ap), "Zeta ATIS");
        assert_eq!(callsign_from_name("TOWER", ap), "Zeta Tower");
        assert_eq!(callsign_from_name("ARR-ATIS", ap), "Zeta Arrival ATIS");
        assert_eq!(callsign_from_name("dep-atis", ap), "Zeta Departure ATIS");
        assert_eq!(callsign_from_name("GND", ap), "Zeta Ground");
        // A name with a place keeps its own.
        assert_eq!(callsign_from_name("OMEGA TOWER", ap), "Omega Tower");
        assert_eq!(callsign_from_name("ZETA DELIVERY", ap), "Zeta Delivery");
        assert_eq!(callsign_from_name("ZETA ATIS", ap), "Zeta ATIS");
        // No place to add.
        assert_eq!(callsign_from_name("TOWER", ""), "Tower");
    }
}
