//! Procedures of one airport with every fix resolved to a position, from the X-Plane
//! `earth_fix.dat`/`earth_nav.dat` and the CIFP runway thresholds. The two big files are
//! streamed and only the fixes the procedures use are kept.

use std::collections::{BTreeMap, HashMap};
use std::io::BufRead;

use atc_geo::{LatLon, distance_m};

use crate::cifp::{ApproachKind, Cifp, CifpRunway, Leg, ProcKind, Procedure};
use crate::xplane::ResolveError;

/// A leg with the position of its fix.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedLeg {
    /// The CIFP leg.
    pub leg: Leg,
    /// Position of the leg's fix; `None` for legs without one (`VM`, `FA`).
    pub pos: Option<LatLon>,
}

/// A procedure (or one transition) with resolved legs.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedProcedure {
    /// The CIFP procedure.
    pub procedure: Procedure,
    /// Its legs, same order as `procedure.legs`.
    pub legs: Vec<ResolvedLeg>,
}

/// All procedures of an airport, resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcedureSet {
    /// Airport ICAO code.
    pub icao: String,
    /// AIRAC cycle (`"1802"`) from the `earth_fix.dat` header; empty if the header has none.
    pub cycle: String,
    /// Runway thresholds from the CIFP.
    pub runways: Vec<CifpRunway>,
    /// Procedures in CIFP order.
    pub procedures: Vec<ResolvedProcedure>,
    /// Position of every fix used by a procedure, by identifier (`RW27` included).
    pub fixes: BTreeMap<String, LatLon>,
}

impl ProcedureSet {
    fn by_runway(&self, kind: ProcKind, runway: &str) -> Vec<&ResolvedProcedure> {
        let rw = format!("RW{runway}");
        self.procedures
            .iter()
            .filter(|p| {
                p.procedure.kind == kind
                    && (p.procedure.transition.is_empty() || p.procedure.transition == rw)
            })
            .collect()
    }

    /// SIDs usable from `runway` (`"27"`); a blank transition applies to every runway.
    pub fn sids(&self, runway: &str) -> Vec<&ResolvedProcedure> {
        self.by_runway(ProcKind::Sid, runway)
    }

    /// STARs ending at `runway` (`"27"`); a blank transition applies to every runway.
    pub fn stars(&self, runway: &str) -> Vec<&ResolvedProcedure> {
        self.by_runway(ProcKind::Star, runway)
    }

    /// Final approach segment of `kind` to `runway` (no transition).
    pub fn approach(&self, runway: &str, kind: ApproachKind) -> Option<&ResolvedProcedure> {
        self.procedures.iter().find(|p| {
            p.procedure.kind == ProcKind::Approach
                && p.procedure.approach == Some(kind)
                && p.procedure.transition.is_empty()
                && crate::cifp::approach_runway(&p.procedure.ident) == runway
        })
    }

    /// Position of any fix used by a procedure of this airport.
    pub fn fix(&self, ident: &str) -> Option<LatLon> {
        self.fixes.get(ident).copied()
    }
}

/// One entry of a fix or navaid file that a procedure asks for.
#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    /// Airport column (`ENRT` or an ICAO code).
    airport: String,
    pos: LatLon,
}

/// Wanted fixes by (ident, region) with what the files offered.
pub(crate) type Wanted = HashMap<(String, String), Vec<Candidate>>;

/// Reads a line at a time, lossily decoded; `f` gets each line without the line break.
fn for_each_line(mut reader: impl BufRead, mut f: impl FnMut(&str) -> bool) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        if reader.read_until(b'\n', &mut buffer)? == 0 {
            return Ok(());
        }
        let line = String::from_utf8_lossy(&buffer);
        if !f(line.trim_end_matches(['\r', '\n'])) {
            return Ok(());
        }
    }
}

fn offer(wanted: &mut Wanted, ident: &str, region: &str, airport: &str, lat: &str, lon: &str) {
    let Some(list) = wanted.get_mut(&(ident.to_string(), region.to_string())) else {
        return;
    };
    if let (Ok(lat), Ok(lon)) = (lat.parse(), lon.parse()) {
        list.push(Candidate {
            airport: airport.to_string(),
            pos: LatLon { lat, lon },
        });
    }
}

