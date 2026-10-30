//! WebSocket link to the server with automatic reconnection.
//!
//! [`run_connection`] connects, says `Hello`, and after `Welcome` re-sends the latest
//! weather and telemetry so the server re-tunes the client. When the link drops (or a
//! connection attempt fails) it waits per [`Backoff`] and tries again. While disconnected,
//! telemetry and weather only update the "latest" copy; PTT, audio and text are dropped.

use std::time::Duration;

use atc_proto::{
    AudioFrame, CallsignSource, ClientMsg, PROTOCOL_VERSION, ServerMsg, TelemetryFrame,
    WeatherFrame,
};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

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
    pub callsign: String,
    pub aircraft_type: String,
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

/// Runs until `from_ui` is closed or `to_ui` is dropped.
pub async fn run_connection(
    mut cfg: ConnConfig,
    mut from_ui: mpsc::Receiver<Outgoing>,
    to_ui: mpsc::Sender<ServerEvent>,
) -> anyhow::Result<()> {
    let mut latest = Latest::default();
    loop {
        let attempt = tokio::time::timeout(
            CONNECT_TIMEOUT,
            tokio_tungstenite::connect_async(cfg.url.as_str()),
        );
        match attempt.await {
            Ok(Ok((ws, _))) => {
                tracing::debug!(url = %cfg.url, "connected");
                if let End::Exit = session(ws, &mut cfg, &mut latest, &mut from_ui, &to_ui).await {
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

async fn session(
    mut ws: Ws,
    cfg: &mut ConnConfig,
    latest: &mut Latest,
    from_ui: &mut mpsc::Receiver<Outgoing>,
    to_ui: &mpsc::Sender<ServerEvent>,
) -> End {
    let hello = ClientMsg::Hello {
        protocol: PROTOCOL_VERSION,
        callsign: cfg.callsign.clone(),
        aircraft_type: cfg.aircraft_type.clone(),
        callsign_source: CallsignSource::Config,
    };
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
                if let ServerEvent::Msg(ServerMsg::Welcome { .. }) = event {
                    welcomed = true;
                    cfg.backoff.reset();
                    for m in latest.messages() {
                        if !send(&mut ws, Outgoing::Msg(m)).await {
                            return End::Lost;
                        }
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
