//! Deterministic fakes for tests (enable the `fakes` feature from other crates).

use crate::{Pcm, SpeechToText, TextToSpeech, Transcript, VoiceError, VoiceId};
use std::collections::VecDeque;
use std::sync::Mutex;

/// Replays queued replies in order and records each prompt and requested language. Like
/// Whisper, a reply to a forced language reports that language. When the queue is empty it
/// returns `VoiceError::Engine("FakeStt: no queued reply")`.
#[derive(Default)]
pub struct FakeStt {
    pub reply: Mutex<VecDeque<Result<Transcript, VoiceError>>>,
    pub prompts: Mutex<Vec<String>>,
    pub languages: Mutex<Vec<Option<String>>>,
}

impl SpeechToText for FakeStt {
    fn transcribe(
        &self,
        _pcm16k: &Pcm,
        prompt: &str,
        language: Option<&str>,
    ) -> Result<Transcript, VoiceError> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        self.languages
            .lock()
            .unwrap()
            .push(language.map(str::to_owned));
        let reply = self
            .reply
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(VoiceError::Engine("FakeStt: no queued reply".into())));
        reply.map(|t| match language {
            Some(l) => Transcript {
                lang: l.to_string(),
                ..t
            },
            None => t,
        })
    }
}

/// Returns 0.1 s of silence at 22050 Hz per word, or an `Engine` error if `fail`. Records
/// each `(voice, lang)` asked for.
#[derive(Default)]
pub struct FakeTts {
    pub fail: bool,
    pub calls: Mutex<Vec<(VoiceId, String)>>,
}

impl FakeTts {
    /// A TTS whose every synthesis fails.
    pub fn failing() -> Self {
        FakeTts {
            fail: true,
            ..FakeTts::default()
        }
    }
}

impl TextToSpeech for FakeTts {
    fn synthesize(&self, voice: VoiceId, lang: &str, text: &str) -> Result<Pcm, VoiceError> {
        self.calls.lock().unwrap().push((voice, lang.to_string()));
        if self.fail {
            return Err(VoiceError::Engine("FakeTts: forced failure".into()));
        }
        let words = text.split_whitespace().count();
        Ok(Pcm {
            sample_rate: 22050,
            samples: vec![0; words * 2205],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_tts_is_100ms_per_word() {
        let p = FakeTts::default()
            .synthesize(VoiceId::Tower, "en", "cleared for takeoff")
            .unwrap();
        assert_eq!(p.sample_rate, 22050);
        assert!((p.duration_s() - 0.3).abs() < 1e-9);
        assert!(
            FakeTts::failing()
                .synthesize(VoiceId::Atis, "en", "x")
                .is_err()
        );
    }

    #[test]
    fn fake_stt_pops_in_order_then_errors() {
        let stt = FakeStt::default();
        stt.reply.lock().unwrap().push_back(Ok(Transcript {
            text: "hello".into(),
            lang: "en".into(),
            confidence: 0.9,
        }));
        let pcm = Pcm {
            sample_rate: 16000,
            samples: vec![],
        };
        stt.reply.lock().unwrap().push_back(Ok(Transcript {
            text: "witam".into(),
            lang: "ru".into(),
            confidence: 0.9,
        }));
        assert_eq!(stt.transcribe(&pcm, "", None).unwrap().text, "hello");
        // A forced language is the reported one, whatever the queued reply says.
        assert_eq!(stt.transcribe(&pcm, "", Some("pl")).unwrap().lang, "pl");
        assert!(matches!(
            stt.transcribe(&pcm, "", Some("pl")),
            Err(VoiceError::Engine(_))
        ));
        assert_eq!(
            *stt.languages.lock().unwrap(),
            [None, Some("pl".into()), Some("pl".into())]
        );
    }
}
