//! Line-by-line apt.dat (format 1100/1200) parser.

use std::str::FromStr;

use atc_geo::LatLon;

use crate::model::{
    ActiveZone, Airport, EdgeKind, FreqKind, Frequency, Runway, RunwayEnd, Stand, TaxiEdge,
    TaxiNetwork, TaxiNode, ZoneKind,
};

#[derive(Debug, thiserror::Error)]
pub enum NavdataError {
    #[error("airport {0} not found in apt.dat")]
    AirportNotFound(String),
    #[error("malformed apt.dat line {line}: {reason}")]
    Malformed { line: usize, reason: String },
}

fn bad(line: usize, reason: impl Into<String>) -> NavdataError {
    NavdataError::Malformed {
        line,
        reason: reason.into(),
    }
}

fn field<T: FromStr>(tok: &[&str], idx: usize, what: &str, line: usize) -> Result<T, NavdataError> {
    let s = tok
        .get(idx)
        .ok_or_else(|| bad(line, format!("missing {what}")))?;
    s.parse()
        .map_err(|_| bad(line, format!("invalid {what}: {s}")))
}

fn tail(tok: &[&str], from: usize) -> String {
    tok.get(from..).unwrap_or_default().join(" ")
}

fn freq_kind(offset: u32) -> Option<FreqKind> {
    Some(match offset {
        0 => FreqKind::Atis,
        1 => FreqKind::Unicom,
        2 => FreqKind::Delivery,
        3 => FreqKind::Ground,
        4 => FreqKind::Tower,
        5 => FreqKind::Approach,
        6 => FreqKind::Departure,
        _ => return None,
    })
}

fn parse_end(tok: &[&str], at: usize, line: usize) -> Result<RunwayEnd, NavdataError> {
    Ok(RunwayEnd {
        ident: field(tok, at, "runway ident", line)?,
        threshold: LatLon {
            lat: field(tok, at + 1, "latitude", line)?,
            lon: field(tok, at + 2, "longitude", line)?,
        },
        displaced_m: field(tok, at + 3, "displaced threshold", line)?,
    })
}

/// Parses the airport with the given ICAO code out of an apt.dat text.
pub fn parse_apt_dat(text: &str, icao: &str) -> Result<Airport, NavdataError> {
    let mut airport: Option<Airport> = None;

    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let tok: Vec<&str> = raw.split_whitespace().collect();
        let Some(&code) = tok.first() else { continue };

        if matches!(code, "1" | "16" | "17" | "99") {
            if airport.is_some() {
                break;
            }
            if code != "99" && tok.get(4) == Some(&icao) {
                airport = Some(Airport {
                    icao: icao.to_string(),
                    name: tail(&tok, 5),
                    elevation_ft: field(&tok, 1, "elevation", line)?,
                    runways: Vec::new(),
                    frequencies: Vec::new(),
                    taxi: TaxiNetwork::default(),
                    stands: Vec::new(),
                });
            }
            continue;
        }
        let Some(ap) = airport.as_mut() else { continue };

        match code {
            "100" => ap.runways.push(Runway {
                width_m: field(&tok, 1, "runway width", line)?,
                ends: [parse_end(&tok, 8, line)?, parse_end(&tok, 17, line)?],
            }),
            c if matches!(c.parse::<u32>(), Ok(50..=56 | 1050..=1056)) => {
                let n: u32 = field(&tok, 0, "code", line)?;
                let (offset, scale) = if n >= 1000 {
                    (n - 1050, 1)
                } else {
                    (n - 50, 10)
                };
                if let Some(kind) = freq_kind(offset) {
                    let raw_freq: u32 = field(&tok, 1, "frequency", line)?;
                    ap.frequencies.push(Frequency {
                        kind,
                        khz: raw_freq * scale,
                        name: tail(&tok, 2),
                    });
                }
            }
            "1201" => ap.taxi.nodes.push(TaxiNode {
                id: field(&tok, 4, "node id", line)?,
                pos: LatLon {
                    lat: field(&tok, 1, "latitude", line)?,
                    lon: field(&tok, 2, "longitude", line)?,
                },
                name: tail(&tok, 5),
            }),
            "1202" => {
                let one_way = match tok.get(3).copied() {
                    Some("oneway") => true,
                    Some("twoway") => false,
                    other => return Err(bad(line, format!("invalid direction: {other:?}"))),
                };
                let kind = match tok.get(4).copied() {
                    Some("runway") => EdgeKind::Runway,
                    Some(t) if t == "taxiway" || t.starts_with("taxiway_") => EdgeKind::Taxiway,
                    other => return Err(bad(line, format!("invalid edge type: {other:?}"))),
                };
                ap.taxi.edges.push(TaxiEdge {
                    from: field(&tok, 1, "from node", line)?,
                    to: field(&tok, 2, "to node", line)?,
                    one_way,
                    kind,
                    name: tail(&tok, 5),
                    active_zones: Vec::new(),
                });
            }
            "1204" => {
                let kind = match tok.get(1).copied() {
                    Some("departure") => ZoneKind::Departure,
                    Some("arrival") => ZoneKind::Arrival,
                    Some("ils") => ZoneKind::Ils,
                    other => return Err(bad(line, format!("invalid zone kind: {other:?}"))),
                };
                let runways = tok
                    .get(2)
                    .ok_or_else(|| bad(line, "missing zone runways"))?
                    .split(',')
                    .map(str::to_string)
                    .collect();
                let edge = ap
                    .taxi
                    .edges
                    .last_mut()
                    .ok_or_else(|| bad(line, "active zone without preceding edge"))?;
                edge.active_zones.push(ActiveZone { kind, runways });
            }
            "1300" => ap.stands.push(Stand {
                pos: LatLon {
                    lat: field(&tok, 1, "latitude", line)?,
                    lon: field(&tok, 2, "longitude", line)?,
                },
                heading_deg: field(&tok, 3, "heading", line)?,
                name: tail(&tok, 6),
            }),
            _ => {}
        }
    }

    airport.ok_or_else(|| NavdataError::AirportNotFound(icao.to_string()))
}
