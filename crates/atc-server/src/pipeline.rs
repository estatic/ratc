//! Speech pipeline: pilot audio -> STT -> network -> text + TTS to the frequency's listeners.

use std::sync::Arc;
use std::time::Instant;

use atc_core::engine::{AtcOutput, LanguageMode};
use atc_core::state::SimTime;
use atc_phraseology::{Callsign, Lang};
use atc_proto::{AudioFrame, ServerMsg};
use atc_voice::{Pcm, SpeechToText, TextToSpeech, Transcript, VoiceError, VoiceId, radio_effect};
use tokio::sync::mpsc;

use crate::app::{Out, Server};
use crate::hub::ClientId;

/// Client -> server pilot audio rate.
pub(crate) const PILOT_RATE: u32 = 16_000;
/// Transcripts below this confidence get "say again" instead of reaching the engine.
const MIN_CONFIDENCE: f32 = 0.3;
/// Server -> client audio frame length: 20 ms of the voice's sample rate (441 samples at
/// 22050 Hz). Frames are queued at once, not paced; the client buffers them.
const FRAMES_PER_S: u32 = 50;

/// One pilot transmission, processed in order per client.
pub(crate) enum Job {
    /// PTT audio at [`PILOT_RATE`].
    Voice {
        freq_khz: u32,
        pcm: Vec<i16>,
        t: f64,
    },
    Text {
        freq_khz: u32,
        text: String,
        t: f64,
    },
    /// The pilot switched aircraft: later jobs are the new callsign's.
    Rename {
        from: Callsign,
        to: String,
    },
}

/// Processes a client's transmissions one after another, so the engine hears them (and the
/// client gets the replies) in the order they were made.
pub(crate) async fn worker(
    server: Arc<Server>,
    id: ClientId,
    mut callsign: String,
    mut jobs: mpsc::UnboundedReceiver<Job>,
) {
    let mut cs = Callsign::new(&callsign);
    while let Some(job) = jobs.recv().await {
        match job {
            Job::Rename { from, to } => {
                // Jobs queued before the change have run: drop what they re-created under
                // the old callsign. Not for the same callsign (that is the new flight), nor
                // once another client has joined under the old one (that flight is theirs).
                let taken = server.state.hub.lock().unwrap().client(&from.0).is_some();
                if from.0 != to && !taken {
                    server.state.network.end_flight(&from);
                }
                cs = Callsign::new(&to);
                callsign = to;
            }
            Job::Voice { freq_khz, pcm, t } => {
                voice(&server, id, &callsign, freq_khz, pcm, t).await
            }
            Job::Text { freq_khz, text, t } => {
                transmission(&server, &cs, freq_khz, &text, Lang::En, t, 0).await
            }
        }
    }
}

async fn voice(
    server: &Arc<Server>,
    id: ClientId,
    callsign: &str,
    freq_khz: u32,
    pcm: Vec<i16>,
    t: f64,
) {
    let start = Instant::now();
    let cs = Callsign::new(callsign);
    let network = &server.state.network;
    // The aircraft's language; the ATIS language if it has no strip (yet).
    let (mode, session) = (network.language(), network.pilot_lang(&cs, freq_khz));
    let (stt, prompt) = (
        server.state.stt.clone(),
        server.stt_prompt(freq_khz, session, &cs),
    );
    let pcm = Pcm {
        sample_rate: PILOT_RATE,
        samples: pcm,
    };
    let result =
        tokio::task::spawn_blocking(move || transcribe(&*stt, &pcm, &prompt, mode, session)).await;
    let stt_ms = start.elapsed().as_millis();
    let tr = match result {
        Ok(Ok(tr)) => tr,
        Ok(Err(e)) => return tracing::error!(%e, freq_khz, "speech-to-text failed"),
        Err(e) => return tracing::error!(%e, freq_khz, "speech-to-text task failed"),
    };
    server.send(
        &[id],
        Out::Msg(ServerMsg::PilotTranscript {
            freq_khz,
            callsign: Some(callsign.to_string()),
            text: tr.text.clone(),
            lang: tr.lang.clone(),
        }),
    );
    tracing::info!(freq_khz, stt_ms, confidence = tr.confidence, text = %tr.text, "pilot transcript");
    if tr.text.trim().is_empty() {
        return tracing::info!("transcript dropped: empty");
    }
    if tr.confidence < MIN_CONFIDENCE {
        tracing::info!("transcript unclear: say again");
        // In the speaker's language: another pilot's language would not be understood.
        let outputs = network.say_again(&cs, freq_khz, session);
        emit_all(server, outputs).await;
        return;
    }
    let lang = transcript_lang(&tr.lang);
    transmission(server, &cs, freq_khz, &tr.text, lang, t, stt_ms).await;
}

/// The engine language of a transcript language code: Polish for `"pl"`, else English.
fn transcript_lang(code: &str) -> Lang {
    if code == "pl" { Lang::Pl } else { Lang::En }
}

/// The ISO code of `lang` (Whisper's and the voices' language key).
pub(crate) fn lang_code(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "en",
        Lang::Pl => "pl",
    }
}

