//! Polish text normalization: Whisper output -> the canonical English tokens the scanner
//! reads. Polish phrases are rewritten by a table; English words pass through, so mixed
//! transmissions ("holding point pas dwa pięć, ready for departure") are understood.
//! Diacritics are optional: the input and the table are compared folded to ASCII.

use super::numbers::{DIGITS, HUNDREDS, TEENS, TENS};
use crate::normalize::{Token, normalize as tokenize};

/// What must follow a phrase for it to take its meaning (one pause comma allowed before a
/// letter or a number).
#[derive(Clone, Copy)]
enum Next {
    Any,
    /// A taxiway letter: "drogą Alfa".
    Letter,
    /// A number: "pas dwa pięć".
    Number,
    /// No word or number: "czekam, Sierra Bravo Charlie" (not "czekam na ...").
    End,
}

/// Polish phrases (lowercase, inflected forms listed) and the canonical English words, both
/// space-separated.
const RULES: &[(&str, &str, Next)] = &[
    // Stations.
    ("wieża", "tower", Next::Any),
    ("wieży", "tower", Next::Any),
    ("wieżo", "tower", Next::Any),
    ("ziemia", "ground", Next::Any),
    // Requests.
    ("proszę o kołowanie", "request taxi", Next::Any),
    ("prośba o kołowanie", "request taxi", Next::Any),
    ("proszę o pomoc", "request progressive taxi", Next::Any),
    ("proszę o zezwolenie", "request clearance", Next::Any),
    ("prośba o zezwolenie", "request clearance", Next::Any),
    ("proszę o uruchomienie", "request start up", Next::Any),
    ("proszę o wypychanie", "request push back", Next::Any),
    ("proszę o wektory", "request vectors", Next::Any),
    ("proszę o podejście", "request approach", Next::Any),
    ("proszę o wejście", "request joining", Next::Any),
    ("proszę o wyjście", "request leaving", Next::Any),
    ("pełną procedurę", "full procedure", Next::Any),
    ("proszę o", "request", Next::Any),
    ("prośba o", "request", Next::Any),
    ("proszę powtórzyć", "say again", Next::Any),
    ("powtórz", "say again", Next::Any),
    ("nie zrozumiałem", "say again", Next::Any),
    ("nie zrozumiałam", "say again", Next::Any),
    ("zgubiłem się", "request progressive taxi", Next::Any),
    ("zgubiłam się", "request progressive taxi", Next::Any),
    ("zgubiłem", "request progressive taxi", Next::Any),
    ("zgubiłam", "request progressive taxi", Next::Any),
    ("nie mogę znaleźć", "request progressive taxi", Next::Any),
    ("do kręgów", "for circuits", Next::Any),
    ("do kręgu", "for circuits", Next::Any),
    ("na kręgi", "for circuits", Next::Any),
    ("na krąg", "for circuits", Next::Any),
    ("mam informację", "information", Next::Any),
    ("z informacją", "information", Next::Any),
    ("informacja", "information", Next::Any),
    ("informację", "information", Next::Any),
    ("informacji", "information", Next::Any),
    ("do stanowiska", "to stand", Next::Any),
    ("na stanowisko", "to stand", Next::Any),
    ("na stanowisku", "on stand", Next::Any),
    ("gotowy do startu", "ready for departure", Next::Any),
    ("gotowy do odlotu", "ready for departure", Next::Any),
    ("gotowa do startu", "ready for departure", Next::Any),
    ("gotowa do odlotu", "ready for departure", Next::Any),
    ("do startu", "for departure", Next::Any),
    // Runway, holding point, route.
    (
        "będę oczekiwać przed pasem",
        "hold short runway",
        Next::Number,
    ),
    ("oczekuję przed pasem", "hold short runway", Next::Number),
    ("oczekuj przed pasem", "hold short runway", Next::Number),
    ("oczekiwać przed pasem", "hold short runway", Next::Number),
    ("przed pasem", "hold short runway", Next::Number),
    ("pas", "runway", Next::Number),
    ("pasem", "runway", Next::Number),
    ("pasa", "runway", Next::Number),
    ("punkt oczekiwania", "holding point", Next::Any),
    ("punktu oczekiwania", "holding point", Next::Any),
    ("punkcie oczekiwania", "holding point", Next::Any),
    ("drogą kołowania", "via", Next::Letter),
    ("drogą", "via", Next::Letter),
    ("przez", "via", Next::Letter),
    ("kołuję", "taxi", Next::Any),
    ("kołujemy", "taxi", Next::Any),
    ("kołuj", "taxi", Next::Any),
    ("kołować", "taxi", Next::Any),
    ("kołowanie", "taxi", Next::Any),
    ("kołowania", "taxi", Next::Any),
    ("skłok", "squawk", Next::Any),
    ("skwok", "squawk", Next::Any),
    ("kod", "squawk", Next::Number),
    ("stoję", "holding position", Next::Any),
    ("stój", "holding position", Next::Any),
    ("czekam", "holding position", Next::End),
    // Clearances.
    ("zezwalam startować", "cleared for takeoff", Next::Any),
    ("zezwalasz startować", "cleared for takeoff", Next::Any),
    ("zezwalam na start", "cleared for takeoff", Next::Any),
    ("zezwolenie na start", "cleared for takeoff", Next::Any),
    ("start dozwolony", "cleared for takeoff", Next::Any),
    ("zezwalam lądować", "cleared to land", Next::Any),
    ("zezwalasz lądować", "cleared to land", Next::Any),
    ("zezwalam na lądowanie", "cleared to land", Next::Any),
    ("zezwolenie na lądowanie", "cleared to land", Next::Any),
    ("konwojer", "touch and go", Next::Any),
    ("dotyk i start", "touch and go", Next::Any),
    ("przetnij", "cross", Next::Any),
    ("przecinam", "cross", Next::Any),
    ("przecinamy", "cross", Next::Any),
    ("przetnę", "cross", Next::Any),
    ("zajmij", "line up", Next::Any),
    ("zajmuję", "line up", Next::Any),
    ("zajmujemy", "line up", Next::Any),
    ("i czekaj", "and wait", Next::Any),
    ("i czekam", "and wait", Next::Any),
    ("przejdź na drugi krąg", "going around", Next::Any),
    ("przechodzę na drugi krąg", "going around", Next::Any),
    ("odchodzę na drugi krąg", "going around", Next::Any),
    ("na drugi krąg", "going around", Next::Any),
    ("drugi krąg", "going around", Next::Any),
    ("łączność z", "contact", Next::Any),
    ("kontakt z", "contact", Next::Any),
    ("zgłoszę", "report", Next::Any),
    ("zgłoś", "report", Next::Any),
    // Circuit.
    ("z wiatrem", "downwind", Next::Any),
    ("pod wiatr", "upwind", Next::Any),
    ("boczna", "crosswind", Next::Any),
    ("boczną", "crosswind", Next::Any),
    ("bocznej", "crosswind", Next::Any),
    ("bejs", "base", Next::Any),
    ("baza", "base", Next::Any),
    ("prosta", "final", Next::Any),
    ("prostą", "final", Next::Any),
    ("prostej", "final", Next::Any),
    ("z pełnym zatrzymaniem", "full stop", Next::Any),
    ("do pełnego zatrzymania", "full stop", Next::Any),
    // Instrument procedures and levels.
    ("odlot", "departure", Next::Any),
    ("odlotem", "departure", Next::Any),
    ("dolot", "arrival", Next::Any),
    ("dolotem", "arrival", Next::Any),
    ("przylot", "arrival", Next::Any),
    ("bezpośrednio", "direct", Next::Any),
    ("podejście", "approach", Next::Any),
    ("podejścia", "approach", Next::Any),
    ("ustalony", "established", Next::Any),
    ("ustalona", "established", Next::Any),
    ("wznoszenie na", "climb", Next::Any),
    ("wznoś się na", "climb", Next::Any),
    ("schodzenie na", "descend", Next::Any),
    ("wysokość", "altitude", Next::Number),
    ("poziom lotu", "flight level", Next::Number),
    ("stóp", "feet", Next::Any),
    ("stopy", "feet", Next::Any),
    ("nad", "over", Next::Any),
    ("zbliżam się", "inbound", Next::Any),
    // Numbers.
    ("kropka", "decimal", Next::Any),
    ("przecinek", "decimal", Next::Any),
    ("numer", "number", Next::Number),
    // Acknowledgements and courtesy, as their English counterparts.
    ("przyjąłem", "roger", Next::Any),
    ("przyjęłam", "roger", Next::Any),
    ("przyjęte", "roger", Next::Any),
    ("wykonuję", "roger", Next::Any),
    ("zrozumiałem", "roger", Next::Any),
    ("zrozumiałam", "roger", Next::Any),
    ("dzień dobry", "good morning", Next::Any),
    ("dobry wieczór", "good evening", Next::Any),
    ("cześć", "hello", Next::Any),
    ("witam", "hello", Next::Any),
    ("do widzenia", "bye", Next::Any),
    ("do usłyszenia", "bye", Next::Any),
    ("miłego dnia", "nice day", Next::Any),
    ("miłego lotu", "nice flight", Next::Any),
    ("dziękuję", "thanks", Next::Any),
    ("dzięki", "thanks", Next::Any),
    ("yyy", "uh", Next::Any),
    ("eee", "uh", Next::Any),
];

