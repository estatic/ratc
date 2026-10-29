//! The app's event loop with channels in place of X-Plane, the server and the devices.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use atc_client::XpUpdate;
use atc_client::app::{App, Input};
use atc_client::connection::{Outgoing, ServerEvent};
use atc_client::ptt::PttInput;
use atc_proto::{ClientMsg, ServerMsg, TelemetryFrame};
use tokio::sync::mpsc;

fn frame(com1_khz: u32) -> TelemetryFrame {
    TelemetryFrame {
        t: 1.0,
        lat: 50.0,
        lon: 19.0,
        alt_ft_msl: 791.0,
        heading_deg: 257.0,
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

struct Harness {
    inputs: mpsc::Sender<Input>,
    out: mpsc::Receiver<Outgoing>,
    printed: Arc<Mutex<Vec<String>>>,
}

fn start() -> Harness {
    let (to_conn, out) = mpsc::channel(64);
    let printed = Arc::new(Mutex::new(Vec::new()));
    let p = printed.clone();
    let app = App::new(to_conn, Box::new(move |s| p.lock().unwrap().push(s.into())));
    let (inputs, rx) = mpsc::channel(64);
    tokio::spawn(app.run(rx));
    Harness {
        inputs,
        out,
        printed,
    }
}

impl Harness {
    async fn feed(&self, i: Input) {
        self.inputs.send(i).await.unwrap();
    }

    async fn next_out(&mut self) -> Outgoing {
        tokio::time::timeout(Duration::from_secs(5), self.out.recv())
            .await
            .unwrap()
            .unwrap()
    }

    /// Waits until the app has processed everything fed so far: it takes the no-op
    /// line only after finishing the previous input, so an empty channel after it
    /// means all earlier inputs are done.
    async fn settle(&self) {
        self.feed(Input::Line(String::new())).await;
        drop(self.inputs.reserve_many(64).await.unwrap());
    }
}

#[tokio::test]
async fn ptt_flow_and_frequency_busy() {
    let mut h = start();
    // disconnected: PTT ignored
    h.feed(Input::Ptt(PttInput::KeyToggle)).await;
    h.feed(Input::Xp(XpUpdate::Telemetry(frame(118_105)))).await;
    assert!(matches!(
        h.next_out().await,
        Outgoing::Msg(ClientMsg::Telemetry(_))
    ));
    h.feed(Input::Server(ServerEvent::Connected)).await;
    h.feed(Input::Ptt(PttInput::KeyToggle)).await;
    assert_eq!(
        h.next_out().await,
        Outgoing::Msg(ClientMsg::PttStart { freq_khz: 118_105 })
    );
    h.feed(Input::Mic(vec![1; 320])).await;
    h.feed(Input::Mic(vec![2; 320])).await;
    for seq in 0..2 {
        let Outgoing::Audio(f) = h.next_out().await else {
            panic!()
        };
        assert_eq!((f.stream, f.seq, f.pcm.len()), (0, seq, 320));
    }
    h.feed(Input::Server(ServerEvent::Msg(
        ServerMsg::FrequencyBlocked { freq_khz: 118_105 },
    )))
    .await;
    h.feed(Input::Mic(vec![3; 320])).await;
    h.feed(Input::Ptt(PttInput::KeyToggle)).await;
    // no audio after the block, straight to the end
    assert_eq!(h.next_out().await, Outgoing::Msg(ClientMsg::PttEnd));
    h.settle().await;
    let printed = h.printed.lock().unwrap().clone();
    assert!(printed[0].contains("PTT ignored"), "{printed:?}");
    assert!(
        printed.iter().any(|l| l == "frequency 118.105 busy"),
        "{printed:?}"
    );
}

#[tokio::test]
async fn text_lines_need_telemetry_and_connection() {
    let mut h = start();
    h.feed(Input::Line("request taxi".into())).await;
    h.feed(Input::Xp(XpUpdate::Telemetry(frame(118_105)))).await;
    assert!(matches!(
        h.next_out().await,
        Outgoing::Msg(ClientMsg::Telemetry(_))
    ));
    h.feed(Input::Line("request taxi".into())).await;
    h.feed(Input::Server(ServerEvent::Connected)).await;
    h.feed(Input::Line("  request taxi ".into())).await;
    assert_eq!(
        h.next_out().await,
        Outgoing::Msg(ClientMsg::TextTransmission {
            freq_khz: 118_105,
            text: "request taxi".into()
        })
    );
    h.feed(Input::Server(ServerEvent::Msg(ServerMsg::AtcText {
        station: "Kraków Tower".into(),
        freq_khz: 123_255,
        text: "not for us".into(),
        utterance: 1,
    })))
    .await;
    h.feed(Input::Server(ServerEvent::Msg(
        ServerMsg::PilotTranscript {
            freq_khz: 118_105,
            callsign: None,
            text: "hello".into(),
            lang: "en".into(),
        },
    )))
    .await;
    h.settle().await;
    let printed = h.printed.lock().unwrap().clone();
    assert!(
        printed[0].starts_with("no telemetry from X-Plane yet"),
        "{printed:?}"
    );
    assert_eq!(printed[1], "X-Plane connected, COM 118.105");
    assert!(printed[2].starts_with("not connected"), "{printed:?}");
    assert_eq!(printed[3], "connected to the server");
    assert_eq!(printed[4], "[you] hello");
    assert_eq!(printed.len(), 5, "{printed:?}");
}