/// Transcribes in the fixed language, or (`Auto`) detects it; a detected language other than
/// English or Polish (an accented pilot) is transcribed again in the `session` language.
fn transcribe(
    stt: &dyn SpeechToText,
    pcm: &Pcm,
    prompt: &str,
    mode: LanguageMode,
    session: Lang,
) -> Result<Transcript, VoiceError> {
    if let LanguageMode::Fixed(l) = mode {
        return stt.transcribe(pcm, prompt, Some(lang_code(l)));
    }
    let tr = stt.transcribe(pcm, prompt, None)?;
    if matches!(tr.lang.as_str(), "en" | "pl") {
        return Ok(tr);
    }
    let forced = lang_code(session);
    tracing::info!(detected = %tr.lang, forced, text = %tr.text, "unsupported language: transcribing again");
    stt.transcribe(pcm, prompt, Some(forced))
}

/// A transmission from `from` (the client that keyed the PTT) through the network.
async fn transmission(
    server: &Arc<Server>,
    from: &Callsign,
    freq_khz: u32,
    text: &str,
    lang: Lang,
    t: f64,
    stt_ms: u128,
) {
    let start = Instant::now();
    let network = &server.state.network;
    let outputs = network.on_transmission(from, freq_khz, text, lang, SimTime(t));
    let orders = network.take_vehicle_orders();
    let engine_ms = start.elapsed().as_millis();
    // The car is shown while Ground's "on the way" is still being synthesized.
    server.send_vehicle_orders(orders);
    let tts_ms = emit_all(server, outputs).await;
    tracing::info!(freq_khz, stt_ms, engine_ms, tts_ms, "transmission");
}

/// Emits each output in turn; returns the total TTS time in ms.
pub(crate) async fn emit_all(server: &Arc<Server>, outputs: Vec<AtcOutput>) -> u128 {
    let mut tts_ms = 0;
    for o in outputs {
        tts_ms += emit(server, o).await;
    }
    tts_ms
}

/// `AtcText` to every listener of the output's frequency, then its audio. A TTS failure
/// leaves the text only. Returns the TTS time in ms.
async fn emit(server: &Arc<Server>, out: AtcOutput) -> u128 {
    let utterance = server.next_utterance();
    server.remember_said(out.freq_khz, &out.text);
    let listeners = server.listeners(out.freq_khz);
    server.send(
        &listeners,
        Out::Msg(ServerMsg::AtcText {
            station: out.station.clone(),
            freq_khz: out.freq_khz,
            text: out.text.clone(),
            utterance,
        }),
    );
    let Some(voice) = server.state.stations.voice_for(out.freq_khz) else {
        return 0;
    };
    let start = Instant::now();
    let lang = lang_code(out.message.lang);
    let pcm = synthesize(server.state.tts.clone(), voice, lang, out.text, utterance).await;
    let tts_ms = start.elapsed().as_millis();
    if let Some(pcm) = pcm {
        send_audio(
            server,
            &listeners,
            utterance,
            &out.station,
            out.freq_khz,
            &pcm,
            false,
        );
    }
    tts_ms
}

/// TTS in `lang` plus the radio effect (seeded by the utterance) off the async runtime. Errors are
/// logged and give `None`.
pub(crate) async fn synthesize(
    tts: Arc<dyn TextToSpeech>,
    voice: VoiceId,
    lang: &'static str,
    text: String,
    utterance: u32,
) -> Option<Pcm> {
    let result = tokio::task::spawn_blocking(move || {
        tts.synthesize(voice, lang, &text)
            .map(|pcm| radio_effect(&pcm, u64::from(utterance)))
    })
    .await;
    match result {
        Ok(Ok(pcm)) => Some(pcm),
        Ok(Err(e)) => {
            tracing::error!(%e, ?voice, "text-to-speech failed; sending text only");
            None
        }
        Err(e) => {
            tracing::error!(%e, ?voice, "text-to-speech task failed; sending text only");
            None
        }
    }
}

/// `AtcAudioStart`, 20 ms frames on stream `utterance`, `AtcAudioEnd`.
pub(crate) fn send_audio(
    server: &Server,
    ids: &[ClientId],
    utterance: u32,
    station: &str,
    freq_khz: u32,
    pcm: &Pcm,
    looping: bool,
) {
    server.send(
        ids,
        Out::Msg(ServerMsg::AtcAudioStart {
            utterance,
            station: station.to_string(),
            freq_khz,
            sample_rate: pcm.sample_rate,
            looping,
        }),
    );
    let frame_len = (pcm.sample_rate / FRAMES_PER_S).max(1) as usize;
    for (seq, chunk) in pcm.samples.chunks(frame_len).enumerate() {
        server.send(
            ids,
            Out::Audio(AudioFrame {
                stream: utterance,
                seq: seq as u32,
                pcm: chunk.to_vec(),
            }),
        );
    }
    server.send(ids, Out::Msg(ServerMsg::AtcAudioEnd { utterance }));
}
