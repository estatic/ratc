//! WebSocket link to the server with automatic reconnection.
//!
//! [`run_connection`] waits for the aircraft identity, connects, says `Hello`, and after
//! `Welcome` re-sends the latest weather and telemetry so the server re-tunes the client.
//! A changed identity while connected goes out as `AircraftChanged`. When the link drops (or a
//! connection attempt fails) it waits per [`Backoff`] and tries again. While disconnected,
//! telemetry and weather only update the "latest" copy; PTT, audio and text are dropped.

use std::time::Duration;

use atc_proto::{AudioFrame, ClientMsg, PROTOCOL_VERSION, ServerMsg, TelemetryFrame, WeatherFrame};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::aircraft::Identity;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A connection attempt (TCP + WebSocket handshake) taking longer than this fails.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Reconnect delays: doubling from `first` up to `max`; [`Backoff::reset`] after a
/// successful `Welcome`. The default is 0.5 → 1 → 2 → 4 → 8 s, then 8 s.
#[derive(Clone, Debug)]
pub struct Backoff {
    first: Duration,
    max: Duration,
    next: Duration,
}

impl Backoff {
    pub fn new(first: Duration, max: Duration) -> Self {
        Backoff {
            first,
            max,
            next: first,
        }
    }

    /// The delay before the next attempt; each call doubles the following one.
    pub fn next_delay(&mut self) -> Duration {
        let d = self.next;
        self.next = (d * 2).min(self.max);
        d
    }

    pub fn reset(&mut self) {
        self.next = self.first;
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff::new(Duration::from_millis(500), Duration::from_secs(8))
    }
}

#[derive(Clone, Debug)]
pub struct ConnConfig {
    /// e.g. `ws://127.0.0.1:8740/ws`
    pub url: String,
    pub backoff: Backoff,
}

/// What the client sends.
#[derive(Clone, Debug, PartialEq)]
pub enum Outgoing {
    Msg(ClientMsg),
    Audio(AudioFrame),
}

/// What the connection reports.
#[derive(Clone, Debug, PartialEq)]
pub enum ServerEvent {
    Msg(ServerMsg),
    Audio(AudioFrame),
    /// `Welcome` received (sent right after it; the `Welcome` itself follows as `Msg`).
    Connected,
    /// A welcomed session ended; reconnecting.
    Disconnected,
}

/// The latest state messages, re-sent after every `Welcome`.
#[derive(Default)]
struct Latest {
    weather: Option<WeatherFrame>,
    telemetry: Option<TelemetryFrame>,
    /// What the server said about our identity; survives reconnects like the rest.
    known: Known,
}

impl Latest {
    fn note(&mut self, out: &Outgoing) {
        match out {
            Outgoing::Msg(ClientMsg::Telemetry(t)) => self.telemetry = Some(t.clone()),
            Outgoing::Msg(ClientMsg::Weather(w)) => self.weather = Some(w.clone()),
            _ => {}
        }
    }

    fn messages(&self) -> impl Iterator<Item = ClientMsg> {
        let w = self.weather.clone().map(ClientMsg::Weather);
        let t = self.telemetry.clone().map(ClientMsg::Telemetry);
        w.into_iter().chain(t)
    }
}

enum End {
    /// The UI went away: stop.
    Exit,
    /// The link dropped: reconnect.
    Lost,
}

/// Waits until an identity is published; `None` when the UI side went away first.
async fn wait_identity(
    identity: &mut watch::Receiver<Option<Identity>>,
    latest: &mut Latest,
    from_ui: &mut mpsc::Receiver<Outgoing>,
    to_ui: &mpsc::Sender<ServerEvent>,
) -> Option<Identity> {
    loop {
        if let Some(id) = identity.borrow_and_update().clone() {
            return Some(id);
        }
        tokio::select! {
            c = identity.changed() => c.ok()?,
            m = from_ui.recv() => note_offline(latest, &m?),
            _ = to_ui.closed() => return None,
        }
    }
}

