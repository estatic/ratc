//! The simulator boundary: what the client reads from a simulator (own aircraft, radios,
//! transponder, weather, sim clock, draw range) and what it shows there (on-screen text,
//! follow-me car). Each simulator is an adapter module implementing [`SimReader`] and
//! [`SimDisplay`].

pub mod xplane; // sim-neutral: adapter list

use crate::aircraft::SimAircraft;
use atc_proto::{ServerMsg, TelemetryFrame, WeatherFrame};
use tokio::sync::mpsc;

/// What a [`SimReader`] reports.
#[derive(Debug, Clone, PartialEq)]
pub enum SimUpdate {
    /// Contract: `utc_s`/`day_of_year` in UTC; `xpdr_mode` in protocol codes; `t` advances only
    /// while the sim runs (see [`SimClock`]); `listen_khz` per [`TelemetryFrame::listen_khz`].
    Telemetry(TelemetryFrame),
    Weather(WeatherFrame),
    /// The aircraft type or tail number changed (also: first reading). Not debounced.
    Aircraft(SimAircraft),
    /// How far the sim draws other aircraft. Not produced before phase 5.
    DrawRange {
        metres: f64,
    },
}

/// Which simulator an adapter talks to, for console lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimInfo {
    /// The simulator's name as the user knows it.
    pub name: &'static str,
    /// Where the adapter reads from, as configured.
    pub address: String,
    /// The client config key that sets `address`.
    pub config_key: &'static str,
}

/// Reads the own aircraft, radios, transponder, type, weather, sim clock and draw range.
pub trait SimReader: Send + 'static {
    fn info(&self) -> SimInfo;
    /// Starts reading on the tokio runtime; updates go to `tx` until it closes; errors are
    /// logged here.
    fn spawn(self: Box<Self>, tx: mpsc::Sender<SimUpdate>);
}

// Phase 5 adds `draw_traffic(&mut self, bots: &[TrafficState])` here, and a `SimControl`
// trait next to this one with `hold_real_time(&mut self, on: bool)` and
// `set_utc(&mut self, utc: WorldUtc)`. The names are reserved.
/// Shows things in the simulator: on-screen text and the follow-me car.
pub trait SimDisplay: Send {
    fn show_text(&mut self, line: &str);
    /// Only `ServerMsg::FollowMe` / `FollowMeCancel` are passed.
    fn follow_me(&mut self, msg: &ServerMsg);
}

/// `msg` is a follow-me car command, for [`SimDisplay::follow_me`].
pub fn is_follow_me(msg: &ServerMsg) -> bool {
    matches!(
        msg,
        ServerMsg::FollowMe { .. } | ServerMsg::FollowMeCancel { .. }
    )
}

/// Client-side monotonic time. Advances only while the sim runs: not paused and
/// `total_running_time_sec` growing. If the running time goes backwards (flight restarted)
/// `t` simply holds and continues from the last value; the position jump is handled
/// downstream as a telemetry discontinuity.
#[derive(Debug, Default, Clone)]
pub struct SimClock {
    t: f64,
    last_running: Option<f64>,
}

impl SimClock {
    pub fn update(&mut self, running_sec: f64, paused: bool) -> f64 {
        if let Some(prev) = self.last_running
            && !paused
            && running_sec > prev
        {
            self.t += running_sec - prev;
        }
        self.last_running = Some(running_sec);
        self.t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atc_proto::ServerMsg;

    #[test]
    fn time_backwards_and_pause_are_forwarded_monotonic() {
        let mut c = SimClock::default();
        assert_eq!(c.update(100.0, false), 0.0);
        assert!((c.update(100.2, false) - 0.2).abs() < 1e-9);
        let t = c.update(100.4, false);
        // paused: time frozen even though running time moves
        assert_eq!(c.update(105.0, true), t);
        assert_eq!(c.update(106.0, true), t);
        // unpaused: only the delta since the last sample counts
        let t2 = c.update(106.2, false);
        assert!((t2 - t - 0.2).abs() < 1e-9);
        // restart: running time jumps back; t holds, then keeps growing from there
        let t3 = c.update(3.0, false);
        assert_eq!(t3, t2);
        let t4 = c.update(3.2, false);
        assert!((t4 - t2 - 0.2).abs() < 1e-9);
    }

    #[test]
    fn is_follow_me_matches_only_vehicle_commands() {
        let fm = ServerMsg::FollowMe {
            id: 1,
            path: vec![[50.0, 19.0], [50.001, 19.0]],
            end_heading_deg: 0.0,
            aside_left: false,
        };
        assert!(is_follow_me(&fm));
        assert!(is_follow_me(&ServerMsg::FollowMeCancel { id: 1 }));
        assert!(!is_follow_me(&ServerMsg::AtcText {
            station: "TWR".into(),
            freq_khz: 118_105,
            text: "x".into(),
            utterance: 1,
        }));
    }
}
