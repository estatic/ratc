//! Token scanner: extracts addressee, callsign, intent and clearance items
//! from a normalized pilot transmission.

use crate::normalize::{Token, normalize};
use crate::pl;
use crate::pl::normalize::fold;
use crate::types::{
    ApproachKind, Callsign, CircuitLeg, ClearanceKind, Courtesy, FlightRules, HeardItems, Lang,
    PilotIntent, PilotMessage,
};

/// What the parser may recognise by name: the data of the airport and the neighbourhood.
#[derive(Clone, Debug, Default)]
pub struct Vocabulary {
    /// Callsigns on frequency (see [`parse_pilot_known`]).
    pub callsigns: Vec<Callsign>,
    /// VFR reporting points, uppercase ("ZULU"). One is a point only after `at`, `via`, `over`,
    /// `from`, `to`, `overhead` (Polish `nad`, `od`, `przy`, `do`), `inbound`, `joining`,
    /// `leaving` or a request word; elsewhere the word stays a letter (taxiway, callsign).
    pub points: Vec<String>,
    /// Fix names of the procedures, uppercase ("SUDOL"). A fix followed by a number and a
    /// letter is a procedure designator.
    pub fixes: Vec<String>,
    /// Place names for destinations: (spoken name, lowercase ASCII; ICAO code). A word
    /// matches the last word of the name when it is the name or begins with its stem (the name
    /// without trailing vowels, at least four letters), so the Polish inflections "Katowic",
    /// "Katowicach" and "Krakowa" are found for "katowice" and "krakow".
    pub places: Vec<(String, String)>,
}

/// Parses a pilot transmission in English or Polish. Polish is rewritten into the canonical
/// English tokens first ([`crate::pl::normalize`]), so both are read by the same scanner.
pub fn parse_pilot(text: &str, lang: Lang) -> PilotMessage {
    parse_pilot_known(text, lang, &[])
}

/// [`parse_pilot`] knowing the callsigns on frequency. The letters and digits opening the
/// message (after the addressee) or ending it, pause commas ignored, are the callsign when
/// they begin, respectively end, with a known callsign in full or abbreviated ("Sierra,
/// Sierra Bravo Charlie" after a route: the first "Sierra" is a taxiway). Otherwise the
/// callsign is found as by [`parse_pilot`].
pub fn parse_pilot_known(text: &str, lang: Lang, known: &[Callsign]) -> PilotMessage {
    let vocab = Vocabulary {
        callsigns: known.to_vec(),
        ..Vocabulary::default()
    };
    parse_pilot_with(text, lang, &vocab)
}

/// [`parse_pilot_known`] with the whole [`Vocabulary`]: callsigns, VFR points, procedure
/// fixes and destination places.
pub fn parse_pilot_with(text: &str, lang: Lang, vocab: &Vocabulary) -> PilotMessage {
    let known = &vocab.callsigns[..];
    let mut msg = PilotMessage {
        addressee: None,
        station_called: false,
        callsign: None,
        intent: PilotIntent::Unknown,
        items: HeardItems::default(),
        greeting: None,
        farewell: None,
        ack: false,
        ack_words: Vec::new(),
        greeting_words: Vec::new(),
        residual: Vec::new(),
        raw: text.to_string(),
    };
    // The words as said, each with the canonical words it was read as (`None`: itself).
    let (toks, said) = match lang {
        Lang::En => {
            let toks = normalize(text);
            let said = words(&toks).into_iter().map(|(_, w)| (w, None)).collect();
            (toks, said)
        }
        Lang::Pl => pl::normalize::normalize_said(text),
    };
    let mut scan = Scan::new(&toks, vocab);
    scan.scan_addressee();
    let found = scan.scan_known_callsign(known);
    scan.scan_items();
    msg.callsign = found.or_else(|| scan.find_callsign());
    msg.intent = scan.intent();
    msg.addressee = scan.addressee;
    msg.station_called = scan.station_called;
    msg.items = scan.items;
    msg.greeting = greeting(&toks);
    msg.farewell = farewell(&toks);
    msg.ack = acknowledges(&toks);
    for (w, read_as) in said {
        let canon: Vec<&str> = read_as.map_or_else(|| vec![w.as_str()], |c| c.split(' ').collect());
        if canon.iter().all(|c| ACK_WORDS.contains(c)) {
            msg.ack_words.push(w);
        } else if canon.iter().all(|c| GREETING_WORDS.contains(c)) {
            msg.greeting_words.push(w);
        } else if let [station @ ("tower" | "ground")] = canon[..] {
            // "Wieża": the station's name as the engine knows it.
            msg.residual.push(station.to_string());
        } else {
            msg.residual.push(w);
        }
    }
    msg
}

/// Words of an acknowledgement or a farewell ("roger", "understood", "have a nice day").
const ACK_WORDS: &[&str] = &[
    "roger",
    "wilco",
    "copy",
    "copied",
    "understand",
    "understood",
    "thanks",
    "thank",
    "you",
    "day",
    "bye",
    "cheers",
    "ok",
    "okay",
    "have",
    "a",
    "nice",
    "great",
    "flight",
    "see",
    "later",
    "safe",
    "pleasant",
    "landing",
    "trip",
];
/// Words of a call that carry no message.
const GREETING_WORDS: &[&str] = &[
    "hello",
    "hi",
    "good",
    "morning",
    "afternoon",
    "evening",
    "uh",
    "um",
    "er",
    "eh",
];

