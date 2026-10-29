//! Finds the apt.dat block X-Plane itself uses for an airport: scenery packs in
//! `scenery_packs.ini` order, then Global Airports, then the default apt.dat. Global apt.dat
//! files are hundreds of MB, so they are streamed and only the wanted block is kept.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use atc_geo::{LatLon, M_PER_NM, distance_m};

use crate::apt::{NavdataError, parse_apt_dat};
use crate::cifp::parse_cifp;
use crate::fixes::{ProcedureSet, resolve, scan_fix_file, scan_nav_file, wanted_of};
use crate::model::{Airport, Destination};

const CUSTOM_SCENERY: &str = "Custom Scenery";
const GLOBAL_AIRPORTS: &str = "Global Airports";
/// XP 11.50+ ini marker for `Custom Scenery/Global Airports`.
const GLOBAL_AIRPORTS_MARKER: &str = "*GLOBAL_AIRPORTS*";

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("airport {icao} not found in X-Plane scenery (searched {searched} apt.dat files)")]
    NotFound { icao: String, searched: usize },
    #[error(
        "no CIFP procedures found for {icao} in the X-Plane install (neither Custom Data/CIFP nor \
         Resources/default data/CIFP has {icao}.dat)"
    )]
    CifpNotFound { icao: String },
    #[error(
        "fix {ident} of the procedures of {icao} is in neither earth_fix.dat nor earth_nav.dat"
    )]
    UnresolvedFix { icao: String, ident: String },
    #[error("malformed CIFP of {icao}: {source}")]
    Cifp {
        icao: String,
        #[source]
        source: NavdataError,
    },
    #[error("read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// One airport's apt.dat block and the file it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAirport {
    /// The apt.dat file.
    pub source: PathBuf,
    /// Airport id on the header row; differs from the ICAO when matched via `1302 icao_code`.
    pub ident: String,
    /// Header row up to (excluding) the next airport header or `99`.
    pub text: String,
}

impl ResolvedAirport {
    /// Parses the block; `Airport::icao` is set to `icao`.
    pub fn parse(&self, icao: &str) -> Result<Airport, NavdataError> {
        let mut airport = parse_apt_dat(&self.text, &self.ident)?;
        airport.icao = icao.to_string();
        Ok(airport)
    }
}

/// Pushes `<pack>/Earth nav data/apt.dat` if it exists and is not listed yet.
fn push_pack(out: &mut Vec<PathBuf>, pack: &Path) {
    let apt = pack.join("Earth nav data").join("apt.dat");
    if apt.is_file() && !out.contains(&apt) {
        out.push(apt);
    }
}

/// Enabled pack paths in ini order, and the folder names the ini mentions at all.
fn read_ini(xp_root: &Path) -> (Vec<PathBuf>, HashSet<OsString>) {
    let ini = xp_root.join(CUSTOM_SCENERY).join("scenery_packs.ini");
    let (mut enabled, mut mentioned) = (Vec::new(), HashSet::new());
    let Ok(text) = std::fs::read_to_string(ini) else {
        return (enabled, mentioned);
    };
    for line in text.lines().map(str::trim) {
        let (on, raw) = if let Some(p) = line.strip_prefix("SCENERY_PACK ") {
            (true, p)
        } else if let Some(p) = line.strip_prefix("SCENERY_PACK_DISABLED ") {
            (false, p)
        } else {
            continue;
        };
        let raw = raw.trim().trim_end_matches(['/', '\\']);
        let pack = match raw {
            "" => continue,
            GLOBAL_AIRPORTS_MARKER => xp_root.join(CUSTOM_SCENERY).join(GLOBAL_AIRPORTS),
            p if Path::new(p).is_absolute() => PathBuf::from(p),
            p => xp_root.join(p),
        };
        if let Some(name) = pack.file_name() {
            mentioned.insert(name.to_os_string());
        }
        if on {
            enabled.push(pack);
        }
    }
    (enabled, mentioned)
}

