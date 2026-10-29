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
        let index = |v: &'_ [T]| -> BTreeMap<String, usize> {
            let mut m = BTreeMap::new();
            for (i, item) in v.iter().enumerate() {
                m.entry(key(item)).or_insert(i);
            }
            m
        };
        let (ia, ib) = (index(a), index(b));
        let keys: BTreeSet<&String> = ia.keys().chain(ib.keys()).collect();
        for k in keys {
            let name = format!("{field}[{k}]");
            match (ia.get(k), ib.get(k)) {
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

fn polygon_eq(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(p, q)| feq(p[0], q[0]) && feq(p[1], q[1]))
}

/// Differences between `a` (left) and `b` (right); empty when they agree. Lists are compared
/// as sets, floats within 1e-6, and the order of the result is fixed.
pub fn compare(a: &Procedures, b: &Procedures) -> Vec<Difference> {
    let mut o = Out(Vec::new());
    o.scalar("icao", &a.icao, &b.icao);
    o.scalar("source", &a.source, &b.source);
    o.scalar("airac", &a.airac, &b.airac);
    o.scalar(
        "transition_altitude_ft",
        &a.transition_altitude_ft,
        &b.transition_altitude_ft,
    );
    o.list(
        "circuits",
        &a.circuits,
        &b.circuits,
        |c| c.runway.clone(),
        |c| format!("{:?} {} ft", c.side, c.altitude_ft_msl),
        |x, y| x == y,
    );
    o.scalar("preferred_runway", &a.preferred_runway, &b.preferred_runway);
    o.list(
        "positions",
        &a.positions,
        &b.positions,
        |p| format!("{} {:?}", p.khz, p.role),
        |p| format!("{} {:?} {}", mhz(p.khz), p.role, p.callsign),
        |x, y| x == y,
    );
    o.list(
        "vfr_points",
        &a.vfr_points,
        &b.vfr_points,
        |p| p.name.clone(),
        |p| format!("{} {} south={}", p.lat, p.lon, p.south),
        |x, y| feq(x.lat, y.lat) && feq(x.lon, y.lon) && x.south == y.south,
    );
    o.list(
        "vfr_holds",
        &a.vfr_holds,
        &b.vfr_holds,
        Clone::clone,
        Clone::clone,
        |_, _| true,
    );
    o.scalar(
        "vfr_hold_max_ft",
        &opt(&a.vfr_hold_max_ft),
        &opt(&b.vfr_hold_max_ft),
    );
    o.list(
        "adjacent",
        &a.adjacent,
        &b.adjacent,
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
    match (&a.tma, &b.tma) {
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
    match (&a.delivery_hours, &b.delivery_hours) {
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
    match (&a.ifr, &b.ifr) {
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
        &a.approach_alt_khz,
        &b.approach_alt_khz,
        u32::to_string,
        |k| mhz(*k),
        |_, _| true,
    );
    o.float(
        "magnetic_variation_deg",
        a.magnetic_variation_deg,
        b.magnetic_variation_deg,
    );
    o.list(
        "airspaces",
        &a.airspaces,
        &b.airspaces,
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
}
