//! ATIS generation: runway in use, information letter and spoken text.

use atc_geo::bearing_deg;
use atc_navdata::model::{Airport, Runway};
use atc_navdata::procedures::Procedures;
use atc_phraseology::pl::numbers::{self as pl_numbers, cardinal, plural};
use atc_phraseology::{
    Lang, Wind, altitude_words, digits_words, frequency_words, number_words, pl, runway_words,
    spell_char,
};

const MIN_HEADWIND_KT: f64 = 3.0;
const WIND_SPEED_STEP_KT: i32 = 5;
const WIND_DIR_STEP_DEG: i32 = 30;
const MIN_DIRECTION_SPEED_KT: u16 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CloudCover {
    Few,
    Sct,
    Bkn,
    Ovc,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Weather {
    pub wind: Wind,
    pub visibility_m: u32,
    /// Cloud layers: cover and base in ft AGL.
    pub clouds: Vec<(CloudCover, u32)>,
    pub temp_c: i16,
    pub dewpoint_c: i16,
    pub qnh_hpa: u16,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Atis {
    pub letter: char,
    pub runway: String,
    pub weather: Weather,
}

/// The unit a departure calls first, as the ATIS names it (AIP AD 2.20).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InitialContact {
    /// Unit as spoken ("Delivery", "Ground").
    pub unit: String,
    pub khz: u32,
    /// For the clearance (Delivery); otherwise for start-up (Ground).
    pub clearance: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AtisService {
    current: Atis,
}

/// Wind direction is compared with runway headings as given: no magnetic
/// variation is applied (both are treated as the same reference).
fn headwind_kt(wind: &Wind, runway_heading_deg: f64) -> f64 {
    f64::from(wind.speed_kt)
        * (f64::from(wind.dir_deg) - runway_heading_deg)
            .to_radians()
            .cos()
}

/// Greatest tailwind (kt) the runway in use tolerates before the ATIS switches.
pub const MAX_TAILWIND_KT: f64 = 5.0;

fn end_heading_deg(rwy: &Runway, i: usize) -> f64 {
    bearing_deg(rwy.ends[i].threshold, rwy.ends[1 - i].threshold)
}

/// Tailwind component (kt) on runway end `runway`: positive is a tailwind,
/// `Some(0.0)` when calm, `None` for an unknown runway ident.
pub fn tailwind_kt(wind: &Wind, airport: &Airport, runway: &str) -> Option<f64> {
    airport.runways.iter().find_map(|rwy| {
        let i = rwy.ends.iter().position(|e| e.ident == runway)?;
        Some(if wind.calm {
            0.0
        } else {
            -headwind_kt(wind, end_heading_deg(rwy, i))
        })
    })
}

/// The runway in use while its tailwind stays within [`MAX_TAILWIND_KT`];
/// otherwise the end with the greatest headwind, or the preferred runway when
/// the wind is calm or the best headwind component is below 3 kt.
fn select_runway(
    wind: &Wind,
    airport: &Airport,
    procs: &Procedures,
    current: Option<&str>,
) -> String {
    if let Some(r) = current
        && tailwind_kt(wind, airport, r).is_some_and(|t| t <= MAX_TAILWIND_KT)
    {
        return r.to_string();
    }
    let mut best: Option<(&str, f64)> = None;
    if !wind.calm {
        for rwy in &airport.runways {
            for (i, end) in rwy.ends.iter().enumerate() {
                let hw = headwind_kt(wind, end_heading_deg(rwy, i));
                if best.is_none_or(|(_, b)| hw > b) {
                    best = Some((end.ident.as_str(), hw));
                }
            }
        }
    }
    match best {
        Some((ident, hw)) if hw.abs() >= MIN_HEADWIND_KT => ident.to_string(),
        _ => procs.preferred_runway.clone(),
    }
}

fn visibility_category(m: u32) -> u8 {
    match m {
        10_000.. => 0,
        5_000.. => 1,
        1_500.. => 2,
        _ => 3,
    }
}

fn ceiling_category(clouds: &[(CloudCover, u32)]) -> u8 {
    let ceiling = clouds
        .iter()
        .filter(|(c, _)| matches!(c, CloudCover::Bkn | CloudCover::Ovc))
        .map(|&(_, base)| base)
        .min();
    match ceiling {
        None | Some(5_000..) => 0,
        Some(1_500..) => 1,
        Some(500..) => 2,
        Some(_) => 3,
    }
}

fn wind_changed(a: &Wind, b: &Wind) -> bool {
    let speed = |w: &Wind| if w.calm { 0 } else { i32::from(w.speed_kt) };
    if (speed(a) - speed(b)).abs() >= WIND_SPEED_STEP_KT {
        return true;
    }
    let direction_known = |w: &Wind| !w.calm && w.speed_kt >= MIN_DIRECTION_SPEED_KT;
    if !direction_known(a) || !direction_known(b) {
        return false;
    }
    let d = (i32::from(a.dir_deg) - i32::from(b.dir_deg)).rem_euclid(360);
    d.min(360 - d) >= WIND_DIR_STEP_DEG
}

fn needs_new_letter(old: &Atis, runway: &str, w: &Weather) -> bool {
    let o = &old.weather;
    old.runway != runway
        || o.qnh_hpa != w.qnh_hpa
        || wind_changed(&o.wind, &w.wind)
        || visibility_category(o.visibility_m) != visibility_category(w.visibility_m)
        || ceiling_category(&o.clouds) != ceiling_category(&w.clouds)
}

impl AtisService {
    /// Issues the first ATIS (information Alpha) for `w`.
    pub fn new(w: Weather, airport: &Airport, procs: &Procedures) -> Self {
        let runway = select_runway(&w.wind, airport, procs, None);
        AtisService {
            current: Atis {
                letter: 'A',
                runway,
                weather: w,
            },
        }
    }

    /// Compares `w` with the current ATIS. A significant change (runway, QNH,
    /// wind, visibility or ceiling category) issues the next letter (Z wraps to A)
    /// and returns it; otherwise the current ATIS is kept and `None` is returned.
    pub fn update(&mut self, w: Weather, airport: &Airport, procs: &Procedures) -> Option<&Atis> {
        let runway = select_runway(&w.wind, airport, procs, Some(&self.current.runway));
        if !needs_new_letter(&self.current, &runway, &w) {
            return None;
        }
        let letter = if self.current.letter == 'Z' {
            'A'
        } else {
            char::from(self.current.letter as u8 + 1)
        };
        self.current = Atis {
            letter,
            runway,
            weather: w,
        };
        Some(&self.current)
    }

    pub fn current(&self) -> &Atis {
        &self.current
    }

    /// Spoken ATIS text for TTS, in `lang`.
    pub fn text(&self, airport_name: &str, lang: Lang) -> String {
        self.text_with(airport_name, lang, None)
    }

    /// [`AtisService::text`] naming the initial-contact unit before the closing request to
    /// advise the letter: "for clearance contact Delivery one two one decimal niner eight"
    /// (Polish: "w sprawie zezwolenia łączność z Delivery ...", project wording).
    pub fn text_with(
        &self,
        airport_name: &str,
        lang: Lang,
        contact: Option<&InitialContact>,
    ) -> String {
        match lang {
            Lang::En => self.text_en(airport_name, contact),
            Lang::Pl => self.text_pl(airport_name, contact),
        }
    }

    fn text_en(&self, airport_name: &str, contact: Option<&InitialContact>) -> String {
        let Atis {
            letter,
            runway,
            weather: w,
        } = &self.current;
        let letter = spell_char(*letter);
        let wind = if w.wind.calm {
            "wind calm".to_string()
        } else {
            format!(
                "wind {} degrees {} knots",
                digits_words(u32::from(w.wind.dir_deg), 3),
                digits_words(u32::from(w.wind.speed_kt), 1)
            )
        };
        let contact = contact.map_or_else(String::new, |c| {
            format!(
                "for {} contact {} {}, ",
                if c.clearance { "clearance" } else { "start-up" },
                c.unit,
                frequency_words(c.khz)
            )
        });
        format!(
            "{airport_name} information {letter}, runway in use {}, {wind}, {}, {}, temperature {}, dew point {}, QNH {}, {contact}on initial contact advise you have information {letter}",
            runway_words(runway),
            visibility_words(w.visibility_m),
            clouds_words(&w.clouds),
            number_words(i32::from(w.temp_c)),
            number_words(i32::from(w.dewpoint_c)),
            digits_words(u32::from(w.qnh_hpa), 4),
        )
    }

    /// Polish ATIS: numbers as cardinals (wind, visibility, cloud bases, temperatures),
    /// QNH as two 2-digit cardinals, the runway digit by digit.
    fn text_pl(&self, airport_name: &str, contact: Option<&InitialContact>) -> String {
        let Atis {
            letter,
            runway,
            weather: w,
        } = &self.current;
        let letter = pl::render::spell_char(*letter);
        let wind = if w.wind.calm {
            "wiatr cisza".to_string()
        } else {
            format!(
                "wiatr {}",
                pl_numbers::wind(w.wind.dir_deg, w.wind.speed_kt)
            )
        };
        let contact = contact.map_or_else(String::new, |c| {
            format!(
                "w sprawie {} łączność z {} {}, ",
                if c.clearance {
                    "zezwolenia"
                } else {
                    "uruchomienia"
                },
                c.unit,
                pl_numbers::frequency(c.khz)
            )
        });
        format!(
            "{airport_name}, informacja {letter}, pas w użyciu {}, {wind}, {}, {}, temperatura {}, punkt rosy {}, QNH {}, {contact}przy pierwszym kontakcie potwierdź odbiór informacji {letter}",
            pl::render::runway_words(runway),
            visibility_words_pl(w.visibility_m),
            clouds_words_pl(&w.clouds),
            temperature_pl(w.temp_c),
            temperature_pl(w.dewpoint_c),
            pl_numbers::qnh(w.qnh_hpa),
        )
    }
}

fn visibility_words_pl(m: u32) -> String {
    match m {
        10_000.. => "widzialność dziesięć kilometrów lub więcej".to_string(),
        5_000.. => {
            let km = m / 1000;
            format!(
                "widzialność {} {}",
                cardinal(km),
                plural(km, "kilometr", "kilometry", "kilometrów")
            )
        }
        _ => format!(
            "widzialność {} {}",
            cardinal(m),
            plural(m, "metr", "metry", "metrów")
        ),
    }
}

fn clouds_words_pl(clouds: &[(CloudCover, u32)]) -> String {
    if clouds.is_empty() {
        return "brak chmur znaczących".to_string();
    }
    let layers: Vec<String> = clouds
        .iter()
        .map(|&(cover, base)| {
            let cover = match cover {
                CloudCover::Few => "nieliczne",
                CloudCover::Sct => "rozproszone",
                CloudCover::Bkn => "poprzerywane",
                CloudCover::Ovc => "całkowite",
            };
            format!("{cover} {}", pl_numbers::altitude_ft(base))
        })
        .collect();
    layers.join(", ")
}

/// Degrees Celsius as a cardinal, "minus" below zero.
fn temperature_pl(c: i16) -> String {
    let n = cardinal(u32::from(c.unsigned_abs()));
    if c < 0 { format!("minus {n}") } else { n }
}

fn visibility_words(m: u32) -> String {
    match m {
        10_000.. => "visibility one zero kilometres or more".to_string(),
        5_000.. => format!("visibility {} kilometres", digits_words(m / 1000, 1)),
        _ => format!("visibility {} metres", altitude_words(m)),
    }
}

fn clouds_words(clouds: &[(CloudCover, u32)]) -> String {
    if clouds.is_empty() {
        return "no significant cloud".to_string();
    }
    let layers: Vec<String> = clouds
        .iter()
        .map(|&(cover, base)| {
            let cover = match cover {
                CloudCover::Few => "few clouds",
                CloudCover::Sct => "scattered clouds",
                CloudCover::Bkn => "broken clouds",
                CloudCover::Ovc => "overcast",
            };
            format!("{cover} {} feet", altitude_words(base))
        })
        .collect();
    layers.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use atc_geo::{LatLon, destination};
    use atc_navdata::model::{Runway, RunwayEnd, TaxiNetwork};

    fn airport() -> Airport {
        let t07 = LatLon {
            lat: 50.0777,
            lon: 19.7848,
        };
        let t25 = destination(t07, 70.0, 2500.0);
        let end = |ident: &str, threshold| RunwayEnd {
            ident: ident.into(),
            threshold,
            displaced_m: 0.0,
        };
        Airport {
            icao: "EPKK".into(),
            name: "Kraków".into(),
            elevation_ft: 791,
            runways: vec![Runway {
                width_m: 60.0,
                ends: [end("07", t07), end("25", t25)],
            }],
            frequencies: vec![],
            taxi: TaxiNetwork::default(),
            stands: vec![],
        }
    }

    fn procs() -> Procedures {
        Procedures {
            icao: "EPKK".into(),
            source: "test".into(),
            airac: "test".into(),
            transition_altitude_ft: 6500,
            circuits: vec![],
            preferred_runway: "25".into(),
            positions: vec![],
            vfr_points: vec![],
            vfr_holds: vec![],
            vfr_hold_max_ft: None,
            adjacent: vec![],
            tma: None,
            delivery_hours: None,
            ifr: None,
            approach_alt_khz: vec![],
            magnetic_variation_deg: 0.0,
            airspaces: Vec::new(),
        }
    }

    fn wind(dir_deg: u16, speed_kt: u16) -> Wind {
        Wind {
            dir_deg,
            speed_kt,
            calm: false,
        }
    }

    fn wx() -> Weather {
        Weather {
            wind: wind(240, 10),
            visibility_m: 10_000,
            clouds: vec![(CloudCover::Few, 2500)],
            temp_c: 15,
            dewpoint_c: 8,
            qnh_hpa: 1013,
        }
    }

    fn service(w: Weather) -> AtisService {
        AtisService::new(w, &airport(), &procs())
    }

    fn changed(w: Weather) -> bool {
        service(wx()).update(w, &airport(), &procs()).is_some()
    }

    #[test]
    fn runway_by_headwind() {
        let mut w = wx();
        assert_eq!(service(w.clone()).current().runway, "25");
        w.wind = wind(60, 10);
        assert_eq!(service(w.clone()).current().runway, "07");
        w.wind = wind(20, 12);
        assert_eq!(service(w).current().runway, "07");
    }

    #[test]
    fn calm_uses_preferred() {
        let mut w = wx();
        w.wind = Wind {
            dir_deg: 60,
            speed_kt: 0,
            calm: true,
        };
        assert_eq!(service(w.clone()).current().runway, "25");
        // Headwind component below 3 kt: perpendicular 3 kt wind from 340.
        w.wind = wind(340, 3);
        assert_eq!(service(w.clone()).current().runway, "25");
        w.wind = wind(60, 2);
        assert_eq!(service(w).current().runway, "25");
    }

    fn calm_wind() -> Wind {
        Wind {
            dir_deg: 0,
            speed_kt: 0,
            calm: true,
        }
    }

    fn with_wind(w: Wind) -> Weather {
        Weather { wind: w, ..wx() }
    }

    #[test]
    fn calm_keeps_runway_in_use() {
        for (dir, runway) in [(250, "25"), (70, "07")] {
            // 10 kt -> calm is a wind change (new letter) but the runway stays.
            let mut s = service(with_wind(wind(dir, 10)));
            assert_eq!(s.current().runway, runway);
            s.update(with_wind(calm_wind()), &airport(), &procs());
            assert_eq!(s.current().runway, runway);
            // 4 kt -> calm is no significant change: no new letter at all.
            let mut s = service(with_wind(wind(dir, 4)));
            assert!(
                s.update(with_wind(calm_wind()), &airport(), &procs())
                    .is_none()
            );
            assert_eq!(s.current().runway, runway);
        }
    }

    #[test]
    fn light_tailwind_keeps_runway_in_use() {
        let mut s = service(with_wind(wind(250, 10)));
        s.update(with_wind(wind(70, 5)), &airport(), &procs());
        assert_eq!(s.current().runway, "25");
    }

    #[test]
    fn tailwind_above_limit_switches_runway() {
        let mut s = service(with_wind(wind(250, 10)));
        let letter = s.current().letter;
        let new = s
            .update(with_wind(wind(70, 6)), &airport(), &procs())
            .expect("new letter");
        assert_eq!(new.runway, "07");
        assert_ne!(new.letter, letter);
    }

    #[test]
    fn first_atis_unchanged() {
        assert_eq!(service(with_wind(calm_wind())).current().runway, "25");
        assert_eq!(service(with_wind(wind(70, 10))).current().runway, "07");
    }

    #[test]
    fn tailwind_kt_signs() {
        let a = airport();
        let w = wind(250, 10);
        assert!((tailwind_kt(&w, &a, "07").unwrap() - 10.0).abs() < 0.01);
        assert!((tailwind_kt(&w, &a, "25").unwrap() + 10.0).abs() < 0.01);
        assert_eq!(tailwind_kt(&calm_wind(), &a, "25"), Some(0.0));
        assert_eq!(tailwind_kt(&w, &a, "99"), None);
    }

    #[test]
    fn first_letter_is_a_and_wraps_after_z() {
        let mut s = service(wx());
        assert_eq!(s.current().letter, 'A');
        let mut w = wx();
        for i in 0..26 {
            w.qnh_hpa = 1000 + i;
            assert!(s.update(w.clone(), &airport(), &procs()).is_some());
        }
        assert_eq!(s.current().letter, 'A');
    }

    #[test]
    fn letter_advances_on_qnh_change() {
        let mut s = service(wx());
        let mut w = wx();
        w.qnh_hpa = 1012;
        let atis = s.update(w, &airport(), &procs()).unwrap();
        assert_eq!(atis.letter, 'B');
        assert_eq!(atis.weather.qnh_hpa, 1012);
    }

    #[test]
    fn letter_advances_on_runway_change() {
        let mut s = service(wx());
        let mut w = wx();
        w.wind = wind(60, 10);
        assert_eq!(s.update(w, &airport(), &procs()).unwrap().runway, "07");
        assert_eq!(s.current().letter, 'B');
    }

    #[test]
    fn letter_stable_on_small_change() {
        let mut s = service(wx());
        let mut w = wx();
        w.wind = wind(250, 13);
        w.temp_c = 16;
        w.dewpoint_c = 9;
        w.visibility_m = 12_000;
        w.clouds = vec![(CloudCover::Sct, 3000)];
        assert!(s.update(w, &airport(), &procs()).is_none());
        assert_eq!(s.current().letter, 'A');
        assert_eq!(s.current().weather, wx());
    }

    #[test]
    fn wind_thresholds() {
        let mut w = wx();
        w.wind = wind(270, 10); // 30 degrees, still runway 25
        assert!(changed(w.clone()));
        w.wind = wind(210, 10);
        assert!(changed(w.clone()));
        w.wind = wind(240, 15); // 5 kt
        assert!(changed(w.clone()));
        w.wind = wind(240, 14);
        assert!(!changed(w.clone()));
        w.wind = wind(355, 10); // 115 degrees: runway changes anyway
        assert!(changed(w));
    }

    #[test]
    fn wind_direction_ignored_when_calm_or_light() {
        let mut s = service(Weather {
            wind: wind(240, 2),
            ..wx()
        });
        let w = Weather {
            wind: wind(330, 2),
            ..wx()
        };
        assert!(s.update(w, &airport(), &procs()).is_none());
        let mut s = service(Weather {
            wind: Wind {
                dir_deg: 0,
                speed_kt: 0,
                calm: true,
            },
            ..wx()
        });
        let w = Weather {
            wind: wind(100, 2),
            ..wx()
        };
        assert!(s.update(w, &airport(), &procs()).is_none());
    }

    #[test]
    fn visibility_category_change() {
        let mut w = wx();
        w.visibility_m = 4_000;
        assert!(changed(w.clone()));
        w.visibility_m = 9_000;
        assert!(changed(w.clone()));
        w.visibility_m = 12_000;
        assert!(!changed(w.clone()));
        let mut s = service(Weather {
            visibility_m: 2_000,
            ..wx()
        });
        assert!(
            s.update(
                Weather {
                    visibility_m: 1_400,
                    ..wx()
                },
                &airport(),
                &procs()
            )
            .is_some()
        );
        assert!(
            s.update(
                Weather {
                    visibility_m: 1_400,
                    ..wx()
                },
                &airport(),
                &procs()
            )
            .is_none()
        );
    }

    #[test]
    fn ceiling_category_change() {
        let mut w = wx();
        w.clouds = vec![(CloudCover::Bkn, 4000)];
        assert!(changed(w.clone()));
        w.clouds = vec![(CloudCover::Ovc, 6000)];
        assert!(!changed(w.clone()));
        w.clouds = vec![(CloudCover::Few, 300), (CloudCover::Sct, 400)];
        assert!(!changed(w.clone()));
        w.clouds = vec![(CloudCover::Few, 300), (CloudCover::Bkn, 1000)];
        assert!(changed(w));
    }

    #[test]
    fn atis_text_exact() {
        let s = service(wx());
        assert_eq!(
            s.text("Kraków", Lang::En),
            "Kraków information Alpha, runway in use two five, wind two four zero degrees one zero knots, visibility one zero kilometres or more, few clouds two thousand five hundred feet, temperature one five, dew point eight, QNH one zero one three, on initial contact advise you have information Alpha"
        );
    }

    #[test]
    fn atis_text_variants() {
        let w = Weather {
            wind: Wind {
                dir_deg: 0,
                speed_kt: 0,
                calm: true,
            },
            visibility_m: 4_850,
            clouds: vec![
                (CloudCover::Sct, 1000),
                (CloudCover::Bkn, 800),
                (CloudCover::Ovc, 12000),
            ],
            temp_c: -3,
            dewpoint_c: -5,
            qnh_hpa: 998,
        };
        let t = service(w).text("Kraków", Lang::En);
        assert!(t.contains("wind calm,"), "{t}");
        assert!(
            t.contains("visibility four thousand eight hundred metres,"),
            "{t}"
        );
        assert!(
            t.contains("scattered clouds one thousand feet, broken clouds eight hundred feet, overcast one two thousand feet,"),
            "{t}"
        );
        assert!(
            t.contains(
                "temperature minus three, dew point minus five, QNH zero niner niner eight,"
            ),
            "{t}"
        );
        let w = Weather {
            visibility_m: 7_400,
            clouds: vec![],
            ..wx()
        };
        let t = service(w).text("Kraków", Lang::En);
        assert!(
            t.contains("visibility seven kilometres, no significant cloud,"),
            "{t}"
        );
    }

    #[test]
    fn polish_atis_text() {
        let w = Weather {
            wind: wind(250, 8),
            clouds: vec![],
            temp_c: 16,
            dewpoint_c: -24,
            ..wx()
        };
        assert_eq!(
            service(w).text("Kraków", Lang::Pl),
            "Kraków, informacja Alfa, pas w użyciu dwa pięć, wiatr dwieście pięćdziesiąt stopni osiem węzłów, widzialność dziesięć kilometrów lub więcej, brak chmur znaczących, temperatura szesnaście, punkt rosy minus dwadzieścia cztery, QNH dziesięć trzynaście, przy pierwszym kontakcie potwierdź odbiór informacji Alfa"
        );
    }

    #[test]
    fn polish_atis_variants() {
        let w = Weather {
            wind: Wind {
                dir_deg: 0,
                speed_kt: 0,
                calm: true,
            },
            visibility_m: 5_400,
            clouds: vec![
                (CloudCover::Few, 1000),
                (CloudCover::Sct, 2500),
                (CloudCover::Bkn, 800),
                (CloudCover::Ovc, 12000),
            ],
            ..wx()
        };
        let t = service(w).text("Kraków", Lang::Pl);
        assert!(t.contains("wiatr cisza,"), "{t}");
        assert!(t.contains("widzialność pięć kilometrów,"), "{t}");
        assert!(
            t.contains("nieliczne tysiąc stóp, rozproszone dwa tysiące pięćset stóp, poprzerywane osiemset stóp, całkowite dwanaście tysięcy stóp,"),
            "{t}"
        );
        let w = Weather {
            visibility_m: 800,
            ..wx()
        };
        let t = service(w).text("Kraków", Lang::Pl);
        assert!(t.contains("widzialność osiemset metrów,"), "{t}");
    }
}
