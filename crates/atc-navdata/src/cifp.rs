//! X-Plane CIFP (`Resources/default data/CIFP/<ICAO>.dat`) parser: SIDs, STARs and approaches.

use atc_geo::LatLon;

use crate::apt::NavdataError;

/// Kind of a coded procedure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcKind {
    /// Standard instrument departure.
    Sid,
    /// Standard terminal arrival route.
    Star,
    /// Instrument approach (final segment or approach transition).
    Approach,
}

/// Approach type, from the CIFP route type (I / D / R).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApproachKind {
    /// ILS (route type `I`).
    Ils,
    /// VOR/DME (route type `D`).
    VorDme,
    /// RNAV (route type `R`).
    Rnav,
}

/// ARINC 424 path terminator; exactly the leg types the X-Plane default data uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathTerm {
    /// Initial fix.
    If,
    /// Track to fix.
    Tf,
    /// Course to fix.
    Cf,
    /// Direct to fix.
    Df,
    /// Fix to altitude.
    Fa,
    /// Heading to manual termination.
    Vm,
}

/// Altitude restriction of a leg, in feet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AltConstraint {
    /// No restriction.
    None,
    /// At the altitude.
    At(i32),
    /// At or above the altitude.
    AtOrAbove(i32),
    /// At or below the altitude.
    AtOrBelow(i32),
    /// Between two altitudes: at or above `above` and at or below `below`.
    Between {
        /// Lower bound.
        above: i32,
        /// Upper bound.
        below: i32,
    },
}

/// Reference to a fix, resolved later against `earth_fix.dat`/`earth_nav.dat`/`apt.dat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixRef {
    /// Fix identifier (`KK599`, `RW25`, a navaid ident).
    pub ident: String,
    /// ICAO region code (`EP`).
    pub region: String,
    /// Section code: `P` airport, `E` en route, `D` navaid, `G` runway.
    pub section: char,
}

/// One leg of a procedure.
#[derive(Clone, Debug, PartialEq)]
pub struct Leg {
    /// Terminating fix; none for legs such as `VM`.
    pub fix: Option<FixRef>,
    /// Path terminator.
    pub path: PathTerm,
    /// Altitude restriction.
    pub alt: AltConstraint,
    /// Magnetic course in degrees, where the leg gives one.
    pub course_deg: Option<f64>,
    /// Initial approach fix.
    pub iaf: bool,
    /// Final approach fix.
    pub faf: bool,
    /// Missed approach point.
    pub map: bool,
    /// Leg of the missed approach (after the MAP).
    pub missed: bool,
}

/// A procedure (or one transition of it) with its legs in flying order.
#[derive(Clone, Debug, PartialEq)]
pub struct Procedure {
    /// SID, STAR or approach.
    pub kind: ProcKind,
    /// Designator (`SUDO2J`, `I25`).
    pub ident: String,
    /// Transition (`RW25`, an IAF name) or empty for the final approach segment.
    pub transition: String,
    /// Approach type; `None` for SIDs and STARs.
    pub approach: Option<ApproachKind>,
    /// Legs in order.
    pub legs: Vec<Leg>,
}

/// A runway threshold from the `RWY` records.
#[derive(Clone, Debug, PartialEq)]
pub struct CifpRunway {
    /// Runway identifier without the `RW` prefix (`25`).
    pub ident: String,
    /// Threshold position.
    pub threshold: LatLon,
    /// Threshold elevation in feet.
    pub elevation_ft: i32,
}

/// All procedures of one airport.
#[derive(Clone, Debug, PartialEq)]
pub struct Cifp {
    /// Airport ICAO code.
    pub icao: String,
    /// Runways.
    pub runways: Vec<CifpRunway>,
    /// Procedures in file order, one entry per (kind, ident, transition).
    pub procedures: Vec<Procedure>,
}

fn bad(line: usize, reason: impl Into<String>) -> NavdataError {
    NavdataError::Malformed {
        line,
        reason: reason.into(),
    }
}

