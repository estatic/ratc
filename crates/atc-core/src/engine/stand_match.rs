//! Stand names as pilots say them: "apron four golf alpha" for the stand `APRON4 GA`.
//!
//! Names are compared on their ASCII letters and digits only, with one leading stand
//! keyword (`STAND`, `PARKING`, `APRON`) optional on either side. Both sides are first
//! put in a canonical spelling: phonetic words and digit words become their letters and
//! digits, Whisper's mis-hearings of "apron" become `APRON`, "for" after an apron is the
//! digit 4 and `1-4` is `14`. When nothing matches exactly, the one stand a single
//! letter off (digits are never guessed) is taken.

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

/// Letters of the ICAO alphabet and the way Whisper writes the ones it gets wrong.
const PHONETIC: [(&str, char); 30] = [
    ("alfa", 'A'),
    ("alpha", 'A'),
    ("bravo", 'B'),
    ("charlie", 'C'),
    ("delta", 'D'),
    ("echo", 'E'),
    ("foxtrot", 'F'),
    ("golf", 'G'),
    ("gulf", 'G'),
    ("hotel", 'H'),
    ("india", 'I'),
    ("juliett", 'J'),
    ("juliet", 'J'),
    ("kilo", 'K'),
    ("lima", 'L'),
    ("mike", 'M'),
    ("november", 'N'),
    ("oscar", 'O'),
    ("papa", 'P'),
    ("quebec", 'Q'),
    ("romeo", 'R'),
    ("sierra", 'S'),
    ("tango", 'T'),
    ("uniform", 'U'),
    ("victor", 'V'),
    ("whiskey", 'W'),
    ("whisky", 'W'),
    ("xray", 'X'),
    ("yankee", 'Y'),
    ("zulu", 'Z'),
];

fn digit_word(w: &str) -> Option<char> {
    Some(match w {
        "zero" => '0',
        "one" => '1',
        "two" => '2',
        "three" | "tree" => '3',
        "four" => '4',
        "five" | "fife" => '5',
        "six" => '6',
        "seven" => '7',
        "eight" => '8',
        "nine" | "niner" => '9',
        _ => return None,
    })
}

fn phonetic(w: &str) -> Option<char> {
    PHONETIC.iter().find(|(p, _)| *p == w).map(|(_, c)| *c)
}

/// "apron" as Whisper writes it.
fn is_apron(w: &str) -> bool {
    matches!(
        w,
        "apron" | "aprone" | "upron" | "aparon" | "apran" | "aprin"
    )
}

/// Lowercase alphabetic and numeric runs of `s`; a hyphen between two digits joins them
/// (`1-4` is `14`), any other punctuation separates.
fn pieces(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.to_lowercase().chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut digits = false;
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_alphanumeric() {
            if !cur.is_empty() && c.is_ascii_digit() != digits {
                out.push(std::mem::take(&mut cur));
            }
            digits = c.is_ascii_digit();
            cur.push(c);
        } else if c == '-'
            && digits
            && !cur.is_empty()
            && chars.get(i + 1).is_some_and(char::is_ascii_digit)
        {
            // Joins the digits on either side.
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The canonical spelling of a stand name or heard stand: uppercase letters and digits,
/// with spoken letters and digits written out and Whisper's "apron" mis-hearings fixed.
pub(crate) fn canon_stand(s: &str) -> String {
    let ps = pieces(s);
    let mut out = String::new();
    let mut after_apron = false;
    let mut i = 0;
    while i < ps.len() {
        let w = ps[i].as_str();
        let mut apron = is_apron(w);
        let mut used = 1;
        // "a prone" for "apron".
        if w == "a" && ps.get(i + 1).is_some_and(|n| n == "prone") {
            apron = true;
            used = 2;
        }
        if apron {
            out.push_str("APRON");
        } else if w == "for" && after_apron {
            out.push('4');
        } else if let Some(d) = digit_word(w) {
            out.push(d);
        } else if let Some(l) = phonetic(w) {
            out.push(l);
        } else if let Some(l) = w
            .strip_prefix('g')
            .and_then(|r| phonetic(r).filter(|_| r.len() > 1))
        {
            // "galpha": the stand's two letters run together.
            out.push('G');
            out.push(l);
        } else {
            out.push_str(&w.to_ascii_uppercase());
        }
        after_apron = apron;
        i += used;
    }
    out
}

/// `a` and `b` differ by exactly one inserted, dropped or changed letter (never a digit),
/// and are long enough that one letter is not most of the name.
fn one_letter_off(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a == b || a.len().min(b.len()) < 5 || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let letter = |c: u8| c.is_ascii_alphabetic();
    let at = a
        .iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()));
    let (rest_a, rest_b) = match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Equal => (&a[at + 1..], &b[at + 1..]),
        std::cmp::Ordering::Greater => (&a[at + 1..], &b[at..]),
        std::cmp::Ordering::Less => (&a[at..], &b[at + 1..]),
    };
    if rest_a != rest_b {
        return false;
    }
    let ca = a.get(at).copied();
    let cb = b.get(at).copied();
    ca.is_none_or(letter) && cb.is_none_or(letter)
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

