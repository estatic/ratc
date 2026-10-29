//! What to play: ATC utterances, the looping ATIS and relayed pilots, filtered by the
//! frequencies heard (the transmit radio and every COM radio selected for listening).
//!
//! No audio I/O: [`Playback`] takes server events and the frequencies heard, and the output
//! device pulls mono samples at its rate with [`Playback::fill`]. Audio of a frequency not
//! heard is never played (the radios may have changed after the server sent it). Retuning
//! stops everything of a frequency no longer heard: queued speech and the ATIS loop.

use std::collections::{HashMap, VecDeque};

use atc_proto::{AudioFrame, ServerMsg};
use atc_voice::{Pcm, resample};

/// Audio on `stream_freq` is audible only with a heard radio tuned to it.
pub fn should_play(stream_freq: u32, heard: &[u32]) -> bool {
    heard
        .iter()
        .any(|&c| atc_proto::same_channel(c, stream_freq))
}

/// An ATC utterance being received (frames arrive between `AtcAudioStart` and `AtcAudioEnd`).
struct Incoming {
    freq_khz: u32,
    sample_rate: u32,
    looping: bool,
    samples: Vec<i16>,
}

/// The looping ATIS, resampled to the output rate.
struct Loop {
    freq_khz: u32,
    samples: Vec<i16>,
    pos: usize,
}

struct Relay {
    from: String,
    freq_khz: u32,
    sample_rate: u32,
}

pub struct Playback {
    out_rate: u32,
    heard: Vec<u32>,
    incoming: HashMap<u32, Incoming>,
    relay: Option<Relay>,
    /// One-shot speech at `out_rate`, frequency-tagged so a retune can drop it.
    queue: VecDeque<(u32, Vec<i16>)>,
    /// Read position in the queue's front buffer.
    front_pos: usize,
    atis: Option<Loop>,
}

impl Playback {
    pub fn new(out_rate: u32) -> Self {
        Playback {
            out_rate,
            heard: Vec::new(),
            incoming: HashMap::new(),
            relay: None,
            queue: VecDeque::new(),
            front_pos: 0,
            atis: None,
        }
    }

    /// Retuning drops all audio of frequencies no longer heard (queued, incoming, loop,
    /// relay).
    pub fn set_heard(&mut self, heard: &[u32]) {
        if self.heard == heard {
            return;
        }
        self.heard = heard.to_vec();
        let keep = |f: u32| should_play(f, heard);
        if self.queue.front().is_some_and(|(f, _)| !keep(*f)) {
            self.front_pos = 0;
        }
        self.queue.retain(|(f, _)| keep(*f));
        self.incoming.retain(|_, u| keep(u.freq_khz));
        if self.atis.as_ref().is_some_and(|l| !keep(l.freq_khz)) {
            self.atis = None;
        }
        if self.relay.as_ref().is_some_and(|r| !keep(r.freq_khz)) {
            self.relay = None;
        }
    }

    pub fn on_msg(&mut self, m: &ServerMsg) {
        match m {
            ServerMsg::AtcAudioStart {
                utterance,
                freq_khz,
                sample_rate,
                looping,
                ..
            } => {
                if should_play(*freq_khz, &self.heard) {
                    self.incoming.insert(
                        *utterance,
                        Incoming {
                            freq_khz: *freq_khz,
                            sample_rate: *sample_rate,
                            looping: *looping,
                            samples: Vec::new(),
                        },
                    );
                }
            }
            ServerMsg::AtcAudioEnd { utterance } => {
                if let Some(u) = self.incoming.remove(utterance) {
                    self.finish(u);
                }
            }
            ServerMsg::RelayStart {
                from,
                freq_khz,
                sample_rate,
            } => {
                if should_play(*freq_khz, &self.heard) {
                    self.relay = Some(Relay {
                        from: from.clone(),
                        freq_khz: *freq_khz,
                        sample_rate: *sample_rate,
                    });
                }
            }
            ServerMsg::RelayEnd { from }
                if self.relay.as_ref().is_some_and(|r| &r.from == from) =>
            {
                self.relay = None;
            }
            _ => {}
        }
    }