/// Data cycle number out of a header line such as `1101 Version - data cycle 1802, ...`.
fn cycle_of(line: &str) -> Option<String> {
    let rest = line.split_once("data cycle ")?.1;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

/// Streams an `earth_fix.dat`, adding the wanted fixes to `wanted`; returns the data cycle.
pub(crate) fn scan_fix_file(
    reader: impl BufRead,
    wanted: &mut Wanted,
) -> std::io::Result<Option<String>> {
    let mut cycle = None;
    for_each_line(reader, |line| {
        // ` lat lon IDENT AIRPORT REGION code`
        if cycle.is_none() {
            cycle = cycle_of(line);
        }
        let t: Vec<&str> = line.split_whitespace().take(5).collect();
        if let [lat, lon, ident, airport, region] = t[..] {
            offer(wanted, ident, region, airport, lat, lon);
        }
        true
    })?;
    Ok(cycle)
}

/// Streams an `earth_nav.dat`, adding the wanted NDB, VOR and DME entries to `wanted`.
pub(crate) fn scan_nav_file(reader: impl BufRead, wanted: &mut Wanted) -> std::io::Result<()> {
    for_each_line(reader, |line| {
        // `type lat lon elev freq range var ident airport region name...`
        let t: Vec<&str> = line.split_whitespace().take(10).collect();
        if let [
            "2" | "3" | "12" | "13",
            lat,
            lon,
            _,
            _,
            _,
            _,
            ident,
            airport,
            region,
        ] = t[..]
        {
            offer(wanted, ident, region, airport, lat, lon);
        }
        true
    })
}

/// Threshold of the runway a fix ident like `RW27` names, if the CIFP has that runway (the
/// section code of such fixes is `P`, subsection `G`, so the ident decides).
fn runway_fix(cifp: &Cifp, ident: &str) -> Option<LatLon> {
    let rw = ident.strip_prefix("RW")?;
    cifp.runways
        .iter()
        .find(|r| r.ident == rw)
        .map(|r| r.threshold)
}

/// The wanted (ident, region) keys of a CIFP: every fix that is not a runway.
pub(crate) fn wanted_of(cifp: &Cifp) -> Wanted {
    cifp.procedures
        .iter()
        .flat_map(|p| &p.legs)
        .filter_map(|l| l.fix.as_ref())
        .filter(|f| runway_fix(cifp, &f.ident).is_none())
        .map(|f| ((f.ident.clone(), f.region.clone()), Vec::new()))
        .collect()
}

/// Picks the entry of the airport itself, else the one nearest to `near`.
fn pick(list: &[Candidate], icao: &str, near: Option<LatLon>) -> Option<LatLon> {
    let own: Vec<&Candidate> = list.iter().filter(|c| c.airport == icao).collect();
    let pool: Vec<&Candidate> = if own.is_empty() {
        list.iter().collect()
    } else {
        own
    };
    pool.into_iter()
        .min_by(|a, b| match near {
            Some(n) => distance_m(a.pos, n).total_cmp(&distance_m(b.pos, n)),
            None => std::cmp::Ordering::Equal,
        })
        .map(|c| c.pos)
}

/// Builds the set from a parsed CIFP and the fixes found in the data files.
pub(crate) fn resolve(
    cifp: Cifp,
    found: &Wanted,
    cycle: String,
) -> Result<ProcedureSet, ResolveError> {
    let near = cifp.runways.first().map(|r| r.threshold);
    let mut fixes = BTreeMap::new();
    let mut procedures = Vec::with_capacity(cifp.procedures.len());
    for procedure in &cifp.procedures {
        let mut legs = Vec::with_capacity(procedure.legs.len());
        for leg in &procedure.legs {
            let pos = match &leg.fix {
                None => None,
                Some(f) => {
                    let pos = runway_fix(&cifp, &f.ident).or_else(|| {
                        found
                            .get(&(f.ident.clone(), f.region.clone()))
                            .and_then(|list| pick(list, &cifp.icao, near))
                    });
                    let pos = pos.ok_or_else(|| ResolveError::UnresolvedFix {
                        icao: cifp.icao.clone(),
                        ident: f.ident.clone(),
                    })?;
                    fixes.insert(f.ident.clone(), pos);
                    Some(pos)
                }
            };
            legs.push(ResolvedLeg {
                leg: leg.clone(),
                pos,
            });
        }
        procedures.push(ResolvedProcedure {
            procedure: procedure.clone(),
            legs,
        });
    }
    Ok(ProcedureSet {
        icao: cifp.icao,
        cycle,
        runways: cifp.runways,
        procedures,
        fixes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(keys: &[(&str, &str)]) -> Wanted {
        keys.iter()
            .map(|(i, r)| ((i.to_string(), r.to_string()), Vec::new()))
            .collect()
    }

    #[test]
    fn fix_file_keeps_only_wanted_and_reads_cycle() {
        let text = "I\n1101 Version - data cycle 1802, build 1\n\n \
            50.5 18.4  SUDOL ENRT EP 1\n 1.0 2.0  OTHER ENRT EP 2\n 3.0 4.0  SUDOL ENRT XX 3\n99\n";
        let mut w = wanted(&[("SUDOL", "EP")]);
        let cycle = scan_fix_file(text.as_bytes(), &mut w).unwrap();
        assert_eq!(cycle.as_deref(), Some("1802"));
        assert_eq!(w.len(), 1);
        let list = &w[&("SUDOL".to_string(), "EP".to_string())];
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].pos,
            LatLon {
                lat: 50.5,
                lon: 18.4
            }
        );
    }

    #[test]
    fn nav_file_reads_vor_ndb_dme_only() {
        let text = "I\n1150 Version - data cycle 1802\n\n\
            3  50.0 19.7  787 11280 130 5.000  KAK ENRT EP KRAKOW VOR/DME\n\
            4  51.0 19.0  787 11030 18 270.000 IKK EPKK EP ILS-cat-I\n\
            2  52.0 20.0  100 350 25 0.000  NDB1 EPKK EP SOME NDB\n99\n";
        let mut w = wanted(&[("KAK", "EP"), ("IKK", "EP"), ("NDB1", "EP")]);
        scan_nav_file(text.as_bytes(), &mut w).unwrap();
        let n = |i: &str| w[&(i.to_string(), "EP".to_string())].len();
        assert_eq!((n("KAK"), n("IKK"), n("NDB1")), (1, 0, 1));
    }

    #[test]
    fn own_airport_beats_nearer_enroute_entry() {
        let c = |airport: &str, lat| Candidate {
            airport: airport.into(),
            pos: LatLon { lat, lon: 10.0 },
        };
        let list = [c("ENRT", 10.0), c("ZZZZ", 11.0), c("ZZZZ", 12.0)];
        let near = Some(LatLon {
            lat: 10.0,
            lon: 10.0,
        });
        assert_eq!(pick(&list, "ZZZZ", near).unwrap().lat, 11.0);
        // Nobody matches the airport: the nearest wins.
        assert_eq!(pick(&list, "AAAA", near).unwrap().lat, 10.0);
    }
}