/// A word as said (lowercase) and the canonical words its phrase became (`None` when it
/// passed through unchanged).
pub(crate) type Said = (String, Option<&'static str>);

/// Polish letters folded to ASCII ("kołuję" -> "koluje").
pub(crate) fn fold(w: &str) -> String {
    w.chars()
        .map(|c| match c {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            c => c,
        })
        .collect()
}

/// Tokenizes a Polish transmission: Polish phrases become the canonical English words the
/// scanner expects, English words pass through, digits and Polish number words become
/// `Number`, ICAO letters `Letter`. A comma between digits is a decimal comma ("123,255").
pub fn normalize(text: &str) -> Vec<Token> {
    normalize_said(text).0
}

/// [`normalize`] together with the words as said, each with the meaning it was given.
pub(crate) fn normalize_said(text: &str) -> (Vec<Token>, Vec<Said>) {
    rewrite(&numbers(tokenize(&decimal_comma(text))))
}

/// "123,255" -> "123.255": Polish writes the decimal point as a comma.
fn decimal_comma(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let between_digits = c == ','
                && i > 0
                && chars[i - 1].is_ascii_digit()
                && chars.get(i + 1).is_some_and(char::is_ascii_digit);
            if between_digits { '.' } else { c }
        })
        .collect()
}

