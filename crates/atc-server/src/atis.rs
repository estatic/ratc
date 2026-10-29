//! ATIS broadcast: the current letter as text plus looping audio, synthesized once per letter
//! and language.

use std::sync::{Arc, Mutex};

use atc_phraseology::Lang;
use atc_proto::ServerMsg;
use atc_voice::{Pcm, VoiceId};

use crate::app::{Out, Server};
use crate::hub::ClientId;
use crate::pipeline::{lang_code, send_audio, synthesize};

#[derive(Clone)]
struct Cached {
    letter: char,
    lang: Lang,
    utterance: u32,
    pcm: Arc<Pcm>,
}

/// Synthesized audio of the current letter and language. A failed synthesis is not cached
/// (retried on the next send).
#[derive(Default)]
pub(crate) struct AtisCache {
    current: Mutex<Option<Cached>>,
}

impl AtisCache {
    fn get(&self, letter: char, lang: Lang) -> Option<Cached> {
        self.current
            .lock()
            .unwrap()
            .clone()
            .filter(|c| (c.letter, c.lang) == (letter, lang))
    }

    /// Stores `c` unless its letter and language are already cached (a concurrent synthesis
    /// won); returns the cached entry.
    fn put(&self, c: Cached) -> Cached {
        let mut cur = self.current.lock().unwrap();
        match &*cur {
            Some(old) if (old.letter, old.lang) == (c.letter, c.lang) => old.clone(),
            _ => {
                *cur = Some(c.clone());
                c
            }
        }
    }
}

/// Sends the current ATIS (`AtcText`, then `AtcAudioStart { looping: true }` with its frames)
/// to `ids`, spoken by the ATIS voice of the engine's ATIS language. Nothing before the first
/// weather report or without an ATIS position.
pub(crate) async fn send(server: Arc<Server>, ids: Vec<ClientId>) {
    if ids.is_empty() {
        return;
    }
    let stations = &server.state.stations;
    let Some(freq_khz) = stations.atis_khz() else {
        return;
    };
    let station = stations
        .station_name(freq_khz)
        .unwrap_or_default()
        .to_string();
    let Some((letter, lang, text)) = broadcast(&server) else {
        return;
    };
    let cached = match server.atis.get(letter, lang) {
        Some(c) => Some(c),
        None => {
            let utterance = server.next_utterance();
            let pcm = synthesize(
                server.state.tts.clone(),
                VoiceId::Atis,
                lang_code(lang),
                text.clone(),
                utterance,
            )
            .await;
            // The letter (or language) may have moved on while synthesizing: this result is
            // stale (neither cached nor sent); the newer broadcast's own send delivers it.
            if broadcast(&server).map(|b| (b.0, b.1)) != Some((letter, lang)) {
                return;
            }
            pcm.map(|pcm| {
                server.atis.put(Cached {
                    letter,
                    lang,
                    utterance,
                    pcm: Arc::new(pcm),
                })
            })
        }
    };
    let utterance = cached
        .as_ref()
        .map_or_else(|| server.next_utterance(), |c| c.utterance);
    server.send(
        &ids,
        Out::Msg(ServerMsg::AtcText {
            station: station.clone(),
            freq_khz,
            text,
            utterance,
        }),
    );
    if let Some(c) = cached {
        send_audio(&server, &ids, utterance, &station, freq_khz, &c.pcm, true);
    }
}

/// The engine's current ATIS letter, language and text.
fn broadcast(server: &Server) -> Option<(char, Lang, String)> {
    let engine = server.state.engine.lock().unwrap();
    let (letter, text) = engine.atis_broadcast()?;
    Some((letter, engine.atis_lang(), text))
}
