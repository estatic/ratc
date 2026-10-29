//! One WebSocket client: handshake, telemetry/weather, PTT with relay, transmissions.
//!
//! Each session is one task reading the socket, plus a writer task fed by an unbounded
//! channel (so emits from other sessions never block) and a transmission worker
//! ([`pipeline::worker`]) that runs STT/engine/TTS in order.

use std::sync::Arc;

use atc_core::strip::PilotProfile;
use atc_phraseology::Callsign;
use atc_proto::{AudioFrame, ClientMsg, PROTOCOL_VERSION, ServerMsg, TelemetryFrame, WeatherFrame};
use axum::extract::ws::{Message, WebSocket};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::app::{Out, Server, Tx};
use crate::hub::{ClientId, PttOutcome};
use crate::pipeline::{Job, PILOT_RATE};
use crate::{atis, convert, pipeline};

/// The PTT this client holds.
struct Ptt {
    freq_khz: u32,
    pcm: Vec<i16>,
    /// Listeners that got `RelayStart` (and get `RelayEnd`); frames go only to those of them
    /// still tuned to `freq_khz`.
    relay_to: Vec<ClientId>,
}

struct Session {
    server: Arc<Server>,
    id: ClientId,
    callsign: String,
    cs: Callsign,
    tx: Tx,
    jobs: mpsc::UnboundedSender<Job>,
    /// The frequencies heard ([`TelemetryFrame::heard_khz`]).
    tuned: Vec<u32>,
    /// Sim time of the latest unpaused telemetry.
    last_t: f64,
    ptt: Option<Ptt>,
}

pub(crate) async fn run(server: Arc<Server>, socket: WebSocket) {
    let (sink, mut stream) = socket.split();
    let (tx, rx) = mpsc::unbounded_channel();
    let writer = tokio::spawn(write_loop(sink, rx));
    if let Some(mut s) = handshake(server, &mut stream, tx.clone()).await {
        s.read_loop(&mut stream).await;
        s.leave();
    }
    drop(tx);
    let _ = writer.await;
}

async fn write_loop(mut sink: SplitSink<WebSocket, Message>, mut rx: mpsc::UnboundedReceiver<Out>) {
    while let Some(out) = rx.recv().await {
        let msg = match out {
            Out::Msg(m) => Message::Text(
                serde_json::to_string(&m)
                    .expect("ServerMsg serializes")
                    .into(),
            ),
            Out::Audio(f) => Message::Binary(f.encode().into()),
            Out::Close => {
                let _ = sink.close().await;
                return;
            }
        };
        if sink.send(msg).await.is_err() {
            return;
        }
    }
}

fn reject(tx: &Tx, reason: String) -> Option<Session> {
    tracing::info!(%reason, "client rejected");
    let _ = tx.send(Out::Msg(ServerMsg::Rejected { reason }));
    let _ = tx.send(Out::Close);
    None
}