/// Runs until `from_ui` is closed or `to_ui` is dropped. Connects only once `identity` holds
/// a value; every (re)connect says `Hello` with the latest one.
pub async fn run_connection(
    mut cfg: ConnConfig,
    mut identity: watch::Receiver<Option<Identity>>,
    mut from_ui: mpsc::Receiver<Outgoing>,
    to_ui: mpsc::Sender<ServerEvent>,
) -> anyhow::Result<()> {
    let mut latest = Latest::default();
    loop {
        let Some(id) = wait_identity(&mut identity, &mut latest, &mut from_ui, &to_ui).await else {
            return Ok(());
        };
        // A refused identity must not be presented again: keep the accepted one.
        let id = match (&latest.known.refused, &latest.known.accepted) {
            (Some(r), Some(a)) if *r == id => a.clone(),
            _ => id,
        };
        let attempt = tokio::time::timeout(
            CONNECT_TIMEOUT,
            tokio_tungstenite::connect_async(cfg.url.as_str()),
        );
        match attempt.await {
            Ok(Ok((ws, _))) => {
                tracing::debug!(url = %cfg.url, "connected");
                if let End::Exit = session(
                    ws,
                    &mut cfg,
                    id,
                    &mut identity,
                    &mut latest,
                    &mut from_ui,
                    &to_ui,
                )
                .await
                {
                    return Ok(());
                }
            }
            Ok(Err(e)) => tracing::warn!(url = %cfg.url, "connection failed: {e}"),
            Err(_) => tracing::warn!(url = %cfg.url, "connection timed out"),
        }
        let delay = cfg.backoff.next_delay();
        tracing::info!("reconnecting in {:.1} s", delay.as_secs_f64());
        let sleep = tokio::time::sleep(delay);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => break,
                m = from_ui.recv() => match m {
                    None => return Ok(()),
                    Some(m) => note_offline(&mut latest, &m),
                },
                _ = to_ui.closed() => return Ok(()),
            }
        }
    }
}

fn note_offline(latest: &mut Latest, m: &Outgoing) {
    latest.note(m);
    if !matches!(
        m,
        Outgoing::Msg(ClientMsg::Telemetry(_) | ClientMsg::Weather(_))
    ) {
        tracing::debug!("not connected, dropping {m:?}");
    }
}

async fn send(ws: &mut Ws, out: Outgoing) -> bool {
    let msg = match out {
        Outgoing::Msg(m) => Message::text(serde_json::to_string(&m).expect("ClientMsg serializes")),
        Outgoing::Audio(f) => Message::binary(f.encode()),
    };
    ws.send(msg).await.is_ok()
}

/// What the server has said about our identity, kept across reconnects.
#[derive(Default)]
struct Known {
    /// The last identity the server accepted (`Identified`).
    accepted: Option<Identity>,
    /// An identity the server refused: not sent again, and not used for `Hello` while the
    /// sim still reports it, so a reconnect keeps the old flight.
    refused: Option<Identity>,
    /// Identities sent (`Hello`, `AircraftChanged`) that the server has not answered yet.
    pending: std::collections::VecDeque<Identity>,
}

/// Tells the server about the current identity when it differs from `sent` and was not
/// refused.
async fn announce(
    ws: &mut Ws,
    identity: &mut watch::Receiver<Option<Identity>>,
    sent: &mut Identity,
    known: &mut Known,
) -> bool {
    let Some(now) = identity.borrow_and_update().clone() else {
        return true;
    };
    if now.same_flight(sent) || known.refused.as_ref().is_some_and(|r| now.same_flight(r)) {
        return true;
    }
    let msg = ClientMsg::AircraftChanged {
        callsign: now.callsign.clone(),
        aircraft_type: now.aircraft_type.clone(),
        callsign_source: now.source,
    };
    known.pending.push_back(now.clone());
    *sent = now;
    send(ws, Outgoing::Msg(msg)).await
}