/// apt.dat files X-Plane consults, highest priority first: Custom Scenery folders the ini does
/// not know yet (X-Plane adds them on top at its next start; all folders when there is no
/// ini), the enabled ini packs in order, Global Airports, the default apt.dat.
pub fn apt_dat_candidates(xp_root: &Path) -> Vec<PathBuf> {
    let custom = xp_root.join(CUSTOM_SCENERY);
    let (enabled, mentioned) = read_ini(xp_root);
    let mut unlisted: Vec<PathBuf> = std::fs::read_dir(&custom)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| !mentioned.contains(&e.file_name()))
        .map(|e| e.path())
        .collect();
    unlisted.sort();

    let mut out = Vec::new();
    for pack in unlisted.iter().chain(&enabled) {
        push_pack(&mut out, pack);
    }
    push_pack(&mut out, &custom.join(GLOBAL_AIRPORTS));
    push_pack(
        &mut out,
        &xp_root.join("Resources/default scenery/default apt dat"),
    );
    out
}

/// Streams `reader` line by line and returns (ident, block text) of the airport `icao`.
pub fn extract_airport(
    mut reader: impl BufRead,
    icao: &str,
) -> std::io::Result<Option<(String, String)>> {
    let mut buffer = Vec::new();
    // Current block only; empty outside a block.
    let mut block = String::new();
    let mut ident = String::new();
    let mut found = false;

    loop {
        buffer.clear();
        if reader.read_until(b'\n', &mut buffer)? == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&buffer);
        let line = line.trim_end_matches(['\r', '\n']);
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(&first) = tokens.first() else {
            continue;
        };

        match first {
            "1" | "16" | "17" => {
                if found {
                    break;
                }
                block.clear();
                ident = tokens.get(4).copied().unwrap_or_default().to_string();
                block.push_str(line);
                block.push('\n');
                found = ident == icao;
            }
            "99" => {
                if found {
                    break;
                }
                block.clear();
            }
            _ if !block.is_empty() => {
                block.push_str(line);
                block.push('\n');
                if tokens[..] == ["1302", "icao_code", icao] {
                    found = true;
                }
            }
            _ => {}
        }
    }

    Ok(found.then(|| (ident, block.trim_end_matches('\n').to_string())))
}

/// The block of `icao` from the highest-priority apt.dat that has it.
pub fn resolve_airport(xp_root: &Path, icao: &str) -> Result<ResolvedAirport, ResolveError> {
    let candidates = apt_dat_candidates(xp_root);
    for path in &candidates {
        let io = |source| ResolveError::Io {
            path: path.clone(),
            source,
        };
        let reader = BufReader::new(File::open(path).map_err(io)?);
        if let Some((ident, text)) = extract_airport(reader, icao).map_err(io)? {
            return Ok(ResolvedAirport {
                source: path.clone(),
                ident,
                text,
            });
        }
    }
    Err(ResolveError::NotFound {
        icao: icao.to_string(),
        searched: candidates.len(),
    })
}

/// `Custom Data/<rel>` if it exists, else `Resources/default data/<rel>` if it exists.
fn data_file(xp_root: &Path, rel: &str) -> Option<PathBuf> {
    [
        xp_root.join("Custom Data").join(rel),
        xp_root.join("Resources/default data").join(rel),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

fn open(path: &Path) -> Result<BufReader<File>, ResolveError> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|source| ResolveError::Io {
            path: path.to_path_buf(),
            source,
        })
}

/// Procedures of `icao` from the install's CIFP, with every fix resolved from `earth_fix.dat`
/// and `earth_nav.dat`. `Custom Data/` overrides `Resources/default data/` per file.
///
/// `ResolveError::CifpNotFound` when no CIFP exists for the airport, `UnresolvedFix` when a leg's
/// fix is in neither file. A missing `earth_fix.dat`/`earth_nav.dat` counts as empty.
pub fn load_procedure_set(xp_root: &Path, icao: &str) -> Result<ProcedureSet, ResolveError> {
    let rel = format!("CIFP/{icao}.dat");
    let cifp_path = data_file(xp_root, &rel).ok_or_else(|| ResolveError::CifpNotFound {
        icao: icao.to_string(),
    })?;
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ResolveError::Io { path, source }
    };
    let text = std::fs::read_to_string(&cifp_path).map_err(io(&cifp_path))?;
    let cifp = parse_cifp(&text, icao).map_err(|source| ResolveError::Cifp {
        icao: icao.to_string(),
        source,
    })?;

    let mut found = wanted_of(&cifp);
    let mut cycle = None;
    if let Some(path) = data_file(xp_root, "earth_fix.dat") {
        cycle = scan_fix_file(open(&path)?, &mut found).map_err(io(&path))?;
    }
    if let Some(path) = data_file(xp_root, "earth_nav.dat") {
        scan_nav_file(open(&path)?, &mut found).map_err(io(&path))?;
    }
    resolve(cifp, &found, cycle.unwrap_or_default())
}