/// The first text message must be a `Hello` with our protocol and a usable, unique
/// callsign; otherwise `Rejected` and close.
async fn handshake(
    server: Arc<Server>,
    stream: &mut SplitStream<WebSocket>,
    tx: Tx,
) -> Option<Session> {
    let first = loop {
        match stream.next().await? {
            Ok(Message::Text(t)) => break t,
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    };
    let (callsign, aircraft_type) = match serde_json::from_str::<ClientMsg>(&first) {
        Ok(ClientMsg::Hello {
            protocol,
            callsign,
            aircraft_type,
        }) => {
            if protocol != PROTOCOL_VERSION {
                return reject(
                    &tx,
                    format!("unsupported protocol {protocol}, server speaks {PROTOCOL_VERSION}"),
                );
            }
            (callsign.trim().to_string(), aircraft_type)
        }
        Ok(_) => return reject(&tx, "expected hello".into()),
        Err(e) => return reject(&tx, format!("bad message: {e}")),
    };
    let cs = Callsign::new(&callsign);
    if cs.0.is_empty() {
        return reject(&tx, format!("invalid callsign {callsign:?}"));
    }
    let id = server.next_client_id();
    if let Err(e) = server.state.hub.lock().unwrap().join(id, cs.0.clone()) {
        return reject(&tx, format!("{e}: {callsign}"));
    }
    server.state.network.register_pilot(PilotProfile {
        callsign: cs.clone(),
        aircraft_type,
    });
    server.weather_source.lock().unwrap().get_or_insert(id);
    let _ = tx.send(Out::Msg(ServerMsg::Welcome {
        protocol: PROTOCOL_VERSION,
        session: id.to_string(),
    }));
    server.add_client(id, tx.clone());
    let (jobs, rx) = mpsc::unbounded_channel();
    tokio::spawn(pipeline::worker(server.clone(), id, callsign.clone(), rx));
    tracing::info!(id, %callsign, "client joined");
    Some(Session {
        server,
        id,
        callsign,
        cs,
        tx,
        jobs,
        tuned: Vec::new(),
        last_t: 0.0,
        ptt: None,
    })
}

impl Session {
    async fn read_loop(&mut self, stream: &mut SplitStream<WebSocket>) {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(t) => match serde_json::from_str::<ClientMsg>(&t) {
                    Ok(m) => self.on_msg(m),
                    Err(e) => tracing::warn!(id = self.id, %e, "bad client message"),
                },
                Message::Binary(b) => match AudioFrame::decode(&b) {
                    Ok(f) => self.on_audio(f),
                    Err(e) => tracing::warn!(id = self.id, %e, "bad audio frame"),
                },
                Message::Close(_) => break,
                _ => {}
            }
        }
    }

    fn on_msg(&mut self, m: ClientMsg) {
        let canonical = |khz| self.server.state.stations.canonical(khz);
        match m {
            ClientMsg::Hello { .. } => tracing::warn!(id = self.id, "repeated hello ignored"),
            ClientMsg::Telemetry(mut f) => {
                f.com1_khz = canonical(f.com1_khz);
                for k in &mut f.listen_khz {
                    *k = canonical(*k);
                }
                self.on_telemetry(f)
            }
            ClientMsg::Weather(w) => self.on_weather(w),
            ClientMsg::PttStart { freq_khz } => self.on_ptt_start(canonical(freq_khz)),
            ClientMsg::PttEnd => self.on_ptt_end(),
            ClientMsg::TextTransmission { freq_khz, text } => {
                let _ = self.jobs.send(Job::Text {
                    freq_khz: canonical(freq_khz),
                    text,
                    t: self.last_t,
                });
            }
        }
    }

    /// Tunes the hub to every frequency heard (also while paused); a radio newly tuned to
    /// an ATIS plays it. Unpaused frames go to the network.
    fn on_telemetry(&mut self, f: TelemetryFrame) {
        let server = &self.server;
        let heard = f.heard_khz();
        server.state.hub.lock().unwrap().tune(self.id, &heard);
        let prev = std::mem::replace(&mut self.tuned, heard);
        for atis in server.state.network.atis_khzs() {
            if self.tuned.contains(&atis) && !prev.contains(&atis) {
                tokio::spawn(atis::send(server.clone(), atis, vec![self.id]));
            }
        }
        if f.paused {
            return;
        }
        self.last_t = f.t;
        let network = &server.state.network;
        let outputs = network.on_telemetry(convert::aircraft_state(&self.cs, &f));
        let orders = network.take_vehicle_orders();
        server.send_vehicle_orders(orders);
        if !outputs.is_empty() {
            let server = server.clone();
            tokio::spawn(async move { pipeline::emit_all(&server, outputs).await });
        }
    }

    /// Only the weather source's reports are used (by every station); a new ATIS letter goes
    /// to that ATIS's listeners at once.
    fn on_weather(&mut self, w: WeatherFrame) {
        let server = &self.server;
        {
            let mut source = server.weather_source.lock().unwrap();
            if source.is_some_and(|s| s != self.id) {
                return;
            }
            *source = Some(self.id);
        }
        for khz in server.state.network.on_weather(convert::weather(&w)) {
            tokio::spawn(atis::send(server.clone(), khz, server.listeners(khz)));
        }
    }

    fn on_ptt_start(&mut self, freq_khz: u32) {
        let (outcome, listeners) = {
            let mut hub = self.server.state.hub.lock().unwrap();
            (hub.ptt_start(self.id, freq_khz), hub.listeners(freq_khz))
        };
        match outcome {
            PttOutcome::Blocked => {
                let _ = self
                    .tx
                    .send(Out::Msg(ServerMsg::FrequencyBlocked { freq_khz }));
            }
            // Repeated start on the held frequency: keep the current transmission.
            PttOutcome::Granted if self.ptt.is_some() => {}
            PttOutcome::Granted => {
                let relay_to: Vec<_> = listeners.into_iter().filter(|&l| l != self.id).collect();
                self.server.send(
                    &relay_to,
                    Out::Msg(ServerMsg::RelayStart {
                        from: self.callsign.clone(),
                        freq_khz,
                        sample_rate: PILOT_RATE,
                    }),
                );
                self.ptt = Some(Ptt {
                    freq_khz,
                    pcm: Vec::new(),
                    relay_to,
                });
            }
        }
    }

    /// Own PTT audio (stream 0) is buffered and relayed unchanged; anything else is ignored.
    fn on_audio(&mut self, f: AudioFrame) {
        let Some(ptt) = self.ptt.as_mut().filter(|_| f.stream == 0) else {
            return;
        };
        ptt.pcm.extend_from_slice(&f.pcm);
        // Only those of the RelayStart snapshot who are still tuned to the frequency.
        let current = self.server.listeners(ptt.freq_khz);
        let targets: Vec<_> = ptt
            .relay_to
            .iter()
            .copied()
            .filter(|l| current.contains(l))
            .collect();
        self.server.send(&targets, Out::Audio(f));
    }

    fn on_ptt_end(&mut self) {
        self.server.state.hub.lock().unwrap().ptt_end(self.id);
        let Some(ptt) = self.end_relay() else {
            return;
        };
        let _ = self.jobs.send(Job::Voice {
            freq_khz: ptt.freq_khz,
            pcm: ptt.pcm,
            t: self.last_t,
        });
    }

    /// Takes the PTT and sends `RelayEnd` to its relay listeners.
    fn end_relay(&mut self) -> Option<Ptt> {
        let ptt = self.ptt.take()?;
        self.server.send(
            &ptt.relay_to,
            Out::Msg(ServerMsg::RelayEnd {
                from: self.callsign.clone(),
            }),
        );
        Some(ptt)
    }

    /// Disconnect: releases the PTT lock (an unfinished transmission is discarded) and the
    /// weather source. The engines keep the pilot's strip.
    fn leave(mut self) {
        self.server.state.hub.lock().unwrap().leave(self.id);
        self.end_relay();
        self.server.remove_client(self.id);
        let mut source = self.server.weather_source.lock().unwrap();
        if *source == Some(self.id) {
            *source = None;
        }
        tracing::info!(id = self.id, callsign = %self.callsign, "client left");
    }
}
