//! Read-only X-Plane 11 reader over the UDP RREF protocol.
use crate::datarefs::{ALL, SLOT_COUNT, Slot};
use atc_proto::{CloudLayer, TelemetryFrame, WeatherFrame};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::Instant;

const NAME_LEN: usize = 400;
const REQUEST_LEN: usize = 5 + 4 + 4 + NAME_LEN;
const M_TO_FT: f64 = 1.0 / 0.3048;
const MPS_TO_KT: f64 = 1.943_844_5;
const INHG_TO_HPA: f64 = 33.8639;
const WEATHER_PERIOD: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum XpError {
    #[error("RREF packet too short: {0} bytes")]
    TooShort(usize),
    #[error("not an RREF packet")]
    BadHeader,
    #[error("RREF payload is not a whole number of 8-byte values ({0} bytes)")]
    BadLength(usize),
}

/// Subscription request (or, with `freq_hz == 0`, unsubscribe).
pub fn encode_rref_request(freq_hz: i32, index: i32, name: &str) -> [u8; REQUEST_LEN] {
    let mut buf = [0u8; REQUEST_LEN];
    buf[..5].copy_from_slice(b"RREF\0");
    buf[5..9].copy_from_slice(&freq_hz.to_le_bytes());
    buf[9..13].copy_from_slice(&index.to_le_bytes());
    let n = name.len().min(NAME_LEN - 1);
    buf[13..13 + n].copy_from_slice(&name.as_bytes()[..n]);
    buf
}

pub fn decode_rref(packet: &[u8]) -> Result<Vec<(i32, f32)>, XpError> {
    if packet.len() < 5 {
        return Err(XpError::TooShort(packet.len()));
    }
    if &packet[..4] != b"RREF" {
        return Err(XpError::BadHeader);
    }
    let body = &packet[5..];
    if !body.len().is_multiple_of(8) {
        return Err(XpError::BadLength(body.len()));
    }
    Ok(body
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| {
            (
                i32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                f32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            )
        })
        .collect())
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

#[derive(Debug, Clone)]
pub struct XPlaneState {
    values: [Option<f32>; SLOT_COUNT],
    clock: SimClock,
    t: f64,
}

impl Default for XPlaneState {
    fn default() -> Self {
        Self {
            values: [None; SLOT_COUNT],
            clock: SimClock::default(),
            t: 0.0,
        }
    }
}

impl XPlaneState {
    pub fn apply(&mut self, values: &[(i32, f32)]) {
        for &(idx, v) in values {
            if let Some(slot) = usize::try_from(idx).ok().filter(|&i| i < SLOT_COUNT) {
                self.values[slot] = Some(v);
            }
        }
        if let (Some(rt), Some(p)) = (self.get(Slot::RunningTime), self.get(Slot::Paused)) {
            self.t = self.clock.update(rt, p > 0.5);
        }
    }

    fn get(&self, s: Slot) -> Option<f64> {
        self.values[s as usize].map(f64::from)
    }

    /// The transmit frequency and the frequencies heard (see [`TelemetryFrame::listen_khz`]):
    /// COM2 when it is selected for transmission, else COM1; each radio selected for
    /// listening besides. Without the COM2 datarefs: COM1 only.
    fn radios(&self) -> Option<(u32, Vec<u32>)> {
        let khz = |s| self.get(s).map(|v| v.round() as u32);
        let on = |s| self.get(s).is_some_and(|v| v > 0.5);
        let com1 = khz(Slot::Com1Khz)?;
        let com2 = khz(Slot::Com2Khz);
        let tx = match (self.get(Slot::ComTx), com2) {
            (Some(sel), Some(c2)) if sel.round() as i32 == 7 => c2,
            _ => com1,
        };
        let mut listen = vec![tx];
        for (flag, f) in [(Slot::ListenCom1, Some(com1)), (Slot::ListenCom2, com2)] {
            if let Some(f) = f.filter(|f| on(flag) && !listen.contains(f)) {
                listen.push(f);
            }
        }
        Some((tx, listen))
    }

    /// `None` until every telemetry dataref but the transponder has been received.
    pub fn telemetry(&self) -> Option<TelemetryFrame> {
        let g = |s| self.get(s);
        let (com1_khz, listen_khz) = self.radios()?;
        Some(TelemetryFrame {
            t: self.t,
            lat: g(Slot::Lat)?,
            lon: g(Slot::Lon)?,
            alt_ft_msl: g(Slot::ElevM)? * M_TO_FT,
            heading_deg: g(Slot::Psi)?,
            ias_kt: g(Slot::Ias)?,
            gs_kt: g(Slot::Gs)? * MPS_TO_KT,
            vs_fpm: g(Slot::Vs)?,
            on_ground: g(Slot::OnGround)? > 0.5,
            com1_khz,
            listen_khz,
            xpdr_code: g(Slot::XpdrCode).map(|c| c.round() as u16),
            xpdr_mode: g(Slot::XpdrMode).map(|m| m.round() as u8),
            utc_s: g(Slot::ZuluTime),
            day_of_year: g(Slot::LocalDateDays).map(|d| d.round() as u16),
            paused: g(Slot::Paused)? > 0.5,
        })
    }

