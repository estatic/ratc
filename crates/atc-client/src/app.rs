//! The client's event loop: X-Plane telemetry, server events, typed lines (text mode), PTT
//! and microphone frames (voice mode) in; messages to the server, console lines, playback
//! and follow-me car commands for the X-Plane plugin out. No device I/O here — `main`
//! feeds [`Input`]s and owns the devices.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atc_proto::{AudioFrame, CallsignSource, ClientMsg, ServerMsg, overlay_command};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::aircraft::{Identity, IdentityTracker, UNKNOWN_TYPE};
use crate::capture::PILOT_RATE;
use crate::connection::{ConnConfig, Outgoing, ServerEvent, run_connection};
use crate::playback::{Playback, should_play};
use crate::ptt::{PttEvent, PttInput, PttMerger};
use crate::record::TrackWriter;
use crate::xplane::{XpConfig, XpUpdate, run_xplane};

/// After stdin ends, keep printing replies to what was typed for this long.
const LINGER: Duration = Duration::from_secs(3);

/// How often the latest sim aircraft is re-offered to the debounce.
const IDENTITY_TICK: Duration = Duration::from_millis(500);

/// How often "waiting for X-Plane" is repeated.
const WAIT_NOTICE: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum Input {
    Xp(XpUpdate),
    Server(ServerEvent),
    /// A typed line (text mode).
    Line(String),
    /// stdin closed (text mode).
    Eof,
    Ptt(PttInput),
    /// 20 ms of microphone audio at 16 kHz mono.
    Mic(Vec<i16>),
}

/// Spawns the server connection and the X-Plane reader, both feeding `inputs`; returns the
/// sender for messages to the server.
pub fn spawn_links(
    conn: ConnConfig,
    identity: watch::Receiver<Option<Identity>>,
    xp: XpConfig,
    inputs: mpsc::Sender<Input>,
) -> mpsc::Sender<Outgoing> {
    let (out_tx, out_rx) = mpsc::channel(1024);
    let (ev_tx, mut ev_rx) = mpsc::channel(1024);
    tokio::spawn(async move {
        if let Err(e) = run_connection(conn, identity, out_rx, ev_tx).await {
            tracing::error!("server connection stopped: {e:#}");
        }
    });
    let (xp_tx, mut xp_rx) = mpsc::channel(64);
    tokio::spawn(async move {
        if let Err(e) = run_xplane(xp, xp_tx).await {
            tracing::error!("X-Plane reader stopped: {e:#}");
        }
    });
    tokio::spawn(async move {
        loop {
            let input = tokio::select! {
                Some(e) = ev_rx.recv() => Input::Server(e),
                Some(u) = xp_rx.recv() => Input::Xp(u),
                else => return,
            };
            if inputs.send(input).await.is_err() {
                return;
            }
        }
    });
    out_tx
}

/// `"COM 118.105"`, or `"COM 118.105, also listening 126.130"` with other radios heard.
fn radios_line(tx: u32, heard: &[u32]) -> String {
    let others: Vec<String> = heard
        .iter()
        .filter(|&&f| f != tx)
        .map(|&f| fmt_khz(f))
        .collect();
    if others.is_empty() {
        format!("COM {}", fmt_khz(tx))
    } else {
        format!("COM {}, also listening {}", fmt_khz(tx), others.join(", "))
    }
}

/// `118105` → `"118.105"`.
pub fn fmt_khz(khz: u32) -> String {
    format!("{}.{:03}", khz / 1000, khz % 1000)
}

/// Where lines go: the console (and overlay), or the plugin's vehicle commands.
pub type LineSink = Box<dyn FnMut(&str) + Send>;

struct Transmission {
    seq: u32,
    /// Microphone samples sent (16 kHz), shown on release.
    samples: usize,
    /// `FrequencyBlocked`: stop sending audio for this press.
    blocked: bool,
}

pub struct App {
    to_conn: mpsc::Sender<Outgoing>,
    print: LineSink,
    /// Vehicle commands to the X-Plane plugin (overlay datagrams).
    plugin: Option<LineSink>,
    playback: Option<Arc<Mutex<Playback>>>,
    recorder: Option<TrackWriter<Box<dyn Write + Send>>>,
    /// The transmit frequency (COM1 or COM2, per the audio panel).
    com1: Option<u32>,
    /// Every frequency heard ([`atc_proto::TelemetryFrame::heard_khz`]).
    heard: Vec<u32>,
    connected: bool,
    ptt: PttMerger,
    tx: Option<Transmission>,
    typed_any: bool,
    /// Turns the sim's aircraft into the identity the connection uses.
    identity: Option<IdentityTracker>,
    /// X-Plane address, named in the "waiting" notice.
    xplane_addr: String,
    last_wait_notice: Option<std::time::Instant>,
    /// What the server identified us as, for the refusal line.
    identified: Option<(String, String)>,
    /// The last refusal line printed; the same one is not repeated.
    last_refusal: Option<String>,
}

