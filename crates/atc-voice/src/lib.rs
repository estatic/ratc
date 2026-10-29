//! Voice I/O foundation: PCM, resampling, radio effect, WAV, STT/TTS traits.
//! No I/O beyond WAV files; real engines live behind the `whisper`/`piper` features.

mod dsp;
#[cfg(any(test, feature = "fakes"))]
mod fakes;
mod pcm;
#[cfg(feature = "piper")]
mod piper;
mod prompt;
mod voices_config;
mod wav;
#[cfg(feature = "whisper")]
mod whisper;

pub use dsp::radio_effect;
#[cfg(any(test, feature = "fakes"))]
pub use fakes::{FakeStt, FakeTts};
pub use pcm::{Pcm, resample};
#[cfg(feature = "piper")]
pub use piper::PiperTts;
pub use prompt::initial_prompt;
pub use voices_config::{
    PiperVoice, VoiceKey, ground_if_unconfigured, load_voices_config, pick_voice,
};
pub use wav::{read_wav, write_wav};
#[cfg(feature = "whisper")]
pub use whisper::WhisperStt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum VoiceError {
    #[error("model missing: {0}")]
    ModelMissing(String),
    #[error("engine error: {0}")]
    Engine(String),
    #[error("io error: {0}")]
    Io(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub lang: String,
    pub confidence: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum VoiceId {
    /// Clearance delivery.
    Delivery,
    Ground,
    Tower,
    /// Approach (also the radio voice of Approach on its alternate frequencies).
    Approach,
    /// Adjacent units: area control, flight information, neighbouring towers.
    Radar,
    Atis,
}

impl VoiceId {
    /// `true` for the positions added after Ground, Tower and ATIS: with no entry in
    /// `voices.toml` they speak with the Ground voice.
    pub fn falls_back_to_ground(self) -> bool {
        matches!(self, VoiceId::Delivery | VoiceId::Approach | VoiceId::Radar)
    }
}

pub trait SpeechToText: Send + Sync {
    /// `language`: ISO code to decode in; `None` = the engine's configured language, or
    /// auto-detection when it has none.
    fn transcribe(
        &self,
        pcm16k: &Pcm,
        prompt: &str,
        language: Option<&str>,
    ) -> Result<Transcript, VoiceError>;
}

pub trait TextToSpeech: Send + Sync {
    /// Speaks `text` with `voice` in language `lang` (ISO code, e.g. `"en"`, `"pl"`).
    fn synthesize(&self, voice: VoiceId, lang: &str, text: &str) -> Result<Pcm, VoiceError>;
}
