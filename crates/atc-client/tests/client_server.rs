//! Client logic against a scripted WebSocket peer and against the real server router (with
//! FakeStt/FakeTts) plus the fake X-Plane.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use atc_client::app::{App, Input, spawn_links};
use atc_client::connection::{Backoff, ConnConfig, Outgoing, ServerEvent, run_connection};
use atc_client::{XpConfig, XpUpdate};
use atc_proto::{CallsignSource, ClientMsg, PROTOCOL_VERSION, ServerMsg, TelemetryFrame};
use common::fake_xp::{T, answer, read_subscriptions};
use common::server::{GROUND, TAXI_REQUEST, parked, start, start_on};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout};
use tokio_tungstenite::tungstenite::Message;

fn conn_cfg(url: String, backoff: Backoff) -> ConnConfig {
    ConnConfig {
        url,
        callsign: "SP-ABC".into(),
        aircraft_type: "C172".into(),
        backoff,
    }
}

fn telemetry(com1_khz: u32) -> TelemetryFrame {
    let s = parked();
    TelemetryFrame {
        t: 10.0,
        lat: s.pos.lat,
        lon: s.pos.lon,
        alt_ft_msl: s.alt_ft_msl,
        heading_deg: s.heading_deg,
        ias_kt: 0.0,
        gs_kt: 0.0,
        vs_fpm: 0.0,
        on_ground: true,
        com1_khz,
        listen_khz: Vec::new(),
        xpdr_code: None,
        xpdr_mode: None,
        utc_s: None,
        day_of_year: None,
        paused: false,
    }
}

async fn next_text<S>(ws: &mut S) -> ClientMsg
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match timeout(T, ws.next()).await.unwrap().unwrap().unwrap() {
            Message::Text(t) => return serde_json::from_str(&t).unwrap(),
            Message::Close(_) => panic!("closed"),
            _ => {}
        }
    }
}

