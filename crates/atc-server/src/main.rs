//! `atc-server`: loads the airport data and the speech models and serves `GET /ws`.
//!
//! Usage: `atc-server [--config <path>] [--fresh]` (default `config/server.toml`). The flights
//! in progress are kept across restarts ([`atc_server::persist`]); `--fresh` starts without
//! them. Exit code 2 when a configured model is missing.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use atc_core::engine::{AtcEngine, EngineConfig};
use atc_core::readback::Strictness;
use atc_navdata::procedures::parse_procedures;
use atc_phraseology::{Lang, station_name};
use atc_server::config::{ServerConfig, StrictnessSetting, check_models, load_config};
use atc_server::hub::Hub;
use atc_server::layout::{
    adjacent_names, load_airport, load_destinations_near, load_procedure_set_for,
};
use atc_server::{AppState, StationMap, SttPrompts, persist, router};
use atc_voice::{PiperTts, VoiceError, WhisperStt, initial_prompt, load_voices_config};
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG: &str = "config/server.toml";
const EXIT_MODEL_MISSING: u8 = 2;

/// The config path and whether `--fresh` was given.
fn args() -> Result<(PathBuf, bool)> {
    let mut args = std::env::args().skip(1);
    let mut path = PathBuf::from(DEFAULT_CONFIG);
    let mut fresh = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => path = args.next().context("--config needs a path")?.into(),
            "--fresh" => fresh = true,
            other => {
                bail!("unknown argument {other:?} (usage: atc-server [--config <path>] [--fresh])")
            }
        }
    }
    Ok((path, fresh))
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
    let dir = cfg.data_dir.join(&cfg.airport);
    let read = |name: &str| {
        let p = dir.join(name);
        std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))
    };
    let airport = load_airport(cfg.xplane_root.as_deref(), &dir, &cfg.airport)?;
    let procs = parse_procedures(&read("procedures.toml")?).context("parse procedures.toml")?;
    let stations = StationMap::new(&procs);
    // The prompt is in the language the aircraft speaks, with the stations as said in it.
    let prompt_in = |lang: Lang, code: &str| {
        let names: Vec<String> = procs
            .positions
            .iter()
            .map(|p| station_name(&p.callsign, lang))
            .collect();
        initial_prompt(&airport.name, &names, &[], code)
    };
    let stt_prompts = SttPrompts {
        en: prompt_in(Lang::En, "en"),
        pl: prompt_in(Lang::Pl, "pl"),
    };
    let strictness = match cfg.strictness {
        StrictnessSetting::Standard => Strictness::Standard,
        StrictnessSetting::Training => Strictness::Training,
    };
    let engine_cfg = EngineConfig {
        language: cfg.whisper.language_mode()?,
        ..EngineConfig::new(strictness)
    };
    let set = load_procedure_set_for(cfg.xplane_root.as_deref(), &cfg.airport);
    let destinations = load_destinations_near(
        cfg.xplane_root.as_deref(),
        &airport,
        &adjacent_names(&procs),
    );
    let mut engine = AtcEngine::new(airport, procs, engine_cfg).with_destinations(destinations);
    if let Some(set) = set {
        engine = engine.with_procedure_set(set);
    }

    let stt = WhisperStt::new(&cfg.whisper.model, cfg.whisper.language.as_deref())
        .context("load whisper")?;
    let voices_text = std::fs::read_to_string(&cfg.piper.voices)
        .with_context(|| format!("read {}", cfg.piper.voices.display()))?;
    let voices = load_voices_config(&voices_text, Path::new("."))?;
    let tts = PiperTts::new(cfg.piper.bin.clone(), voices).context("load piper")?;
    Ok(AppState {
        engine: Arc::new(Mutex::new(engine)),
        hub: Arc::new(Mutex::new(Hub::default())),
        stt: Arc::new(stt),
        tts: Arc::new(tts),
        stations,
        stt_prompts,
    })
}

async fn run() -> Result<()> {
    let (path, fresh) = args()?;
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read config {}", path.display()))?;
    let cfg = load_config(&text)?;
    check_models(&cfg).map_err(|e| anyhow::Error::new(ModelsMissing(e)))?;
    // Model loading is blocking (seconds on a cold start).
    let state = tokio::task::spawn_blocking(move || build_state(&cfg).map(|s| (s, cfg))).await??;
    let (state, cfg) = state;
    let listener = tokio::net::TcpListener::bind(&cfg.listen)
        .await
        .with_context(|| format!("bind {}", cfg.listen))?;
    let engine = state.engine.clone();
    let state_path = persist::state_path(&cfg.airport);
    restore_state(&engine, &state_path, fresh);
    let saver = tokio::spawn(persist::save_loop(engine.clone(), state_path.clone()));
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    saver.abort();
    persist::save(&engine, &state_path, &mut String::new());
    tracing::info!("state saved to {}", state_path.display());
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
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
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
