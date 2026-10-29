//! Text normalization: Whisper output -> tokens.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    /// Lowercase word.
    Word(String),
    /// A run of digits (digit words and digits merged; `thousand` and
    /// `hundred` are evaluated, so "one thousand five hundred" is `1500`).
    Number(String),
    /// Phonetic-alphabet letter (or a letter glued to digits, as in `B2`),
    /// uppercase.
    Letter(char),
    /// Punctuation (comma, period, ...): a boundary that numbers, taxiway
    /// lists and callsigns never span. Hyphens are not breaks.
    Break,
}

const PHONETIC: [(&str, char); 28] = [
    ("alfa", 'A'),
    ("alpha", 'A'),
    ("bravo", 'B'),
    ("charlie", 'C'),
    ("delta", 'D'),
    ("echo", 'E'),
    ("foxtrot", 'F'),
    ("golf", 'G'),
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
];

fn digit_word(w: &str) -> Option<&'static str> {
    Some(match w {
        "zero" => "0",
        "one" => "1",
        "two" => "2",
        "three" | "tree" => "3",
        "four" => "4",
        "five" | "fife" => "5",
        "six" => "6",
        "seven" => "7",
        "eight" => "8",
        "nine" | "niner" => "9",
        _ => return None,
    })
}

fn phonetic(w: &str) -> Option<char> {
    // "zolo"/"zoolo": Whisper's mishearings of "zulu"; "tanga" of "tango".
    if matches!(w, "zulu" | "zolo" | "zoolo") {
        return Some('Z');
    }
    if w == "tanga" {
        return Some('T');
    }
    PHONETIC.iter().find(|(p, _)| *p == w).map(|(_, c)| *c)
}

/// Accumulates a number spoken digit by digit, optionally with
/// `hundred`/`thousand`.
#[derive(Default)]
struct Run {
    digits: String,
    total: u64,
    scaled: bool,
    active: bool,
}

impl Run {
    fn push(&mut self, d: &str) {
        self.digits.push_str(d);
        self.active = true;
    }

    fn scale(&mut self, mult: u64) {
        let v: u64 = if self.digits.is_empty() {
            1
        } else {
            self.digits.parse().unwrap_or(0)
        };
        self.total = self.total.saturating_add(v.saturating_mul(mult));
        self.digits.clear();
        self.scaled = true;
    }

    fn flush(&mut self, out: &mut Vec<Token>) {
        if self.active {
            if self.scaled {
                let rest: u64 = self.digits.parse().unwrap_or(0);
                out.push(Token::Number(self.total.saturating_add(rest).to_string()));
            } else {
                out.push(Token::Number(std::mem::take(&mut self.digits)));
            }
        }
        *self = Run::default();
    }
}

const DIGIT_WORDS: [&str; 10] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];

/// Written registrations: `SP-ABC`, `sp-abc` (1-2 letters, dash, 2-5
/// alphanumerics) or, in mixed-case text, an uppercase `SPABC` of 5-7
/// characters.
fn is_literal_callsign(run: &str, mixed_case: bool) -> bool {
    if !run.is_ascii() || run.eq_ignore_ascii_case("x-ray") || run.eq_ignore_ascii_case("up-wind") {
        return false;
    }
    let parts: Vec<&str> = run.split('-').collect();
    match parts[..] {
        [a, b] => {
            (1..=2).contains(&a.len())
                && a.bytes().all(|c| c.is_ascii_alphabetic())
                && (2..=5).contains(&b.len())
                && b.bytes().all(|c| c.is_ascii_alphanumeric())
        }
        [a] => {
            mixed_case
                && (5..=7).contains(&a.len())
                && a.bytes()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                && a.as_bytes()[0].is_ascii_uppercase()
        }
        _ => false,
    }
}

