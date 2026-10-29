//! Field-by-field comparison of two stations.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use atc_navdata::procedures::Procedures;

/// One field that differs between two stations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// Field path, e.g. `vfr_points[ECHO]` or `tma.radius_nm`.
    pub field: String,
    pub left: String,
    pub right: String,
}

impl fmt::Display for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} != {}", self.field, self.left, self.right)
    }
}

const MISSING: &str = "(missing)";
const EPS: f64 = 1e-6;

fn feq(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS
}

fn mhz(khz: u32) -> String {
    format!("{}.{:03}", khz / 1000, khz % 1000)
}

fn opt<T: ToString>(v: &Option<T>) -> String {
    v.as_ref().map_or(MISSING.to_string(), T::to_string)
}

struct Out(Vec<Difference>);

impl Out {
    fn push(
        &mut self,
        field: impl Into<String>,
        left: impl Into<String>,
        right: impl Into<String>,
    ) {
        self.0.push(Difference {
            field: field.into(),
            left: left.into(),
            right: right.into(),
        });
    }

    fn scalar<T: PartialEq + ToString>(&mut self, field: &str, a: &T, b: &T) {
        if a != b {
            self.push(field, a.to_string(), b.to_string());
        }
    }

    fn float(&mut self, field: &str, a: f64, b: f64) {
        if !feq(a, b) {
            self.push(field, a.to_string(), b.to_string());
        }
    }

    /// Compares two lists as sets keyed by `key`; `desc` renders an item for the report.
    fn list<T>(
        &mut self,
        field: &str,
        a: &[T],
        b: &[T],
        key: impl Fn(&T) -> String,
        desc: impl Fn(&T) -> String,
        same: impl Fn(&T, &T) -> bool,
    ) {
        type Group = BTreeMap<String, (String, Vec<usize>)>;
        let index = |v: &'_ [T]| -> Group {
            let mut m: Group = BTreeMap::new();
            for (i, item) in v.iter().enumerate() {
                let k = key(item);
                m.entry(k.to_lowercase())
                    .or_insert((k, Vec::new()))
                    .1
                    .push(i);
            }
            m
        };
        let (ia, ib) = (index(a), index(b));
        let keys: BTreeSet<&String> = ia.keys().chain(ib.keys()).collect();
        for k in keys {
            let (ga, gb) = (ia.get(k), ib.get(k));
            let shown = ga.or(gb).map_or(k.as_str(), |g| g.0.as_str());
            let (ga, gb) = (
                ga.map_or(&[][..], |g| &g.1[..]),
                gb.map_or(&[][..], |g| &g.1[..]),
            );
            let n = ga.len().max(gb.len());
            for i in 0..n {
                let name = if n > 1 {
                    format!("{field}[{shown}#{}]", i + 1)
                } else {
                    format!("{field}[{shown}]")
                };
                match (ga.get(i), gb.get(i)) {
                    (Some(&x), Some(&y)) => {
                        if !same(&a[x], &b[y]) {
                            self.push(name, desc(&a[x]), desc(&b[y]));
                        }
                    }
                    (Some(&x), None) => self.push(name, desc(&a[x]), MISSING),
                    (None, Some(&y)) => self.push(name, MISSING, desc(&b[y])),
                    (None, None) => {}
                }
            }
        }
    }
}

fn polygon_eq(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(p, q)| feq(p[0], q[0]) && feq(p[1], q[1]))
}

