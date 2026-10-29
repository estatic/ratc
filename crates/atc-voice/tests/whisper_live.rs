#![cfg(all(feature = "whisper", feature = "piper"))]
//! Needs `tools/fetch-piper.sh` and `tools/fetch-whisper.sh` first.
//! Run: cargo test -p atc-voice --features whisper,piper -- --include-ignored

use std::path::{Path, PathBuf};

use atc_phraseology::{Lang, PilotIntent, parse_pilot};
use atc_voice::{
    Pcm, PiperTts, SpeechToText, TextToSpeech, VoiceId, WhisperStt, initial_prompt,
    load_voices_config, resample,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn whisper() -> WhisperStt {
    let model = root().join("models/whisper/ggml-large-v3-turbo-q5_0.bin");
    WhisperStt::new(&model, None).unwrap()
}

/// Pilot transmissions of the circuit scenario (those spoken by the pilot, not ATC).
const PILOT_LINES: [&str; 5] = [
    "Krakow Ground, Sierra Papa Alpha Bravo Charlie, information Alpha, request taxi for circuits",
    "Krakow Tower, Sierra Papa Alpha Bravo Charlie, holding point runway two five, ready for departure",
    "Sierra Bravo Charlie, downwind, full stop",
    "Sierra Bravo Charlie, final",
    "Krakow Ground, Sierra Bravo Charlie, request taxi to stand",
];

#[test]
#[ignore]
fn tts_to_stt_roundtrip() {
    // The lines must really be in the scenario.
    let scenario = std::fs::read_to_string(root().join("scenarios/epkk-circuit-vmc.toml")).unwrap();
    for l in PILOT_LINES {
        assert!(scenario.contains(l), "not in scenario: {l}");
    }
    let voices = load_voices_config(
        &std::fs::read_to_string(root().join("config/voices.toml")).unwrap(),
        &root(),
    )
    .unwrap();
    let tts = PiperTts::new(root().join("models/piper/piper/piper"), voices).unwrap();
    let stt = whisper();
    let prompt = initial_prompt(
        "Krakow",
        &["Krakow Ground".into(), "Krakow Tower".into()],
        &["Sierra Papa Alpha Bravo Charlie".into()],
        "en",
    );
    let mut ok = 0;
    for line in PILOT_LINES {
        let pcm = tts.synthesize(VoiceId::Ground, "en", line).unwrap();
        let t = stt
            .transcribe(&resample(&pcm, 16_000), &prompt, None)
            .unwrap();
        let want = parse_pilot(line, Lang::En);
        assert_ne!(
            want.intent,
            PilotIntent::Unknown,
            "source line must parse: {line:?}"
        );
        let got = parse_pilot(&t.text, Lang::En);
        let same = want.intent == got.intent && want.items == got.items;
        eprintln!("{line:?}\n  -> {t:?}\n  same={same}");
        ok += usize::from(same);
    }
    assert!(ok >= 4, "only {ok}/5 matched");
}

#[test]
#[ignore]
fn stt_rejects_silence() {
    let stt = whisper();
    let pcm = Pcm {
        sample_rate: 16_000,
        samples: vec![0; 3 * 16_000],
    };
    let t = stt.transcribe(&pcm, "", None).unwrap();
    assert!(
        t.text.is_empty() || t.confidence < 0.3,
        "text {:?} confidence {}",
        t.text,
        t.confidence
    );
}
