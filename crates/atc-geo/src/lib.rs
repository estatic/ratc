//! Geodesy helpers on a spherical Earth.
use serde::{Deserialize, Serialize};

pub const M_PER_NM: f64 = 1852.0;
pub const FT_PER_M: f64 = 3.28084;
const EARTH_RADIUS_M: f64 = 6_371_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

/// Great-circle distance in metres (haversine).
pub fn distance_m(a: LatLon, b: LatLon) -> f64 {
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dphi = p2 - p1;
    let dlam = (b.lon - a.lon).to_radians();
    let h = (dphi / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dlam / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().min(1.0).asin()
}

/// Initial true bearing from `a` to `b`, degrees in [0, 360).
pub fn bearing_deg(a: LatLon, b: LatLon) -> f64 {
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlam = (b.lon - a.lon).to_radians();
    let y = dlam.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dlam.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// Point reached from `p` travelling `dist_m` along the initial `bearing_deg`.
pub fn destination(p: LatLon, bearing_deg: f64, dist_m: f64) -> LatLon {
    let d = dist_m / EARTH_RADIUS_M;
    let (phi1, lam1, th) = (
        p.lat.to_radians(),
        p.lon.to_radians(),
        bearing_deg.to_radians(),
    );
    let phi2 = (phi1.sin() * d.cos() + phi1.cos() * d.sin() * th.cos()).asin();
    let lam2 = lam1 + (th.sin() * d.sin() * phi1.cos()).atan2(d.cos() - phi1.sin() * phi2.sin());
    LatLon {
        lat: phi2.to_degrees(),
        lon: (lam2.to_degrees() + 540.0).rem_euclid(360.0) - 180.0,
    }
}

/// Ray-casting point-in-polygon on `[lat, lon]` points and rings (planar: fine for airspace
/// sized areas away from the poles and the antimeridian). A closing vertex repeating the
/// first is allowed; a point on the boundary may fall either way.
pub fn point_in_polygon(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a[0] > p[0]) != (b[0] > p[0])
            && p[1] < (b[1] - a[1]) * (p[0] - a[0]) / (b[0] - a[0]) + a[1]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Local runway frame: origin at the threshold, axis along the takeoff direction.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunwayFrame {
    pub origin: LatLon,
    pub course_deg: f64,
}

impl RunwayFrame {
    pub fn new(threshold: LatLon, opposite_threshold: LatLon) -> Self {
        Self {
            origin: threshold,
            course_deg: bearing_deg(threshold, opposite_threshold),
        }
    }

    /// Returns `(along_m, right_m)`; `right_m` is positive to the right of the axis.
    pub fn to_local(&self, p: LatLon) -> (f64, f64) {
        let d = distance_m(self.origin, p);
        let rel = (bearing_deg(self.origin, p) - self.course_deg).to_radians();
        (d * rel.cos(), d * rel.sin())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_known_pair() {
        // EPKK ARP ~ 50.0777 N, 19.7848 E; EPWA ARP ~ 52.1657 N, 20.9671 E.
        // Reference via the spherical law of cosines (independent of haversine).
        let (a, b) = ((50.0777f64).to_radians(), (52.1657f64).to_radians());
        let dl = (20.9671f64 - 19.7848).to_radians();
        let expected = (a.sin() * b.sin() + a.cos() * b.cos() * dl.cos()).acos() * 6_371_000.0;
        let got = distance_m(
            LatLon {
                lat: 50.0777,
                lon: 19.7848,
            },
            LatLon {
                lat: 52.1657,
                lon: 20.9671,
            },
        );
        assert!(
            (got - expected).abs() / expected < 0.005,
            "{got} vs {expected}"
        );
    }

    #[test]
    fn bearing_cardinal() {
        let o = LatLon {
            lat: 50.0,
            lon: 20.0,
        };
        let n = bearing_deg(
            o,
            LatLon {
                lat: 50.01,
                lon: 20.0,
            },
        );
        let e = bearing_deg(
            o,
            LatLon {
                lat: 50.0,
                lon: 20.01,
            },
        );
        assert!(n.abs() < 0.1 || (360.0 - n) < 0.1, "{n}");
        assert!((e - 90.0).abs() < 0.1, "{e}");
    }

    #[test]
    fn destination_roundtrip() {
        let o = LatLon {
            lat: 50.0777,
            lon: 19.7848,
        };
        let p = destination(o, 73.0, 12_345.0);
        assert!((distance_m(o, p) - 12_345.0).abs() < 0.5);
    }

    #[test]
    fn runway_frame_axes() {
        let t = LatLon {
            lat: 50.0777,
            lon: 19.7848,
        };
        let far = destination(t, 85.0, 2500.0);
        let f = RunwayFrame::new(t, far);
        let ahead = destination(t, f.course_deg, 1000.0);
        let (al, rt) = f.to_local(ahead);
        assert!((al - 1000.0).abs() < 1.0 && rt.abs() < 1.0, "{al} {rt}");
        let side = destination(t, f.course_deg + 90.0, 100.0);
        let (al, rt) = f.to_local(side);
        assert!(al.abs() < 1.0 && (rt - 100.0).abs() < 1.0, "{al} {rt}");
    }
    #[test]
    fn point_in_square_and_concave_ring() {
        let square = [[50.0, 20.0], [50.0, 21.0], [51.0, 21.0], [51.0, 20.0]];
        assert!(point_in_polygon([50.5, 20.5], &square));
        assert!(!point_in_polygon([51.5, 20.5], &square));
        assert!(!point_in_polygon([50.5, 19.9], &square));
        // Closed ring (first vertex repeated) gives the same answers.
        let closed = [square.as_slice(), &[square[0]]].concat();
        assert!(point_in_polygon([50.5, 20.5], &closed));
        assert!(!point_in_polygon([51.5, 20.5], &closed));
        // A "U": the notch between the arms is outside.
        let u = [
            [0.0, 0.0],
            [0.0, 3.0],
            [3.0, 3.0],
            [3.0, 2.0],
            [1.0, 2.0],
            [1.0, 1.0],
            [3.0, 1.0],
            [3.0, 0.0],
        ];
        assert!(point_in_polygon([2.0, 0.5], &u));
        assert!(!point_in_polygon([2.0, 1.5], &u));
        assert!(point_in_polygon([2.0, 2.5], &u));
    }

    #[test]
    fn degenerate_rings_contain_nothing() {
        assert!(!point_in_polygon([0.0, 0.0], &[]));
        assert!(!point_in_polygon([0.0, 0.0], &[[0.0, 0.0]]));
    }
}