impl App {
    pub fn new(to_conn: mpsc::Sender<Outgoing>, print: LineSink) -> Self {
        App {
            to_conn,
            print,
            plugin: None,
            playback: None,
            recorder: None,
            com1: None,
            heard: Vec::new(),
            connected: false,
            ptt: PttMerger::default(),
            tx: None,
            typed_any: false,
            identity: None,
            xplane_addr: String::new(),
            last_wait_notice: None,
            identified: None,
            last_refusal: None,
        }
    }

    /// Takes the aircraft from X-Plane (at `xplane_addr`) and publishes the identity.
    pub fn with_identity(mut self, tracker: IdentityTracker, xplane_addr: String) -> Self {
        self.identity = Some(tracker);
        self.xplane_addr = xplane_addr;
        self
    }

    /// Where `FollowMe` / `FollowMeCancel` go, framed for the plugin.
    pub fn with_plugin(mut self, send: LineSink) -> Self {
        self.plugin = Some(send);
        self
    }

    pub fn with_playback(mut self, p: Arc<Mutex<Playback>>) -> Self {
        self.playback = Some(p);
        self
    }

    pub fn with_recorder(mut self, r: TrackWriter<Box<dyn Write + Send>>) -> Self {
        self.recorder = Some(r);
        self
    }

    /// Runs until `Quit`, the end of `inputs`, or [`LINGER`] after `Eof`.
    pub async fn run(mut self, mut inputs: mpsc::Receiver<Input>) {
        let mut deadline: Option<Instant> = None;
        let mut tick = tokio::time::interval(IDENTITY_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let input = tokio::select! {
                i = inputs.recv() => i,
                _ = async {
                    match deadline {
                        Some(d) => tokio::time::sleep_until(d).await,
                        None => std::future::pending().await,
                    }
                } => return,
                _ = tick.tick() => {
                    self.identity_tick(std::time::Instant::now());
                    continue;
                }
            };
            let Some(input) = input else { return };
            match input {
                Input::Eof if !self.typed_any => return,
                Input::Eof => deadline = Some(Instant::now() + LINGER),
                Input::Ptt(PttInput::Quit) => return,
                Input::Ptt(p) => match self.ptt.on_input(p) {
                    Some(PttEvent::Press) => self.ptt_press().await,
                    Some(PttEvent::Release) => self.ptt_release().await,
                    None => {}
                },
                Input::Mic(pcm) => self.mic(pcm).await,
                Input::Line(l) => self.line(&l).await,
                Input::Xp(u) => self.xp(u).await,
                Input::Server(e) => self.server(e),
            }
        }
    }

    fn say(&mut self, s: &str) {
        (self.print)(s);
    }

    async fn send(&mut self, m: ClientMsg) {
        let _ = self.to_conn.send(Outgoing::Msg(m)).await;
    }

    /// Re-offers the sim aircraft to the debounce and repeats the "waiting" notice.
    fn identity_tick(&mut self, now: std::time::Instant) {
        let Some(tracker) = &mut self.identity else {
            return;
        };
        let line = tracker.tick(now);
        let waiting = tracker.waiting_for_sim();
        if let Some(l) = line {
            self.say(&l);
        }
        if waiting
            && self
                .last_wait_notice
                .is_none_or(|t| now.duration_since(t) >= WAIT_NOTICE)
        {
            self.last_wait_notice = Some(now);
            let addr = self.xplane_addr.clone();
            self.say(&format!(
                "Waiting for X-Plane at {addr} for the aircraft type…"
            ));
        }
    }

    async fn xp(&mut self, u: XpUpdate) {
        match u {
            XpUpdate::Telemetry(f) => {
                let heard = f.heard_khz();
                if self.com1 != Some(f.com1_khz) || self.heard != heard {
                    let prefix = if self.com1.is_none() {
                        "X-Plane connected, "
                    } else {
                        ""
                    };
                    self.say(&format!("{prefix}{}", radios_line(f.com1_khz, &heard)));
                    self.com1 = Some(f.com1_khz);
                    if let Some(p) = &self.playback {
                        p.lock().unwrap().set_heard(&heard);
                    }
                    self.heard = heard;
                }
                if let Some(r) = &mut self.recorder
                    && let Err(e) = r.write(&f)
                {
                    tracing::error!("track recording failed, stopping it: {e}");
                    self.recorder = None;
                }
                self.send(ClientMsg::Telemetry(f)).await;
            }
            XpUpdate::Weather(w) => self.send(ClientMsg::Weather(w)).await,
            XpUpdate::Aircraft(a) => {
                if let Some(tracker) = &mut self.identity
                    && let Some(l) = tracker.observe(std::time::Instant::now(), a)
                {
                    self.say(&l);
                }
            }
        }
    }