async fn session(
    mut ws: Ws,
    cfg: &mut ConnConfig,
    mut sent: Identity,
    identity: &mut watch::Receiver<Option<Identity>>,
    latest: &mut Latest,
    from_ui: &mut mpsc::Receiver<Outgoing>,
    to_ui: &mpsc::Sender<ServerEvent>,
) -> End {
    let hello = ClientMsg::Hello {
        protocol: PROTOCOL_VERSION,
        callsign: sent.callsign.clone(),
        aircraft_type: sent.aircraft_type.clone(),
        callsign_source: sent.source,
    };
    latest.known.pending.clear();
    latest.known.pending.push_back(sent.clone());
    if !send(&mut ws, Outgoing::Msg(hello)).await {
        return End::Lost;
    }
    let mut welcomed = false;
    let end = loop {
        tokio::select! {
            m = from_ui.recv() => {
                let Some(m) = m else {
                    let _ = ws.close(None).await;
                    break End::Exit;
                };
                if !welcomed {
                    note_offline(latest, &m);
                    continue;
                }
                latest.note(&m);
                if !send(&mut ws, m).await {
                    break End::Lost;
                }
            }
            c = identity.changed() => {
                if c.is_err() {
                    break End::Exit;
                }
                if welcomed && !announce(&mut ws, identity, &mut sent, &mut latest.known).await {
                    break End::Lost;
                }
            }
            incoming = ws.next() => {
                let event = match incoming {
                    Some(Ok(Message::Text(t))) => match serde_json::from_str::<ServerMsg>(&t) {
                        Ok(m) => ServerEvent::Msg(m),
                        Err(e) => {
                            tracing::warn!("bad server message: {e}");
                            continue;
                        }
                    },
                    Some(Ok(Message::Binary(b))) => match AudioFrame::decode(&b) {
                        Ok(f) => ServerEvent::Audio(f),
                        Err(e) => {
                            tracing::warn!("bad audio frame: {e}");
                            continue;
                        }
                    },
                    Some(Ok(Message::Close(_))) | None => break End::Lost,
                    Some(Err(e)) => {
                        tracing::warn!("connection error: {e}");
                        break End::Lost;
                    }
                    Some(Ok(_)) => continue,
                };
                match &event {
                    ServerEvent::Msg(ServerMsg::Identified { callsign, aircraft_type, callsign_source }) => {
                        let id = Identity {
                            callsign: callsign.clone(),
                            aircraft_type: aircraft_type.clone(),
                            source: *callsign_source,
                        };
                        latest.known.pending.pop_front();
                        if latest.known.pending.is_empty() {
                            sent = id.clone();
                        }
                        latest.known.accepted = Some(id);
                        latest.known.refused = None;
                    }
                    ServerEvent::Msg(ServerMsg::AircraftChangeRefused { .. }) => {
                        latest.known.refused = latest.known.pending.pop_front();
                        if latest.known.pending.is_empty()
                            && let Some(a) = &latest.known.accepted
                        {
                            sent = a.clone();
                        }
                    }
                    _ => {}
                }
                if let ServerEvent::Msg(ServerMsg::Welcome { .. }) = event {
                    welcomed = true;
                    cfg.backoff.reset();
                    for m in latest.messages() {
                        if !send(&mut ws, Outgoing::Msg(m)).await {
                            return End::Lost;
                        }
                    }
                    // The identity may have changed while the server was still welcoming us.
                    if !announce(&mut ws, identity, &mut sent, &mut latest.known).await {
                        return End::Lost;
                    }
                    if to_ui.send(ServerEvent::Connected).await.is_err() {
                        return End::Exit;
                    }
                }
                if to_ui.send(event).await.is_err() {
                    return End::Exit;
                }
            }
        }
    };
    if welcomed && to_ui.send(ServerEvent::Disconnected).await.is_err() {
        return End::Exit;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_sequence_and_reset() {
        let ms = Duration::from_millis;
        let mut b = Backoff::default();
        let seq: Vec<_> = (0..7).map(|_| b.next_delay()).collect();
        assert_eq!(
            seq,
            [
                ms(500),
                ms(1000),
                ms(2000),
                ms(4000),
                ms(8000),
                ms(8000),
                ms(8000)
            ]
        );
        b.reset();
        assert_eq!(b.next_delay(), ms(500));
        assert_eq!(b.next_delay(), ms(1000));
    }

    #[test]
    fn latest_keeps_only_state_messages() {
        let mut l = Latest::default();
        l.note(&Outgoing::Msg(ClientMsg::PttStart { freq_khz: 1 }));
        assert_eq!(l.messages().count(), 0);
        let w = WeatherFrame {
            wind_dir_deg: 250,
            wind_speed_kt: 8,
            visibility_m: 10_000,
            clouds: vec![],
            temp_c: 15,
            dewpoint_c: 10,
            qnh_hpa: 1013,
        };
        l.note(&Outgoing::Msg(ClientMsg::Weather(w.clone())));
        let mut w2 = w.clone();
        w2.qnh_hpa = 1000;
        l.note(&Outgoing::Msg(ClientMsg::Weather(w2.clone())));
        assert_eq!(l.messages().collect::<Vec<_>>(), [ClientMsg::Weather(w2)]);
    }
}
