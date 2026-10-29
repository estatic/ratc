//! En-route FIS and ACC units from OpenAIP airspaces, written next to the stations.

use atc_navdata::enroute::{EnrouteKind, EnrouteUnitDef, SectorDef};

use atc_navdata::procedures::AirspaceKind;

use crate::defaults::{airspace_kind, title_case};
use crate::generate::{first_khz, fis_name, limit_ft};
use crate::store::CountryData;

/// Units for every FIS (type 33) and ACC (type 27) airspace that has a frequency.
pub fn build_enroute(countries: &[CountryData]) -> Vec<EnrouteUnitDef> {
    let mut units: Vec<EnrouteUnitDef> = Vec::new();
    for a in countries.iter().flat_map(|c| &c.airspaces) {
        let kind = match airspace_kind(a.kind) {
            Some(AirspaceKind::Fis) => EnrouteKind::Fis,
            Some(AirspaceKind::Acc) => EnrouteKind::Acc,
            _ => continue,
        };
        let Some((khz, name)) = first_khz(&a.frequencies) else {
            continue;
        };
        let callsign = match kind {
            EnrouteKind::Fis => format!("{} Information", fis_name(&a.name)),
            EnrouteKind::Acc => title_case(name.unwrap_or(&a.name)),
        };
        // No airfield here, so no field elevation: a height above ground is taken as is.
        let sector = SectorDef {
            name: a.name.clone(),
            lower_ft: limit_ft(&a.lower_limit, 0),
            upper_ft: limit_ft(&a.upper_limit, 0),
            polygon: a.geometry.outer_ring_lat_lon(),
        };
        match units
            .iter_mut()
            .find(|u| u.khz == khz && u.callsign.eq_ignore_ascii_case(&callsign))
        {
            Some(u) => u.sectors.push(sector),
            None => units.push(EnrouteUnitDef {
                callsign,
                khz,
                kind,
                sectors: vec![sector],
            }),
        }
    }
    for u in &mut units {
        u.sectors.sort_by(|a, b| a.name.cmp(&b.name));
    }
    units.sort_by(|a, b| {
        a.callsign
            .to_lowercase()
            .cmp(&b.callsign.to_lowercase())
            .then(a.khz.cmp(&b.khz))
    });
    units
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openaip::OaAirspace;

    fn sector(id: &str, name: &str, kind: u8, freqs: &str, lower: &str) -> OaAirspace {
        serde_json::from_str(&format!(
            r#"{{"_id":"{id}","name":"{name}","type":{kind},
            "lowerLimit":{lower},"upperLimit":{{"value":95,"unit":6,"referenceDatum":2}},
            "frequencies":[{freqs}],
            "geometry":{{"coordinates":[[[20.0,50.0],[21.0,50.0],[21.0,51.0],[20.0,50.0]]]}},
            "country":"ZZ"}}"#
        ))
        .unwrap()
    }

    const GND: &str = r#"{"value":0,"unit":1,"referenceDatum":0}"#;
    const F1: &str = r#"{"value":"120.500","type":0,"name":"TESTLAND INFORMATION"}"#;

    fn data(airspaces: Vec<OaAirspace>) -> CountryData {
        CountryData {
            airports: vec![],
            airspaces,
            points: vec![],
            synced_at: String::new(),
        }
    }

    #[test]
    fn fis_and_acc_sectors_become_units() {
        let acc = r#"{"value":"132.250","type":0,"name":"TESTLAND RADAR"}"#;
        let d = data(vec![
            sector("a", "FIS TESTLAND-NORTH", 33, F1, GND),
            sector(
                "b",
                "ACC TESTLAND",
                27,
                acc,
                r#"{"value":95,"unit":6,"referenceDatum":2}"#,
            ),
        ]);
        let units = build_enroute(&[d]);
        assert_eq!(units.len(), 2);
        let acc = &units[1];
        assert_eq!(
            (acc.callsign.as_str(), acc.khz),
            ("Testland Radar", 132_250)
        );
        assert_eq!(acc.kind, EnrouteKind::Acc);
        assert_eq!(acc.sectors[0].lower_ft, 9500);
        let fis = &units[0];
        assert_eq!(fis.callsign, "Testland Information");
        assert_eq!((fis.khz, fis.kind), (120_500, EnrouteKind::Fis));
        assert_eq!(
            (fis.sectors[0].lower_ft, fis.sectors[0].upper_ft),
            (0, 9500)
        );
        assert_eq!(fis.sectors[0].polygon[0], [50.0, 20.0]);
    }

    #[test]
    fn same_callsign_and_freq_merge() {
        let other = r#"{"value":"121.000","type":0,"name":"TESTLAND INFORMATION"}"#;
        let d = data(vec![
            sector("a", "FIS TESTLAND-SOUTH", 33, F1, GND),
            sector("b", "FIS TESTLAND-NORTH", 33, F1, GND),
            sector("c", "FIS TESTLAND-EAST", 33, other, GND),
        ]);
        let units = build_enroute(&[d]);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].khz, 120_500);
        let names: Vec<_> = units[0].sectors.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["FIS TESTLAND-NORTH", "FIS TESTLAND-SOUTH"]);
        assert_eq!(units[1].khz, 121_000);
    }

    #[test]
    fn sector_without_frequency_skipped() {
        let d = data(vec![
            sector("a", "FIS TESTLAND-NORTH", 33, "", GND),
            sector("b", "TESTLAND CTR", 4, F1, GND),
        ]);
        assert!(build_enroute(&[d]).is_empty());
    }
}
