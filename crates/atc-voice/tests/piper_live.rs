#![cfg(feature = "piper")]
//! Needs `tools/fetch-piper.sh` first. Run: cargo test -p atc-voice --features piper -- --include-ignored

use std::path::Path;

use atc_voice::{PiperTts, TextToSpeech, VoiceId, load_voices_config};

#[test]
#[ignore]
fn piper_speaks_atc_phrase() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join("config/voices.toml")).unwrap();
    let voices = load_voices_config(&text, &root).unwrap();
    let tts = PiperTts::new(root.join("models/piper/piper/piper"), voices).unwrap();
    let phrases = [
        ("en", "Sierra Bravo Charlie, cleared for take-off"),
        (
            "pl",
            "Sierra Bravo Charlie, pas dwa pięć, zezwalam startować",
        ),
    ];
    for (lang, text) in phrases {
        for id in [
            VoiceId::Delivery,
            VoiceId::Ground,
            VoiceId::Tower,
            VoiceId::Approach,
            VoiceId::Radar,
            VoiceId::Atis,
        ] {
            let pcm = tts.synthesize(id, lang, text).unwrap();
            let d = pcm.duration_s();
            assert!((1.0..=6.0).contains(&d), "{id:?} {lang} duration {d}");
            let energy: f64 = pcm.samples.iter().map(|&s| (s as f64).powi(2)).sum();
            assert!(energy > 0.0, "{id:?} {lang} silent");
        }
    }
}