/// `N50043143` / `E019460514` -> signed degrees (DD MM SS.ss / DDD MM SS.ss).
fn parse_coord(s: &str, deg_digits: usize, line: usize) -> Result<f64, NavdataError> {
    let err = || bad(line, format!("invalid coordinate: {s}"));
    let (sign, digits) = match s.split_at_checked(1).ok_or_else(err)? {
        ("N" | "E", d) => (1.0, d),
        ("S" | "W", d) => (-1.0, d),
        _ => return Err(err()),
    };
    if digits.len() != deg_digits + 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(err());
    }
    let num = |a: usize, b: usize| digits[a..b].parse::<f64>().map_err(|_| err());
    let d = num(0, deg_digits)?;
    let m = num(deg_digits, deg_digits + 2)?;
    let sec = num(deg_digits + 2, deg_digits + 6)? / 100.0;
    Ok(sign * (d + m / 60.0 + sec / 3600.0))
}

fn parse_runway(text: &str, line: usize) -> Result<CifpRunway, NavdataError> {
    let (head, pos) = text
        .split_once(';')
        .ok_or_else(|| bad(line, "RWY record without position"))?;
    let ident = head
        .split(',')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches("RW")
        .to_string();
    let pos: Vec<&str> = pos
        .trim_end_matches(';')
        .split(',')
        .map(str::trim)
        .collect();
    let [lat, lon, elev, ..] = pos[..] else {
        return Err(bad(
            line,
            "RWY position needs latitude, longitude, elevation",
        ));
    };
    Ok(CifpRunway {
        ident,
        threshold: LatLon {
            lat: parse_coord(lat, 2, line)?,
            lon: parse_coord(lon, 3, line)?,
        },
        elevation_ft: elev
            .parse()
            .map_err(|_| bad(line, format!("invalid runway elevation: {elev}")))?,
    })
}

fn path_term(s: &str, line: usize) -> Result<PathTerm, NavdataError> {
    Ok(match s {
        "IF" => PathTerm::If,
        "TF" => PathTerm::Tf,
        "CF" => PathTerm::Cf,
        "DF" => PathTerm::Df,
        "FA" => PathTerm::Fa,
        "VM" => PathTerm::Vm,
        _ => return Err(bad(line, format!("unknown path terminator: {s:?}"))),
    })
}

fn altitude(s: &str, line: usize) -> Result<i32, NavdataError> {
    let (digits, scale) = match s.strip_prefix("FL") {
        Some(fl) => (fl, 100),
        None => (s, 1),
    };
    digits
        .parse::<i32>()
        .map(|v| v * scale)
        .map_err(|_| bad(line, format!("invalid altitude: {s}")))
}

fn constraint(f: &[&str], line: usize) -> Result<AltConstraint, NavdataError> {
    let get = |i: usize| f.get(i).copied().unwrap_or_default();
    let (desc, a1, a2) = (get(22), get(23), get(24));
    if a1.is_empty() {
        return Ok(AltConstraint::None);
    }
    let first = altitude(a1, line)?;
    Ok(match desc {
        "+" => AltConstraint::AtOrAbove(first),
        "-" => AltConstraint::AtOrBelow(first),
        "B" if !a2.is_empty() => {
            let second = altitude(a2, line)?;
            AltConstraint::Between {
                above: first.min(second),
                below: first.max(second),
            }
        }
        // Blank, `J`/`H` (glide-slope intercept) and anything else: at the altitude.
        _ => AltConstraint::At(first),
    })
}

fn parse_leg(f: &[&str], raw_desc: &str, line: usize) -> Result<Leg, NavdataError> {
    let get = |i: usize| f.get(i).copied().unwrap_or_default();
    let fix = (!get(4).is_empty()).then(|| FixRef {
        ident: get(4).to_string(),
        region: get(5).to_string(),
        section: get(6).chars().next().unwrap_or('P'),
    });
    let course_deg = match get(20) {
        "" => None,
        c => Some(
            c.parse::<f64>()
                .map_err(|_| bad(line, format!("invalid course: {c}")))?
                / 10.0,
        ),
    };
    let marker = raw_desc.chars().nth(3);
    Ok(Leg {
        fix,
        path: path_term(get(11), line)?,
        alt: constraint(f, line)?,
        course_deg,
        iaf: marker == Some('A'),
        faf: marker == Some('F'),
        map: marker == Some('M'),
        missed: false,
    })
}

