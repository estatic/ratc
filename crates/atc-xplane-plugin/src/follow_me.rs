//! The follow-me car's driving, without X-Plane: per frame, the aircraft position in, the
//! car's pose out.
//!
//! The path is flattened to metres east/north of its first point. The car appears
//! [`GAP_M`] ahead of the aircraft's projection on the path and keeps that gap along the
//! path: it waits while the aircraft stands, never drives backwards, stops when more than
//! [`WAIT_GAP_M`] ahead, and drives at most [`MAX_SPEED`] (15 kt). Its heading follows the
//! path's chord over ±3 m, turning at most [`MAX_TURN_DEG_S`]. At the end of the path it
//! leaves along a forward arc (a cubic Bézier, followed at most [`ARC_SPEED`]) to a point
//! [`ASIDE_M`] to the right of the end heading's line and [`ARC_FORWARD_M`] on along it,
//! facing the end heading, and waits; it never moves sideways. It is gone [`LINGER_S`] after the aircraft reaches the end (or
//! [`MAX_PARKED_S`] after parking).
//!
//! The pose also carries what a real car shows: the front-wheel angle from the heading
//! rate, the turn signal (from [`SIGNAL_LEAD_M`] before a path corner turning more than
//! [`SIGNAL_MIN_DEG`] until it is passed, and during the end arc) and the beacon.
//!
//! The path comes from a UDP datagram, so it is checked first ([`check`]): finite
//! coordinates in range, at most [`MAX_POINTS`] points, all within [`MAX_SPREAD_M`] of the
//! first, a finite end heading and a finite, non-zero length.

/// Wanted distance along the path from the aircraft to the car.
const GAP_M: f64 = 40.0;
/// Further ahead than this the car waits.
const WAIT_GAP_M: f64 = 80.0;
/// 15 kt.
const MAX_SPEED: f64 = 7.7;
/// Acceleration and braking, m/s².
const ACCEL: f64 = 2.0;
const MAX_TURN_DEG_S: f64 = 45.0;
/// Half the chord the heading is taken over.
const CHORD_M: f64 = 3.0;
/// At the end the car pulls this far to the right and this far on...
const ASIDE_M: f64 = 12.0;
/// (A car cannot end facing the end heading 12 m to the right of the end and no further on
/// without a tight loop: it must keep moving forward, so it also gains some distance.)
const ARC_FORWARD_M: f64 = 24.0;
/// ...along an arc followed at most this fast (and not slower than the minimum, so it
/// finishes)...
const ARC_SPEED: f64 = 3.0;
const ARC_MIN_SPEED: f64 = 0.3;
/// ...a cubic Bézier whose inner control points lie this far along the end heading from
/// the end of the path and back from the aside point.
const ARC_LEAD_M: f64 = ARC_FORWARD_M / 2.0;
/// Distance between the front and rear axle, for the front-wheel angle.
const WHEELBASE_M: f64 = 2.7;
const MAX_STEER_DEG: f64 = 35.0;
/// The turn signal goes on this far before a corner turning more than this...
const SIGNAL_LEAD_M: f64 = 40.0;
const SIGNAL_MIN_DEG: f64 = 30.0;
/// ...the turn being the heading change over a window of this length (see
/// [`FollowMe::find_corners`]).
const SIGNAL_WINDOW_M: usize = 30;
/// The path's heading counts as changing once it has moved this far from where it was.
const SIGNAL_MOVE_DEG: f64 = 3.0;
/// The car leaves this long after the aircraft is near the end of the path...
const LINGER_S: f64 = 20.0;
const NEAR_END_M: f64 = 30.0;
/// ...or this long after it parked.
const MAX_PARKED_S: f64 = 120.0;
/// The aircraft's projection is searched this far back and ahead of the last one, so a
/// parallel stretch of the path further on is not taken for it.
const BACK_M: f64 = 30.0;
const AHEAD_M: f64 = 150.0;
const EARTH_RADIUS_M: f64 = 6_371_000.0;
/// A follow-me path has at most this many points...
pub const MAX_POINTS: usize = 500;
/// ...all this close to its first one (an airport, not a country).
pub const MAX_SPREAD_M: f64 = 10_000.0;

/// Why a follow-me path is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathError {
    /// Fewer than two distinct points, or no length.
    TooShort,
    TooManyPoints,
    /// A coordinate that is not finite or out of range (also the aircraft's).
    BadPoint,
    TooSpread,
    BadHeading,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PathError::TooShort => "path too short",
            PathError::TooManyPoints => "path has too many points",
            PathError::BadPoint => "path point not a valid position",
            PathError::TooSpread => "path spreads too far",
            PathError::BadHeading => "end heading not a number",
        })
    }
}