/// Differences between `a` (left) and `b` (right); empty when they agree. Lists are compared
/// as sets, floats within 1e-6, and the order of the result is fixed.
pub fn compare(a: &Procedures, b: &Procedures) -> Vec<Difference> {
    // Exhaustive: a new field is a compile error until it is compared below.
    let Procedures {
        icao: a_icao,
        source: a_source,
        airac: a_airac,
        transition_altitude_ft: a_transition_altitude_ft,
        circuits: a_circuits,
        preferred_runway: a_preferred_runway,
        positions: a_positions,
        vfr_points: a_vfr_points,
        vfr_holds: a_vfr_holds,
        vfr_hold_max_ft: a_vfr_hold_max_ft,
        adjacent: a_adjacent,
        tma: a_tma,
        delivery_hours: a_delivery_hours,
        ifr: a_ifr,
        approach_alt_khz: a_approach_alt_khz,
        magnetic_variation_deg: a_magnetic_variation_deg,
        airspaces: a_airspaces,
    } = a;
    let Procedures {
        icao: b_icao,
        source: b_source,
        airac: b_airac,
        transition_altitude_ft: b_transition_altitude_ft,
        circuits: b_circuits,
        preferred_runway: b_preferred_runway,
        positions: b_positions,
        vfr_points: b_vfr_points,
        vfr_holds: b_vfr_holds,
        vfr_hold_max_ft: b_vfr_hold_max_ft,
        adjacent: b_adjacent,
        tma: b_tma,
        delivery_hours: b_delivery_hours,
        ifr: b_ifr,
        approach_alt_khz: b_approach_alt_khz,
        magnetic_variation_deg: b_magnetic_variation_deg,
        airspaces: b_airspaces,
    } = b;
    let mut o = Out(Vec::new());
    o.scalar("icao", a_icao, b_icao);
    o.scalar("source", a_source, b_source);
    o.scalar("airac", a_airac, b_airac);
    o.scalar(
        "transition_altitude_ft",
        a_transition_altitude_ft,
        b_transition_altitude_ft,
    );
    o.list(
        "circuits",
        a_circuits,
        b_circuits,
        |c| c.runway.clone(),
        |c| format!("{:?} {} ft", c.side, c.altitude_ft_msl),
        |x, y| x == y,
    );
    o.scalar("preferred_runway", a_preferred_runway, b_preferred_runway);
    o.list(
        "positions",
        a_positions,
        b_positions,
        |p| format!("{} {:?}", p.khz, p.role),
        |p| format!("{} {:?} {}", mhz(p.khz), p.role, p.callsign),
        |x, y| x == y,
    );
    o.list(
        "vfr_points",
        a_vfr_points,
        b_vfr_points,
        |p| p.name.clone(),
        |p| format!("{} {} south={}", p.lat, p.lon, p.south),
        |x, y| feq(x.lat, y.lat) && feq(x.lon, y.lon) && x.south == y.south,
    );
    o.list(
        "vfr_holds",
        a_vfr_holds,
        b_vfr_holds,
        Clone::clone,
        Clone::clone,
        |_, _| true,
    );
    o.scalar(
        "vfr_hold_max_ft",
        &opt(a_vfr_hold_max_ft),
        &opt(b_vfr_hold_max_ft),
    );
    o.list(
        "adjacent",
        a_adjacent,
        b_adjacent,
        |u| u.callsign.clone(),
        |u| {
            format!(
                "{} {:?} airport={} min_level={}",
                mhz(u.khz),
                u.kind,
                u.airport.as_deref().unwrap_or(MISSING),
                opt(&u.min_level_ft)
            )
        },
        |x, y| x == y,
    );
    match (a_tma, b_tma) {
        (Some(x), Some(y)) => {
            o.float("tma.radius_nm", x.radius_nm, y.radius_nm);
            o.scalar("tma.ceiling_ft", &x.ceiling_ft, &y.ceiling_ft);
        }
        (None, None) => {}
        (x, y) => {
            let show = |t: &Option<atc_navdata::procedures::Tma>| {
                t.map_or(MISSING.to_string(), |t| {
                    format!("{} NM up to {} ft", t.radius_nm, t.ceiling_ft)
                })
            };
            o.push("tma", show(x), show(y));
        }
    }
    match (a_delivery_hours, b_delivery_hours) {
        (Some(x), Some(y)) => {
            o.scalar(
                "delivery_hours.summer_utc",
                &format!("{:?}", x.summer_utc),
                &format!("{:?}", y.summer_utc),
            );
            o.scalar(
                "delivery_hours.winter_utc",
                &format!("{:?}", x.winter_utc),
                &format!("{:?}", y.winter_utc),
            );
        }
        (None, None) => {}
        (x, y) => {
            let show = |t: &Option<atc_navdata::procedures::DeliveryHours>| {
                t.map_or(MISSING.to_string(), |t| {
                    format!("summer {:?} winter {:?}", t.summer_utc, t.winter_utc)
                })
            };
            o.push("delivery_hours", show(x), show(y));
        }
    }
    match (a_ifr, b_ifr) {
        (Some(x), Some(y)) => o.scalar("ifr.cruise_ft", &x.cruise_ft, &y.cruise_ft),
        (None, None) => {}
        (x, y) => o.push(
            "ifr",
            opt(&x.map(|i| i.cruise_ft)),
            opt(&y.map(|i| i.cruise_ft)),
        ),
    }
    o.list(
        "approach_alt_khz",
        a_approach_alt_khz,
        b_approach_alt_khz,
        u32::to_string,
        |k| mhz(*k),
        |_, _| true,
    );
    o.float(
        "magnetic_variation_deg",
        *a_magnetic_variation_deg,
        *b_magnetic_variation_deg,
    );
    o.list(
        "airspaces",
        a_airspaces,
        b_airspaces,
        |s| s.name.clone(),
        |s| {
            format!(
                "{:?} {}-{} ft class={} {} vertices",
                s.kind,
                s.lower_ft,
                s.upper_ft,
                s.class.map_or(MISSING.to_string(), |c| c.to_string()),
                s.polygon.len()
            )
        },
        |x, y| {
            x.kind == y.kind
                && x.class == y.class
                && x.lower_ft == y.lower_ft
                && x.upper_ft == y.upper_ft
                && polygon_eq(&x.polygon, &y.polygon)
        },
    );
    o.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station() -> Procedures {
        toml::from_str(
            r#"
icao = "ZZTT"
source = "test"
airac = "2601"
transition_altitude_ft = 6000
preferred_runway = "07"
magnetic_variation_deg = 5.5
vfr_holds = ["ALFA"]
approach_alt_khz = [119500]

[[circuits]]
runway = "07"
side = "Left"
altitude_ft_msl = 1500

[[positions]]
role = "Tower"
callsign = "Testfield Tower"
khz = 118100

[[vfr_points]]
name = "ALFA"
lat = 50.0
lon = 19.0

[[vfr_points]]
name = "ECHO"
lat = 50.1
lon = 19.1

[tma]
radius_nm = 10.0
ceiling_ft = 4500
"#,
        )
        .unwrap()
    }

    #[test]
    fn compare_equal_is_empty() {
        let mut b = station();
        b.vfr_points.reverse();
        b.magnetic_variation_deg += 1e-9;
        b.vfr_points[0].lat += 1e-8;
        assert!(compare(&station(), &b).is_empty());
    }

    #[test]
    fn compare_lists_missing_point_by_name() {
        let mut b = station();
        b.vfr_points.retain(|p| p.name != "ECHO");
        let d = compare(&station(), &b);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].field, "vfr_points[ECHO]");
        assert_eq!(d[0].right, "(missing)");
        assert!(d[0].left.contains("50.1"), "{}", d[0].left);
        assert!(d[0].to_string().starts_with("vfr_points[ECHO]: "));
    }

    #[test]
    fn compare_scalars_and_nested() {
        let mut b = station();
        b.preferred_runway = "25".into();
        b.tma.as_mut().unwrap().radius_nm = 12.0;
        b.positions[0].callsign = "Other".into();
        b.approach_alt_khz.push(120_000);
        b.tma.as_mut().unwrap().ceiling_ft = 4500;
        let d = compare(&station(), &b);
        let f: Vec<_> = d.iter().map(|x| x.field.as_str()).collect();
        assert_eq!(
            f,
            [
                "preferred_runway",
                "positions[118100 Tower]",
                "tma.radius_nm",
                "approach_alt_khz[120000]"
            ]
        );
        assert_eq!(d[0].to_string(), "preferred_runway: 07 != 25");
        let mut c = station();
        c.tma = None;
        let d = compare(&station(), &c);
        assert_eq!(d[0].field, "tma");
        assert_eq!(d[0].right, "(missing)");
    }

    fn full() -> Procedures {
        toml::from_str(
            r#"
icao = "ZZTT"
source = "test"
airac = "2601"
transition_altitude_ft = 6000
preferred_runway = "07"
magnetic_variation_deg = 5.5
vfr_holds = ["ALFA"]
vfr_hold_max_ft = 3000
approach_alt_khz = [119500]

[[circuits]]
runway = "07"
side = "Left"
altitude_ft_msl = 1500

[[positions]]
role = "Tower"
callsign = "Testfield Tower"
khz = 118100

[[vfr_points]]
name = "ALFA"
lat = 50.0
lon = 19.0

[[adjacent]]
callsign = "Testfield Info"
khz = 119000
kind = "Fis"

[tma]
radius_nm = 10.0
ceiling_ft = 4500

[delivery_hours]
summer_utc = [6, 18]
winter_utc = [7, 19]

[ifr]
cruise_ft = 9000

[[airspaces]]
kind = "Ctr"
name = "TESTFIELD CTR"
lower_ft = 0
upper_ft = 3500
polygon = [[50.0, 19.0], [50.1, 19.0], [50.1, 19.1]]
"#,
        )
        .unwrap()
    }

    #[test]
    fn every_field_mutation_is_reported_once() {
        type Mutation = (&'static str, fn(&mut Procedures));
        let cases: Vec<Mutation> = vec![
            ("icao", |p| p.icao = "ZZXX".into()),
            ("source", |p| p.source = "other".into()),
            ("airac", |p| p.airac = "2602".into()),
            ("transition_altitude_ft", |p| {
                p.transition_altitude_ft = 5000
            }),
            ("circuits[07]", |p| p.circuits[0].altitude_ft_msl = 1000),
            ("preferred_runway", |p| p.preferred_runway = "25".into()),
            ("positions[118100 Tower]", |p| {
                p.positions[0].callsign = "X".into()
            }),
            ("vfr_points[ALFA]", |p| p.vfr_points[0].lon = 19.5),
            ("vfr_holds[ALFA]", |p| p.vfr_holds.clear()),
            ("vfr_hold_max_ft", |p| p.vfr_hold_max_ft = None),
            ("adjacent[Testfield Info]", |p| p.adjacent[0].khz = 119100),
            ("tma.ceiling_ft", |p| p.tma.as_mut().unwrap().ceiling_ft = 1),
            ("delivery_hours.winter_utc", |p| {
                p.delivery_hours.as_mut().unwrap().winter_utc = [8, 20]
            }),
            ("ifr.cruise_ft", |p| {
                p.ifr.as_mut().unwrap().cruise_ft = 8000
            }),
            ("approach_alt_khz[119500]", |p| p.approach_alt_khz.clear()),
            ("magnetic_variation_deg", |p| p.magnetic_variation_deg = 6.0),
            ("airspaces[TESTFIELD CTR]", |p| {
                p.airspaces[0].polygon[1][0] = 50.2
            }),
        ];
        for (field, mutate) in cases {
            let mut b = full();
            mutate(&mut b);
            let d = compare(&full(), &b);
            assert_eq!(d.len(), 1, "{field}: {d:?}");
            assert_eq!(d[0].field, field);
        }
    }

    #[test]
    fn duplicate_keys_are_not_hidden() {
        let mut a = station();
        let mut dup = a.vfr_points[1].clone();
        dup.lat = 51.0;
        a.vfr_points.push(dup);
        let d = compare(&a, &station());
        let f: Vec<_> = d.iter().map(|x| x.field.as_str()).collect();
        assert_eq!(f, ["vfr_points[ECHO#2]"]);
        assert_eq!(d[0].right, "(missing)");
        let mut b = station();
        b.vfr_points[1].lat = 52.0;
        let d = compare(&a, &b);
        assert_eq!(d.len(), 2, "{d:?}");
    }

    #[test]
    fn point_names_compare_case_insensitively() {
        let mut b = station();
        b.vfr_points[1].name = "echo".into();
        b.vfr_holds = vec!["alfa".into()];
        assert!(compare(&station(), &b).is_empty());
    }
}