    /// Stream 0 is relayed pilot audio (played as it arrives); other streams belong to the
    /// utterance with that id (played once complete — the server sends them all at once).
    pub fn on_audio(&mut self, f: &AudioFrame) {
        if f.stream == 0 {
            if let Some(r) = &self.relay {
                let pcm = self.to_out(r.sample_rate, f.pcm.clone());
                self.queue.push_back((r.freq_khz, pcm));
            }
        } else if let Some(u) = self.incoming.get_mut(&f.stream) {
            u.samples.extend_from_slice(&f.pcm);
        }
    }

    fn finish(&mut self, u: Incoming) {
        let samples = self.to_out(u.sample_rate, u.samples);
        if u.looping {
            self.atis = Some(Loop {
                freq_khz: u.freq_khz,
                samples,
                pos: 0,
            });
        } else {
            self.queue.push_back((u.freq_khz, samples));
        }
    }

    fn to_out(&self, rate: u32, samples: Vec<i16>) -> Vec<i16> {
        resample(
            &Pcm {
                sample_rate: rate,
                samples,
            },
            self.out_rate,
        )
        .samples
    }

    /// Fills `out` (mono, `out_rate`): queued speech first, else the ATIS loop, else silence.
    pub fn fill(&mut self, out: &mut [i16]) {
        for s in out.iter_mut() {
            *s = self.next_sample();
        }
    }

    fn next_sample(&mut self) -> i16 {
        while let Some((_, buf)) = self.queue.front() {
            if let Some(&s) = buf.get(self.front_pos) {
                self.front_pos += 1;
                return s;
            }
            self.queue.pop_front();
            self.front_pos = 0;
        }
        match &mut self.atis {
            Some(l) if !l.samples.is_empty() => {
                let s = l.samples[l.pos];
                l.pos = (l.pos + 1) % l.samples.len();
                s
            }
            _ => 0,
        }
    }

    /// True while something is queued or looping.
    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        !self.queue.is_empty() || self.atis.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GND: u32 = 118_105;
    const TWR: u32 = 123_255;
    const ATIS: u32 = 126_130;

    fn utterance(p: &mut Playback, id: u32, freq: u32, looping: bool, samples: &[i16]) {
        p.on_msg(&ServerMsg::AtcAudioStart {
            utterance: id,
            station: "x".into(),
            freq_khz: freq,
            sample_rate: 16_000,
            looping,
        });
        for (seq, chunk) in samples.chunks(2).enumerate() {
            p.on_audio(&AudioFrame {
                stream: id,
                seq: seq as u32,
                pcm: chunk.to_vec(),
            });
        }
        p.on_msg(&ServerMsg::AtcAudioEnd { utterance: id });
    }

    fn take(p: &mut Playback, n: usize) -> Vec<i16> {
        let mut v = vec![0; n];
        p.fill(&mut v);
        v
    }

    #[test]
    fn should_play_only_on_heard_frequencies() {
        assert!(should_play(GND, &[GND]));
        assert!(!should_play(GND, &[TWR]));
        assert!(!should_play(GND, &[]));
        assert!(should_play(GND, &[TWR, GND]));
        // A 25 kHz radio tunes Kraków Ground 118.105 as 118.100.
        assert!(should_play(118_105, &[118_100]));
    }

    #[test]
    fn audio_for_other_freq_not_played() {
        let mut p = Playback::new(16_000);
        p.set_heard(&[GND]);
        utterance(&mut p, 1, TWR, false, &[5, 5, 5, 5]);
        assert!(!p.is_active());
        assert_eq!(take(&mut p, 4), [0, 0, 0, 0]);
        utterance(&mut p, 2, GND, false, &[1, 2, 3]);
        assert_eq!(take(&mut p, 5), [1, 2, 3, 0, 0]);
        assert!(!p.is_active());
    }