fn valid(p: LatLon) -> bool {
    p.lat.is_finite() && p.lon.is_finite() && p.lat.abs() <= 90.0 && p.lon.abs() <= 180.0
}

/// The checks [`FollowMe::new`] makes before using a path (see the module docs).
pub fn check(path: &[LatLon], end_heading_deg: f64, aircraft: LatLon) -> Result<(), PathError> {
    if path.len() > MAX_POINTS {
        return Err(PathError::TooManyPoints);
    }
    let first = *path.first().ok_or(PathError::TooShort)?;
    if !path.iter().copied().chain([aircraft]).all(valid) {
        return Err(PathError::BadPoint);
    }
    // Near a pole the flat frame is meaningless; no airport needs it.
    if first.lat.abs() > 85.0 {
        return Err(PathError::BadPoint);
    }
    if path
        .iter()
        .any(|&p| dist((0.0, 0.0), local(first, p)) > MAX_SPREAD_M)
    {
        return Err(PathError::TooSpread);
    }
    if !end_heading_deg.is_finite() {
        return Err(PathError::BadHeading);
    }
    Ok(())
}

/// A position, degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

/// The car's turn signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Off,
    Left,
    Right,
}

/// Where to draw the car and how; heading true, degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: LatLon,
    pub heading_deg: f64,
    /// Front-wheel angle, degrees, positive to the right; 0 when stopped.
    pub steer_deg: f64,
    pub turn_signal: Signal,
    /// True while the car exists.
    pub beacon_on: bool,
}

/// A path corner: the signal is on for arc lengths `on..off`.
#[derive(Clone, Copy, Debug)]
struct Corner {
    on: f64,
    off: f64,
    side: Signal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Driving,
    /// On the end arc at Bézier parameter `t`.
    Arc {
        t: f64,
    },
    /// Parked for `t` seconds; the aircraft near the end since parked time `near`.
    Parked {
        t: f64,
        near: Option<f64>,
    },
    Gone,
}

pub struct FollowMe {
    id: u32,
    origin: LatLon,
    /// Path points, metres (east, north) of `origin`, and the arc length at each.
    pts: Vec<(f64, f64)>,
    cum: Vec<f64>,
    end_heading: f64,
    /// Arc length of the car and of the aircraft's projection.
    s_car: f64,
    s_ac: f64,
    speed: f64,
    /// Smoothed speed of the aircraft along the path.
    ac_speed: f64,
    heading: f64,
    phase: Phase,
    /// Where the signal is on.
    corners: Vec<Corner>,
    /// Control points of the end arc, and its length.
    arc: [(f64, f64); 4],
    arc_len: f64,
}