/// Parses the CIFP text of airport `icao`.
pub fn parse_cifp(text: &str, icao: &str) -> Result<Cifp, NavdataError> {
    let mut cifp = Cifp {
        icao: icao.to_string(),
        runways: Vec::new(),
        procedures: Vec::new(),
    };
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let Some((tag, rest)) = raw.split_once(':') else {
            continue;
        };
        let kind = match tag {
            "RWY" => {
                cifp.runways.push(parse_runway(rest, line)?);
                continue;
            }
            "SID" => ProcKind::Sid,
            "STAR" => ProcKind::Star,
            "APPCH" => ProcKind::Approach,
            _ => continue,
        };
        let body = rest.split_once(';').map_or(rest, |(b, _)| b);
        let raw_fields: Vec<&str> = body.split(',').collect();
        if raw_fields.len() < 12 {
            return Err(bad(line, "too few fields"));
        }
        let f: Vec<&str> = raw_fields.iter().map(|s| s.trim()).collect();
        let (ident, transition) = (f[2].to_string(), f[3].to_string());
        let approach = if kind == ProcKind::Approach {
            // Final segments (I/D/R) name the type themselves; transitions (A) use the ident.
            let letter = if f[1] == "A" {
                ident.chars().next()
            } else {
                f[1].chars().next()
            };
            match letter {
                Some('I') => Some(ApproachKind::Ils),
                Some('D') => Some(ApproachKind::VorDme),
                Some('R') => Some(ApproachKind::Rnav),
                _ => continue, // other approach types are not used
            }
        } else {
            None
        };
        let leg = parse_leg(&f, raw_fields[8], line)?;
        let idx = cifp
            .procedures
            .iter()
            .position(|p| p.kind == kind && p.ident == ident && p.transition == transition)
            .unwrap_or_else(|| {
                cifp.procedures.push(Procedure {
                    kind,
                    ident,
                    transition,
                    approach,
                    legs: Vec::new(),
                });
                cifp.procedures.len() - 1
            });
        let legs = &mut cifp.procedures[idx].legs;
        let after_map = legs.iter().any(|l| l.map);
        legs.push(Leg {
            missed: after_map,
            ..leg
        });
    }
    Ok(cifp)
}

/// Runway a procedure of `kind` belongs to: `RW25` -> `25`, approach `I25` -> `25`.
pub(crate) fn approach_runway(ident: &str) -> &str {
    ident.get(1..).unwrap_or_default()
}

impl Cifp {
    fn by_runway(&self, kind: ProcKind, runway: &str) -> Vec<&Procedure> {
        let rw = format!("RW{runway}");
        self.procedures
            .iter()
            .filter(|p| p.kind == kind && (p.transition.is_empty() || p.transition == rw))
            .collect()
    }

    /// SIDs usable from `runway` (`"27"`); a blank transition applies to every runway.
    pub fn sids(&self, runway: &str) -> Vec<&Procedure> {
        self.by_runway(ProcKind::Sid, runway)
    }

    /// STARs ending at `runway` (`"27"`); a blank transition applies to every runway.
    pub fn stars(&self, runway: &str) -> Vec<&Procedure> {
        self.by_runway(ProcKind::Star, runway)
    }

    /// Approaches to `runway` (`"27"`): final segments (transition empty) first, then their
    /// transitions.
    pub fn approaches(&self, runway: &str) -> Vec<&Procedure> {
        let for_runway =
            |p: &&Procedure| p.kind == ProcKind::Approach && approach_runway(&p.ident) == runway;
        let all: Vec<&Procedure> = self.procedures.iter().filter(for_runway).collect();
        let (finals, transitions): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|p| p.transition.is_empty());
        finals.into_iter().chain(transitions).collect()
    }
}
