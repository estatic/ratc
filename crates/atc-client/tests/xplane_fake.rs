mod common;

use atc_client::{XpConfig, XpUpdate, run_xplane};
use common::fake_xp::{T, answer, parse_request, read_subscriptions};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::timeout;

/// EPKK stand 1300 at 257 deg, on the ground, COM1 118.105, squawking 4201 in mode ON.
fn value(name: &str) -> f32 {
    match name {
        "sim/flightmodel/position/latitude" => 50.076_115,
        "sim/flightmodel/position/longitude" => 19.796_116,
        "sim/flightmodel/position/elevation" => 241.1,
        "sim/flightmodel/position/psi" => 257.0,
        "sim/flightmodel/failures/onground_any" => 1.0,
        "sim/cockpit2/radios/actuators/com1_frequency_hz_833" => 118_105.0,
        "sim/time/total_running_time_sec" => 42.0,
        "sim/cockpit/radios/transponder_code" => 4201.0,
        "sim/cockpit/radios/transponder_mode" => 2.0,
        "sim/time/zulu_time_sec" => 43_200.0,
        "sim/time/local_date_days" => 200.0,
        "sim/weather/wind_direction_degt[0]" => 270.0,
        "sim/weather/wind_speed_kt[0]" => 8.0,
        "sim/weather/barometer_sealevel_inhg" => 29.92,
        "sim/weather/visibility_reported_m" => 9999.0,
        "sim/weather/temperature_sealevel_c" => 12.0,
        "sim/weather/dewpoi_sealevel_c" => 7.0,
        "sim/weather/cloud_coverage[0]" => 4.0,
        "sim/weather/cloud_base_msl_m[0]" => 1000.0,
        _ => 0.0,
    }
}

async fn setup() -> (UdpSocket, XpConfig) {
    let fake = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut cfg = XpConfig::new(fake.local_addr().unwrap());
    cfg.local_bind = "127.0.0.1:0".parse().unwrap();
    (fake, cfg)
}

#[tokio::test]
async fn fake_xplane_end_to_end() {
    let (fake, cfg) = setup().await;
    let (tx, mut rx) = mpsc::channel(16);
    let task = tokio::spawn(run_xplane(cfg, tx));
    let (client, names) = read_subscriptions(&fake).await;
    answer(&fake, client, &names, value).await;
    let mut got_t = None;
    let mut got_w = None;
    while got_t.is_none() || got_w.is_none() {
        match timeout(T, rx.recv()).await.unwrap().unwrap() {
            XpUpdate::Telemetry(f) => got_t = Some(f),
            XpUpdate::Weather(w) => got_w = Some(w),
            XpUpdate::Aircraft(_) => {}
        }
    }
    let f = got_t.unwrap();
    assert_eq!(f.com1_khz, 118_105);
    assert_eq!((f.xpdr_code, f.xpdr_mode), (Some(4201), Some(2)));
    assert_eq!((f.utc_s, f.day_of_year), (Some(43_200.0), Some(200)));
    assert!(f.on_ground && !f.paused);
    assert!((f.lat - 50.0761).abs() < 1e-3 && (f.lon - 19.7961).abs() < 1e-3);
    assert!((f.alt_ft_msl - 791.0).abs() < 1.0);
    let w = got_w.unwrap();
    assert_eq!((w.qnh_hpa, w.wind_dir_deg), (1013, 270));
    assert_eq!(w.clouds[0].cover, "bkn");

    // closing the channel stops the reader, which unsubscribes (freq 0) on the way out
    drop(rx);
    let mut buf = [0u8; 1024];
    let mut unsub = 0;
    while unsub < atc_client::datarefs::SUBSCRIPTION_COUNT {
        let (n, _) = timeout(T, fake.recv_from(&mut buf)).await.unwrap().unwrap();
        if parse_request(&buf[..n]).0 == 0 {
            unsub += 1;
        }
    }
    // the reader notices the closed channel on its own (no more packets needed)
    timeout(T, task).await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn resubscribes_after_silence() {
    let (fake, mut cfg) = setup().await;
    cfg.silence_timeout = Duration::from_millis(200);
    let (tx, _rx) = mpsc::channel(16);
    let task = tokio::spawn(run_xplane(cfg, tx));
    read_subscriptions(&fake).await;
    // X-Plane stays silent; a second full round of requests must arrive
    read_subscriptions(&fake).await;
    task.abort();
}

#[tokio::test]
async fn survives_unreachable_xplane() {
    // Nothing listens on the X-Plane port at first: sends may bounce (ICMP port unreachable,
    // reported as ECONNREFUSED / WSAECONNRESET on the next socket call). The reader keeps
    // going and picks X-Plane up once it starts.
    let probe = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let mut cfg = XpConfig::new(addr);
    cfg.local_bind = "127.0.0.1:0".parse().unwrap();
    cfg.silence_timeout = Duration::from_millis(100);
    let (tx, mut rx) = mpsc::channel(16);
    let task = tokio::spawn(run_xplane(cfg, tx));
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert!(!task.is_finished(), "reader died: {:?}", task.await);
    let fake = UdpSocket::bind(addr).await.unwrap();
    let (client, names) = read_subscriptions(&fake).await;
    answer(&fake, client, &names, value).await;
    loop {
        if let XpUpdate::Telemetry(f) = timeout(T, rx.recv()).await.unwrap().unwrap() {
            assert_eq!(f.com1_khz, 118_105);
            break;
        }
    }
    task.abort();
}