/// A Polish number word: its value and rank (1 unit or teen, 2 tens, 3 hundreds, 0 zero,
/// 4 thousand).
fn number_word(w: &str) -> Option<(u32, u8)> {
    let w = fold(w);
    let find = |table: &[&str; 10]| table.iter().position(|t| !t.is_empty() && fold(t) == w);
    if let Some(d) = find(&DIGITS) {
        return Some((d as u32, u8::from(d > 0)));
    }
    if let Some(t) = find(&TEENS) {
        return Some((10 + t as u32, 1));
    }
    if let Some(t) = find(&TENS) {
        return Some((10 * t as u32, 2));
    }
    if let Some(h) = find(&HUNDREDS) {
        return Some((100 * h as u32, 3));
    }
    matches!(w.as_str(), "tysiac" | "tysiace" | "tysiecy").then_some((1000, 4))
}

/// Merges digits and Polish number words into `Number` tokens. A cardinal is read as one
/// group ("sto osiemnaście" = 118, "trzy tysiące czterysta" = 3400); groups and digits said
/// one after another are concatenated ("dwa pięć" = 25, "dziesięć trzynaście" = 1013).
fn numbers(toks: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(toks.len());
    let mut groups: Vec<String> = Vec::new();
    // The cardinal being read: its value and the rank of its last word. Saturating: a
    // pilot may say any run of words ("tysiąc tysiąc tysiąc tysiąc pięć").
    let mut cur: Option<(u32, u8)> = None;
    let flush = |groups: &mut Vec<String>, cur: &mut Option<(u32, u8)>, out: &mut Vec<Token>| {
        if let Some((v, _)) = cur.take() {
            groups.push(v.to_string());
        }
        if !groups.is_empty() {
            out.push(Token::Number(groups.concat()));
            groups.clear();
        }
    };
    for t in toks {
        let word = match &t {
            Token::Word(w) => number_word(w),
            _ => None,
        };
        match (t, word) {
            (Token::Number(d), _) => {
                if let Some((v, _)) = cur.take() {
                    groups.push(v.to_string());
                }
                groups.push(d);
            }
            (_, Some((_, 4))) => {
                let mult = match cur.take() {
                    Some((v, _)) => v,
                    None => groups.pop().and_then(|d| d.parse().ok()).unwrap_or(1),
                };
                cur = Some((mult.saturating_mul(1000), 4));
            }
            (_, Some((v, rank))) => {
                // A smaller word continues the cardinal: hundreds take tens, teens or units;
                // tens take units; thousands take anything but zero.
                let extends =
                    cur.is_some_and(|(_, last)| rank > 0 && rank < last && (last != 2 || v < 10));
                match cur {
                    Some((c, _)) if extends => cur = Some((c.saturating_add(v), rank)),
                    _ => {
                        if let Some((c, _)) = cur.take() {
                            groups.push(c.to_string());
                        }
                        if rank == 0 {
                            groups.push(v.to_string());
                        } else {
                            cur = Some((v, rank));
                        }
                    }
                }
            }
            (t, None) => {
                flush(&mut groups, &mut cur, &mut out);
                out.push(t);
            }
        }
    }
    flush(&mut groups, &mut cur, &mut out);
    out
}