/// The words of `toks` with their token positions.
fn words(toks: &[Token]) -> Vec<(usize, String)> {
    toks.iter()
        .enumerate()
        .filter_map(|(k, t)| match t {
            Token::Word(w) => Some((k, w.clone())),
            _ => None,
        })
        .collect()
}

/// Position of the first letter (the callsign usually), or the end.
fn first_letter(toks: &[Token]) -> usize {
    toks.iter()
        .position(|t| matches!(t, Token::Letter(_)))
        .unwrap_or(toks.len())
}

/// The pilot's greeting, returned as said: "hello", "good morning / afternoon / evening",
/// and "good day" in front of the callsign ("good day" after it is a farewell).
fn greeting(toks: &[Token]) -> Option<Courtesy> {
    let (w, callsign) = (words(toks), first_letter(toks));
    if w.iter().any(|(_, x)| matches!(x.as_str(), "hello" | "hi")) {
        return Some(Courtesy::Hello);
    }
    w.windows(2)
        .find_map(|p| match (p[0].1.as_str(), p[1].1.as_str()) {
            ("good", "morning") => Some(Courtesy::GoodMorning),
            ("good", "afternoon") => Some(Courtesy::GoodAfternoon),
            ("good", "evening") => Some(Courtesy::GoodEvening),
            ("good", "day") if p[1].0 < callsign => Some(Courtesy::GoodDay),
            _ => None,
        })
}

/// The reply to a farewell: a wish ("have a nice day", "safe flight") is returned with
/// "thank you, you too"; "good day", "bye", "cheers", "see you" with "good day". "Good day" in front
/// of the callsign is a greeting, not a farewell.
fn farewell(toks: &[Token]) -> Option<Courtesy> {
    let (w, callsign) = (words(toks), first_letter(toks));
    let wish = w.windows(2).any(|p| {
        matches!(
            (p[0].1.as_str(), p[1].1.as_str()),
            (
                "nice" | "great" | "good" | "safe" | "pleasant",
                "flight" | "landing" | "trip"
            ) | ("nice" | "great" | "pleasant", "day" | "evening")
                | ("have", "a" | "nice" | "good" | "great" | "safe")
        )
    });
    if wish {
        return Some(Courtesy::ThankYouYouToo);
    }
    let bye = w
        .iter()
        .any(|(_, x)| matches!(x.as_str(), "bye" | "cheers"))
        || w.windows(2).any(|p| {
            matches!(
                (p[0].1.as_str(), p[1].1.as_str()),
                ("see", "you") | ("good", "evening")
            ) || ((p[0].1.as_str(), p[1].1.as_str()) == ("good", "day") && p[1].0 > callsign)
        });
    bye.then_some(Courtesy::GoodDay)
}

/// An acknowledgement word anywhere in the transmission.
fn acknowledges(toks: &[Token]) -> bool {
    words(toks).iter().any(|(_, w)| {
        matches!(
            w.as_str(),
            "roger" | "wilco" | "copy" | "copied" | "understand" | "understood"
        )
    })
}

struct Scan<'a> {
    toks: &'a [Token],
    vocab: &'a Vocabulary,
    /// The first approach type named anywhere ("ILS", "VOR DME", "RNAV").
    approach_kind: Option<ApproachKind>,
    /// Tokens already explained by an addressee/item; excluded from
    /// callsign detection.
    used: Vec<bool>,
    items: HeardItems,
    atis: Option<char>,
    addressee: Option<String>,
    station_called: bool,
    /// Index where callsign search starts (just after the addressee).
    start: usize,
}

fn push_unique<T: PartialEq>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) {
        v.push(x);
    }
}

fn is_place(w: &str) -> bool {
    matches!(w, "krakow" | "kraków" | "cracow")
}

/// Any form of "request": "request taxi", "requests taxi", "requesting departure".
fn is_request_word(w: &str) -> bool {
    matches!(w, "request" | "requests" | "requesting" | "requested")
}

/// Asking Ground for the way: "request progressive taxi", "unable to find", "lost",
/// "how to taxi", "say route", "request help".
const TAXI_HELP: &[&[&str]] = &[
    &["progressive"],
    &["taxi", "instructions"],
    &["unable", "to", "find"],
    &["lost"],
    &["how", "to", "taxi"],
    &["how", "to", "get"],
    &["how", "do", "i", "get"],
    &["say", "route"],
    &["help"],
];

/// Parked: "on stand", "on the stand", "on blocks".
const ON_STAND: &[&[&str]] = &[
    &["on", "stand"],
    &["on", "the", "stand"],
    &["at", "the", "stand"],
    &["on", "blocks"],
];

fn is_single_letter(w: &str) -> bool {
    w.len() == 1 && w.as_bytes()[0].is_ascii_alphabetic()
}

/// `said` is the place `name`'s last word or an inflected form of it: it begins with the name
/// without its trailing vowels ("katowic" for "katowice"), at most three letters longer than
/// the name. Names with a stem under four letters must be said as they are.
fn inflected_place(said: &str, name: &str) -> bool {
    let stem = name.trim_end_matches(['a', 'e', 'i', 'o', 'u', 'y']);
    said == name || (stem.len() >= 4 && said.starts_with(stem) && said.len() <= name.len() + 3)
}

fn is_role(w: &str) -> bool {
    matches!(w, "ground" | "tower" | "information")
}

impl<'a> Scan<'a> {
    fn new(toks: &'a [Token], vocab: &'a Vocabulary) -> Self {
        Scan {
            toks,
            vocab,
            approach_kind: None,
            used: vec![false; toks.len()],
            items: HeardItems::default(),
            atis: None,
            addressee: None,
            station_called: false,
            start: 0,
        }
    }