/// Indices of the stands (canonical names `norm`) that `heard` names.
fn match_heard(norm: &[String], heard: &str) -> Vec<usize> {
    let matching = |h: &str| -> Vec<usize> {
        let exact: Vec<usize> = (0..norm.len()).filter(|&k| norm[k] == h).collect();
        if !exact.is_empty() {
            return exact;
        }
        let hs = strip_stand_keyword(h);
        // "apron stand 14": a second keyword after the first.
        let hs2 = strip_stand_keyword(hs);
        let loose: Vec<usize> = (0..norm.len())
            .filter(|&k| {
                let ss = strip_stand_keyword(&norm[k]);
                [hs, hs2].iter().any(|&x| x == ss || x == norm[k]) || h == ss
            })
            .collect();
        if !loose.is_empty() {
            return loose;
        }
        // A single letter off (a Whisper slip): taken only when one stand is.
        let near: Vec<usize> = (0..norm.len())
            .filter(|&k| {
                one_letter_off(h, &norm[k]) || one_letter_off(hs, strip_stand_keyword(&norm[k]))
            })
            .collect();
        if near.len() == 1 { near } else { Vec::new() }
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
    for len in (keep.min(words.len())..=words.len()).rev() {
        let found = matching(&canon_stand(&words[..len].join(" ")));
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

impl AtcEngine {
    /// The stand `heard` names, as an index into `airport.stands`: among the stands whose
    /// name matches, the nearest free one to aircraft `i`, else the nearest.
    pub(super) fn resolve_stand(&self, i: usize, heard: &str) -> Option<usize> {
        let stands = &self.airport.stands;
        let norm: Vec<String> = stands.iter().map(|s| canon_stand(&s.name)).collect();
        let candidates = match_heard(&norm, heard);
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
    /// started from it. A stand named without a position (an old strip) stands for the
    /// first stand of that name.
    pub(super) fn stand_is_free(&self, i: usize, k: usize) -> bool {
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
                } || s.home_stand.as_deref().is_some_and(|h| {
                    h == st.name
                        && match s.home_stand_at {
                            Some(p) => distance_m(p, st.pos) < 1.0,
                            None => first_of_name(h),
                        }
                }))
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

    fn matches(stands: &[&str], heard: &str) -> Vec<usize> {
        let norm: Vec<String> = stands.iter().map(|s| canon_stand(s)).collect();
        match_heard(&norm, heard)
    }

    const STANDS: [&str; 5] = ["1", "14", "APRON4 GA", "APRON6 GA", "40"];

    /// Whisper's renderings of "apron four Golf Alpha" all name `APRON4 GA`.
    #[test]
    fn apron_four_golf_alpha_through_mishearings() {
        for heard in [
            "a prone for Golf Alpha",
            "Upron for GALPHA",
            "Aparon 4, Gulf Alpha",
            "Apron for Golf Alpha",
            "apron four golf alpha",
            "apron four Golf Alpha",
            "stand apron four Golf Alpha",
            "APRON4 GA",
            "stand Aparon 4 Gulf Alpha",
        ] {
            assert_eq!(matches(&STANDS, heard), vec![2], "{heard}");
        }
        assert_eq!(matches(&STANDS, "apron six golf alpha"), vec![3]);
    }

    #[test]
    fn hyphenated_digits_join() {
        assert_eq!(matches(&STANDS, "stand 1-4"), vec![1]);
        assert_eq!(matches(&STANDS, "stand 14"), vec![1]);
        assert_eq!(matches(&STANDS, "apron stand 14"), vec![1]);
    }

    #[test]
    fn ambiguous_or_unknown_is_not_guessed() {
        assert!(matches(&STANDS, "stand for").is_empty());
        assert!(matches(&STANDS, "stand 99").is_empty());
        // A wrong digit is a different stand, not a slip.
        assert!(matches(&STANDS, "apron five golf alpha").is_empty());
        assert!(matches(&["APRON4 GA"], "apron five golf alpha").is_empty());
    }

    #[test]
    fn one_letter_slip_takes_the_only_near_stand() {
        assert_eq!(
            matches(&["APRON4 GA", "40"], "apron four golf alfa x"),
            vec![0]
        );
        assert_eq!(
            matches(&["APRON4 GA", "APRON4 GB"], "apron four G"),
            Vec::<usize>::new()
        );
    }
}
