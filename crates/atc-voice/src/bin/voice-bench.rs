//! Measures Whisper STT / Piper TTS latency and AMD VRAM use.
//!
//! `voice-bench --whisper <model> --piper-config config/voices.toml
//!   [--piper-bin models/piper/piper/piper] [--language en] [--runs 10]`

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use atc_voice::{
    Pcm, PiperTts, SpeechToText, TextToSpeech, VoiceId, WhisperStt, initial_prompt,
    load_voices_config, resample,
};

const PILOT_PHRASE: &str = "Krakow Ground, Sierra Alpha Bravo, request taxi";
const DISPATCHER_PHRASE: &str = "Sierra Papa Alpha Bravo Charlie, Krakow Ground, taxi to holding point Alpha one \
     via Alpha, runway two five, QNH one zero one three.";
const WARMUP: usize = 3;
const TARGET_SECS: usize = 3;

struct Args {
    whisper: PathBuf,
    piper_config: PathBuf,
    piper_bin: PathBuf,
    language: Option<String>,
    runs: usize,
}

fn parse_args() -> Result<Args, String> {
    let mut whisper = None;
    let mut piper_config = None;
    let mut piper_bin = PathBuf::from("models/piper/piper/piper");
    let mut language = Some("en".to_owned());
    let mut runs = 10usize;
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--whisper" => whisper = Some(PathBuf::from(val()?)),
            "--piper-config" => piper_config = Some(PathBuf::from(val()?)),
            "--piper-bin" => piper_bin = PathBuf::from(val()?),
            "--language" => {
                let l = val()?;
                language = if l == "auto" { None } else { Some(l) };
            }
            "--runs" => runs = val()?.parse().map_err(|e| format!("--runs: {e}"))?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        whisper: whisper.ok_or("--whisper <model> is required")?,
        piper_config: piper_config.ok_or("--piper-config <file> is required")?,
        piper_bin,
        language,
        runs: runs.max(1),
    })
}

/// VRAM used (bytes) on the AMD card with the largest `mem_info_vram_total`.
fn vram_used() -> Option<u64> {
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir("/sys/class/drm").ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let dev = entry.path().join("device");
        let Some(total) = read_u64(&dev.join("mem_info_vram_total")) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| total > *t) {
            best = Some((total, dev));
        }
    }
    best.and_then(|(_, dev)| read_u64(&dev.join("mem_info_vram_used")))
}

fn read_u64(p: &Path) -> Option<u64> {
    fs::read_to_string(p).ok()?.trim().parse().ok()
}

fn mib(b: Option<u64>) -> String {
    b.map_or("n/a".into(), |b| format!("{:.0}", b as f64 / 1_048_576.0))
}

fn stats(mut ms: Vec<f64>) -> (f64, f64) {
    ms.sort_by(|a, b| a.total_cmp(b));
    let median = ms[ms.len() / 2];
    let p95 = ms[(((ms.len() as f64) * 0.95).ceil() as usize).clamp(1, ms.len()) - 1];
    (median, p95)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(|e| {
        format!(
            "{e}\nusage: voice-bench --whisper <model> --piper-config <voices.toml> \
             [--piper-bin <path>] [--language en|auto] [--runs 10]"
        )
    })?;

    let base = Path::new(".");
    let voices = load_voices_config(&fs::read_to_string(&args.piper_config)?, base)?;
    let tts = PiperTts::new(args.piper_bin.clone(), voices)?;

    // TTS: dispatcher line, warm-up excluded.
    for _ in 0..WARMUP {
        tts.synthesize(VoiceId::Tower, "en", DISPATCHER_PHRASE)?;
    }
    let mut tts_ms = Vec::new();
    let mut tts_audio_s = 0.0;
    for _ in 0..args.runs {
        let t = Instant::now();
        let pcm = tts.synthesize(VoiceId::Tower, "en", DISPATCHER_PHRASE)?;
        tts_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        tts_audio_s = pcm.duration_s();
    }
    let (tts_med, tts_p95) = stats(tts_ms);

    // Test utterance: Piper pilot phrase -> 16 kHz -> padded/trimmed to exactly 3 s.
    let pilot = tts.synthesize(VoiceId::Ground, "en", PILOT_PHRASE)?;
    let mut utt = resample(&pilot, 16_000);
    let natural_s = utt.duration_s();
    utt.samples.resize(16_000 * TARGET_SECS, 0);
    let utt = Pcm {
        sample_rate: 16_000,
        samples: utt.samples,
    };

    let prompt = initial_prompt(
        "Krakow Balice",
        &["Krakow Ground".into(), "Krakow Tower".into()],
        &["Sierra Papa Alpha Bravo Charlie".into()],
        "en",
    );

    let vram_before = vram_used();
    let t = Instant::now();
    let stt = WhisperStt::new(&args.whisper, args.language.as_deref())?;
    let load_ms = t.elapsed().as_secs_f64() * 1000.0;
    let vram_loaded = vram_used();

    let mut text = String::new();
    for _ in 0..WARMUP {
        text = stt.transcribe(&utt, &prompt, None)?.text;
    }
    let mut stt_ms = Vec::new();
    let mut vram_peak = vram_loaded;
    for _ in 0..args.runs {
        let t = Instant::now();
        text = stt.transcribe(&utt, &prompt, None)?.text;
        stt_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        vram_peak = vram_peak.max(vram_used());
    }
    let (stt_med, stt_p95) = stats(stt_ms);

    let model_name = args.whisper.file_name().map_or_else(
        || args.whisper.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    println!(
        "utterance: Piper (ground voice) \"{PILOT_PHRASE}\", {natural_s:.2} s natural, \
         padded/trimmed to {TARGET_SECS} s; runs={} (+{WARMUP} warm-up)",
        args.runs
    );
    println!("transcript: {text:?}");
    println!(
        "| model | load ms | STT median ms | STT p95 ms | TTS median ms | TTS p95 ms | TTS audio s | VRAM before MiB | VRAM loaded MiB | VRAM peak MiB | model VRAM MiB |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    let delta = match (vram_before, vram_peak) {
        (Some(a), Some(b)) => Some(b.saturating_sub(a)),
        _ => None,
    };
    println!(
        "| {model_name} | {load_ms:.0} | {stt_med:.0} | {stt_p95:.0} | {tts_med:.0} | {tts_p95:.0} | {tts_audio_s:.1} | {} | {} | {} | {} |",
        mib(vram_before),
        mib(vram_loaded),
        mib(vram_peak),
        mib(delta)
    );
    Ok(())
}
