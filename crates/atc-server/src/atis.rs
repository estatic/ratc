//! ATIS broadcast: the current letter as text plus looping audio, synthesized once per letter
//! and language, for each station's ATIS frequency.

use std::collections::HashMap;
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

/// Synthesized audio of the current letter and language per ATIS frequency. A failed
/// synthesis is not cached (retried on the next send).
#[derive(Default)]
pub(crate) struct AtisCache {
    current: Mutex<HashMap<u32, Cached>>,
}

impl AtisCache {
    fn get(&self, freq_khz: u32, letter: char, lang: Lang) -> Option<Cached> {
        self.current
            .lock()
            .unwrap()
            .get(&freq_khz)
            .filter(|c| (c.letter, c.lang) == (letter, lang))
            .cloned()
    }

    /// Stores `c` for `freq_khz` unless its letter and language are already cached there (a
    /// concurrent synthesis won); returns the cached entry.
    fn put(&self, freq_khz: u32, c: Cached) -> Cached {
        let mut cur = self.current.lock().unwrap();
        match cur.get(&freq_khz) {
            Some(old) if (old.letter, old.lang) == (c.letter, c.lang) => old.clone(),
            _ => {
                cur.insert(freq_khz, c.clone());
                c
            }
        }
    }
}

/// Sends the current ATIS on `freq_khz` (`AtcText`, then `AtcAudioStart { looping: true }`
/// with its frames) to `ids`, spoken by the ATIS voice of its engine's ATIS language. Nothing
/// before the first weather report or on a frequency without an ATIS position.
pub(crate) async fn send(server: Arc<Server>, freq_khz: u32, ids: Vec<ClientId>) {
    if ids.is_empty() {
        return;
    }
    let station = server
        .state
        .stations
        .station_name(freq_khz)
        .unwrap_or_default()
        .to_string();
    let Some((letter, lang, text)) = server.state.network.atis_broadcast(freq_khz) else {
        return;
    };
    let cached = match server.atis.get(freq_khz, letter, lang) {
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
            let now = server.state.network.atis_broadcast(freq_khz);
            if now.map(|b| (b.0, b.1)) != Some((letter, lang)) {
                return;
            }
            pcm.map(|pcm| {
                server.atis.put(
                    freq_khz,
                    Cached {
                        letter,
                        lang,
                        utterance,
                        pcm: Arc::new(pcm),
                    },
                )
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
