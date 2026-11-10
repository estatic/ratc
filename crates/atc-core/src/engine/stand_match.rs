//! Stand names as pilots say them: "apron four golf alpha" for the stand `APRON4 GA`.
//!
//! Names are compared on their ASCII letters and digits only, with one leading stand
//! keyword (`STAND`, `PARKING`, `APRON`) optional on either side.

use atc_geo::distance_m;

use super::AtcEngine;
use super::stand::ON_STAND_M;
use crate::taxi::TaxiTarget;

const KEYWORDS: [&str; 3] = ["STAND", "PARKING", "APRON"];

/// Uppercase ASCII letters and digits of `s`.
pub(crate) fn normalize_stand(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// `n` (normalised) without one leading stand keyword, when something follows it.
pub(crate) fn strip_stand_keyword(n: &str) -> &str {
    for k in KEYWORDS {
        if let Some(rest) = n.strip_prefix(k)
            && !rest.is_empty()
        {
            return rest;
        }
    }
    n
}

/// The heard stand as told back to the pilot: the words after the keyword.
pub(super) fn display_heard(heard: &str) -> String {
    let mut words = heard.split_whitespace();
    let first = words.clone().next().map(normalize_stand);
    if first.is_some_and(|f| KEYWORDS.contains(&f.as_str())) && words.clone().nth(1).is_some() {
        words.next();
    }
    words.collect::<Vec<_>>().join(" ")
}

impl AtcEngine {
    /// The stand `heard` names, as an index into `airport.stands`: among the stands whose
    /// name matches, the nearest free one to aircraft `i`, else the nearest.
    pub(super) fn resolve_stand(&self, i: usize, heard: &str) -> Option<usize> {
        let stands = &self.airport.stands;
        let norm: Vec<String> = stands.iter().map(|s| normalize_stand(&s.name)).collect();
        let matching = |h: &str| -> Vec<usize> {
            let exact: Vec<usize> = (0..stands.len()).filter(|&k| norm[k] == h).collect();
            if !exact.is_empty() {
                return exact;
            }
            let hs = strip_stand_keyword(h);
            (0..stands.len())
                .filter(|&k| {
                    let ss = strip_stand_keyword(&norm[k]);
                    hs == ss || hs == norm[k] || h == ss
                })
                .collect()
        };
        // The whole heard text, else with trailing words dropped one at a time (a callsign
        // or filler after the stand), keeping the keyword and one more word.
        let words: Vec<&str> = heard.split_whitespace().collect();
        let keep = if words
            .first()
            .is_some_and(|w| KEYWORDS.contains(&normalize_stand(w).as_str()))
        {
            2
        } else {
            1
        };
        let mut candidates = Vec::new();
        for len in (keep.min(words.len())..=words.len()).rev() {
            candidates = matching(&normalize_stand(&words[..len].join(" ")));
            if !candidates.is_empty() {
                break;
            }
        }
        let from = self.last_pos(i);
        let dist = |k: usize| from.map_or(0.0, |p| distance_m(p, stands[k].pos));
        let nearest =
            |ks: &mut dyn Iterator<Item = usize>| ks.min_by(|&a, &b| dist(a).total_cmp(&dist(b)));
        nearest(
            &mut candidates
                .iter()
                .copied()
                .filter(|&k| self.stand_is_free(i, k)),
        )
        .or_else(|| nearest(&mut candidates.iter().copied()))
    }

    /// No other tracked aircraft is on stand `k`, and no other strip is headed for it or
    /// started from it.
    fn stand_is_free(&self, i: usize, k: usize) -> bool {
        let st = &self.airport.stands[k];
        let me = &self.strips[i].callsign;
        let occupied = self
            .tracks
            .iter()
            .any(|(cs, t)| cs != me && distance_m(t.last.pos, st.pos) <= ON_STAND_M);
        // A name without a position stands for the first stand of that name.
        let first_of_name = |name: &str| {
            self.airport
                .stands
                .iter()
                .position(|s| s.name == name)
                .is_some_and(|p| p == k)
        };
        let claimed = self.strips.iter().enumerate().any(|(j, s)| {
            j != i
                && (match &s.taxi_target {
                    Some(TaxiTarget::Stand { name, at: Some(p) }) => {
                        *name == st.name && distance_m(*p, st.pos) < 1.0
                    }
                    Some(TaxiTarget::Stand { name, at: None }) => {
                        *name == st.name && first_of_name(name)
                    }
                    _ => false,
                } || s
                    .home_stand
                    .as_deref()
                    .is_some_and(|h| h == st.name && first_of_name(h)))
        });
        !occupied && !claimed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(normalize_stand("APRON4 GA"), "APRON4GA");
        assert_eq!(normalize_stand("apron 4 G A"), "APRON4GA");
    }

    #[test]
    fn strips_one_keyword() {
        assert_eq!(strip_stand_keyword("STAND5"), "5");
        assert_eq!(strip_stand_keyword("APRON"), "APRON");
        assert_eq!(strip_stand_keyword("PARKINGAPRON4"), "APRON4");
    }

    #[test]
    fn displays_without_the_keyword() {
        assert_eq!(display_heard("stand 99"), "99");
        assert_eq!(display_heard("apron 4 G A"), "4 G A");
        assert_eq!(display_heard("stand"), "stand");
        assert_eq!(display_heard("5"), "5");
    }
}