impl FollowMe {
    /// Refuses a path [`check`] refuses, or of less than two distinct points.
    pub fn new(
        id: u32,
        path: &[LatLon],
        end_heading_deg: f64,
        aircraft: LatLon,
    ) -> Result<Self, PathError> {
        check(path, end_heading_deg, aircraft)?;
        let origin = *path.first().ok_or(PathError::TooShort)?;
        let mut pts: Vec<(f64, f64)> = Vec::with_capacity(path.len());
        let mut cum = Vec::with_capacity(path.len());
        for &p in path {
            let q = local(origin, p);
            match pts.last() {
                None => cum.push(0.0),
                Some(&last) => {
                    let d = dist(last, q);
                    if d < 0.01 {
                        continue;
                    }
                    cum.push(cum.last().copied().unwrap_or(0.0) + d);
                }
            }
            pts.push(q);
        }
        let len = cum.last().copied().unwrap_or(0.0);
        if pts.len() < 2 || !len.is_finite() || len <= 0.0 {
            return Err(PathError::TooShort);
        }
        let mut car = FollowMe {
            id,
            origin,
            pts,
            cum,
            end_heading: end_heading_deg.rem_euclid(360.0),
            s_car: 0.0,
            s_ac: 0.0,
            speed: 0.0,
            ac_speed: 0.0,
            heading: 0.0,
            phase: Phase::Driving,
            corners: Vec::new(),
            arc: [(0.0, 0.0); 4],
            arc_len: 0.0,
        };
        car.corners = car.find_corners();
        car.arc = car.end_arc();
        car.arc_len = (1..=64)
            .map(|k| car.bezier(k as f64 / 64.0))
            .fold((car.arc[0], 0.0), |(last, sum), q| (q, sum + dist(last, q)))
            .1;
        car.s_ac = car.project(local(origin, aircraft), 0.0, car.len());
        car.s_car = (car.s_ac + GAP_M).min(car.len());
        car.heading = car.tangent(car.s_car);
        Ok(car)
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    /// One frame of `dt` seconds with the aircraft at `aircraft`; `None` once the car is gone.
    pub fn update(&mut self, dt: f64, aircraft: LatLon) -> Option<Pose> {
        let dt = dt.max(0.0);
        let ac = local(self.origin, aircraft);
        let end = self.point_at(self.len());
        let aside = self.arc[3];
        let heading_before = self.heading;
        let (pos, phase, turn_signal) = match self.phase {
            Phase::Gone => return None,
            Phase::Driving => {
                self.drive(dt, ac);
                if self.s_car >= self.len() {
                    (end, Phase::Arc { t: 0.0 }, Signal::Right)
                } else {
                    (
                        self.point_at(self.s_car),
                        Phase::Driving,
                        self.signal_at(self.s_car),
                    )
                }
            }
            Phase::Arc { t } => {
                let left = self.arc_len * (1.0 - t);
                let target = (2.0 * ACCEL * left).sqrt().clamp(ARC_MIN_SPEED, ARC_SPEED);
                self.speed += (target - self.speed).clamp(-ACCEL * dt, ACCEL * dt);
                self.speed = self.speed.clamp(0.0, ARC_SPEED);
                let (dx, dy) = self.bezier_d(t);
                let t = (t + self.speed * dt / dx.hypot(dy).max(1e-6)).min(1.0);
                let (dx, dy) = self.bezier_d(t);
                self.heading = dx.atan2(dy).to_degrees().rem_euclid(360.0);
                let phase = if t >= 1.0 {
                    Phase::Parked { t: 0.0, near: None }
                } else {
                    Phase::Arc { t }
                };
                (self.bezier(t), phase, Signal::Right)
            }
            Phase::Parked { t, near } => {
                let t = t + dt;
                self.speed = 0.0;
                self.turn_to(self.end_heading, dt);
                let near = near.or((dist(ac, end) <= NEAR_END_M).then_some(t));
                let gone = t >= MAX_PARKED_S || near.is_some_and(|n| t - n >= LINGER_S);
                let phase = if gone {
                    Phase::Gone
                } else {
                    Phase::Parked { t, near }
                };
                (aside, phase, Signal::Off)
            }
        };
        self.phase = phase;
        let yaw_deg_s = if dt > 0.0 {
            (self.heading - heading_before + 540.0).rem_euclid(360.0) - 180.0
        } else {
            0.0
        } / dt.max(1e-9);
        (phase != Phase::Gone).then(|| Pose {
            pos: global(self.origin, pos),
            heading_deg: self.heading,
            steer_deg: steer_deg(yaw_deg_s, self.speed),
            turn_signal,
            beacon_on: true,
        })
    }

    /// Keeps the gap to the aircraft (see the module docs) and turns along the path.
    fn drive(&mut self, dt: f64, ac: (f64, f64)) {
        let s_ac = self.project(ac, self.s_ac - BACK_M, self.s_ac + AHEAD_M);
        if dt > 0.0 {
            let v = (s_ac - self.s_ac) / dt;
            self.ac_speed = (0.7 * self.ac_speed + 0.3 * v).max(0.0);
        }
        self.s_ac = s_ac;
        let gap = self.s_car - s_ac;
        let target = if gap > WAIT_GAP_M {
            0.0
        } else {
            (self.ac_speed + 0.5 * (GAP_M - gap)).clamp(0.0, MAX_SPEED)
        };
        // Arrive at the end of the path no faster than the end arc is driven.
        let to_end = self.len() - self.s_car;
        let target = target.min((ARC_SPEED * ARC_SPEED + 2.0 * ACCEL * to_end).sqrt());
        self.speed += (target - self.speed).clamp(-ACCEL * dt, ACCEL * dt);
        self.speed = self.speed.max(0.0);
        self.s_car = (self.s_car + self.speed * dt).min(self.len());
        let want = self.tangent(self.s_car);
        self.turn_to(want, dt);
    }

    /// Turns toward `want` the short way, at most [`MAX_TURN_DEG_S`].
    fn turn_to(&mut self, want: f64, dt: f64) {
        let diff = (want - self.heading + 540.0).rem_euclid(360.0) - 180.0;
        let step = MAX_TURN_DEG_S * dt;
        self.heading = (self.heading + diff.clamp(-step, step)).rem_euclid(360.0);
    }

    /// Control points of the end arc: from the end of the path along the end heading, to the
    /// aside point, arriving along the end heading again (an S-shaped lane change, turning
    /// right by at most about 45 degrees and back). Its velocity always has a forward and
    /// a rightward part, so the car never reverses or slides.
    fn end_arc(&self) -> [(f64, f64); 4] {
        let end = self.point_at(self.len());
        let aside = offset(
            offset(end, self.end_heading + 90.0, ASIDE_M),
            self.end_heading,
            ARC_FORWARD_M,
        );
        [
            end,
            offset(end, self.end_heading, ARC_LEAD_M),
            offset(aside, self.end_heading, -ARC_LEAD_M),
            aside,
        ]
    }

    fn bezier(&self, t: f64) -> (f64, f64) {
        let [p0, p1, p2, p3] = self.arc;
        let (u, t2) = (1.0 - t, t * t);
        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t2, t2 * t];
        (
            w[0] * p0.0 + w[1] * p1.0 + w[2] * p2.0 + w[3] * p3.0,
            w[0] * p0.1 + w[1] * p1.1 + w[2] * p2.1 + w[3] * p3.1,
        )
    }