    /// `None` until wind, visibility, temperature, dew point and pressure have arrived.
    /// Clouds are best-effort: layers without data are skipped.
    pub fn weather(&self) -> Option<WeatherFrame> {
        let g = |s| self.get(s);
        let clouds = [
            (Slot::CloudCov0, Slot::CloudBase0),
            (Slot::CloudCov1, Slot::CloudBase1),
            (Slot::CloudCov2, Slot::CloudBase2),
        ]
        .into_iter()
        .filter_map(|(c, b)| {
            let cover = cloud_cover(g(c)?)?;
            let base_msl_m = g(b)?;
            if base_msl_m <= 0.0 {
                return None;
            }
            Some(CloudLayer {
                cover: cover.to_string(),
                base_ft_msl: (base_msl_m * M_TO_FT).round() as u32,
            })
        })
        .collect();
        Some(WeatherFrame {
            wind_dir_deg: g(Slot::WindDir)?.rem_euclid(360.0).round() as u16 % 360,
            wind_speed_kt: g(Slot::WindSpeedKt)?.max(0.0).round() as u16,
            visibility_m: g(Slot::VisM)?.max(0.0).round() as u32,
            clouds,
            temp_c: g(Slot::Temp)?.round() as i16,
            dewpoint_c: g(Slot::Dew)?.round() as i16,
            qnh_hpa: (g(Slot::BaroInhg)? * INHG_TO_HPA).round() as u16,
        })
    }
}