    fn w(&self, i: usize) -> Option<&'a str> {
        match self.toks.get(i) {
            Some(Token::Word(w)) => Some(w.as_str()),
            _ => None,
        }
    }

    fn num(&self, i: usize) -> Option<&'a str> {
        match self.toks.get(i) {
            Some(Token::Number(d)) => Some(d.as_str()),
            _ => None,
        }
    }

    /// A taxiway designator at `i`: one phonetic letter, optionally followed by one digit.
    fn designator(&self, i: usize) -> Option<(String, usize)> {
        if self.used.get(i).copied().unwrap_or(false) {
            return None;
        }
        let Some(Token::Letter(c)) = self.toks.get(i) else {
            return None;
        };
        let mut name = c.to_string();
        let mut end = i + 1;
        if let Some(d) = self.num(end)
            && d.len() == 1
        {
            name.push_str(d);
            end += 1;
        }
        Some((name, end))
    }

    /// Whisper puts a comma at every pause ("QNH, one zero, one three"): a value after a
    /// keyword may follow one Break.
    fn skip_break(&self, j: usize) -> usize {
        if self.toks.get(j) == Some(&Token::Break) {
            j + 1
        } else {
            j
        }
    }

    /// Exactly `len` digits from `j`, read across pause commas between digit groups.
    fn digits(&self, j: usize, len: usize) -> Option<(String, usize)> {
        let mut out = String::new();
        let mut k = j;
        loop {
            out.push_str(self.num(k)?);
            k += 1;
            if out.len() >= len {
                break;
            }
            k = self.skip_break(k);
        }
        (out.len() == len).then_some((out, k))
    }

    /// Number of designators from `j` up to a Break, a word, the callsign or the end; `None` when no
    /// designator starts at `j` or the run ends in something else.
    fn designator_run(&self, j: usize) -> Option<usize> {
        let mut n = 0;
        let mut k = j;
        while let Some((_, e)) = self.designator(k) {
            n += 1;
            k = e;
        }
        let end = self.used.get(k).copied().unwrap_or(false)
            || matches!(self.toks.get(k), None | Some(Token::Break | Token::Word(_)));
        (n > 0 && end).then_some(n)
    }

    fn seq(&self, i: usize, pat: &[&str]) -> bool {
        pat.iter()
            .enumerate()
            .all(|(k, p)| self.w(i + k) == Some(*p))
    }

    fn after_request(&self, i: usize) -> bool {
        i > 0 && self.w(i - 1).is_some_and(is_request_word)
    }

    fn mark(&mut self, from: usize, to: usize) {
        for u in &mut self.used[from..to.min(self.toks.len())] {
            *u = true;
        }
    }

    /// Letters and digits as spelled ("SBC").
    fn spelled(&self, idx: &[usize]) -> String {
        idx.iter()
            .filter_map(|&k| match &self.toks[k] {
                Token::Letter(c) => Some(c.to_string()),
                Token::Number(d) => Some(d.clone()),
                _ => None,
            })
            .collect()
    }

    /// The longest prefix (`front`) or suffix of the letter/digit tokens `idx` spelling a
    /// known callsign, full or abbreviated.
    fn known_run<'i>(
        &self,
        idx: &'i [usize],
        known: &[Callsign],
        front: bool,
    ) -> Option<&'i [usize]> {
        (1..=idx.len()).rev().find_map(|len| {
            let part = if front {
                &idx[..len]
            } else {
                &idx[idx.len() - len..]
            };
            let s = self.spelled(part);
            known
                .iter()
                .any(|c| c.0 == s || c.abbreviated() == s)
                .then_some(part)
        })
    }

    /// Letter/digit tokens from `k` onwards (or backwards), over Breaks, up to a word.
    fn letter_run(&self, mut k: usize, forward: bool) -> Vec<usize> {
        let mut run = Vec::new();
        while k >= self.start && k < self.toks.len() {
            match self.toks[k] {
                Token::Letter(_) | Token::Number(_) => run.push(k),
                Token::Break => {}
                Token::Word(_) => break,
            }
            if forward {
                k += 1;
            } else if k == 0 {
                break;
            } else {
                k -= 1;
            }
        }
        if !forward {
            run.reverse();
        }
        run
    }

    /// The callsign opening or ending the message when it is a known one (see
    /// [`parse_pilot_known`]); its tokens are marked used so no route takes them.
    fn scan_known_callsign(&mut self, known: &[Callsign]) -> Option<String> {
        let lead = self.letter_run(self.start, true);
        let trail = self.letter_run(self.toks.len().saturating_sub(1), false);
        let lead = self.known_run(&lead, known, true).map(<[usize]>::to_vec);
        let trail = self.known_run(&trail, known, false).map(<[usize]>::to_vec);
        for &k in lead.iter().chain(&trail).flatten() {
            self.used[k] = true;
        }
        lead.or(trail).map(|run| self.spelled(&run))
    }

    /// Addressee must be within the first three tokens.
    fn scan_addressee(&mut self) {
        let limit = self.toks.len().min(3);
        for i in 0..limit {
            let Some(w) = self.w(i) else { continue };
            if is_place(w) {
                match self.w(i + 1) {
                    Some(r) if is_role(r) || matches!(r, "approach" | "delivery") => {
                        self.addressee = Some(r.to_string());
                        self.station_called = i == 0;
                        self.mark(i, i + 2);
                        self.start = i + 2;
                    }
                    _ => {
                        self.addressee = Some("krakow".to_string());
                        self.mark(i, i + 1);
                        self.start = i + 1;
                    }
                }
                return;
            }
            // "<name> <station>" opening the message is a call to that station ("Zeta
            // Approach, ..."), unlike "contact Approach" or "request approach".
            let named = i == 1
                && self
                    .w(0)
                    .is_some_and(|n| !is_request_word(n) && !matches!(n, "contact" | "monitor"));
            if is_role(w) || (named && matches!(w, "approach" | "delivery")) {
                self.addressee = Some(w.to_string());
                self.station_called = named;
                self.mark(i, i + 1);
                self.start = i + 1;
                return;
            }
        }
    }

    fn scan_items(&mut self) {
        let toks = self.toks;
        for (i, tok) in toks.iter().enumerate() {
            if self.used[i] {
                continue;
            }
            if self.procedure_at(i) {
                continue;
            }
            match tok {
                Token::Number(d) => self.number_at(i, d),
                Token::Word(w) => self.word_at(i, w),
                Token::Letter(_) => self.point_at(i),
                Token::Break => {}
            }
        }
    }

    /// Runway designator starting at token `j`: number 1..=36 plus optional
    /// left/right/center. Returns the name and the end index.
    fn runway_at(&self, j: usize) -> Option<(String, usize)> {
        let d = self.num(j)?;
        if d.len() > 2 {
            return None;
        }
        let n: u8 = d.parse().ok()?;
        if !(1..=36).contains(&n) {
            return None;
        }
        let (suffix, end) = match self.w(j + 1) {
            Some("left") => ("L", j + 2),
            Some("right") => ("R", j + 2),
            Some("center" | "centre") => ("C", j + 2),
            _ => ("", j + 1),
        };
        Some((format!("{n:02}{suffix}"), end))
    }

    fn kind_at(&mut self, i: usize, kind: ClearanceKind) {
        if !self.after_request(i) {
            push_unique(&mut self.items.kinds, kind);
        }
    }

    fn number_at(&mut self, i: usize, d: &str) {
        if d.len() == 6 {
            if let Ok(f) = d.parse() {
                self.items.freqs_khz.push(f);
                self.mark(i, i + 1);
            }
        } else if d.len() == 3
            && matches!(self.w(i + 1), Some("decimal" | "point"))
            && let Some(frac) = self.num(i + 2)
            && frac.len() <= 3
            && let (Ok(mhz), Ok(k)) = (d.parse::<u32>(), format!("{frac:0<3}").parse::<u32>())
        {
            self.items.freqs_khz.push(mhz * 1000 + k);
            self.mark(i, i + 3);
        }
    }

    fn word_at(&mut self, i: usize, w: &str) {
        match w {
            "runway" => {
                if let Some((r, end)) = self.runway_at(self.skip_break(i + 1)) {
                    push_unique(&mut self.items.runways, r);
                    self.mark(i, end);
                }
            }
            "hold" | "holding" => {
                if self.w(i + 1) == Some("short") {
                    let mut j = self.skip_break(i + 2);
                    if self.w(j) == Some("of") {
                        j += 1;
                    }
                    if self.w(j) == Some("runway") {
                        j = self.skip_break(j + 1);
                    }
                    if let Some((r, end)) = self.runway_at(j) {
                        push_unique(&mut self.items.hold_short, r);
                        self.mark(i, end);
                    }
                } else if self.w(i + 1) == Some("position") {
                    self.kind_at(i, ClearanceKind::HoldPosition);
                }
            }
            // "fiw"/"fia": Whisper's mishearings of "via".
            "via" | "fiw" | "fia" | "taxiway" => {
                // "joining via Zulu": a reporting point, unless the message is about taxiing.
                if w == "via" && self.point_name(i + 1).is_some() && !self.taxi_context() {
                    return;
                }
                let mut j = i + 1;
                if self.w(j) == Some("taxiway") {
                    j += 1;
                }
                while let Some((name, end)) = self.designator(j) {
                    // The same taxiway twice in a row is a stutter, never a route.
                    if self.items.taxiways.last() != Some(&name) {
                        self.items.taxiways.push(name);
                    }
                    j = end;
                    // Transcribers put commas inside the list ("via Sierra, Zulu six Alpha,
                    // hold short"): continue over a Break when one or two designators follow
                    // it. Three or more letters after a comma are a callsign, not taxiways.
                    if self.toks.get(j) == Some(&Token::Break)
                        && let Some(run) = self.designator_run(j + 1)
                        && run <= 2
                    {
                        j += 1;
                    }
                }
                self.mark(i, j);
            }
            "qnh" => {
                if let Some((d, end)) = self.digits(self.skip_break(i + 1), 4)
                    && let Ok(v) = d.parse()
                {
                    self.items.qnh.push(v);
                    self.mark(i, end);
                }
            }
            "squawk" => {
                if let Some((d, end)) = self.digits(self.skip_break(i + 1), 4)
                    && let Ok(v) = d.parse()
                {
                    self.items.squawk.push(v);
                    self.mark(i, end);
                }
            }
            "heading" => {
                if let Some(d) = self.num(i + 1)
                    && d.len() == 3
                    && let Ok(v) = d.parse::<u16>()
                    && v <= 360
                {
                    self.items.headings.push(v);
                    self.mark(i, i + 2);
                }
            }
            "altitude" => {
                if let Some(d) = self.num(i + 1)
                    && let Ok(v) = d.parse()
                {
                    self.items.altitudes_ft.push(v);
                    self.mark(i, i + 2);
                }
            }
            "feet" | "foot" => {
                if i > 0
                    && !self.used[i - 1]
                    && let Some(d) = self.num(i - 1)
                    && let Ok(v) = d.parse()
                {
                    self.items.altitudes_ft.push(v);
                    self.mark(i - 1, i + 1);
                }
            }
            "information" | "atis" => {
                if let Some(Token::Letter(c)) = self.toks.get(i + 1) {
                    self.atis = Some(*c);
                    self.mark(i, i + 2);
                }
            }
            "cleared" => {
                if self.seq(i + 1, &["for", "takeoff"])
                    || self.seq(i + 1, &["for", "take", "off"])
                    || self.seq(i + 1, &["for", "departure"])
                {
                    self.kind_at(i, ClearanceKind::Takeoff);
                } else if self.seq(i + 1, &["to", "land"]) {
                    self.kind_at(i, ClearanceKind::Land);
                }
            }
            "touch" if self.seq(i, &["touch", "and", "go"]) => {
                self.kind_at(i, ClearanceKind::TouchAndGo)
            }
            "line" if self.seq(i, &["line", "up"]) => self.kind_at(i, ClearanceKind::LineUp),
            "cross" | "crossing" if self.w(i + 1) == Some("runway") => {
                self.kind_at(i, ClearanceKind::Cross)
            }
            "going" | "go" if self.w(i + 1) == Some("around") => {
                self.kind_at(i, ClearanceKind::GoAround)
            }
            "contact" => self.kind_at(i, ClearanceKind::Contact),
            "taxi" => self.kind_at(i, ClearanceKind::Taxi),
            "climb" | "climbing" | "descend" | "descending" | "maintain" | "maintaining"
            | "passing" => self.level_after(i + 1),
            "flight" if self.w(i + 1) == Some("level") => {
                self.flight_level(self.skip_break(i + 2), i);
            }
            "fl" => {
                self.flight_level(self.skip_break(i + 1), i);
            }
            "direct" | "proceed" => self.direct_at(i),
            "to" | "do" | "for" | "destination" | "into" => self.destination_at(i),
            "ils" | "vor" | "rnav" | "i" | "r" => self.approach_at(i),
            _ => {}
        }
    }

    /// An altitude after a climb/descend/maintain/passing verb: "climb (to) five thousand".
    /// "altitude N" and "flight level N" are read by their own keywords.
    fn level_after(&mut self, j: usize) {
        let mut j = self.skip_break(j);
        if matches!(self.w(j), Some("to" | "through")) {
            j += 1;
        }
        if let Some(d) = self.num(j)
            && !self.used[j]
            && let Ok(v) = d.parse::<i32>()
            && v >= 500
        {
            push_unique(&mut self.items.altitudes_ft, v);
            self.mark(j, j + 1);
        }
    }

    /// The number of a flight level at `j` (the keyword starts at `from`).
    fn flight_level(&mut self, j: usize, from: usize) {
        if let Some(d) = self.num(j)
            && let Ok(v) = d.parse::<u16>()
            && (10..=600).contains(&v)
        {
            push_unique(&mut self.items.flight_levels, v);
            self.mark(from, j + 1);
        }
    }

    /// "direct (to) SUDOL", "proceed direct SUDOL".
    fn direct_at(&mut self, i: usize) {
        let mut j = i + 1;
        if self.w(i) == Some("proceed") {
            if self.w(j) != Some("direct") {
                return;
            }
            j += 1;
        }
        if matches!(self.w(j), Some("to" | "do")) {
            j += 1;
        }
        if let Some((fix, end)) = self.fix_at(j) {
            push_unique(&mut self.items.directs, fix);
            self.mark(i, end);
        }
    }

    /// The destination after "to", "for", "destination", "into" (Polish "do"): a spelled ICAO
    /// code, a code of a known place, or a place name. Four-letter words are not taken as codes
    /// unless the code belongs to a known place ("to land" is no destination).
    fn destination_at(&mut self, i: usize) {
        if self.items.destination.is_some() {
            return;
        }
        let j = self.skip_break(i + 1);
        let found = self.icao_at(j).or_else(|| self.place_at(j));
        if let Some((icao, end)) = found {
            self.items.destination = Some(icao);
            self.mark(j, end);
        }
    }

    /// Four spelled letters ("echo papa kilo tango"), or one word that is the ICAO code of a
    /// known place.
    fn icao_at(&self, j: usize) -> Option<(String, usize)> {
        let letters: Option<String> = (j..j + 4)
            .map(|k| match self.toks.get(k) {
                Some(Token::Letter(c)) if !self.used[k] => Some(*c),
                _ => None,
            })
            .collect();
        if let Some(code) = letters {
            return Some((code, j + 4));
        }
        let w = self.w(j)?.to_uppercase();
        self.vocab
            .places
            .iter()
            .any(|(_, icao)| icao.to_uppercase() == w)
            .then_some((w, j + 1))
    }

    /// A place name from the vocabulary at `j`; the longest name wins.
    fn place_at(&self, j: usize) -> Option<(String, usize)> {
        self.vocab
            .places
            .iter()
            .filter_map(|(name, icao)| {
                let words: Vec<&str> = name.split(' ').collect();
                let (last, init) = words.split_last()?;
                let said = |k: usize| match self.toks.get(j + k) {
                    Some(Token::Word(w)) if !self.used[j + k] => Some(fold(w)),
                    _ => None,
                };
                for (k, n) in init.iter().enumerate() {
                    if said(k)? != *n {
                        return None;
                    }
                }
                inflected_place(&said(init.len())?, last)
                    .then(|| (words.len(), icao.to_uppercase(), j + words.len()))
            })
            .max_by_key(|(len, ..)| *len)
            .map(|(_, icao, end)| (icao, end))
    }

    /// An approach type at `i` and the end of its words: "ILS", "I L S", "VOR" ("DME" /
    /// "D M E"), "RNAV", "R NAV".
    fn approach_kind_at(&self, i: usize) -> Option<(ApproachKind, usize)> {
        match self.w(i)? {
            "ils" => Some((ApproachKind::Ils, i + 1)),
            "i" if self.seq(i, &["i", "l", "s"]) => Some((ApproachKind::Ils, i + 3)),
            "rnav" => Some((ApproachKind::Rnav, i + 1)),
            "r" if self.seq(i, &["r", "nav"]) => Some((ApproachKind::Rnav, i + 2)),
            "vor" => {
                let j = self.skip_break(i + 1);
                if self.w(j) == Some("dme") {
                    Some((ApproachKind::VorDme, j + 1))
                } else if self.seq(j, &["d", "m", "e"]) {
                    Some((ApproachKind::VorDme, j + 3))
                } else {
                    Some((ApproachKind::VorDme, i + 1))
                }
            }
            _ => None,
        }
    }

    /// "ILS (approach) runway two five": notes the type; with a runway it is an approach item
    /// (and the runway is not listed in `runways`).
    fn approach_at(&mut self, i: usize) {
        let Some((kind, end)) = self.approach_kind_at(i) else {
            return;
        };
        self.approach_kind.get_or_insert(kind);
        let mut j = self.skip_break(end);
        if self.w(j) == Some("approach") {
            j = self.skip_break(j + 1);
        }
        if self.w(j) == Some("runway")
            && let Some((rwy, rend)) = self.runway_at(self.skip_break(j + 1))
        {
            push_unique(&mut self.items.approaches, (kind, rwy));
            self.mark(i, rend);
        } else {
            self.mark(i, end);
        }
    }

    /// The vocabulary point spelled by the token at `i`, uppercase.
    fn point_name(&self, i: usize) -> Option<String> {
        let tok = self.toks.get(i)?;
        if !matches!(tok, Token::Letter(_) | Token::Word(_)) || self.used.get(i) == Some(&true) {
            return None;
        }
        self.vocab
            .points
            .iter()
            .find(|p| normalize(&p.to_lowercase()) == [tok.clone()])
            .map(|p| p.to_uppercase())
    }

    /// A taxi instruction or a holding point is in the message: "via Zulu" is a taxiway.
    fn taxi_context(&self) -> bool {
        (0..self.toks.len()).any(|k| {
            matches!(
                self.w(k),
                Some("taxi" | "taxiway" | "holding" | "hold" | "runway")
            )
        })
    }

    /// A vocabulary point at `i` after a preposition of place or a request word is a VFR point.
    fn point_at(&mut self, i: usize) {
        let Some(name) = self.point_name(i) else {
            return;
        };
        let p = if i > 0 && self.toks[i - 1] == Token::Break {
            i - 1
        } else {
            i
        };
        let after_word = p > 0
            && self.w(p - 1).is_some_and(|w| {
                is_request_word(w)
                    || matches!(
                        w,
                        "at" | "via"
                            | "over"
                            | "from"
                            | "to"
                            | "overhead"
                            | "inbound"
                            | "joining"
                            | "leaving"
                            | "do"
                            | "nad"
                            | "od"
                            | "przy"
                    )
            });
        // "via Zulu five" is a taxiway.
        let taxiway =
            p > 0 && self.w(p - 1) == Some("via") && self.num(i + 1).is_some_and(|d| d.len() == 1);
        if after_word && !taxiway {
            push_unique(&mut self.items.points, name);
            self.mark(i, i + 1);
        }
    }

    /// The fix name spelled by the tokens from `i` (one word, a word split in two by the
    /// transcriber, or letters), uppercase, with the end index; the longest spelling wins.
    fn fix_at(&self, i: usize) -> Option<(String, usize)> {
        let mut spelled = String::new();
        let mut best = None;
        for k in i..self.toks.len().min(i + 7) {
            match &self.toks[k] {
                Token::Word(w) if !self.used[k] => {
                    spelled.extend(w.chars().flat_map(char::to_uppercase))
                }
                Token::Letter(c) if !self.used[k] => spelled.push(*c),
                _ => break,
            }
            if self.vocab.fixes.contains(&spelled) {
                best = Some((spelled.clone(), k + 1));
            }
        }
        best
    }

    /// A procedure designator at `i`: fix, number, letter ("SUDOL two Juliett", "Sudol 2 J",
    /// "Sudol 2J"), then "departure" / "arrival" (Polish "odlot" / "dolot" are rewritten to
    /// these). Without either word it is listed as a departure.
    fn procedure_at(&mut self, i: usize) -> bool {
        let Some((fix, end)) = self.fix_at(i) else {
            return false;
        };
        let j = self.skip_break(end);
        let (number, letter, mut after) = match (self.toks.get(j), self.toks.get(j + 1)) {
            (Some(Token::Number(d)), Some(Token::Letter(c))) => (d.as_str(), *c, j + 2),
            (Some(Token::Number(d)), Some(Token::Word(l))) if is_single_letter(l) => (
                d.as_str(),
                l.to_ascii_uppercase().chars().next().unwrap_or('A'),
                j + 2,
            ),
            (Some(Token::Word(w)), _)
                if w.len() == 2
                    && w.as_bytes()[0].is_ascii_digit()
                    && w.as_bytes()[1].is_ascii_alphabetic() =>
            {
                (
                    &w[..1],
                    w.to_ascii_uppercase().chars().nth(1).unwrap_or('A'),
                    j + 1,
                )
            }
            _ => return false,
        };
        let Ok(number) = number.parse::<u8>() else {
            return false;
        };
        if number == 0 {
            return false;
        }
        let arrival = matches!(self.w(after), Some("arrival" | "arrivals"));
        if arrival || matches!(self.w(after), Some("departure" | "departures")) {
            after += 1;
        }
        let proc = (fix, number, letter);
        if arrival {
            push_unique(&mut self.items.stars, proc);
        } else {
            push_unique(&mut self.items.sids, proc);
        }
        self.mark(i, after);
        true
    }

    /// First series of unexplained letters/digits, after the addressee,
    /// that is at least 3 characters long and contains a letter.
    fn find_callsign(&self) -> Option<String> {
        let mut cur = String::new();
        let mut has_letter = false;
        for i in self.start..=self.toks.len() {
            let tok = if self.used.get(i).copied().unwrap_or(true) {
                None
            } else {
                self.toks.get(i)
            };
            match tok {
                Some(Token::Letter(c)) => {
                    cur.push(*c);
                    has_letter = true;
                }
                Some(Token::Number(d)) => cur.push_str(d),
                // A pause comma inside a spoken callsign ("Sierra Papa Alpha, Bravo Charlie").
                Some(Token::Break)
                    if !cur.is_empty()
                        && !self.used.get(i + 1).copied().unwrap_or(true)
                        && matches!(
                            self.toks.get(i + 1),
                            Some(Token::Letter(_) | Token::Number(_))
                        ) => {}
                _ => {
                    if has_letter && cur.chars().count() >= 3 {
                        return Some(cur);
                    }
                    cur.clear();
                    has_letter = false;
                }
            }
        }
        None
    }

    /// Some word sequence of `pats` anywhere in the message.
    fn any_seq(&self, pats: &[&[&str]]) -> bool {
        (0..self.toks.len()).any(|i| pats.iter().any(|p| self.seq(i, p)))
    }

    /// Asking for a follow-me car: "follow me" ("follow-me"), "marshaller". Not when read
    /// back: "follow the follow-me (car)".
    fn follow_me(&self) -> bool {
        (0..self.toks.len()).any(|i| {
            matches!(self.w(i), Some("marshaller" | "marshaler" | "marshall"))
                || (self.seq(i, &["follow", "me"]) && (i == 0 || self.w(i - 1) != Some("the")))
        })
    }

    /// A request word followed within `window` tokens by one satisfying `pred`.
    fn request_then(&self, window: usize, pred: impl Fn(usize) -> bool) -> bool {
        (0..self.toks.len()).any(|i| {
            self.w(i).is_some_and(is_request_word)
                && (i + 1..self.toks.len().min(i + 1 + window)).any(&pred)
        })
    }

    /// "IFR", "I F R" or "instrument" anywhere.
    fn is_ifr(&self) -> bool {
        (0..self.toks.len()).any(|i| {
            matches!(self.w(i), Some("ifr" | "instrument")) || self.seq(i, &["i", "f", "r"])
        })
    }

    /// "VFR", "V F R" or "visual flight rules".
    fn is_vfr(&self) -> bool {
        (0..self.toks.len()).any(|i| {
            self.w(i) == Some("vfr")
                || self.seq(i, &["v", "f", "r"])
                || self.seq(i, &["visual", "flight", "rules"])
        })
    }

    /// "Request (departure) runway zero seven (for departure)": a request word followed by
    /// "runway" and its number, at once or within 3 tokens with "departure" in the call. Not
    /// a request to take off, stop, cross, backtrack, touch and go, land or line up nor a
    /// circuit report ("downwind runway two seven"; "request take-off runway two seven" is
    /// ready for departure).
    fn departure_runway_request(&self) -> Option<String> {
        let n = self.toks.len();
        let other = (0..n).any(|i| {
            matches!(
                self.w(i),
                Some("takeoff" | "cross" | "crossing" | "backtrack" | "landing")
            ) || self.seq(i, &["take", "off"])
                || self.seq(i, &["full", "stop"])
                || self.seq(i, &["back", "track"])
                || self.seq(i, &["touch", "and", "go"])
                || self.seq(i, &["line", "up"])
                || (matches!(
                    self.w(i),
                    Some("upwind" | "crosswind" | "downwind" | "base" | "final")
                ) && (matches!(self.toks.get(i + 1), None | Some(Token::Break))
                    || self.w(i + 1) == Some("runway")))
        });
        if other {
            return None;
        }
        let departure = (0..n).any(|i| self.w(i) == Some("departure"));
        let asked = (0..n).any(|i| {
            self.w(i).is_some_and(is_request_word)
                && (i + 1..n.min(i + 4)).any(|k| {
                    self.w(k) == Some("runway")
                        && matches!(self.toks.get(k + 1), Some(Token::Number(_)))
                        && (k == i + 1 || departure)
                })
        });
        asked.then(|| self.items.runways.first().cloned()).flatten()
    }

    /// IFR requests and reports, and the VFR joining/leaving requests.
    fn procedure_intent(&self) -> Option<PilotIntent> {
        let n = self.toks.len();
        let has = |words: &[&str]| (0..n).any(|i| self.w(i).is_some_and(|w| words.contains(&w)));
        let point = self.items.points.first().cloned();
        if (0..n).any(|i| {
            self.w(i) == Some("established") && (i == 0 || self.w(i - 1) != Some("report"))
        }) {
            return Some(PilotIntent::ReportEstablished);
        }
        if self.request_then(4, |k| self.w(k) == Some("clearance"))
            || (has(&["clearance"]) && self.is_ifr())
        {
            let level_ft = self.items.altitudes_ft.first().copied().or_else(|| {
                self.items
                    .flight_levels
                    .first()
                    .map(|&fl| i32::from(fl) * 100)
            });
            return Some(PilotIntent::RequestClearance {
                destination: self.items.destination.clone(),
                rules: if self.is_ifr() {
                    Some(FlightRules::Ifr)
                } else if self.is_vfr() {
                    Some(FlightRules::Vfr)
                } else {
                    None
                },
                level_ft,
            });
        }
        if self.request_then(4, |k| {
            matches!(self.w(k), Some("startup" | "push" | "pushback"))
                || self.seq(k, &["start", "up"])
        }) {
            return Some(PilotIntent::RequestStartUp {
                push: has(&["push", "pushback"]),
            });
        }
        if self.request_then(4, |k| matches!(self.w(k), Some("join" | "joining"))) {
            return Some(PilotIntent::RequestJoin { point });
        }
        if self.request_then(4, |k| matches!(self.w(k), Some("leave" | "leaving"))) {
            return Some(PilotIntent::RequestLeave { point });
        }
        if self.request_then(5, |k| {
            matches!(
                self.w(k),
                Some("vectors" | "approach" | "procedure" | "ils" | "vor" | "rnav")
            ) || self.approach_kind_at(k).is_some()
        }) {
            return Some(PilotIntent::RequestApproach {
                kind: self.approach_kind,
                full_procedure: self.any_seq(&[&["full", "procedure"]]),
            });
        }
        // "Inbound" first: a flight from a neighbouring airport reports airborne and inbound.
        if has(&["inbound"]) {
            return Some(PilotIntent::ReportInbound { point });
        }
        if has(&["airborne"]) || (has(&["passing"]) && has(&["climbing"])) {
            return Some(PilotIntent::ReportDeparture);
        }
        None
    }

    fn intent(&self) -> PilotIntent {
        let n = self.toks.len();
        // "say again all after ...": the whole transmission is repeated.
        if self.any_seq(&[&["say", "again"], &["repeat"]]) {
            return PilotIntent::SayAgain;
        }
        if self.follow_me() {
            return PilotIntent::RequestFollowMe;
        }
        if let Some(intent) = self.procedure_intent() {
            return intent;
        }
        if self.any_seq(ON_STAND) {
            return PilotIntent::ReportOnStand;
        }
        if self.any_seq(TAXI_HELP) {
            return PilotIntent::RequestTaxiHelp;
        }
        for i in 0..n {
            if !self.w(i).is_some_and(is_request_word) {
                continue;
            }
            let Some(k) = (i + 1..n.min(i + 4)).find(|&k| self.w(k) == Some("taxi")) else {
                continue;
            };
            let mut s = k + 1;
            if self.w(s) == Some("to") {
                s += 1;
            }
            if matches!(self.w(s), Some("stand" | "parking" | "apron")) {
                let mut stand = String::new();
                for t in self.toks[s + 1..n.min(s + 4)].iter() {
                    match t {
                        Token::Letter(c) => stand.push(*c),
                        Token::Number(d) => stand.push_str(d),
                        Token::Word(_) | Token::Break => break,
                    }
                }
                return PilotIntent::RequestTaxiToStand {
                    stand: (!stand.is_empty()).then_some(stand),
                };
            }
            let vfr_circuits = (0..n).any(|j| matches!(self.w(j), Some("circuit" | "circuits")));
            return PilotIntent::RequestTaxi {
                atis: self.atis,
                vfr_circuits,
            };
        }

        if let Some(runway) = self.departure_runway_request() {
            return PilotIntent::RequestDepartureRunway { runway };
        }

        let ready = (0..n).any(|i| {
            self.seq(i, &["ready", "for", "departure"])
                || self.seq(i, &["ready", "for", "takeoff"])
                || self.seq(i, &["ready", "for", "take", "off"])
                // "request (permission to) take-off / departure": not ICAO, but clear.
                || (self.w(i).is_some_and(|w| is_request_word(w) || w == "permission")
                    && (i + 1..n.min(i + 4)).any(|k| {
                        matches!(self.w(k), Some("takeoff" | "departure"))
                            || self.seq(k, &["take", "off"])
                    }))
        });
        if ready {
            let runway = self
                .items
                .runways
                .first()
                .or_else(|| self.items.hold_short.first())
                .cloned();
            return PilotIntent::ReadyForDeparture { runway };
        }

        // "report final" is a controller instruction being read back, not a
        // position report.
        let leg = (0..n).find_map(|i| {
            let leg = match self.w(i)? {
                "upwind" => CircuitLeg::Upwind,
                "crosswind" => CircuitLeg::Crosswind,
                "downwind" => CircuitLeg::Downwind,
                "base" => CircuitLeg::Base,
                "final" => CircuitLeg::Final,
                _ => return None,
            };
            (i == 0 || self.w(i - 1) != Some("report")).then_some(leg)
        });
        if let Some(leg) = leg {
            return PilotIntent::ReportLeg(leg);
        }

        if (0..n).any(|i| self.seq(i, &["full", "stop"])) {
            return PilotIntent::RequestFullStop;
        }
        if self.items.is_empty() {
            PilotIntent::Unknown
        } else {
            PilotIntent::Readback
        }
    }
}