/// Rewrites written registrations as spoken phonetics so the rest of the
/// pipeline sees `Letter`/`Number` tokens.
fn expand_literal_callsigns(text: &str) -> String {
    let mixed_case = text.chars().any(|c| c.is_ascii_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if is_literal_callsign(run, mixed_case) {
            for c in run.chars().filter(|c| *c != '-') {
                let word = if let Some(d) = c.to_digit(10) {
                    DIGIT_WORDS[d as usize]
                } else if c.eq_ignore_ascii_case(&'z') {
                    "zulu"
                } else {
                    let up = c.to_ascii_uppercase();
                    PHONETIC
                        .iter()
                        .rev()
                        .find(|(_, l)| *l == up)
                        .map_or("", |(w, _)| w)
                };
                out.push(' ');
                out.push_str(word);
            }
            out.push(' ');
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '-' {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Lowercases, turns punctuation and hyphens into spaces and tokenizes.
///
/// `118.1` becomes `118 decimal 1`; `25L` becomes `25 left`; `B2` becomes
/// `Letter('B')`, `2`. Adjacent digits and digit words merge into one
/// `Number`. Punctuation (other than hyphens and the decimal point inside
/// a number) yields `Break`. Written registrations such as `SP-ABC` become
/// letter tokens.
pub fn normalize(text: &str) -> Vec<Token> {
    let lower = expand_literal_callsigns(&hash_numbers(text))
        .to_lowercase()
        .replace("x-ray", "xray");
    let chars: Vec<char> = lower.chars().collect();
    let mut s = String::with_capacity(lower.len());
    for (i, &c) in chars.iter().enumerate() {
        let between_digits = c == '.'
            && i > 0
            && chars[i - 1].is_ascii_digit()
            && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit());
        let thousands_sep = c == ','
            && i > 0
            && chars[i - 1].is_ascii_digit()
            && chars
                .get(i + 1..i + 4)
                .is_some_and(|d| d.iter().all(char::is_ascii_digit))
            && !chars.get(i + 4).is_some_and(char::is_ascii_digit);
        if thousands_sep {
            continue;
        }
        if between_digits {
            s.push_str(" decimal ");
        } else if c.is_alphanumeric() {
            s.push(c);
        } else if c == '-' || c.is_whitespace() {
            s.push(' ');
        } else {
            s.push_str(" \u{1} ");
        }
    }

    let mut out = Vec::new();
    let mut run = Run::default();
    for w in s.split_whitespace() {
        if w == "\u{1}" {
            run.flush(&mut out);
            if out.last() != Some(&Token::Break) {
                out.push(Token::Break);
            }
        } else if w.chars().all(|c| c.is_ascii_digit()) {
            run.push(w);
        } else if let Some(d) = digit_word(w) {
            run.push(d);
        } else if run.active && (w == "hundred" || w == "thousand") {
            run.scale(if w == "hundred" { 100 } else { 1000 });
        } else {
            run.flush(&mut out);
            word_token(w, &mut run, &mut out);
        }
    }
    run.flush(&mut out);
    join_downwind(out)
}

/// Whisper writes "number one" as "#1": `#` glued to digits becomes "number".
fn hash_numbers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '#' && chars.peek().is_some_and(char::is_ascii_digit) {
            out.push_str(" number ");
        } else {
            out.push(c);
        }
    }
    out
}

/// Whisper's ways of splitting "downwind": "down wind", "Dunn, wind", "dawn wind".
fn join_downwind(toks: Vec<Token>) -> Vec<Token> {
    let is = |t: Option<&Token>, words: &[&str]| match t {
        Some(Token::Word(w)) => words.contains(&w.as_str()),
        _ => false,
    };
    let mut out = Vec::with_capacity(toks.len());
    let mut k = 0;
    while k < toks.len() {
        if is(toks.get(k), &["down", "dunn", "dawn"]) {
            let wind = if toks.get(k + 1) == Some(&Token::Break) {
                k + 2
            } else {
                k + 1
            };
            if is(toks.get(wind), &["wind"]) {
                out.push(Token::Word("downwind".into()));
                k = wind + 1;
                continue;
            }
        }
        out.push(toks[k].clone());
        k += 1;
    }
    out
}

fn word_token(w: &str, run: &mut Run, out: &mut Vec<Token>) {
    if let Some(c) = phonetic(w) {
        out.push(Token::Letter(c));
        return;
    }
    let mut cs = w.chars();
    let first = cs.next();
    let rest = cs.as_str();
    // Letter glued to digits: "b2".
    if let Some(f) = first
        && f.is_ascii_alphabetic()
        && !rest.is_empty()
        && rest.chars().all(|c| c.is_ascii_digit())
    {
        out.push(Token::Letter(f.to_ascii_uppercase()));
        run.push(rest);
        return;
    }
    // Digits glued to a runway-suffix letter: "25l".
    if let Some(last) = w.chars().last()
        && matches!(last, 'l' | 'r' | 'c')
        && w.len() > 1
        && w[..w.len() - 1].chars().all(|c| c.is_ascii_digit())
    {
        out.push(Token::Number(w[..w.len() - 1].to_string()));
        let side = match last {
            'l' => "left",
            'r' => "right",
            _ => "center",
        };
        out.push(Token::Word(side.to_string()));
        return;
    }
    // Whisper hears "IFR" as "EFR".
    let w = if w == "efr" { "ifr" } else { w };
    out.push(Token::Word(w.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> Vec<Token> {
        normalize(s)
    }

    #[test]
    fn digits_merge() {
        assert_eq!(n("one niner tree fife"), vec![Token::Number("1935".into())]);
        assert_eq!(n("zero seven"), vec![Token::Number("07".into())]);
        assert_eq!(
            n("one thousand five hundred"),
            vec![Token::Number("1500".into())]
        );
    }

    #[test]
    fn literal_callsigns_and_breaks() {
        let spabc: Vec<Token> = "SPABC".chars().map(Token::Letter).collect();
        assert_eq!(n("SP-ABC"), spabc);
        assert_eq!(n("sp-abc"), spabc);
        assert_eq!(
            n("Ground, SPABC"),
            [vec![Token::Word("ground".into()), Token::Break], spabc].concat()
        );
        assert_eq!(
            n("one, two"),
            vec![
                Token::Number("1".into()),
                Token::Break,
                Token::Number("2".into())
            ]
        );
        assert_eq!(
            n("RUNWAY TWO FIVE."),
            vec![
                Token::Word("runway".into()),
                Token::Number("25".into()),
                Token::Break
            ]
        );
    }

    #[test]
    fn letters_and_words() {
        assert_eq!(
            n("Sierra, X-Ray cessna"),
            vec![
                Token::Letter('S'),
                Token::Break,
                Token::Letter('X'),
                Token::Word("cessna".into())
            ]
        );
        assert_eq!(
            n("118.1"),
            vec![
                Token::Number("118".into()),
                Token::Word("decimal".into()),
                Token::Number("1".into())
            ]
        );
        assert_eq!(
            n("25L"),
            vec![Token::Number("25".into()), Token::Word("left".into())]
        );
    }

    #[test]
    fn whisper_misspellings() {
        let w = |s: &str| Token::Word(s.into());
        for text in ["Dunn, wind", "down wind", "dawn wind", "downwind"] {
            assert_eq!(n(text), vec![w("downwind")], "{text}");
        }
        assert_eq!(
            n("down, wind, full stop"),
            vec![w("downwind"), Token::Break, w("full"), w("stop")]
        );
        assert_eq!(n("wind down"), vec![w("wind"), w("down")]);
        assert_eq!(
            n("#1, report final"),
            vec![
                w("number"),
                Token::Number("1".into()),
                Token::Break,
                w("report"),
                w("final")
            ]
        );
        assert_eq!(n("# 1"), vec![Token::Break, Token::Number("1".into())]);
        assert_eq!(n("via Tanga"), vec![w("via"), Token::Letter('T')]);
        assert_eq!(n("EFR to Katowice")[0], w("ifr"));
    }
}