    /// Derivative of [`Self::bezier`] by `t`.
    fn bezier_d(&self, t: f64) -> (f64, f64) {
        let [p0, p1, p2, p3] = self.arc;
        let u = 1.0 - t;
        let w = [3.0 * u * u, 6.0 * u * t, 3.0 * t * t];
        (
            w[0] * (p1.0 - p0.0) + w[1] * (p2.0 - p1.0) + w[2] * (p3.0 - p2.0),
            w[0] * (p1.1 - p0.1) + w[1] * (p2.1 - p1.1) + w[2] * (p3.1 - p2.1),
        )
    }

    /// The signal wanted with the car at arc length `s`.
    fn signal_at(&self, s: f64) -> Signal {
        self.corners
            .iter()
            .find(|c| s >= c.on && s < c.off)
            .map_or(Signal::Off, |c| c.side)
    }

    /// The corners of the path that turn more than [`SIGNAL_MIN_DEG`].
    ///
    /// The path arrives with rounded corners sampled every 2 m, so no single step turns
    /// much. The heading is therefore sampled every metre (over a 4 m chord, which smooths
    /// the sampling), unwrapped into a continuous angle, and a corner is a stretch where it
    /// changes by more than the minimum over a window of [`SIGNAL_WINDOW_M`]: long enough
    /// to see a wide rounded corner, short enough not to merge two corners or count a
    /// slow drift. The stretch is then narrowed to where the heading really starts and
    /// stops changing, which is where the signal starts (minus [`SIGNAL_LEAD_M`]) and ends.
    fn find_corners(&self) -> Vec<Corner> {
        let n = self.len().floor() as usize + 1;
        let mut u: Vec<f64> = Vec::with_capacity(n);
        let (mut acc, mut last): (f64, Option<f64>) = (0.0, None);
        for i in 0..n {
            let s = i as f64;
            let h = heading(self.point_at(s - 2.0), self.point_at(s + 2.0));
            if let Some(prev) = last {
                acc += (h - prev + 540.0).rem_euclid(360.0) - 180.0;
            }
            last = Some(h);
            u.push(acc);
        }
        let half = SIGNAL_WINDOW_M / 2;
        let change = |i: usize| u[(i + half).min(n - 1)] - u[i.saturating_sub(half)];
        let mut corners = Vec::new();
        let mut i = 0;
        while i < n {
            let d = change(i);
            if d.abs() <= SIGNAL_MIN_DEG {
                i += 1;
                continue;
            }
            let right = d > 0.0;
            let r0 = i;
            while i < n && change(i).abs() > SIGNAL_MIN_DEG && (change(i) > 0.0) == right {
                i += 1;
            }
            let r1 = i - 1;
            let before = r0.saturating_sub(SIGNAL_WINDOW_M);
            let after = (r1 + SIGNAL_WINDOW_M).min(n - 1);
            let start = (before..=r1)
                .find(|&j| (u[j] - u[before]).abs() > SIGNAL_MOVE_DEG)
                .unwrap_or(r0);
            let stop = (start..=after)
                .rev()
                .find(|&j| (u[after] - u[j]).abs() > SIGNAL_MOVE_DEG)
                .unwrap_or(r1);
            corners.push(Corner {
                on: start as f64 - SIGNAL_LEAD_M,
                off: stop as f64,
                side: if right { Signal::Right } else { Signal::Left },
            });
        }
        corners
    }