/// "KATOWICE PYRZOWICE" -> "Katowice Pyrzowice". Words that are all upper or all lower case are
/// capitalised; mixed-case words stay as they are. Non-ASCII letters are kept.
fn title_case(name: &str) -> String {
    let word = |w: &str| {
        let has_lower = w.chars().any(char::is_lowercase);
        let has_upper = w.chars().any(char::is_uppercase);
        if has_lower && has_upper {
            return w.to_string();
        }
        let mut out = String::with_capacity(w.len());
        let mut start = true;
        for c in w.chars() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = matches!(c, '-' | '/');
        }
        out
    };
    name.split_whitespace()
        .map(word)
        .collect::<Vec<_>>()
        .join(" ")
}

/// An airport block while it is being read.
struct DestBlock {
    icao: String,
    name: String,
    datum: (Option<f64>, Option<f64>),
    mid: Option<LatLon>,
    elevation_ft: Option<i32>,
}

impl DestBlock {
    fn pos(&self) -> Option<LatLon> {
        match self.datum {
            (Some(lat), Some(lon)) => Some(LatLon { lat, lon }),
            _ => self.mid,
        }
    }
}

/// Streams one apt.dat, passing each land airport to `done` (first occurrence of an ICAO only).
fn scan_destinations(
    mut reader: impl BufRead,
    seen: &mut HashSet<String>,
    mut done: impl FnMut(DestBlock),
) -> std::io::Result<()> {
    let mut finish = |block: Option<DestBlock>, seen: &mut HashSet<String>| {
        if let Some(b) = block
            && seen.insert(b.icao.clone())
        {
            done(b);
        }
    };
    let mut current: Option<DestBlock> = None;
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        if reader.read_until(b'\n', &mut buffer)? == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&buffer);
        let mut tokens = line.split_whitespace();
        match tokens.next() {
            Some("1") => {
                finish(current.take(), seen);
                let t: Vec<&str> = line.split_whitespace().collect();
                if let Some(&icao) = t.get(4) {
                    current = Some(DestBlock {
                        icao: icao.to_string(),
                        name: t[5..].join(" "),
                        datum: (None, None),
                        mid: None,
                        elevation_ft: t.get(1).and_then(|e| e.parse().ok()),
                    });
                }
            }
            Some("16" | "17" | "99") => finish(current.take(), seen),
            Some("100") => {
                if let Some(b) = current.as_mut().filter(|b| b.mid.is_none()) {
                    let t: Vec<f64> = line
                        .split_whitespace()
                        .map(|s| s.parse().unwrap_or(f64::NAN))
                        .collect();
                    if let (Some(&a), Some(&b_), Some(&c), Some(&d)) =
                        (t.get(9), t.get(10), t.get(18), t.get(19))
                        && [a, b_, c, d].iter().all(|v| v.is_finite())
                    {
                        b.mid = Some(LatLon {
                            lat: (a + c) / 2.0,
                            lon: (b_ + d) / 2.0,
                        });
                    }
                }
            }
            Some("1302") => {
                if let Some(b) = current.as_mut() {
                    match (tokens.next(), tokens.next()) {
                        (Some("datum_lat"), Some(v)) => b.datum.0 = v.parse().ok(),
                        (Some("datum_lon"), Some(v)) => b.datum.1 = v.parse().ok(),
                        (Some("icao_code"), Some(v)) => b.icao = v.to_string(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    finish(current.take(), seen);
    Ok(())
}

/// Airports of the apt.dat files X-Plane loads (the search order of [`resolve_airport`], first
/// occurrence of an ICAO wins) within `radius_nm` of `around`, sorted by ICAO. Nothing is
/// excluded, the home airport included. Position: the `1302 datum` or the midpoint of the first
/// runway (airports with neither are skipped). Name: the header-row name, title-cased, unless
/// `names` maps the ICAO to a preferred one.
pub fn load_destinations(
    xp_root: &Path,
    around: LatLon,
    radius_nm: f64,
    names: &[(String, String)],
) -> Result<Vec<Destination>, ResolveError> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for path in apt_dat_candidates(xp_root) {
        let io = |source| ResolveError::Io {
            path: path.clone(),
            source,
        };
        scan_destinations(open(&path)?, &mut seen, |b| {
            let Some(pos) = b.pos() else { return };
            if distance_m(around, pos) / M_PER_NM > radius_nm {
                return;
            }
            let name = names
                .iter()
                .find(|(icao, _)| *icao == b.icao)
                .map_or_else(|| title_case(&b.name), |(_, n)| n.clone());
            out.push(Destination {
                icao: b.icao,
                name,
                pos,
                elevation_ft: b.elevation_ft,
            });
        })
        .map_err(io)?;
    }
    out.sort_by(|a, b| a.icao.cmp(&b.icao));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(text: &str, icao: &str) -> Option<(String, String)> {
        extract_airport(text.as_bytes(), icao).unwrap()
    }

    #[test]
    fn block_ends_at_next_header() {
        let text = "I\n1100 v\n1 10 0 0 AAAA A\n100 x\n1 20 0 0 BBBB B\n100 y\n99\n";
        let (ident, block) = extract(text, "AAAA").unwrap();
        assert_eq!(ident, "AAAA");
        assert_eq!(block, "1 10 0 0 AAAA A\n100 x");
    }

    #[test]
    fn block_ends_at_99() {
        let text = "1 10 0 0 AAAA A\r\n1201 n\r\n99\r\n1300 junk\n";
        assert_eq!(extract(text, "AAAA").unwrap().1, "1 10 0 0 AAAA A\n1201 n");
    }

    #[test]
    fn matches_icao_code_row_and_keeps_header_ident() {
        let text = "1 10 0 0 XAAA A\n1302 city X\n1302 icao_code AAAA\n1201 n\n99\n";
        let (ident, block) = extract(text, "AAAA").unwrap();
        assert_eq!(ident, "XAAA");
        assert!(block.ends_with("1201 n"), "{block}");
    }

    #[test]
    fn seaplane_and_heliport_headers_end_a_block() {
        let text = "1 10 0 0 AAAA A\n100 x\n16 0 0 0 SEAS S\n17 0 0 0 HELI H\n100 h\n";
        assert_eq!(extract(text, "AAAA").unwrap().1, "1 10 0 0 AAAA A\n100 x");
        assert_eq!(extract(text, "HELI").unwrap().1, "17 0 0 0 HELI H\n100 h");
    }

    #[test]
    fn title_case_keeps_non_ascii_and_mixed_case() {
        assert_eq!(title_case("KATOWICE PYRZOWICE"), "Katowice Pyrzowice");
        assert_eq!(title_case("ŁÓDŹ LUBLINEK"), "Łódź Lublinek");
        assert_eq!(title_case("Krakow-Balice"), "Krakow-Balice");
        assert_eq!(title_case("SAINT-DENIS"), "Saint-Denis");
    }

    #[test]
    fn destinations_use_datum_else_runway_midpoint_and_first_occurrence() {
        let text = "1 10 0 0 AAAA ALPHA FIELD\n1302 datum_lat 1.0\n1302 datum_lon 2.0\n\
            100 30 1 0 0.25 1 0 1 09 5.0 5.0 0 0 3 0 0 0 27 5.0 7.0 0 0 3 0 0 0\n\
            1 10 0 0 BBBB BETA\n100 30 1 0 0.25 1 0 1 09 5.0 5.0 0 0 3 0 0 0 27 7.0 7.0 0 0 3 0 0 0\n\
            1 10 0 0 AAAA DUP\n1302 datum_lat 9.0\n1302 datum_lon 9.0\n16 0 0 0 SEAS S\n99\n";
        let mut seen = HashSet::new();
        let mut got = Vec::new();
        scan_destinations(text.as_bytes(), &mut seen, |b| {
            got.push((b.icao.clone(), b.name.clone(), b.pos()));
        })
        .unwrap();
        let p = |lat, lon| Some(LatLon { lat, lon });
        assert_eq!(
            got,
            [
                ("AAAA".into(), "ALPHA FIELD".into(), p(1.0, 2.0)),
                ("BBBB".into(), "BETA".into(), p(6.0, 6.0)),
            ]
        );
    }

    #[test]
    fn absent_airport_is_none() {
        assert_eq!(extract("1 10 0 0 AAAA A\n99\n", "ZZZZ"), None);
        assert_eq!(extract("1302 icao_code ZZZZ\n", "ZZZZ"), None);
    }
}
