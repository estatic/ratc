//! `atc-server`: loads the airport data and the speech models and serves `GET /ws`.
//!
//! Usage: `atc-server [--config <path>] [--fresh]` (default `config/server.toml`). The flights
//! in progress are kept across restarts ([`atc_server::persist`]); `--fresh` starts without
//! them. Exit code 2 when a configured model is missing.
//!
//! `atc-server airports [--config <path>]` runs the interactive airport wizard (stations from
//! OpenAIP) instead of serving.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use atc_core::engine::{AtcEngine, EngineConfig};
use atc_core::readback::Strictness;
use atc_phraseology::{Lang, station_name};
use atc_server::config::{ServerConfig, StrictnessSetting, check_models, load_config};
use atc_server::hub::Hub;
use atc_server::network::Network;
use atc_server::stations::load_stations;
use atc_server::{AppState, StationMap, SttPrompts, persist, router};
use atc_voice::{PiperTts, VoiceError, WhisperStt, initial_prompt, load_voices_config};
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG: &str = "config/server.toml";
const EXIT_MODEL_MISSING: u8 = 2;

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Serve { config: PathBuf, fresh: bool },
    Airports { config: PathBuf },
}

const USAGE: &str =
    "usage: atc-server [--config <path>] [--fresh] | atc-server airports [--config <path>]";

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command> {
    let mut args = args.into_iter().peekable();
    let airports = args.peek().map(String::as_str) == Some("airports");
    if airports {
        args.next();
    }
    let mut config = PathBuf::from(DEFAULT_CONFIG);
    let mut fresh = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => config = args.next().context("--config needs a path")?.into(),
            "--fresh" if !airports => fresh = true,
            other => bail!("unknown argument {other:?} ({USAGE})"),
        }
    }
    Ok(if airports {
        Command::Airports { config }
    } else {
        Command::Serve { config, fresh }
    })
}

fn read_config(path: &Path) -> Result<ServerConfig> {
    let text = std::fs::read_to_string(path).with_context(|| missing_hint("read config", path))?;
    load_config(&text)
}

/// `"<verb> <path>"`, plus how to create the file from its `.example.toml` when it is missing.
fn missing_hint(verb: &str, path: &Path) -> String {
    let example = path.with_extension("example.toml");
    if !path.exists() && example.exists() {
        format!(
            "{verb} {}: not found; copy {} to {} and edit it",
            path.display(),
            example.display(),
            path.display()
        )
    } else {
        format!("{verb} {}", path.display())
    }
}

/// Runs the airport wizard on the console (blocking).
fn run_wizard(config: &Path) -> Result<()> {
    use atc_stations::openaip::HttpApi;
    use atc_stations::prompt::Prompt;
    use atc_stations::store::{Store, config_dir};
    use atc_stations::wizard::{WizardEnv, run};
    let cfg = read_config(config)?;
    let store =
        Store::default_location().context("no home directory for the OpenAIP data store")?;
    let key_dir = config_dir().context("no home directory for the OpenAIP key")?;
    let env = WizardEnv::new(cfg.data_dir, cfg.xplane_root, store, key_dir);
    let stdin = std::io::stdin();
    let mut prompt = Prompt::new(stdin.lock(), std::io::stdout());
    run(&env, |key| Box::new(HttpApi::new(key)), &mut prompt)?;
    Ok(())
}

/// Restores every station's saved flights unless `fresh`.
fn restore_all(network: &Network, fresh: bool) {
    for s in network.stations() {
        restore_state(&s.engine, &persist::state_path(&s.id), fresh);
    }
}

/// Restores the saved flights unless `fresh`; logs what happened.
fn restore_state(engine: &Mutex<AtcEngine>, path: &Path, fresh: bool) {
    if fresh {
        tracing::info!("--fresh: saved state at {} not restored", path.display());
        return;
    }
    match persist::load(path, persist::MAX_AGE, SystemTime::now()) {
        persist::Loaded::Snapshot(s) => match engine.lock().unwrap().restore(*s) {
            Ok(()) => tracing::info!("flights in progress restored from {}", path.display()),
            Err(e) => tracing::warn!("saved state at {} not restored: {e}", path.display()),
        },
        persist::Loaded::Skipped(why) => tracing::info!("starting afresh ({why})"),
    }
}