/// Maps X-Plane 11 `cloud_coverage` (float, 0..6: 0 clear, 1 cirrus, 2 few, 3 scattered,
/// 4 broken, 5 overcast, 6 stratus) to a METAR cover. Thresholds sit halfway between the
/// integer steps: < 0.5 clear (skipped), < 2.5 few (cirrus counts as few), < 3.5 sct,
/// < 4.5 bkn, otherwise ovc.
fn cloud_cover(coverage: f64) -> Option<&'static str> {
    match coverage {
        c if c < 0.5 => None,
        c if c < 2.5 => Some("few"),
        c if c < 3.5 => Some("sct"),
        c if c < 4.5 => Some("bkn"),
        _ => Some("ovc"),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum XpUpdate {
    Telemetry(TelemetryFrame),
    Weather(WeatherFrame),
}

#[derive(Debug, Clone)]
pub struct XpConfig {
    /// X-Plane host and RREF port (default port 49000).
    pub xplane_addr: SocketAddr,
    /// Local address to receive replies on; X-Plane answers to the sender's port.
    pub local_bind: SocketAddr,
    /// Resend all subscriptions after this long without a packet (X-Plane restarted).
    pub silence_timeout: Duration,
}

impl XpConfig {
    pub fn new(xplane_addr: SocketAddr) -> Self {
        Self {
            xplane_addr,
            local_bind: ([0, 0, 0, 0], 0).into(),
            silence_timeout: Duration::from_secs(5),
        }
    }
}

async fn subscribe(sock: &UdpSocket, addr: SocketAddr, on: bool) -> std::io::Result<()> {
    for (slot, name, hz) in ALL {
        let freq = if on { hz } else { 0 };
        sock.send_to(&encode_rref_request(freq, slot as i32, name), addr)
            .await?;
    }
    Ok(())
}

/// Subscribes, logging (not propagating) a failure; the silence timeout retries.
async fn resubscribe(sock: &UdpSocket, addr: SocketAddr) {
    if let Err(e) = subscribe(sock, addr, true).await {
        tracing::warn!("X-Plane subscription to {addr} failed: {e}");
        tokio::time::sleep(ERROR_PAUSE).await;
    }
}

/// Best-effort unsubscribe when the reader stops, including when its task is cancelled.
struct Unsubscribe {
    sock: Arc<UdpSocket>,
    addr: SocketAddr,
}

impl Drop for Unsubscribe {
    fn drop(&mut self) {
        for (slot, name, _) in ALL {
            let _ = self
                .sock
                .try_send_to(&encode_rref_request(0, slot as i32, name), self.addr);
        }
    }
}

/// Pause after a socket error so a persistent failure does not spin.
const ERROR_PAUSE: Duration = Duration::from_millis(200);

/// Subscribes (read-only) and forwards telemetry at the subscription rate and weather every
/// 5 s until the receiver is dropped or the task is cancelled. Only binding the local socket
/// is fatal: send/receive errors (X-Plane not running yet, ICMP port unreachable reported on
/// the next call) are logged and retried.
pub async fn run_xplane(cfg: XpConfig, tx: mpsc::Sender<XpUpdate>) -> anyhow::Result<()> {
    let sock = Arc::new(UdpSocket::bind(cfg.local_bind).await?);
    let _guard = Unsubscribe {
        sock: sock.clone(),
        addr: cfg.xplane_addr,
    };
    resubscribe(&sock, cfg.xplane_addr).await;
    let mut state = XPlaneState::default();
    let mut buf = [0u8; 2048];
    let mut last_weather: Option<Instant> = None;
    loop {
        let recv = tokio::time::timeout(cfg.silence_timeout, sock.recv_from(&mut buf));
        let n = tokio::select! {
            _ = tx.closed() => return Ok(()),
            r = recv => match r {
                Err(_) => {
                    tracing::warn!("no X-Plane data, resubscribing");
                    resubscribe(&sock, cfg.xplane_addr).await;
                    continue;
                }
                Ok(Err(e)) => {
                    tracing::warn!("X-Plane socket receive failed: {e}");
                    tokio::time::sleep(ERROR_PAUSE).await;
                    continue;
                }
                Ok(Ok((n, _))) => n,
            },
        };
        let values = match decode_rref(&buf[..n]) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("ignoring packet: {e}");
                continue;
            }
        };
        let has_telemetry = values.iter().any(|&(i, _)| {
            ALL.get(usize::try_from(i).unwrap_or(usize::MAX))
                .is_some_and(|a| a.0.is_telemetry())
        });
        state.apply(&values);
        if has_telemetry
            && let Some(f) = state.telemetry()
            && tx.send(XpUpdate::Telemetry(f)).await.is_err()
        {
            return Ok(());
        }
        if last_weather.is_none_or(|t| t.elapsed() >= WEATHER_PERIOD)
            && let Some(w) = state.weather()
        {
            last_weather = Some(Instant::now());
            if tx.send(XpUpdate::Weather(w)).await.is_err() {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkt(vals: &[(i32, f32)]) -> Vec<u8> {
        let mut p = b"RREF,".to_vec();
        for (i, v) in vals {
            p.extend(i.to_le_bytes());
            p.extend(v.to_le_bytes());
        }
        p
    }

    fn full_state() -> Vec<(i32, f32)> {
        let v = [
            50.076, 19.796, 241.0, 90.0, 0.0, 0.0, 0.0, 1.0, 118_105.0, 121_075.0, 6.0, 1.0, 0.0,
            10.0, 0.0, 4201.0, 2.0, 43_200.0, 200.0, 270.0, 8.0, 29.92, 9999.0, 12.0, 7.0, 3.0,
            0.0, 0.0, 1000.0, 0.0, 0.0,
        ];
        v.iter().enumerate().map(|(i, &x)| (i as i32, x)).collect()
    }

    #[test]
    fn rref_request_layout() {
        let b = encode_rref_request(5, 7, "sim/time/paused");
        assert_eq!(b.len(), 413);
        assert_eq!(&b[..5], b"RREF\0");
        assert_eq!(i32::from_le_bytes(b[5..9].try_into().unwrap()), 5);
        assert_eq!(i32::from_le_bytes(b[9..13].try_into().unwrap()), 7);
        assert_eq!(&b[13..28], b"sim/time/paused");
        assert!(b[28..].iter().all(|&x| x == 0));
    }

    #[test]
    fn decode_rref_multiple_values() {
        let r = decode_rref(&pkt(&[(0, 1.5), (3, -2.0)])).unwrap();
        assert_eq!(r, vec![(0, 1.5), (3, -2.0)]);
    }

    #[test]
    fn decode_rejects_short_or_wrong_header() {
        assert_eq!(decode_rref(b"RRE"), Err(XpError::TooShort(3)));
        assert_eq!(
            decode_rref(b"DATA,\0\0\0\0\0\0\0\0"),
            Err(XpError::BadHeader)
        );
        assert_eq!(decode_rref(b"RREF,\0\0\0"), Err(XpError::BadLength(3)));
    }

    #[test]
    fn state_needs_all_required_values() {
        let all = full_state();
        let mut s = XPlaneState::default();
        s.apply(&all[..14]);
        assert!(s.telemetry().is_none());
        s.apply(&all[14..15]);
        let f = s.telemetry().expect("all telemetry present");
        assert_eq!(f.com1_khz, 118_105);
        assert!((f.alt_ft_msl - 790.7).abs() < 0.1);
        // The transponder is optional.
        assert_eq!((f.xpdr_code, f.xpdr_mode), (None, None));
        s.apply(&all[15..17]);
        let f = s.telemetry().unwrap();
        assert_eq!((f.xpdr_code, f.xpdr_mode), (Some(4201), Some(2)));
        // The sim time is optional too.
        assert_eq!((f.utc_s, f.day_of_year), (None, None));
        s.apply(&all[17..19]);
        let f = s.telemetry().unwrap();
        assert_eq!((f.utc_s, f.day_of_year), (Some(43_200.0), Some(200)));
        assert!(s.weather().is_none());
        s.apply(&all[19..]);
        let w = s.weather().unwrap();
        assert_eq!(
            (w.wind_dir_deg, w.wind_speed_kt, w.visibility_m),
            (270, 8, 9999)
        );
        assert_eq!(
            w.clouds,
            vec![CloudLayer {
                cover: "sct".into(),
                base_ft_msl: 3281
            }]
        );
    }

    /// The audio panel picks the transmit radio; every radio selected for listening is heard.
    /// Without the COM2 datarefs the pilot transmits and listens on COM1.
    #[test]
    fn transmit_and_listen_radios() {
        let at = |slot: Slot, v: f32| (slot as i32, v);
        let mut s = XPlaneState::default();
        let all = full_state();
        let without_com2: Vec<_> = all
            .iter()
            .copied()
            .filter(|&(i, _)| !(9..=12).contains(&i))
            .collect();
        s.apply(&without_com2);
        let f = s.telemetry().unwrap();
        assert_eq!((f.com1_khz, f.listen_khz), (118_105, vec![118_105]));
        s.apply(&all);
        let f = s.telemetry().unwrap();
        assert_eq!((f.com1_khz, f.listen_khz), (118_105, vec![118_105]));
        s.apply(&[at(Slot::ComTx, 7.0)]);
        let f = s.telemetry().unwrap();
        assert_eq!(
            (f.com1_khz, f.listen_khz),
            (121_075, vec![121_075, 118_105])
        );
        s.apply(&[at(Slot::ListenCom1, 0.0), at(Slot::ListenCom2, 1.0)]);
        assert_eq!(s.telemetry().unwrap().listen_khz, vec![121_075]);
        s.apply(&[at(Slot::ComTx, 6.0), at(Slot::Com2Khz, 126_130.0)]);
        let f = s.telemetry().unwrap();
        assert_eq!(
            (f.com1_khz, f.listen_khz),
            (118_105, vec![118_105, 126_130])
        );
    }

    #[test]
    fn qnh_from_inhg() {
        let mut s = XPlaneState::default();
        s.apply(&full_state());
        assert_eq!(s.weather().unwrap().qnh_hpa, 1013);
    }

    #[test]
    fn cloud_coverage_mapping() {
        assert_eq!(cloud_cover(0.0), None);
        assert_eq!(cloud_cover(1.0), Some("few"));
        assert_eq!(cloud_cover(2.0), Some("few"));
        assert_eq!(cloud_cover(3.0), Some("sct"));
        assert_eq!(cloud_cover(4.0), Some("bkn"));
        assert_eq!(cloud_cover(5.0), Some("ovc"));
        assert_eq!(cloud_cover(6.0), Some("ovc"));
        // unset base (<= 0) skips the layer even when coverage is set
        let mut s = XPlaneState::default();
        let mut v = full_state();
        v[Slot::CloudBase0 as usize].1 = 0.0;
        s.apply(&v);
        assert!(s.weather().unwrap().clouds.is_empty());
    }

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
    fn state_pause_and_restart_keep_frames_flowing() {
        let set = |s: &mut XPlaneState, rt: f32, paused: f32| {
            s.apply(&[
                (Slot::RunningTime as i32, rt),
                (Slot::Paused as i32, paused),
            ]);
            s.telemetry().expect("telemetry")
        };
        let mut s = XPlaneState::default();
        s.apply(&full_state());
        let t0 = set(&mut s, 11.0, 0.0).t;
        let a = set(&mut s, 20.0, 1.0);
        let b = set(&mut s, 30.0, 1.0);
        assert!(a.paused && b.paused);
        assert_eq!((a.t, b.t), (t0, t0));
        let mut last = set(&mut s, 31.0, 0.0);
        assert!(!last.paused);
        // flight restarted: running time jumps back, frames keep coming with t not decreasing
        for rt in [2.0, 2.5, 3.0] {
            let f = set(&mut s, rt, 0.0);
            assert!(f.t >= last.t, "{} < {}", f.t, last.t);
            last = f;
        }
        assert!(last.t > t0);
    }
}