    #[test]
    fn resampled_to_output_rate() {
        let mut p = Playback::new(32_000);
        p.set_heard(&[GND]);
        utterance(&mut p, 1, GND, false, &[100; 160]);
        let out = take(&mut p, 400);
        assert_eq!(out.iter().filter(|&&s| s == 100).count(), 320);
    }

    #[test]
    fn atis_on_com2_keeps_looping_beside_the_transmit_radio() {
        let mut p = Playback::new(16_000);
        p.set_heard(&[TWR, ATIS]);
        utterance(&mut p, 7, ATIS, true, &[1, 2]);
        utterance(&mut p, 8, TWR, false, &[9]);
        // Speech first, then the ATIS loop.
        assert_eq!(take(&mut p, 4), [9, 1, 2, 1]);
        // COM2 no longer listened: the loop stops, the transmit radio stays.
        p.set_heard(&[TWR]);
        assert_eq!(take(&mut p, 2), [0, 0]);
        utterance(&mut p, 9, TWR, false, &[5]);
        assert_eq!(take(&mut p, 1), [5]);
    }

    #[test]
    fn atis_loops_until_retune() {
        let mut p = Playback::new(16_000);
        p.set_heard(&[ATIS]);
        utterance(&mut p, 7, ATIS, true, &[1, 2, 3]);
        assert_eq!(take(&mut p, 7), [1, 2, 3, 1, 2, 3, 1]);
        p.set_heard(&[GND]);
        assert!(!p.is_active());
        assert_eq!(take(&mut p, 3), [0, 0, 0]);
        // back on ATIS: nothing until the server sends it again (it does on tuning)
        p.set_heard(&[ATIS]);
        assert_eq!(take(&mut p, 2), [0, 0]);
    }

    #[test]
    fn retune_drops_queued_and_incoming_speech() {
        let mut p = Playback::new(16_000);
        p.set_heard(&[GND]);
        utterance(&mut p, 1, GND, false, &[9, 9, 9, 9]);
        assert_eq!(take(&mut p, 1), [9]);
        p.on_msg(&ServerMsg::AtcAudioStart {
            utterance: 2,
            station: "x".into(),
            freq_khz: GND,
            sample_rate: 16_000,
            looping: false,
        });
        p.set_heard(&[TWR]);
        p.on_audio(&AudioFrame {
            stream: 2,
            seq: 0,
            pcm: vec![4, 4],
        });
        p.on_msg(&ServerMsg::AtcAudioEnd { utterance: 2 });
        assert_eq!(take(&mut p, 3), [0, 0, 0]);
    }

    #[test]
    fn relay_plays_other_pilots_on_com1() {
        let mut p = Playback::new(16_000);
        p.set_heard(&[GND]);
        let frame = |v: i16| AudioFrame {
            stream: 0,
            seq: 0,
            pcm: vec![v, v],
        };
        // not relaying yet: stream 0 is ignored
        p.on_audio(&frame(1));
        p.on_msg(&ServerMsg::RelayStart {
            from: "SPXYZ".into(),
            freq_khz: GND,
            sample_rate: 16_000,
        });
        p.on_audio(&frame(2));
        p.on_msg(&ServerMsg::RelayEnd {
            from: "SPXYZ".into(),
        });
        p.on_audio(&frame(3));
        assert_eq!(take(&mut p, 3), [2, 2, 0]);
        // a relay on another frequency is not played
        p.on_msg(&ServerMsg::RelayStart {
            from: "SPXYZ".into(),
            freq_khz: TWR,
            sample_rate: 16_000,
        });
        p.on_audio(&frame(4));
        assert_eq!(take(&mut p, 2), [0, 0]);
    }
}