    async fn line(&mut self, line: &str) {
        let text = line.trim();
        if text.is_empty() {
            return;
        }
        let Some(freq_khz) = self.com1 else {
            self.say("no telemetry from X-Plane yet (is it running? check `xplane` in the config) - line dropped");
            return;
        };
        if !self.connected {
            self.say("not connected to the server - line dropped");
            return;
        }
        self.typed_any = true;
        self.send(ClientMsg::TextTransmission {
            freq_khz,
            text: text.to_string(),
        })
        .await;
    }

    async fn ptt_press(&mut self) {
        let reason = match (self.connected, self.com1) {
            (false, _) => "not connected to the server",
            (true, None) => "no COM radio from X-Plane yet",
            (true, Some(freq_khz)) => {
                self.send(ClientMsg::PttStart { freq_khz }).await;
                self.tx = Some(Transmission {
                    seq: 0,
                    samples: 0,
                    blocked: false,
                });
                self.say(&format!("[ptt] transmitting on {}", fmt_khz(freq_khz)));
                return;
            }
        };
        tracing::info!("PTT ignored: {reason}");
        self.say(&format!("[ptt] {reason} - PTT ignored"));
        self.ptt.reset();
    }

    async fn ptt_release(&mut self) {
        if let Some(tx) = self.tx.take() {
            self.send(ClientMsg::PttEnd).await;
            let secs = tx.samples as f64 / f64::from(PILOT_RATE);
            if tx.samples == 0 {
                self.say("[ptt] off - no microphone audio was sent (check input_device)");
            } else {
                self.say(&format!("[ptt] off - sent {secs:.1} s of audio"));
            }
        }
    }

    async fn mic(&mut self, pcm: Vec<i16>) {
        let Some(tx) = self.tx.as_mut().filter(|t| !t.blocked) else {
            return;
        };
        let frame = AudioFrame {
            stream: 0,
            seq: tx.seq,
            pcm,
        };
        tx.seq += 1;
        tx.samples += frame.pcm.len();
        let _ = self.to_conn.send(Outgoing::Audio(frame)).await;
    }

    fn server(&mut self, e: ServerEvent) {
        match e {
            ServerEvent::Connected => {
                self.connected = true;
                self.say("connected to the server");
            }
            ServerEvent::Disconnected => {
                self.connected = false;
                if self.tx.take().is_some() {
                    self.ptt.reset();
                }
                self.say("disconnected from the server, reconnecting");
            }
            ServerEvent::Audio(f) => {
                if let Some(p) = &self.playback {
                    p.lock().unwrap().on_audio(&f);
                }
            }
            ServerEvent::Msg(m) => {
                self.server_msg(&m);
                if let Some(p) = &self.playback {
                    p.lock().unwrap().on_msg(&m);
                }
            }
        }
    }

