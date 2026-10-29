//! Whisper speech-to-text via whisper-rs (whisper.cpp, Vulkan backend).

use std::path::Path;
use std::sync::{Mutex, Once};

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, get_lang_str,
    install_logging_hooks,
};

use crate::{Pcm, SpeechToText, Transcript, VoiceError};

/// Segments Whisper itself thinks are probably not speech are dropped.
const NO_SPEECH_MAX: f32 = 0.6;

pub struct WhisperStt {
    ctx: WhisperContext,
    /// `None` = auto-detect.
    language: Option<String>,
}

impl WhisperStt {
    /// Loads the model once. `language`: ISO code, or `None` to auto-detect.
    pub fn new(model: &Path, language: Option<&str>) -> Result<Self, VoiceError> {
        if !model.is_file() {
            return Err(VoiceError::ModelMissing(model.display().to_string()));
        }
        // whisper.cpp / ggml logs go to stderr; without a log backend this silences them.
        static HOOKS: Once = Once::new();
        HOOKS.call_once(install_logging_hooks);
        // Concurrent GPU backend initialisation crashes (SIGSEGV), so serialise loads.
        static LOAD: Mutex<()> = Mutex::new(());
        let _guard = LOAD.lock().unwrap_or_else(|p| p.into_inner());
        let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default())
            .map_err(|e| VoiceError::Engine(format!("load whisper model: {e}")))?;
        Ok(Self {
            ctx,
            language: language.map(str::to_owned),
        })
    }
}

impl SpeechToText for WhisperStt {
    fn transcribe(
        &self,
        pcm16k: &Pcm,
        prompt: &str,
        language: Option<&str>,
    ) -> Result<Transcript, VoiceError> {
        // `None` = the configured language, or auto-detect without one.
        let language = language.or(self.language.as_deref());
        if pcm16k.sample_rate != 16_000 {
            return Err(VoiceError::Engine(format!(
                "whisper needs 16 kHz input, got {}",
                pcm16k.sample_rate
            )));
        }
        let audio: Vec<f32> = pcm16k
            .samples
            .iter()
            .map(|&s| f32::from(s) / 32768.0)
            .collect();
        // Digital silence makes Whisper hallucinate ("Thank you."); nothing to decode.
        if pcm16k.samples.iter().all(|&s| s.unsigned_abs() < 16) {
            return Ok(Transcript {
                text: String::new(),
                lang: language.unwrap_or("en").to_string(),
                confidence: 0.0,
            });
        }
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| VoiceError::Engine(format!("whisper state: {e}")))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(language.unwrap_or("auto")));
        params.set_no_context(true);
        params.set_single_segment(false);
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if !prompt.is_empty() {
            params.set_initial_prompt(prompt);
        }
        state
            .full(params, &audio)
            .map_err(|e| VoiceError::Engine(format!("whisper full: {e}")))?;

        let mut text = String::new();
        let (mut prob_sum, mut n) = (0.0f32, 0u32);
        for seg in state.as_iter() {
            if seg.no_speech_probability() > NO_SPEECH_MAX {
                continue;
            }
            text.push_str(
                &seg.to_str_lossy()
                    .map_err(|e| VoiceError::Engine(format!("segment text: {e}")))?,
            );
            for i in 0..seg.n_tokens() {
                let Some(tok) = seg.get_token(i) else {
                    continue;
                };
                // Skip special tokens ([_BEG_], timestamps, ...): they distort the mean.
                if tok.token_data().id >= self.ctx.token_eot() {
                    continue;
                }
                prob_sum += tok.token_probability();
                n += 1;
            }
        }
        let lang = match language {
            Some(l) => l.to_string(),
            None => get_lang_str(state.full_lang_id_from_state())
                .unwrap_or("en")
                .to_string(),
        };
        Ok(Transcript {
            text: text.trim().to_string(),
            lang,
            confidence: if n == 0 { 0.0 } else { prob_sum / n as f32 },
        })
    }
}