/// Whether `next` is satisfied at token `k`.
fn next_ok(toks: &[Token], k: usize, next: Next) -> bool {
    let after_pause = |k: usize| {
        if toks.get(k) == Some(&Token::Break) {
            k + 1
        } else {
            k
        }
    };
    match next {
        Next::Any => true,
        Next::Letter => matches!(toks.get(after_pause(k)), Some(Token::Letter(_))),
        Next::Number => matches!(toks.get(after_pause(k)), Some(Token::Number(_))),
        Next::End => !matches!(toks.get(k), Some(Token::Word(_) | Token::Number(_))),
    }
}

/// The longest rule matching at token `i`.
fn rule_at(toks: &[Token], i: usize) -> Option<(usize, &'static str)> {
    RULES
        .iter()
        .filter_map(|(pl, en, next)| {
            let len = pl.split(' ').count();
            let said = pl
                .split(' ')
                .enumerate()
                .all(|(k, p)| match toks.get(i + k) {
                    Some(Token::Word(w)) => fold(w) == fold(p),
                    _ => false,
                });
            (said && next_ok(toks, i + len, *next)).then_some((len, *en))
        })
        .max_by_key(|(len, _)| *len)
}

/// Rewrites Polish phrases into canonical English words.
fn rewrite(toks: &[Token]) -> (Vec<Token>, Vec<Said>) {
    let (mut out, mut said) = (Vec::with_capacity(toks.len()), Vec::new());
    let mut i = 0;
    while i < toks.len() {
        if let Some((len, en)) = rule_at(toks, i) {
            for t in &toks[i..i + len] {
                if let Token::Word(w) = t {
                    said.push((w.clone(), Some(en)));
                }
            }
            out.extend(en.split(' ').map(|w| Token::Word(w.to_string())));
            i += len;
            continue;
        }
        match &toks[i] {
            Token::Word(w) => {
                said.push((w.clone(), None));
                out.push(Token::Word(fold(w)));
            }
            t => out.push(t.clone()),
        }
        i += 1;
    }
    (out, said)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(s: &str) -> Vec<Token> {
        normalize(s)
    }

    fn n(d: &str) -> Token {
        Token::Number(d.into())
    }

    #[test]
    fn number_words() {
        assert_eq!(num("dwa pięć"), vec![n("25")]);
        assert_eq!(num("dwadzieścia pięć"), vec![n("25")]);
        assert_eq!(num("dziesięć trzynaście"), vec![n("1013")]);
        assert_eq!(num("dziesięć zero osiem"), vec![n("1008")]);
        assert_eq!(num("dziewięć dziewięć osiem"), vec![n("998")]);
        assert_eq!(num("sto osiemnaście"), vec![n("118")]);
        assert_eq!(num("dwieście pięćdziesiąt"), vec![n("250")]);
        assert_eq!(num("tysiąc"), vec![n("1000")]);
        assert_eq!(num("tysiąc osiemset"), vec![n("1800")]);
        assert_eq!(num("trzy tysiące czterysta"), vec![n("3400")]);
        assert_eq!(num("dwanaście tysięcy"), vec![n("12000")]);
        assert_eq!(num("siedem tysięcy"), vec![n("7000")]);
        assert_eq!(num("7 tysięcy"), vec![n("7000")]);
        assert_eq!(num("cztery dwa zero jeden"), vec![n("4201")]);
        assert_eq!(num("dziesiec trzynascie"), vec![n("1013")]);
        assert_eq!(num("2 5"), vec![n("25")]);
    }

    #[test]
    fn huge_cardinal_saturates() {
        let max = u32::MAX;
        assert_eq!(
            num("tysiąc tysiąc tysiąc tysiąc pięć"),
            vec![n(&max.to_string())]
        );
    }

    #[test]
    fn number_words_agree_with_render() {
        use super::super::numbers::{cardinal, frequency, qnh, squawk};
        for v in [0, 7, 13, 25, 118, 250, 999, 1000, 1800, 3400, 12_000] {
            assert_eq!(num(&cardinal(v)), vec![n(&v.to_string())], "{v}");
        }
        for v in [998, 1000, 1008, 1010, 1013] {
            assert_eq!(num(&qnh(v)), vec![n(&v.to_string())], "{v}");
        }
        for v in [4201, 7000, 7700] {
            assert_eq!(num(&squawk(v)), vec![n(&v.to_string())], "{v}");
        }
        let w = |s: &str| Token::Word(s.into());
        assert_eq!(
            num(&frequency(123_255)),
            vec![n("123"), w("decimal"), n("255")]
        );
    }

    #[test]
    fn decimal_comma_and_rules() {
        let w = |s: &str| Token::Word(s.into());
        assert_eq!(num("123,255"), vec![n("123"), w("decimal"), n("255")]);
        assert_eq!(num("pas, 2 5"), vec![w("runway"), Token::Break, n("25")]);
        // Without a number "pas" is no runway, without a letter "przez" is no route.
        assert_eq!(num("pas"), vec![w("pas")]);
        assert_eq!(num("przez radio"), vec![w("przez"), w("radio")]);
        assert_eq!(num("przez Alfa"), vec![w("via"), Token::Letter('A')]);
        assert_eq!(num("wiatr"), vec![w("wiatr")]);
        assert_eq!(num("Z WIATREM"), vec![w("downwind")]);
        assert_eq!(num("czekam na"), vec![w("czekam"), w("na")]);
    }

    #[test]
    fn rules_mean_something() {
        for (pl, en, _) in RULES {
            let words = |s: &str| s.split(' ').all(|w| !w.is_empty() && w == w.to_lowercase());
            assert!(words(pl) && words(en), "{pl:?}");
        }
    }
}