    fn server_msg(&mut self, m: &ServerMsg) {
        if let (Some(send), Some(cmd)) = (&mut self.plugin, overlay_command(m)) {
            send(&cmd);
            return;
        }
        match m {
            ServerMsg::AtcText {
                station,
                freq_khz,
                text,
                ..
            } if should_play(*freq_khz, &self.heard) => self.say(&format!("[{station}] {text}")),
            ServerMsg::PilotTranscript { text, .. } => {
                let text = if text.trim().is_empty() {
                    "(nothing recognised)"
                } else {
                    text
                };
                self.say(&format!("[you] {text}"));
            }
            ServerMsg::FrequencyBlocked { freq_khz } => {
                if let Some(tx) = &mut self.tx {
                    tx.blocked = true;
                }
                self.say(&format!("frequency {} busy", fmt_khz(*freq_khz)));
            }
            ServerMsg::Identified {
                callsign,
                aircraft_type,
                callsign_source,
            } => {
                let from = match callsign_source {
                    CallsignSource::Config => "from config",
                    CallsignSource::Sim => "X-Plane tail number",
                };
                let unknown = if aircraft_type == UNKNOWN_TYPE {
                    " (type unknown)"
                } else {
                    ""
                };
                self.say(&format!(
                    "Aircraft: {aircraft_type}{unknown}, callsign {callsign} ({from})"
                ));
                self.identified = Some((callsign.clone(), aircraft_type.clone()));
                self.last_refusal = None;
            }
            ServerMsg::AircraftChangeRefused { reason } => {
                let still = match &self.identified {
                    Some((c, t)) => format!("; still flying as {c} ({t})"),
                    None => String::new(),
                };
                let line = format!("Aircraft change refused: {reason}{still}");
                if self.last_refusal.as_ref() != Some(&line) {
                    self.say(&line);
                    self.last_refusal = Some(line);
                }
            }
            ServerMsg::Rejected { reason } => {
                self.say(&format!("server rejected the connection: {reason}"));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_me_goes_to_the_plugin_only() {
        let (tx, _rx) = mpsc::channel(1);
        let printed = Arc::new(Mutex::new(Vec::<String>::new()));
        let sent = Arc::new(Mutex::new(Vec::<String>::new()));
        let (p, s) = (printed.clone(), sent.clone());
        let mut app = App::new(tx, Box::new(move |l| p.lock().unwrap().push(l.into())))
            .with_plugin(Box::new(move |l| s.lock().unwrap().push(l.into())));
        let m = ServerMsg::FollowMe {
            id: 3,
            path: vec![[50.0, 19.0], [50.001, 19.0]],
            end_heading_deg: 0.0,
            aside_left: true,
        };
        app.server_msg(&m);
        app.server_msg(&ServerMsg::FollowMeCancel { id: 3 });
        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(atc_proto::parse_overlay_command(&sent[0]), Some(m));
        assert!(printed.lock().unwrap().is_empty());
    }

    fn capture() -> (App, Arc<Mutex<Vec<String>>>, mpsc::Receiver<Outgoing>) {
        let (tx, rx) = mpsc::channel(8);
        let printed = Arc::new(Mutex::new(Vec::<String>::new()));
        let p = printed.clone();
        let app = App::new(tx, Box::new(move |l| p.lock().unwrap().push(l.into())));
        (app, printed, rx)
    }

    #[test]
    fn identified_prints_line() {
        let (mut app, printed, _rx) = capture();
        for (source, from) in [
            (CallsignSource::Config, "from config"),
            (CallsignSource::Sim, "X-Plane tail number"),
        ] {
            app.server_msg(&ServerMsg::Identified {
                callsign: "SP-ABC".into(),
                aircraft_type: "C172".into(),
                callsign_source: source,
            });
            let last = printed.lock().unwrap().last().cloned().unwrap();
            assert_eq!(last, format!("Aircraft: C172, callsign SP-ABC ({from})"));
        }
    }

    #[test]
    fn unknown_type_is_said() {
        let (mut app, printed, _rx) = capture();
        app.server_msg(&ServerMsg::Identified {
            callsign: "SPABC".into(),
            aircraft_type: "ZZZZ".into(),
            callsign_source: CallsignSource::Sim,
        });
        let last = printed.lock().unwrap().last().cloned().unwrap();
        assert_eq!(
            last,
            "Aircraft: ZZZZ (type unknown), callsign SPABC (X-Plane tail number)"
        );
    }

    #[test]
    fn refusal_printed_once() {
        let (mut app, printed, _rx) = capture();
        app.server_msg(&ServerMsg::Identified {
            callsign: "SP-ABC".into(),
            aircraft_type: "C172".into(),
            callsign_source: CallsignSource::Sim,
        });
        let refused = ServerMsg::AircraftChangeRefused {
            reason: "callsign SP-XYZ is already in use".into(),
        };
        app.server_msg(&refused);
        app.server_msg(&refused);
        let printed = printed.lock().unwrap();
        assert_eq!(
            printed[1..],
            [
                "Aircraft change refused: callsign SP-XYZ is already in use; still flying as SP-ABC (C172)"
            ]
        );
        // Not resending is the connection's job: see `refused_change_is_not_resent` in
        // tests/client_server.rs.
    }

    #[test]
    fn waiting_notice_names_the_address_every_10_s() {
        let (app, printed, _rx) = capture();
        let (tx, _id) = watch::channel(None);
        let mut app = app.with_identity(IdentityTracker::new(None, tx), "127.0.0.1:49000".into());
        let t0 = std::time::Instant::now();
        let s = Duration::from_secs;
        app.identity_tick(t0);
        app.identity_tick(t0 + s(5));
        app.identity_tick(t0 + s(10));
        assert_eq!(
            *printed.lock().unwrap(),
            ["Waiting for X-Plane at 127.0.0.1:49000 for the aircraft type…"; 2]
        );
    }

    #[test]
    fn khz_format() {
        assert_eq!(fmt_khz(118_105), "118.105");
        assert_eq!(fmt_khz(121_500), "121.500");
    }
}
