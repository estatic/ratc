//! Polish number words per ICAO Doc 9432 (Polish edition) 2.4 and Appendix 1.

pub(crate) const DIGITS: [&str; 10] = [
    "zero",
    "jeden",
    "dwa",
    "trzy",
    "cztery",
    "pięć",
    "sześć",
    "siedem",
    "osiem",
    "dziewięć",
];
pub(crate) const TEENS: [&str; 10] = [
    "dziesięć",
    "jedenaście",
    "dwanaście",
    "trzynaście",
    "czternaście",
    "piętnaście",
    "szesnaście",
    "siedemnaście",
    "osiemnaście",
    "dziewiętnaście",
];
pub(crate) const TENS: [&str; 10] = [
    "",
    "",
    "dwadzieścia",
    "trzydzieści",
    "czterdzieści",
    "pięćdziesiąt",
    "sześćdziesiąt",
    "siedemdziesiąt",
    "osiemdziesiąt",
    "dziewięćdziesiąt",
];
pub(crate) const HUNDREDS: [&str; 10] = [
    "",
    "sto",
    "dwieście",
    "trzysta",
    "czterysta",
    "pięćset",
    "sześćset",
    "siedemset",
    "osiemset",
    "dziewięćset",
];

/// One spoken digit (0..=9).
pub fn digit(d: u32) -> &'static str {
    DIGITS[(d % 10) as usize]
}

/// `n` zero-padded to `width`, spoken digit by digit.
pub fn digits(n: u32, width: usize) -> String {
    format!("{n:0width$}")
        .chars()
        .filter_map(|c| c.to_digit(10))
        .map(digit)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Words for 1..=999 (empty for 0).
fn below_thousand(n: u32) -> Vec<&'static str> {
    let mut out = Vec::new();
    if n >= 100 {
        out.push(HUNDREDS[(n / 100) as usize]);
    }
    let rest = n % 100;
    if rest >= 20 {
        out.push(TENS[(rest / 10) as usize]);
        if !rest.is_multiple_of(10) {
            out.push(DIGITS[(rest % 10) as usize]);
        }
    } else if rest >= 10 {
        out.push(TEENS[(rest - 10) as usize]);
    } else if rest > 0 {
        out.push(DIGITS[rest as usize]);
    }
    out
}

/// Cardinal number 0..=999_999, nominative masculine.
pub fn cardinal(n: u32) -> String {
    if n == 0 {
        return DIGITS[0].to_string();
    }
    let mut words = Vec::new();
    let thousands = (n / 1000) % 1000;
    if thousands == 1 {
        words.push("tysiąc");
    } else if thousands > 1 {
        words.extend(below_thousand(thousands));
        words.push(plural(thousands, "tysiąc", "tysiące", "tysięcy"));
    }
    words.extend(below_thousand(n % 1000));
    words.join(" ")
}

/// Noun form agreeing with `n`: 1 singular, 2-4 (not 12-14) few, else many.
pub fn plural(n: u32, one: &'static str, few: &'static str, many: &'static str) -> &'static str {
    if n == 1 {
        one
    } else if (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100)) {
        few
    } else {
        many
    }
}

/// QNH in hPa: two 2-digit cardinals, three digits one by one, whole thousands as a cardinal.
pub fn qnh(hpa: u16) -> String {
    let n = u32::from(hpa);
    if n < 1000 {
        digits(n, 3)
    } else if n.is_multiple_of(1000) {
        cardinal(n)
    } else {
        let low = n % 100;
        let tail = if low < 10 {
            format!("{} {}", digit(0), digit(low))
        } else {
            cardinal(low)
        };
        format!("{} {}", cardinal(n / 100), tail)
    }
}

/// Transponder code: whole thousands as a cardinal ("siedem tysięcy"), else digit by digit.
pub fn squawk(code: u16) -> String {
    let n = u32::from(code);
    if n != 0 && n.is_multiple_of(1000) {
        let k = n / 1000;
        format!(
            "{} {}",
            cardinal(k),
            plural(k, "tysiąc", "tysiące", "tysięcy")
        )
    } else {
        digits(n, 4)
    }
}

/// Frequency in kHz: MHz as a cardinal, then the decimals one by one
/// (see [`crate::render::frequency_decimals`]).
pub fn frequency(khz: u32) -> String {
    let frac = crate::render::frequency_decimals(khz);
    let spoken: Vec<&str> = frac
        .chars()
        .filter_map(|c| c.to_digit(10))
        .map(digit)
        .collect();
    format!("{} kropka {}", cardinal(khz / 1000), spoken.join(" "))
}

/// Wind as spoken after "wiatr": direction in degrees, then speed in knots.
pub fn wind(dir_deg: u16, speed_kt: u16) -> String {
    let speed = u32::from(speed_kt);
    format!(
        "{} stopni {} {}",
        cardinal(u32::from(dir_deg)),
        cardinal(speed),
        plural(speed, "węzeł", "węzły", "węzłów")
    )
}

/// Altitude or height in feet as a cardinal plus "stopa/stopy/stóp".
pub fn altitude_ft(ft: u32) -> String {
    format!("{} {}", cardinal(ft), plural(ft, "stopa", "stopy", "stóp"))
}