fn build_state(cfg: &ServerConfig) -> Result<AppState> {
    let strictness = match cfg.strictness {
        StrictnessSetting::Standard => Strictness::Standard,
        StrictnessSetting::Training => Strictness::Training,
    };
    let engine_cfg = EngineConfig {
        language: cfg.whisper.language_mode()?,
        ..EngineConfig::new(strictness)
    };
    let loaded = load_stations(
        &cfg.data_dir,
        cfg.airport.as_deref(),
        cfg.xplane_root.as_deref(),
        &engine_cfg,
    )?;
    let ids: Vec<&str> = loaded.iter().map(|s| s.id.as_str()).collect();
    tracing::info!("stations: {}", ids.join(", "));
    let stations = StationMap::union(loaded.iter().map(|s| &s.procs));
    // The prompt is in the language the aircraft speaks, with the stations as said in it
    // (every station's for now).
    let prompt_in = |lang: Lang, code: &str| {
        let airports: Vec<&str> = loaded.iter().map(|s| s.airport.name.as_str()).collect();
        let names: Vec<String> = loaded
            .iter()
            .flat_map(|s| &s.procs.positions)
            .map(|p| station_name(&p.callsign, lang))
            .collect();
        initial_prompt(&airports.join(", "), &names, &[], code)
    };
    let stt_prompts = SttPrompts {
        en: prompt_in(Lang::En, "en"),
        pl: prompt_in(Lang::Pl, "pl"),
    };

    let stt = WhisperStt::new(&cfg.whisper.model, cfg.whisper.language.as_deref())
        .context("load whisper")?;
    let voices_text = std::fs::read_to_string(&cfg.piper.voices)
        .with_context(|| format!("read {}", cfg.piper.voices.display()))?;
    let voices = load_voices_config(&voices_text, Path::new("."))?;
    let tts = PiperTts::new(cfg.piper.bin.clone(), voices).context("load piper")?;
    let state = AppState {
        network: Arc::new(Network::new(loaded)),
        hub: Arc::new(Mutex::new(Hub::default())),
        stt: Arc::new(stt),
        tts: Arc::new(tts),
        stations,
        stt_prompts,
    };
    Ok(state)
}

async fn run() -> Result<()> {
    let (path, fresh) = match parse_args(std::env::args().skip(1))? {
        Command::Serve { config, fresh } => (config, fresh),
        Command::Airports { config } => return run_wizard(&config),
    };
    let cfg = read_config(&path)?;
    check_models(&cfg).map_err(|e| anyhow::Error::new(ModelsMissing(e)))?;
    // Model loading is blocking (seconds on a cold start).
    let built = tokio::task::spawn_blocking(move || build_state(&cfg).map(|s| (s, cfg))).await??;
    let (state, cfg) = built;
    let listener = tokio::net::TcpListener::bind(&cfg.listen)
        .await
        .with_context(|| format!("bind {}", cfg.listen))?;
    let network = state.network.clone();
    restore_all(&network, fresh);
    let saver = tokio::spawn(persist::save_loop(network.clone()));
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    saver.abort();
    persist::save_all(&network, &mut HashMap::new());
    tracing::info!("state saved to {}", persist::STATE_DIR);
    Ok(())
}

#[derive(Debug)]
struct ModelsMissing(String);

impl std::fmt::Display for ModelsMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ModelsMissing {}

/// A model file that turned out missing only when the engine tried to load it.
fn is_model_missing(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.is::<ModelsMissing>()
            || matches!(
                c.downcast_ref::<VoiceError>(),
                Some(VoiceError::ModelMissing(_))
            )
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if std::env::args().nth(1).as_deref() == Some("airports") {
        // The wizard owns stdout; log lines go to stderr as plain text (bare Windows console).
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if is_model_missing(&e) => {
            // The startup check's message already names the fetch script; a model that failed
            // to load later (e.g. a voice file) needs the generic hint.
            if e.chain().any(|c| c.is::<ModelsMissing>()) {
                eprintln!("error: {e:#}");
            } else {
                eprintln!(
                    "error: {e:#}\nmissing model: run tools/fetch-whisper.sh / tools/fetch-piper.sh"
                );
            }
            ExitCode::from(EXIT_MODEL_MISSING)
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(a: &[&str]) -> Result<Command> {
        parse_args(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn airports_subcommand_parses() {
        assert_eq!(
            parse(&["airports"]).unwrap(),
            Command::Airports {
                config: DEFAULT_CONFIG.into()
            }
        );
        assert_eq!(
            parse(&["airports", "--config", "x.toml"]).unwrap(),
            Command::Airports {
                config: "x.toml".into()
            }
        );
        assert_eq!(
            parse(&["--fresh"]).unwrap(),
            Command::Serve {
                config: DEFAULT_CONFIG.into(),
                fresh: true
            }
        );
        assert!(parse(&["airports", "--fresh"]).is_err());
        assert!(parse(&["--config"]).is_err());
    }
}
