//! `atc-client [--config <file>] [--text] [--record <track.csv>]`
//!
//! Voice mode (default): microphone and speakers, push-to-talk on the joystick button (hold)
//! or the terminal key (toggle). `--text`: type transmissions on stdin instead; no audio
//! devices or PTT are opened. Both modes read X-Plane 11 for position, COM1 and weather.

use std::io::Write;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, anyhow, bail};
use atc_client::app::{App, Input, spawn_links};
use atc_client::config::ClientConfig;
use atc_client::connection::{Backoff, ConnConfig};
use atc_client::overlay::{DEFAULT_ADDR, Overlay};
use atc_client::ptt::{PttInput, parse_key, spawn_joystick, spawn_keyboard};
use atc_client::record::TrackWriter;
use atc_client::{XpConfig, audio};
use tokio::sync::mpsc;

const USAGE: &str = "usage: atc-client [--config <file>] [--text] [--record <track.csv>]";

struct Args {
    config: PathBuf,
    text: bool,
    record: Option<PathBuf>,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut args = Args {
        config: PathBuf::from("config/client.toml"),
        text: false,
        record: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--text" => args.text = true,
            "--record" => {
                args.record = Some(
                    it.next()
                        .ok_or_else(|| anyhow!("--record needs a file"))?
                        .into(),
                )
            }
            "--config" => {
                args.config = it
                    .next()
                    .ok_or_else(|| anyhow!("--config needs a file"))?
                    .into()
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other:?}\n{USAGE}"),
        }
    }
    Ok(args)
}

/// Raw mode needs `\r\n`; this writer adds the `\r` for log lines.
struct CrLf<W: Write>(W);

impl<W: Write> Write for CrLf<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let fixed = String::from_utf8_lossy(buf).replace('\n', "\r\n");
        self.0.write_all(fixed.as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

fn init_logging(raw_terminal: bool) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if raw_terminal {
        builder.with_writer(|| CrLf(std::io::stderr())).init();
    } else {
        builder.with_writer(std::io::stderr).init();
    }
}

/// Forwards `rx` into the app's inputs as `wrap(item)`.
fn forward<T: Send + 'static>(
    mut rx: mpsc::Receiver<T>,
    inputs: mpsc::Sender<Input>,
    wrap: fn(T) -> Input,
) {
    tokio::spawn(async move {
        while let Some(x) = rx.recv().await {
            if inputs.send(wrap(x)).await.is_err() {
                return;
            }
        }
    });
}

/// The X-Plane overlay at `addr`; `None`, with a warning, when the address is bad.
fn overlay(addr: &str) -> Option<Overlay> {
    let to = match addr.to_socket_addrs().map(|mut a| a.next()) {
        Ok(Some(to)) => to,
        _ => {
            tracing::warn!("overlay address {addr:?} does not resolve; no on-screen text");
            return None;
        }
    };
    Overlay::new(to)
        .inspect_err(|e| tracing::warn!("overlay socket: {e}; no on-screen text"))
        .ok()
}

/// Prints a console line (`\r\n` in raw mode) and sends it to the overlay.
fn printer(overlay: Option<Arc<Overlay>>, raw: bool) -> Box<dyn FnMut(&str) + Send> {
    Box::new(move |s| {
        if raw {
            print!("{s}\r\n");
        } else {
            println!("{s}");
        }
        if let Some(o) = &overlay {
            o.send(s);
        }
    })
}

/// stdin on a plain thread (tokio's stdin would keep the runtime from shutting down).
fn spawn_stdin(inputs: mpsc::Sender<Input>) {
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            if inputs.blocking_send(Input::Line(line)).is_err() {
                return;
            }
        }
        let _ = inputs.blocking_send(Input::Eof);
    });
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    let cfg = ClientConfig::load(&args.config)?;
    let ptt_key = parse_key(&cfg.ptt.key)?;
    init_logging(!args.text);

    let xplane = cfg
        .xplane
        .to_socket_addrs()
        .with_context(|| format!("resolving xplane address {:?}", cfg.xplane))?
        .next()
        .ok_or_else(|| anyhow!("xplane address {:?} resolves to nothing", cfg.xplane))?;
    let conn = ConnConfig {
        url: cfg.server.clone(),
        callsign: cfg.callsign.clone(),
        aircraft_type: cfg.aircraft_type.clone(),
        backoff: Backoff::default(),
    };
    let overlay = overlay(cfg.overlay.as_deref().unwrap_or(DEFAULT_ADDR)).map(Arc::new);
    // Follow-me car commands go to the plugin on the overlay socket.
    let plugin = |overlay: &Option<Arc<Overlay>>| -> Box<dyn FnMut(&str) + Send> {
        let o = overlay.clone();
        Box::new(move |line| {
            if let Some(o) = &o {
                o.send(line);
            }
        })
    };
    let (inputs, inputs_rx) = mpsc::channel(1024);
    let to_conn = spawn_links(
        conn,
        XpConfig::new(xplane, cfg.field_elev_ft),
        inputs.clone(),
    );

    let recorder = match &args.record {
        Some(path) => {
            let file = std::fs::File::create(path)
                .with_context(|| format!("creating {}", path.display()))?;
            let out: Box<dyn Write + Send> = Box::new(std::io::BufWriter::new(file));
            Some(TrackWriter::new(out)?)
        }
        None => None,
    };

    if args.text {
        let mut app =
            App::new(to_conn, printer(overlay.clone(), false)).with_plugin(plugin(&overlay));
        if let Some(r) = recorder {
            app = app.with_recorder(r);
        }
        println!(
            "{} text mode: type a transmission and press Enter (sent on the transmit radio); Ctrl-D quits",
            cfg.callsign
        );
        spawn_stdin(inputs);
        app.run(inputs_rx).await;
        return Ok(());
    }

    let (_speakers, playback) = audio::start_playback(cfg.output_device.as_deref())?;
    let (mic_tx, mic_rx) = mpsc::channel(64);
    let _mic = audio::start_capture(cfg.input_device.as_deref(), mic_tx)?;
    forward(mic_rx, inputs.clone(), Input::Mic);
    let (ptt_tx, ptt_rx) = mpsc::channel::<PttInput>(64);
    if let Some(button) = cfg.ptt.joystick_button {
        spawn_joystick(button, ptt_tx.clone());
    }
    let _raw = spawn_keyboard(ptt_key, ptt_tx)?;
    forward(ptt_rx, inputs, Input::Ptt);

    let mut app = App::new(to_conn, printer(overlay.clone(), true))
        .with_plugin(plugin(&overlay))
        .with_playback(playback);
    if let Some(r) = recorder {
        app = app.with_recorder(r);
    }
    let joystick = cfg
        .ptt
        .joystick_button
        .map(|b| format!(", hold joystick button {b}"))
        .unwrap_or_default();
    print!(
        "{} voice mode: press {:?} to start and again to stop transmitting{joystick}; q, Esc or Ctrl-C quits\r\n",
        cfg.callsign, cfg.ptt.key
    );
    app.run(inputs_rx).await;
    Ok(())
}