async fn next_event(rx: &mut mpsc::Receiver<ServerEvent>) -> ServerEvent {
    timeout(T, rx.recv()).await.unwrap().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_reconnects_with_backoff() {
    let ms = Duration::from_millis;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", listener.local_addr().unwrap());
    let (to_conn, from_ui) = mpsc::channel(16);
    let (to_ui, mut events) = mpsc::channel(16);
    let task = tokio::spawn(run_connection(
        conn_cfg(url, Backoff::new(ms(40), ms(320))),
        from_ui,
        to_ui,
    ));
    // while disconnected: telemetry is kept for later, PTT is dropped
    to_conn
        .send(Outgoing::Msg(ClientMsg::Telemetry(telemetry(GROUND))))
        .await
        .unwrap();
    to_conn
        .send(Outgoing::Msg(ClientMsg::PttStart { freq_khz: GROUND }))
        .await
        .unwrap();

    // a broken server: every attempt is accepted and dropped; attempts follow the schedule
    let mut at = Vec::new();
    for _ in 0..5 {
        let (s, _) = timeout(T, listener.accept()).await.unwrap().unwrap();
        at.push(Instant::now());
        drop(s);
    }
    let gaps: Vec<Duration> = at.windows(2).map(|w| w[1] - w[0]).collect();
    for (gap, want) in gaps.iter().zip([40, 80, 160, 320]) {
        assert!(*gap >= ms(want - 2), "gaps {gaps:?}");
    }

    // the server is back: Hello, Welcome, then the latest telemetry (not the stale PTT)
    let welcome = |n: &str| {
        Message::text(
            serde_json::to_string(&ServerMsg::Welcome {
                protocol: PROTOCOL_VERSION,
                session: n.into(),
            })
            .unwrap(),
        )
    };
    let hello = ClientMsg::Hello {
        protocol: PROTOCOL_VERSION,
        callsign: "SP-ABC".into(),
        aircraft_type: "C172".into(),
        callsign_source: CallsignSource::Config,
    };
    let (s, _) = timeout(T, listener.accept()).await.unwrap().unwrap();
    let capped = at[4].elapsed();
    assert!(capped >= ms(318) && capped < ms(600), "cap: {capped:?}");
    let mut ws = tokio_tungstenite::accept_async(s).await.unwrap();
    assert_eq!(next_text(&mut ws).await, hello);
    ws.send(welcome("1")).await.unwrap();
    assert_eq!(
        next_text(&mut ws).await,
        ClientMsg::Telemetry(telemetry(GROUND))
    );
    assert_eq!(next_event(&mut events).await, ServerEvent::Connected);
    assert!(matches!(
        next_event(&mut events).await,
        ServerEvent::Msg(ServerMsg::Welcome { .. })
    ));

    // the server drops the session: the client says Hello again after the reset (short) delay
    let dropped = Instant::now();
    ws.close(None).await.unwrap();
    drop(ws);
    assert_eq!(next_event(&mut events).await, ServerEvent::Disconnected);
    let (s, _) = timeout(T, listener.accept()).await.unwrap().unwrap();
    let after_reset = dropped.elapsed();
    assert!(after_reset < ms(250), "backoff not reset: {after_reset:?}");
    let mut ws = tokio_tungstenite::accept_async(s).await.unwrap();
    assert_eq!(next_text(&mut ws).await, hello);
    ws.send(welcome("2")).await.unwrap();
    assert_eq!(
        next_text(&mut ws).await,
        ClientMsg::Telemetry(telemetry(GROUND))
    );
    assert_eq!(next_event(&mut events).await, ServerEvent::Connected);

    // closing the UI side stops the connection task
    drop(to_conn);
    timeout(T, task).await.unwrap().unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_joins_real_server_once_it_starts() {
    // nothing listens yet: connection refused, retried
    let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let (to_conn, from_ui) = mpsc::channel(16);
    let (to_ui, mut events) = mpsc::channel(16);
    let backoff = Backoff::new(Duration::from_millis(50), Duration::from_millis(200));
    tokio::spawn(run_connection(
        conn_cfg(format!("ws://{addr}/ws"), backoff),
        from_ui,
        to_ui,
    ));
    to_conn
        .send(Outgoing::Msg(ClientMsg::Telemetry(telemetry(GROUND))))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let srv = start_on(TcpListener::bind(addr).await.unwrap()).await;
    assert_eq!(next_event(&mut events).await, ServerEvent::Connected);
    // the re-sent telemetry tuned the client to Ground
    timeout(T, async {
        while srv.state.hub.lock().unwrap().listeners(GROUND).is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("client tuned to ground");
}

/// X-Plane values for the testkit's parked position on Ground.
fn parked_value(name: &str) -> f32 {
    let s = parked();
    match name {
        "sim/flightmodel/position/latitude" => s.pos.lat as f32,
        "sim/flightmodel/position/longitude" => s.pos.lon as f32,
        "sim/flightmodel/position/elevation" => (s.alt_ft_msl * 0.3048) as f32,
        "sim/flightmodel/position/psi" => s.heading_deg as f32,
        "sim/flightmodel/failures/onground_any" => 1.0,
        "sim/cockpit2/radios/actuators/com1_frequency_hz_833" => GROUND as f32,
        "sim/time/total_running_time_sec" => 42.0,
        "sim/weather/wind_direction_degt[0]" => 250.0,
        "sim/weather/wind_speed_kt[0]" => 8.0,
        "sim/weather/barometer_sealevel_inhg" => 29.92,
        "sim/weather/visibility_reported_m" => 10_000.0,
        "sim/weather/temperature_sealevel_c" => 15.0,
        "sim/weather/dewpoi_sealevel_c" => 10.0,
        _ => 0.0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn text_mode_roundtrip() {
    let srv = start().await;
    let fake = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut xp = XpConfig::new(fake.local_addr().unwrap());
    xp.local_bind = "127.0.0.1:0".parse().unwrap();
    let fake_xp = tokio::spawn(async move {
        let (client, names) = read_subscriptions(&fake).await;
        loop {
            answer(&fake, client, &names, parked_value).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });

    let (inputs, inputs_rx) = mpsc::channel(256);
    let backoff = Backoff::new(Duration::from_millis(50), Duration::from_millis(200));
    let to_conn = spawn_links(conn_cfg(srv.url.clone(), backoff), xp, inputs.clone());
    let (lines_tx, mut lines) = mpsc::unbounded_channel::<String>();
    let printed = Arc::new(Mutex::new(Vec::new()));
    let p = printed.clone();
    let app = App::new(
        to_conn,
        Box::new(move |s| {
            p.lock().unwrap().push(s.to_string());
            let _ = lines_tx.send(s.to_string());
        }),
    );
    let app = tokio::spawn(app.run(inputs_rx));

    // wait until both X-Plane and the server are there
    let (mut xplane, mut server) = (false, false);
    while !(xplane && server) {
        let l = timeout(T, lines.recv()).await.unwrap().unwrap();
        xplane |= l == "X-Plane connected, COM 118.105";
        server |= l == "connected to the server";
    }
    inputs.send(Input::Line(TAXI_REQUEST.into())).await.unwrap();
    let reply = loop {
        let l = timeout(T, lines.recv()).await.unwrap().unwrap();
        if l.starts_with("[Kraków Ground] ") {
            break l;
        }
    };
    assert!(reply.contains("taxi to holding point"), "{reply}");

    // stdin closed: the app lingers briefly for replies, then ends by itself
    inputs.send(Input::Eof).await.unwrap();
    timeout(Duration::from_secs(10), app)
        .await
        .unwrap()
        .unwrap();
    fake_xp.abort();
    let printed = printed.lock().unwrap();
    assert!(
        !printed.iter().any(|l| l.contains("dropped")),
        "{printed:?}"
    );
}

#[tokio::test]
async fn xplane_reader_feeds_links() {
    // spawn_links forwards X-Plane updates as inputs even with no server around
    let fake = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut xp = XpConfig::new(fake.local_addr().unwrap());
    xp.local_bind = "127.0.0.1:0".parse().unwrap();
    let (inputs, mut rx) = mpsc::channel(64);
    let _to_conn = spawn_links(
        conn_cfg("ws://127.0.0.1:9/ws".into(), Backoff::default()),
        xp,
        inputs,
    );
    let (client, names) = read_subscriptions(&fake).await;
    answer(&fake, client, &names, parked_value).await;
    loop {
        if let Input::Xp(XpUpdate::Telemetry(f)) = timeout(T, rx.recv()).await.unwrap().unwrap() {
            assert_eq!(f.com1_khz, GROUND);
            break;
        }
    }
}