    fn len(&self) -> f64 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    /// The point at arc length `s` (clamped to the path).
    fn point_at(&self, s: f64) -> (f64, f64) {
        let s = s.clamp(0.0, self.len());
        for k in 1..self.pts.len() {
            if s <= self.cum[k] {
                let (a, b) = (self.pts[k - 1], self.pts[k]);
                let seg = self.cum[k] - self.cum[k - 1];
                let f = if seg > 0.0 {
                    (s - self.cum[k - 1]) / seg
                } else {
                    0.0
                };
                return (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
            }
        }
        self.pts.last().copied().unwrap_or((0.0, 0.0))
    }

    /// Heading of the path's chord around arc length `s`.
    fn tangent(&self, s: f64) -> f64 {
        let (a, b) = (self.point_at(s - CHORD_M), self.point_at(s + CHORD_M));
        heading(a, b)
    }

    /// Arc length of the point of the path nearest `p`, among the segments overlapping
    /// `[lo, hi]`.
    fn project(&self, p: (f64, f64), lo: f64, hi: f64) -> f64 {
        let mut best = (f64::INFINITY, lo.max(0.0));
        for k in 1..self.pts.len() {
            if self.cum[k] < lo || self.cum[k - 1] > hi {
                continue;
            }
            let (a, b) = (self.pts[k - 1], self.pts[k]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = dx * dx + dy * dy;
            let f = if len2 > 0.0 {
                (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = dist(p, (a.0 + dx * f, a.1 + dy * f));
            if d < best.0 {
                best = (d, self.cum[k - 1] + f * (self.cum[k] - self.cum[k - 1]));
            }
        }
        best.1
    }
}

/// Front-wheel angle, degrees (+ right), of a car of [`WHEELBASE_M`] yawing `yaw_deg_s`
/// (+ clockwise) at `speed`; 0 when stopped.
fn steer_deg(yaw_deg_s: f64, speed: f64) -> f64 {
    if speed < 0.1 {
        return 0.0;
    }
    (WHEELBASE_M * yaw_deg_s.to_radians() / speed)
        .atan()
        .to_degrees()
        .clamp(-MAX_STEER_DEG, MAX_STEER_DEG)
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

/// Degrees clockwise from north, from `a` to `b`.
fn heading(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).atan2(b.1 - a.1).to_degrees().rem_euclid(360.0)
}

fn offset(p: (f64, f64), heading_deg: f64, m: f64) -> (f64, f64) {
    let h = heading_deg.to_radians();
    (p.0 + m * h.sin(), p.1 + m * h.cos())
}

/// Metres east and north of `origin` (flat Earth: fine across an airport).
fn local(origin: LatLon, p: LatLon) -> (f64, f64) {
    let east = (p.lon - origin.lon).to_radians() * EARTH_RADIUS_M * origin.lat.to_radians().cos();
    let north = (p.lat - origin.lat).to_radians() * EARTH_RADIUS_M;
    (east, north)
}

fn global(origin: LatLon, p: (f64, f64)) -> LatLon {
    LatLon {
        lat: origin.lat + (p.1 / EARTH_RADIUS_M).to_degrees(),
        lon: origin.lon + (p.0 / (EARTH_RADIUS_M * origin.lat.to_radians().cos())).to_degrees(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: LatLon = LatLon {
        lat: 50.0,
        lon: 19.8,
    };
    const DT: f64 = 0.1;

    /// `e` metres east and `n` north of [`O`].
    fn at(e: f64, n: f64) -> LatLon {
        global(O, (e, n))
    }

    fn xy(p: LatLon) -> (f64, f64) {
        local(O, p)
    }

    /// 400 m north, then 200 m east; ends facing east.
    fn car(aircraft: LatLon) -> FollowMe {
        let path = [at(0.0, 0.0), at(0.0, 400.0), at(200.0, 400.0)];
        FollowMe::new(1, &path, 90.0, aircraft).unwrap()
    }

    /// The aircraft at arc length `s` of the test path.
    fn on_path(s: f64) -> LatLon {
        if s <= 400.0 {
            at(0.0, s)
        } else {
            at((s - 400.0).min(200.0), 400.0)
        }
    }

    #[test]
    fn appears_ahead() {
        let mut c = car(at(2.0, 10.0));
        let p = xy(c.update(DT, at(2.0, 10.0)).unwrap().pos);
        assert!(p.0.abs() < 0.01 && (p.1 - 50.0).abs() < 0.01, "{p:?}");
        assert_eq!(c.id(), 1);
    }

    #[test]
    fn waits_for_a_standing_aircraft() {
        let mut c = car(at(0.0, 0.0));
        for _ in 0..300 {
            let p = xy(c.update(DT, at(0.0, 0.0)).unwrap().pos);
            assert!(p.1 <= 41.0, "{p:?}");
        }
    }

    #[test]
    fn follows_a_moving_aircraft() {
        let mut c = car(on_path(0.0));
        let mut last = c.s_car;
        for k in 1..=1000 {
            let t = k as f64 * DT;
            c.update(DT, on_path(5.0 * t)).unwrap();
            assert!(c.s_car >= last, "backwards at {t}");
            last = c.s_car;
            if t > 20.0 && c.s_car < c.len() {
                let gap = c.s_car - c.s_ac;
                assert!((25.0..=60.0).contains(&gap), "gap {gap} at {t}");
            }
        }
    }

    #[test]
    fn waits_when_far_ahead() {
        let mut c = car(on_path(0.0));
        // The aircraft drops back 100 m behind the car.
        c.s_car = 150.0;
        c.speed = 5.0;
        for _ in 0..50 {
            c.update(DT, on_path(50.0)).unwrap();
        }
        assert_eq!(c.speed, 0.0);
    }

    #[test]
    fn turns_smoothly() {
        let mut c = car(on_path(0.0));
        let mut last = c.heading;
        let mut turned = false;
        for k in 1..=1500 {
            let Some(p) = c.update(DT, on_path(5.0 * k as f64 * DT)) else {
                break;
            };
            let step = (p.heading_deg - last + 540.0).rem_euclid(360.0) - 180.0;
            assert!(step.abs() <= MAX_TURN_DEG_S * DT + 1e-6, "{step}");
            turned |= (p.heading_deg - 90.0).abs() < 1.0;
            last = p.heading_deg;
        }
        assert!(turned);
    }

    #[test]
    fn parks_aside_and_leaves() {
        let mut c = car(on_path(0.0));
        let mut s = 0.0;
        // Drive until the car is parked.
        while !matches!(c.phase, Phase::Parked { .. }) {
            s = (s + 5.0 * DT).min(560.0);
            c.update(DT, on_path(s)).unwrap();
        }
        // 12 m right of the east-facing end (south of it) and further on, facing east.
        let p = c.update(DT, on_path(s)).unwrap();
        let q = xy(p.pos);
        assert!(
            (q.0 - 200.0 - ARC_FORWARD_M).abs() < 0.1 && (q.1 - 388.0).abs() < 0.1,
            "{q:?}"
        );
        assert!((p.heading_deg - 90.0).abs() < 1.0, "{}", p.heading_deg);
        // The aircraft reaches the end: gone 20 s later, and stays gone.
        let mut t = 0.0;
        while c.update(DT, on_path(580.0)).is_some() {
            t += DT;
            assert!(t < 25.0);
        }
        assert!((t - LINGER_S).abs() < 0.2, "{t}");
        assert_eq!(c.update(DT, on_path(580.0)), None);
    }

    #[test]
    fn rejects_degenerate_paths() {
        let short = Some(PathError::TooShort);
        assert_eq!(FollowMe::new(1, &[at(0.0, 0.0)], 0.0, O).err(), short);
        let same = [at(0.0, 0.0), at(0.0, 0.0)];
        assert_eq!(FollowMe::new(1, &same, 0.0, O).err(), short);
        assert_eq!(FollowMe::new(1, &[], 0.0, O).err(), short);
    }

    /// Datagram input that must never reach the driver.
    #[test]
    fn rejects_malformed_paths() {
        let ok = [at(0.0, 0.0), at(0.0, 100.0)];
        assert_eq!(check(&ok, 90.0, O), Ok(()));
        let with = |p: LatLon| [at(0.0, 0.0), p];
        for bad in [
            LatLon {
                lat: f64::NAN,
                lon: 19.8,
            },
            LatLon {
                lat: 50.0,
                lon: f64::INFINITY,
            },
            LatLon {
                lat: 91.0,
                lon: 19.8,
            },
            LatLon {
                lat: 50.0,
                lon: -180.5,
            },
        ] {
            assert_eq!(
                check(&with(bad), 0.0, O),
                Err(PathError::BadPoint),
                "{bad:?}"
            );
        }
        let nan_aircraft = LatLon {
            lat: f64::NAN,
            lon: 0.0,
        };
        assert_eq!(check(&ok, 0.0, nan_aircraft), Err(PathError::BadPoint));
        let polar = [
            LatLon {
                lat: 89.0,
                lon: 0.0,
            },
            LatLon {
                lat: 89.001,
                lon: 0.0,
            },
        ];
        assert_eq!(check(&polar, 0.0, polar[0]), Err(PathError::BadPoint));
        assert_eq!(
            check(&with(at(0.0, MAX_SPREAD_M + 1.0)), 0.0, O),
            Err(PathError::TooSpread)
        );
        assert_eq!(
            check(
                &with(LatLon {
                    lat: -50.0,
                    lon: -160.0
                }),
                0.0,
                O
            ),
            Err(PathError::TooSpread)
        );
        for h in [f64::NAN, f64::INFINITY] {
            assert_eq!(check(&ok, h, O), Err(PathError::BadHeading));
        }
        let many: Vec<LatLon> = (0..=MAX_POINTS).map(|k| at(0.0, k as f64)).collect();
        assert_eq!(check(&many, 0.0, O), Err(PathError::TooManyPoints));
        assert!(FollowMe::new(1, &many[..MAX_POINTS], 0.0, O).is_ok());
        assert_eq!(
            FollowMe::new(1, &with(bad_lat()), 0.0, O).err(),
            Some(PathError::BadPoint)
        );
    }

    /// 400 m north, then 200 m west: a left corner at 400 m.
    fn left_car() -> FollowMe {
        let path = [at(0.0, 0.0), at(0.0, 400.0), at(-200.0, 400.0)];
        FollowMe::new(1, &path, 270.0, at(0.0, 0.0)).unwrap()
    }

    /// The aircraft at arc length `s` of [`left_car`]'s path.
    fn on_left_path(s: f64) -> LatLon {
        if s <= 400.0 {
            at(0.0, s)
        } else {
            at(-(s - 400.0).min(200.0), 400.0)
        }
    }

    #[test]
    fn steer_follows_turn_direction() {
        // Left corner: wheels go left (negative) and never right; straight: about 0.
        let mut c = left_car();
        let (mut min, mut max, mut straight) = (0.0_f64, 0.0_f64, 0.0_f64);
        for k in 1..=800 {
            let t = k as f64 * DT;
            let p = c.update(DT, on_left_path(5.0 * t)).unwrap();
            if c.phase != Phase::Driving {
                break;
            }
            min = min.min(p.steer_deg);
            max = max.max(p.steer_deg);
            if c.s_car < 350.0 {
                straight = straight.max(p.steer_deg.abs());
            }
        }
        assert!(min < -5.0 && max < 0.5, "{min} {max}");
        assert!(straight < 0.5, "{straight}");
        // Right corner: positive.
        let mut c = car(on_path(0.0));
        let mut max = 0.0_f64;
        for k in 1..=800 {
            let p = c.update(DT, on_path(5.0 * k as f64 * DT)).unwrap();
            max = max.max(p.steer_deg);
            assert!(p.steer_deg.abs() <= MAX_STEER_DEG + 1e-9);
        }
        assert!(max > 5.0, "{max}");
    }

    #[test]
    fn steer_zero_when_stopped() {
        let mut c = car(on_path(0.0));
        c.s_car = 150.0;
        c.speed = 5.0;
        let mut last = None;
        for _ in 0..80 {
            last = c.update(DT, on_path(50.0));
        }
        assert_eq!(c.speed, 0.0);
        assert_eq!(last.unwrap().steer_deg, 0.0);
        // Turning on the spot (parked, heading off) does not steer either.
        c.phase = Phase::Parked { t: 0.0, near: None };
        c.heading = 0.0;
        let p = c.update(DT, on_path(50.0)).unwrap();
        assert!(p.heading_deg != 0.0);
        assert_eq!(p.steer_deg, 0.0);
    }

    /// Frames from the start of the end arc to the car being parked, for a path ending on
    /// `end_heading`.
    fn end_frames(end_heading: f64) -> Vec<Pose> {
        let path = [at(0.0, 0.0), at(0.0, 400.0), at(200.0, 400.0)];
        let mut c = FollowMe::new(1, &path, end_heading, on_path(0.0)).unwrap();
        let mut s = 0.0;
        let mut frames = Vec::new();
        for _ in 0..20_000 {
            s = (s + 5.0 * DT).min(560.0);
            let p = c.update(DT, on_path(s)).unwrap();
            if matches!(c.phase, Phase::Arc { .. } | Phase::Parked { .. }) {
                frames.push(p);
            }
            if matches!(c.phase, Phase::Parked { .. }) {
                return frames;
            }
        }
        panic!("never parked");
    }

    #[test]
    fn parking_never_moves_sideways() {
        // The last leg heads east; the stand heading is east, then the opposite.
        for end_heading in [90.0, 270.0] {
            let frames = end_frames(end_heading);
            assert!(frames.len() > 20);
            for w in frames.windows(2) {
                let (a, b) = (xy(w[0].pos), xy(w[1].pos));
                let d = dist(a, b);
                if d < 1e-6 {
                    continue;
                }
                let off = (heading(a, b) - w[1].heading_deg + 540.0).rem_euclid(360.0) - 180.0;
                assert!(
                    off.abs() < 5.0,
                    "{end_heading}: moved {off} deg off the heading"
                );
                assert!(d <= ARC_SPEED * DT * 1.02, "{d}");
                assert_eq!(w[1].turn_signal, Signal::Right);
            }
            let last = frames.last().unwrap();
            let diff = (last.heading_deg - end_heading + 540.0).rem_euclid(360.0) - 180.0;
            assert!(diff.abs() < 2.0, "{end_heading}: {}", last.heading_deg);
            // 12 m right of the end of the path (200, 400), further on along the heading.
            let right = offset((200.0, 400.0), end_heading + 90.0, ASIDE_M);
            let expect = offset(right, end_heading, ARC_FORWARD_M);
            let q = xy(last.pos);
            assert!(dist(q, expect) < 0.5, "{end_heading}: {q:?} {expect:?}");
            assert!(last.beacon_on);
        }
    }

    /// The signal per metre of the car's way along `c`'s path, driven by an aircraft at
    /// `ac(s)`, as (arc length, signal) while the car is still on the path.
    fn signals(c: &mut FollowMe, ac: impl Fn(f64) -> LatLon) -> Vec<(f64, Signal)> {
        let mut out = Vec::new();
        for k in 1..=3000 {
            let p = c.update(DT, ac(5.0 * k as f64 * DT)).unwrap();
            if c.phase != Phase::Driving {
                break;
            }
            out.push((c.s_car, p.turn_signal));
        }
        out
    }

    #[test]
    fn signal_before_corner() {
        let mut c = left_car();
        let out = signals(&mut c, on_left_path);
        for &(s, sig) in &out {
            if (362.0..398.0).contains(&s) {
                assert_eq!(sig, Signal::Left, "at {s}");
            }
            if !(355.0..405.0).contains(&s) {
                assert_eq!(sig, Signal::Off, "at {s}");
            }
        }
        assert!(out.iter().any(|&(s, _)| s > 410.0));
        // A right corner signals right.
        let mut c = car(on_path(0.0));
        let out = signals(&mut c, on_path);
        assert!(
            out.iter()
                .any(|&(s, g)| (365.0..397.0).contains(&s) && g == Signal::Right)
        );
    }

    #[test]
    fn signal_on_rounded_corner() {
        // North for 100 m, a 20 m radius left turn sampled every 2 m, west for 100 m.
        let mut path = vec![];
        for k in 0..=50 {
            path.push(at(0.0, 2.0 * k as f64));
        }
        let arc_len = 20.0 * std::f64::consts::FRAC_PI_2;
        for k in 1..=(arc_len / 2.0) as usize {
            let a = 2.0 * k as f64 / 20.0;
            path.push(at(-20.0 + 20.0 * a.cos(), 100.0 + 20.0 * a.sin()));
        }
        for k in 1..=50 {
            path.push(at(-20.0 - 2.0 * k as f64, 120.0));
        }
        let mut c = FollowMe::new(1, &path, 270.0, at(0.0, 0.0)).unwrap();
        let len = c.len();
        let ac = |s: f64| c_point(&path, s.min(len));
        let out = signals(&mut c, ac);
        for &(s, sig) in &out {
            if (63.0..125.0).contains(&s) {
                assert_eq!(sig, Signal::Left, "at {s}");
            }
            if !(55.0..135.0).contains(&s) {
                assert_eq!(sig, Signal::Off, "at {s}");
            }
        }
    }

    /// The point `s` metres along a polyline.
    fn c_point(path: &[LatLon], s: f64) -> LatLon {
        let mut left = s;
        for w in path.windows(2) {
            let d = dist(xy(w[0]), xy(w[1]));
            if left <= d {
                let f = left / d;
                return LatLon {
                    lat: w[0].lat + (w[1].lat - w[0].lat) * f,
                    lon: w[0].lon + (w[1].lon - w[0].lon) * f,
                };
            }
            left -= d;
        }
        *path.last().unwrap()
    }

    #[test]
    fn signal_off_on_straight() {
        let path = [at(0.0, 0.0), at(0.0, 300.0), at(3.0, 600.0)];
        let mut c = FollowMe::new(1, &path, 0.0, at(0.0, 0.0)).unwrap();
        let out = signals(&mut c, |s| at(0.0, s));
        assert!(out.len() > 100);
        assert!(out.iter().all(|&(_, sig)| sig == Signal::Off));
    }

    fn bad_lat() -> LatLon {
        LatLon {
            lat: f64::NAN,
            lon: f64::NAN,
        }
    }
}
