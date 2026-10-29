//! Initial prompt for Whisper: biases decoding towards aviation phraseology.

/// Builds a short phraseology prompt in `lang` (`"pl"` = Polish, else English). `stations`
/// are e.g. "Krakow Ground" (as spoken in `lang`); `callsigns` are spoken forms. Kept well
/// under Whisper's ~224-token prompt window.
pub fn initial_prompt(
    airport_name: &str,
    stations: &[String],
    callsigns: &[String],
    lang: &str,
) -> String {
    let polish = lang == "pl";
    let mut parts: Vec<String> = Vec::new();
    parts.push(if polish {
        format!("{airport_name}, lotnisko, łączność IFR i VFR.")
    } else {
        format!("{airport_name} airport radio, IFR and VFR.")
    });
    parts.extend(stations.iter().cloned());
    parts.extend(callsigns.iter().cloned());
    parts.push(
        if polish {
            "pas dwa pięć, punkt oczekiwania, oczekuj przed pasem, QNH, zezwalam startować, \
             zezwalam lądować, z wiatrem, prosta, z pełnym zatrzymaniem, \
             kołuj drogą Alfa, Bravo, Charlie, skłok, informacja Alfa, \
             Sierra Papa Alfa Bravo Charlie"
        } else {
            "IFR, request clearance, departure, approach, climb, descend, heading, \
             runway two five, holding point, hold short, QNH, cleared for take-off, \
             cleared to land, line up and wait, downwind, final, full stop, \
             taxi via Alpha, Bravo, Charlie, squawk, information Alpha, \
             Sierra Papa Alpha Bravo Charlie"
        }
        .to_string(),
    );
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_contains_stations_and_is_short() {
        let p = initial_prompt(
            "Krakow",
            &["Krakow Ground".to_string(), "Krakow Tower".to_string()],
            &["Sierra Papa Alpha Bravo Charlie".to_string()],
            "en",
        );
        assert!(p.contains("Krakow Ground"));
        assert!(p.contains("Krakow Tower"));
        assert!(p.contains("hold short"));
        assert!(p.contains("IFR") && !p.contains("VFR radio"), "{p}");
        assert!(p.contains("Sierra Papa Alpha Bravo Charlie"));
        // Token proxy: <= 200 whitespace-separated words (real token count is a bit higher,
        // so also cap characters conservatively).
        assert!(p.split_whitespace().count() <= 200, "{p}");
        assert!(p.chars().count() <= 800, "{p}");
    }

    #[test]
    fn polish_prompt_is_short_and_polish() {
        let p = initial_prompt(
            "Kraków",
            &["Kraków Ground".to_string(), "Kraków Wieża".to_string()],
            &["Sierra Papa Alfa Bravo Charlie".to_string()],
            "pl",
        );
        assert!(
            p.starts_with("Kraków, lotnisko, łączność IFR i VFR."),
            "{p}"
        );
        assert!(p.contains("Kraków Wieża"), "{p}");
        assert!(p.contains("Sierra Papa Alfa Bravo Charlie"), "{p}");
        assert!(p.contains("zezwalam startować"), "{p}");
        assert!(!p.contains("cleared"), "{p}");
        assert!(p.split_whitespace().count() <= 200, "{p}");
        assert!(p.chars().count() <= 800, "{p}");
    }
}
